"""Recoverable exclusive-GPU state machine for VoiceOS chat and speech."""

from __future__ import annotations

import json
import os
import socket
import socketserver
import subprocess
import threading
import time
import uuid
from enum import Enum
from pathlib import Path
from urllib.request import Request, urlopen


SOCKET_PATH = Path(
    os.environ.get("VOICEOS_GPU_SCHEDULER_SOCKET", "/run/voiceos-gpu-scheduler/control.sock")
)
OLLAMA_URL = os.environ.get("VOICEOS_OLLAMA_URL", "http://127.0.0.1:11434").rstrip("/")
OLLAMA_MODEL = os.environ.get("VOICEOS_OLLAMA_MODEL", "").strip()
MOSHI_HOST = os.environ.get("VOICEOS_MOSHI_HOST", "127.0.0.1")
MOSHI_PORT = int(os.environ.get("VOICEOS_MOSHI_PORT", "8998"))
MOSHI_START_TIMEOUT = int(os.environ.get("VOICEOS_MOSHI_START_TIMEOUT", "1200"))


class GpuState(str, Enum):
    CHAT = "chat"
    STARTING_SPEECH = "starting_speech"
    SPEECH = "speech"
    RESTORING_CHAT = "restoring_chat"
    FAILED = "failed"


TRANSITION_STATES = {GpuState.STARTING_SPEECH, GpuState.RESTORING_CHAT}


class Scheduler:
    def __init__(self) -> None:
        self._condition = threading.Condition(threading.RLock())
        self._leases: dict[str, float] = {}
        if _port_ready(MOSHI_HOST, MOSHI_PORT):
            self._state = GpuState.SPEECH
        elif _service_active("voiceos-moshi.service"):
            self._state = GpuState.FAILED
        else:
            self._state = GpuState.CHAT
        self._last_error: str | None = None
        self._transition_id: str | None = None
        self._last_transition_unix = int(time.time())

    def dispatch(self, request: dict[str, object]) -> dict[str, object]:
        action = str(request.get("action", ""))
        lease_id = str(request.get("lease_id", "")).strip()
        if action == "status":
            with self._condition:
                self._expire_locked()
                return self._status_locked()
        if action == "acquire":
            return self.acquire(lease_id, _bounded_ttl(request.get("ttl_seconds", 900)))
        if action == "renew":
            return self.renew(lease_id, _bounded_ttl(request.get("ttl_seconds", 900)))
        if action == "release":
            return self.release(lease_id)
        raise ValueError("unsupported_scheduler_action")

    def acquire(self, lease_id: str, ttl_seconds: int) -> dict[str, object]:
        _validate_lease_id(lease_id)
        with self._condition:
            self._expire_locked()
            self._leases[lease_id] = time.monotonic() + ttl_seconds
            self._wait_for_transition_locked()
            if self._state == GpuState.SPEECH and _port_ready(MOSHI_HOST, MOSHI_PORT):
                return self._status_locked()
            if self._state == GpuState.FAILED:
                self._leases.pop(lease_id, None)
                raise RuntimeError("gpu_scheduler_failed_recovery_pending")
            self._begin_transition_locked(GpuState.STARTING_SPEECH)

        try:
            self._perform_start_speech()
        except Exception as error:
            rollback_error = self._best_effort_restore_chat()
            detail = str(error)
            if rollback_error:
                detail = f"{detail}; rollback: {rollback_error}"
            with self._condition:
                self._leases.clear()
                self._finish_transition_locked(GpuState.FAILED, detail)
            raise RuntimeError(f"speech_transition_failed: {detail}") from error

        with self._condition:
            self._finish_transition_locked(GpuState.SPEECH)
            return self._status_locked()

    def renew(self, lease_id: str, ttl_seconds: int) -> dict[str, object]:
        _validate_lease_id(lease_id)
        with self._condition:
            self._expire_locked()
            if lease_id not in self._leases:
                raise ValueError("speech_lease_not_found")
            if self._state != GpuState.SPEECH:
                raise RuntimeError(f"speech_lease_unavailable_in_{self._state.value}")
            self._leases[lease_id] = time.monotonic() + ttl_seconds
            return self._status_locked()

    def release(self, lease_id: str) -> dict[str, object]:
        if lease_id:
            _validate_lease_id(lease_id)
        with self._condition:
            self._leases.pop(lease_id, None)
            self._wait_for_transition_locked()
            if self._leases or self._state == GpuState.CHAT:
                return self._status_locked()
            self._begin_transition_locked(GpuState.RESTORING_CHAT)

        try:
            self._perform_restore_chat()
        except Exception as error:
            with self._condition:
                self._finish_transition_locked(GpuState.FAILED, str(error))
            raise RuntimeError(f"chat_restore_failed: {error}") from error

        with self._condition:
            self._finish_transition_locked(GpuState.CHAT)
            return self._status_locked()

    def maintain(self) -> None:
        while True:
            time.sleep(10)
            try:
                self.maintain_once()
            except Exception as error:
                self._emit("gpu.maintenance_error", detail=str(error))

    def maintain_once(self) -> None:
        with self._condition:
            self._expire_locked()
            if self._leases or self._state in {GpuState.CHAT, *TRANSITION_STATES}:
                return
            self._begin_transition_locked(GpuState.RESTORING_CHAT)
        try:
            self._perform_restore_chat()
        except Exception as error:
            with self._condition:
                self._finish_transition_locked(GpuState.FAILED, str(error))
            return
        with self._condition:
            self._finish_transition_locked(GpuState.CHAT)

    def _perform_start_speech(self) -> None:
        _run("systemctl", "stop", "voiceos-model-warm.service", check=False)
        if _service_active("ollama.service"):
            _unload_ollama()
        _run("systemctl", "stop", "ollama.service")
        _run("systemctl", "start", "voiceos-moshi.service")
        deadline = time.monotonic() + MOSHI_START_TIMEOUT
        while time.monotonic() < deadline:
            if _port_ready(MOSHI_HOST, MOSHI_PORT):
                return
            if not _service_active("voiceos-moshi.service"):
                raise RuntimeError("moshi_service_failed_during_startup")
            time.sleep(2)
        raise TimeoutError("moshi_backend_start_timeout")

    def _perform_restore_chat(self) -> None:
        _run("systemctl", "stop", "voiceos-moshi.service", check=False)
        _run("systemctl", "start", "ollama.service")
        _run("systemctl", "restart", "voiceos-model-warm.service")

    def _best_effort_restore_chat(self) -> str | None:
        try:
            self._perform_restore_chat()
            return None
        except Exception as error:
            return str(error)

    def _wait_for_transition_locked(self) -> None:
        deadline = time.monotonic() + MOSHI_START_TIMEOUT + 300
        while self._state in TRANSITION_STATES:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("gpu_transition_wait_timeout")
            self._condition.wait(timeout=min(remaining, 5))

    def _begin_transition_locked(self, state: GpuState) -> None:
        previous = self._state
        self._state = state
        self._transition_id = str(uuid.uuid4())
        self._last_transition_unix = int(time.time())
        self._last_error = None
        self._emit("gpu.transition_started", previous=previous.value, state=state.value)

    def _finish_transition_locked(self, state: GpuState, error: str | None = None) -> None:
        previous = self._state
        self._state = state
        self._last_transition_unix = int(time.time())
        self._last_error = error
        self._emit(
            "gpu.transition_finished",
            previous=previous.value,
            state=state.value,
            detail=error,
        )
        self._condition.notify_all()

    def _expire_locked(self) -> None:
        now = time.monotonic()
        self._leases = {key: expiry for key, expiry in self._leases.items() if expiry > now}

    def _status_locked(self) -> dict[str, object]:
        mode = "speech" if self._state in {GpuState.STARTING_SPEECH, GpuState.SPEECH} else "chat"
        return {
            "ok": self._state != GpuState.FAILED,
            "mode": mode,
            "state": self._state.value,
            "transition_id": self._transition_id,
            "last_transition_unix": self._last_transition_unix,
            "speech_leases": len(self._leases),
            "moshi_ready": _port_ready(MOSHI_HOST, MOSHI_PORT),
            "last_error": self._last_error,
        }

    def _emit(self, event: str, **detail: object) -> None:
        print(
            json.dumps(
                {
                    "timestamp_unix": int(time.time()),
                    "event": event,
                    "transition_id": self._transition_id,
                    **detail,
                },
                separators=(",", ":"),
                sort_keys=True,
            ),
            flush=True,
        )


