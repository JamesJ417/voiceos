"""Typed, allowlisted system tools with explicit approval policies."""

from __future__ import annotations

import os
import platform
import shutil
import socket
import subprocess
import sys
import time
import uuid
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable

from .system_health import collect_system_health
from .root_broker import RootBrokerClient


@dataclass(frozen=True)
class ToolSpec:
    name: str
    description: str
    approval: str
    read_only: bool
    parameters: dict[str, object]


@dataclass(frozen=True)
class ToolResult:
    request_id: str
    name: str
    arguments: dict[str, object]
    status: str
    approval_required: bool
    result: dict[str, object] | None
    error: str | None
    processing_ms: int

    def as_dict(self) -> dict[str, object]:
        return asdict(self)


ToolFunction = Callable[[dict[str, object]], dict[str, object]]


class ToolBroker:
    """Executes only registered functions; it never accepts a command string."""

    def __init__(self, project_root: Path | None = None) -> None:
        self.project_root = (project_root or Path.cwd()).resolve()
        self._specs = {
            "system.health": ToolSpec(
                "system.health", "Report deterministic host health evidence.", "none", True,
                _empty_parameters(),
            ),
            "disk.space": ToolSpec(
                "disk.space", "Report disk capacity for the VoiceOS project volume.", "none", True,
                _empty_parameters(),
            ),
            "network.status": ToolSpec(
                "network.status", "Report local host and interface addresses.", "none", True,
                _empty_parameters(),
            ),
            "service.status": ToolSpec(
                "service.status", "Check an allowlisted operating-system service.", "none", True,
                {
                    "type": "object",
                    "properties": {
                        "service": {
                            "type": "string",
                            "enum": ["tailscale", "voiceos", "ollama"],
                        }
                    },
                    "additionalProperties": False,
                },
            ),
            "project.tests": ToolSpec(
                "project.tests", "Run the fixed VoiceOS gateway test suite.", "confirm", False,
                {
                    "type": "object",
                    "properties": {"suite": {"type": "string", "enum": ["gateway"]}},
                    "additionalProperties": False,
                },
            ),
        }
        self._functions: dict[str, ToolFunction] = {
            "system.health": self._system_health,
            "disk.space": self._disk_space,
            "network.status": self._network_status,
            "service.status": self._service_status,
            "project.tests": self._project_tests,
        }
        if os.environ.get("VOICEOS_ROOT_BROKER_ENABLED") == "1":
            self._specs["rig.root_command"] = ToolSpec(
                "rig.root_command",
                "Run an exact administrative argv on the rig after explicit physical approval.",
                "confirm",
                False,
                {
                    "type": "object",
                    "properties": {
                        "argv": {
                            "type": "array",
                            "items": {"type": "string"},
                            "minItems": 1,
                            "maxItems": 128,
                        },
                        "cwd": {"type": "string"},
                        "timeout_seconds": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 300,
                        },
                        "rollback": {
                            "type": "string",
                            "description": "Exact recovery or reversal procedure shown with the approval.",
                            "minLength": 1,
                            "maxLength": 4000,
                        },
                    },
                    "required": ["argv", "cwd", "rollback"],
                    "additionalProperties": False,
                },
            )
        self._model_aliases = {
            spec.name.replace(".", "_"): spec.name for spec in self._specs.values()
        }

    def describe(self) -> list[dict[str, object]]:
        return [asdict(spec) for spec in self._specs.values()]

    def model_schemas(self) -> list[dict[str, object]]:
        return [
            {
                "type": "function",
                "function": {
                    "name": spec.name.replace(".", "_"),
                    "description": spec.description,
                    "parameters": spec.parameters,
                },
            }
            for spec in self._specs.values()
        ]

    def execute(
        self,
        name: str,
        arguments: dict[str, object] | None = None,
        *,
        approved: bool = False,
        request_id: str | None = None,
    ) -> ToolResult:
        started = time.perf_counter()
        selected_request_id = request_id or str(uuid.uuid4())
        safe_arguments = arguments or {}
        canonical_name = self._model_aliases.get(name, name)
        spec = self._specs.get(canonical_name)
        if spec is None:
            return ToolResult(
                selected_request_id, name, safe_arguments, "denied", False, None,
                "tool_not_allowlisted", _elapsed(started)
            )
        validation_error = self._validate_arguments(canonical_name, safe_arguments)
        if validation_error is not None:
            return ToolResult(
                selected_request_id, canonical_name, safe_arguments, "denied", False, None,
                validation_error, _elapsed(started)
            )
        if spec.approval == "confirm" and not approved:
            return ToolResult(
                selected_request_id, canonical_name, safe_arguments, "approval_required", True, None, None,
                _elapsed(started)
            )

        try:
            if canonical_name == "rig.root_command":
                result = self._root_command(selected_request_id, safe_arguments)
            else:
                result = self._functions[canonical_name](safe_arguments)
            return ToolResult(
                selected_request_id, canonical_name, safe_arguments, "completed", False, result, None,
                _elapsed(started)
            )
        except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
            return ToolResult(
                selected_request_id, canonical_name, safe_arguments, "error", False, None, str(error),
                _elapsed(started)
            )

    def _validate_arguments(self, name: str, arguments: dict[str, object]) -> str | None:
        if name in {"system.health", "disk.space", "network.status"}:
            return None if not arguments else "arguments_not_allowed"
        if name == "service.status":
            if set(arguments) - {"service"}:
                return "arguments_not_allowlisted"
            service = arguments.get("service", "tailscale")
            if not isinstance(service, str):
                return "service_not_allowlisted"
            return None if service.casefold() in {"tailscale", "voiceos", "ollama"} else "service_not_allowlisted"
        if name == "project.tests":
            if set(arguments) - {"suite"}:
                return "arguments_not_allowlisted"
            return None if arguments.get("suite", "gateway") == "gateway" else "suite_not_allowlisted"
        if name == "rig.root_command":
            if set(arguments) - {"argv", "cwd", "timeout_seconds", "rollback"}:
                return "arguments_not_allowlisted"
            argv = arguments.get("argv")
            cwd = arguments.get("cwd")
            timeout = arguments.get("timeout_seconds", 60)
            rollback = arguments.get("rollback")
            if not isinstance(argv, list) or not argv or any(not isinstance(item, str) or not item for item in argv):
                return "valid_argv_required"
            if not Path(argv[0]).is_absolute():
                return "absolute_executable_required"
            if not isinstance(cwd, str) or not Path(cwd).is_absolute():
                return "absolute_cwd_required"
            if not isinstance(timeout, int) or not 1 <= timeout <= 300:
                return "timeout_out_of_range"
            if not isinstance(rollback, str) or not rollback.strip() or len(rollback) > 4_000:
                return "rollback_information_required"
            return None
        return "tool_not_allowlisted"

    def _root_command(
        self, request_id: str, arguments: dict[str, object]
    ) -> dict[str, object]:
        client = RootBrokerClient(
            Path(os.environ.get("VOICEOS_ROOT_BROKER_SOCKET", "/run/voiceos-root-broker/broker.sock")),
            Path(os.environ.get("VOICEOS_ROOT_BROKER_KEY", "/etc/voiceos/root-broker.key")),
        )
        return client.execute(request_id, arguments)

    def _system_health(self, arguments: dict[str, object]) -> dict[str, object]:
        _require_no_arguments(arguments)
        return collect_system_health(self.project_root)

    def _disk_space(self, arguments: dict[str, object]) -> dict[str, object]:
        _require_no_arguments(arguments)
        disk = shutil.disk_usage(self.project_root)
        return {
            "path": str(self.project_root.anchor),
            "total_bytes": disk.total,
            "used_bytes": disk.used,
            "free_bytes": disk.free,
            "free_percent": round((disk.free / disk.total) * 100, 1) if disk.total else 0.0,
        }

    def _network_status(self, arguments: dict[str, object]) -> dict[str, object]:
        _require_no_arguments(arguments)
        hostname = socket.gethostname()
        addresses = sorted(
            {
                item[4][0]
                for item in socket.getaddrinfo(hostname, None)
                if item[0] in (socket.AF_INET, socket.AF_INET6)
            }
        )
        return {"hostname": hostname, "addresses": addresses}

    def _service_status(self, arguments: dict[str, object]) -> dict[str, object]:
        requested = arguments.get("service", "tailscale")
        if not isinstance(requested, str):
            raise ValueError("service_must_be_string")
        aliases = {
            "tailscale": "Tailscale" if sys.platform == "win32" else "tailscaled",
            "voiceos": "VoiceOSGateway" if sys.platform == "win32" else "voiceos-gateway",
            "ollama": "Ollama" if sys.platform == "win32" else "ollama",
        }
        service = aliases.get(requested.casefold())
        if service is None:
            raise ValueError("service_not_allowlisted")

        if sys.platform == "win32":
            completed = subprocess.run(
                ["sc.exe", "query", service],
                capture_output=True,
                text=True,
                timeout=10,
                check=False,
            )
            output = (completed.stdout or completed.stderr).strip()
            active = "RUNNING" in output
        else:
            completed = subprocess.run(
                ["systemctl", "is-active", service],
                capture_output=True,
                text=True,
                timeout=10,
                check=False,
            )
            output = (completed.stdout or completed.stderr).strip()
            active = completed.returncode == 0 and output == "active"
        return {
            "service": requested.casefold(),
            "active": active,
            "status": output[:2_000],
        }

    def _project_tests(self, arguments: dict[str, object]) -> dict[str, object]:
        suite = arguments.get("suite", "gateway")
        if suite != "gateway":
            raise ValueError("suite_not_allowlisted")
        completed = subprocess.run(
            [
                sys.executable,
                "-m",
                "unittest",
                "discover",
                "-s",
                "services/gateway/tests",
                "-v",
            ],
            cwd=self.project_root,
            capture_output=True,
            text=True,
            timeout=120,
            check=False,
            env={**os.environ, "PYTHONUNBUFFERED": "1"},
        )
        output = "\n".join(part for part in (completed.stdout, completed.stderr) if part).strip()
        return {
            "suite": "gateway",
            "passed": completed.returncode == 0,
            "exit_code": completed.returncode,
            "output": output[-20_000:],
        }


def _require_no_arguments(arguments: dict[str, object]) -> None:
    if arguments:
        raise ValueError("arguments_not_allowed")


def _elapsed(started: float) -> int:
    return max(1, round((time.perf_counter() - started) * 1000))


def _empty_parameters() -> dict[str, object]:
    return {"type": "object", "properties": {}, "additionalProperties": False}