def _bounded_ttl(value: object) -> int:
    try:
        return max(30, min(int(value), 3600))
    except (TypeError, ValueError) as error:
        raise ValueError("invalid_lease_ttl") from error


def _validate_lease_id(lease_id: str) -> None:
    if not lease_id or len(lease_id) > 128:
        raise ValueError("valid_lease_id_required")


def _run(*command: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(command, check=check, text=True, capture_output=True, timeout=240)


def _service_active(name: str) -> bool:
    return _run("systemctl", "is-active", "--quiet", name, check=False).returncode == 0


def _port_ready(host: str, port: int) -> bool:
    try:
        with socket.create_connection((host, port), timeout=0.5):
            return True
    except OSError:
        return False


def _unload_ollama() -> None:
    if not OLLAMA_MODEL:
        raise RuntimeError("VOICEOS_OLLAMA_MODEL_not_configured")
    body = json.dumps({"model": OLLAMA_MODEL, "keep_alive": 0}).encode()
    request = Request(
        f"{OLLAMA_URL}/api/generate",
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urlopen(request, timeout=120) as response:
        response.read()


class RequestHandler(socketserver.StreamRequestHandler):
    def handle(self) -> None:
        try:
            raw = self.rfile.readline(4097)
            if not raw or len(raw) > 4096:
                raise ValueError("invalid_scheduler_request")
            request = json.loads(raw)
            if not isinstance(request, dict):
                raise ValueError("scheduler_request_must_be_an_object")
            response = self.server.scheduler.dispatch(request)  # type: ignore[attr-defined]
        except Exception as error:
            response = {"ok": False, "error": type(error).__name__, "detail": str(error)}
        self.wfile.write(json.dumps(response, separators=(",", ":")).encode() + b"\n")


_SchedulerServerBase = getattr(
    socketserver, "ThreadingUnixStreamServer", socketserver.ThreadingTCPServer
)


class SchedulerServer(_SchedulerServerBase):
    daemon_threads = True

    def __init__(self, scheduler: Scheduler) -> None:
        SOCKET_PATH.parent.mkdir(parents=True, exist_ok=True)
        SOCKET_PATH.unlink(missing_ok=True)
        super().__init__(str(SOCKET_PATH), RequestHandler)
        self.scheduler = scheduler
        os.chmod(SOCKET_PATH, 0o660)


def main() -> None:
    scheduler = Scheduler()
    threading.Thread(target=scheduler.maintain, daemon=True).start()
    with SchedulerServer(scheduler) as server:
        server.serve_forever(poll_interval=0.5)


if __name__ == "__main__":
    main()
