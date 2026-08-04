"""Quarantine and approval worker for Hermes-created skills.

The worker runs as the Hermes service account so the public VoiceOS gateway
never receives direct write access to Hermes's active skill tree.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import secrets
import threading
import time
import uuid
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, cast
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

MAX_SKILL_BYTES = 256 * 1024


class SkillControlError(RuntimeError):
    pass


class SkillController:
    def __init__(self, skills_root: Path, state_root: Path, rust_url: str) -> None:
        self.skills_root = skills_root.resolve()
        self.state_root = state_root.resolve()
        self.rust_url = rust_url.rstrip("/")
        self.state_path = self.state_root / "state.json"
        self.snapshots = self.state_root / "snapshots"
        self.quarantine = self.state_root / "quarantine"
        self._lock = threading.Lock()
        self.state_root.mkdir(parents=True, exist_ok=True)
        self.snapshots.mkdir(mode=0o750, exist_ok=True)
        self.quarantine.mkdir(mode=0o750, exist_ok=True)
        self._state = self._load_state()
        if not self._state["initialized"]:
            self._initialize_baseline()

    def scan(self, run_id: str) -> list[dict[str, object]]:
        with self._lock:
            proposals: list[dict[str, object]] = []
            current = self._scan_files()
            baseline = cast(dict[str, dict[str, str]], self._state["files"])
            for relative_path, discovered in current.items():
                previous = baseline.get(relative_path)
                if previous is not None and previous["sha256"] == discovered["sha256"]:
                    continue
                proposals.append(
                    self._quarantine_change(relative_path, discovered, previous, run_id)
                )
            self._save_state()
            return proposals

    def decide(self, proposal_id: str, approve: bool) -> dict[str, object]:
        with self._lock:
            proposals = cast(dict[str, dict[str, Any]], self._state["proposals"])
            metadata = proposals.get(proposal_id)
            if metadata is None:
                raise SkillControlError("skill proposal is unknown")
            if metadata.get("status") != "quarantined":
                raise SkillControlError("skill proposal is not pending")
            if not approve:
                metadata["status"] = "rejected"
                metadata["decided_at"] = _now()
                self._save_state()
                return metadata
            validation = metadata.get("validation")
            if not isinstance(validation, dict) or validation.get("passed") is not True:
                raise SkillControlError("skill validation must pass before approval")
            relative_path = str(metadata["relative_path"])
            target = self._safe_target(relative_path)
            quarantine_path = self.quarantine / proposal_id / "SKILL.md"
            content = quarantine_path.read_bytes()
            if _sha256(content) != metadata["sha256"]:
                raise SkillControlError("quarantined skill hash does not match provenance")
            target.parent.mkdir(parents=True, exist_ok=True)
            _atomic_write(target, content)
            self._snapshot(content)
            files = cast(dict[str, dict[str, str]], self._state["files"])
            files[relative_path] = {"sha256": metadata["sha256"]}
            metadata["status"] = "approved"
            metadata["decided_at"] = _now()
            self._save_state()
            return metadata

    def rollback(self, proposal_id: str) -> dict[str, object]:
        with self._lock:
            proposals = cast(dict[str, dict[str, Any]], self._state["proposals"])
            metadata = proposals.get(proposal_id)
            if metadata is None or metadata.get("status") != "approved":
                raise SkillControlError("approved skill proposal is required for rollback")
            relative_path = str(metadata["relative_path"])
            target = self._safe_target(relative_path)
            previous_sha = metadata.get("previous_sha256")
            files = cast(dict[str, dict[str, str]], self._state["files"])
            if isinstance(previous_sha, str) and previous_sha:
                prior = (self.snapshots / f"{previous_sha}.md").read_bytes()
                _atomic_write(target, prior)
                files[relative_path] = {"sha256": previous_sha}
            else:
                if target.exists():
                    rolled_back = self.quarantine / proposal_id / "rolled-back-SKILL.md"
                    _atomic_write(rolled_back, target.read_bytes())
                    target.unlink()
                files.pop(relative_path, None)
            metadata["status"] = "rolled_back"
            metadata["rolled_back_at"] = _now()
            self._save_state()
            return metadata

    def _initialize_baseline(self) -> None:
        files = cast(dict[str, dict[str, str]], self._state["files"])
        for relative_path, discovered in self._scan_files().items():
            self._snapshot(discovered["content"])
            files[relative_path] = {"sha256": discovered["sha256"]}
        self._state["initialized"] = True
        self._state["initialized_at"] = _now()
        self._save_state()

    def _quarantine_change(
        self,
        relative_path: str,
        discovered: dict[str, Any],
        previous: dict[str, str] | None,
        run_id: str,
    ) -> dict[str, object]:
        target = self._safe_target(relative_path)
        content = cast(bytes, discovered["content"])
        validation = validate_skill(content)
        capabilities = infer_capabilities(content.decode("utf-8", errors="replace"))
        pending_id = f"pending-{uuid.uuid4()}"
        pending_dir = self.quarantine / pending_id
        pending_dir.mkdir(mode=0o750)
        quarantine_target = pending_dir / "SKILL.md"
        _atomic_write(quarantine_target, content)
        # systemd's writable bind mounts can make these two roots return EXDEV
        # even when backed by one physical disk. Verify before removing source.
        with target.open("rb") as skill_file:
            current_content = skill_file.read(MAX_SKILL_BYTES + 1)
        if _sha256(current_content) != discovered["sha256"]:
            quarantine_target.unlink(missing_ok=True)
            raise SkillControlError("skill changed during quarantine; rescan required")
        target.unlink()
        if previous is not None:
            prior = (self.snapshots / f"{previous['sha256']}.md").read_bytes()
            _atomic_write(target, prior)
        evidence = [
            {
                "source": "hermes-skill-worker",
                "run_id": run_id,
                "relative_path": relative_path,
                "sha256": discovered["sha256"],
                "previous_sha256": previous["sha256"] if previous else None,
                "validation": validation,
                "quarantined_at": _now(),
                "rollback": "restore_previous_snapshot" if previous else "remove_new_skill",
            }
        ]
        name = validation.get("name") or Path(relative_path).parent.name
        request_body = {
            "name": str(name)[:120],
            "content": content.decode("utf-8", errors="replace"),
            "required_capabilities": capabilities,
            "evidence": evidence,
        }
        try:
            imported = self._post_rust("/internal/v1/skills/import", request_body)
            proposal = imported.get("proposal")
            proposal_id = proposal.get("id") if isinstance(proposal, dict) else None
            if not isinstance(proposal_id, str) or not proposal_id:
                raise SkillControlError("Rust skill store returned no proposal ID")
        except Exception:
            failure_metadata = {
                "status": "import_failed",
                "relative_path": relative_path,
                "sha256": discovered["sha256"],
                "previous_sha256": previous["sha256"] if previous else None,
                "quarantine_path": str(pending_dir),
            }
            cast(dict[str, Any], self._state["proposals"])[pending_id] = failure_metadata
            self._save_state()
            raise
        final_dir = self.quarantine / proposal_id
        pending_dir.replace(final_dir)
        metadata: dict[str, object] = {
            "proposal_id": proposal_id,
            "status": "quarantined",
            "relative_path": relative_path,
            "sha256": discovered["sha256"],
            "previous_sha256": previous["sha256"] if previous else None,
            "required_capabilities": capabilities,
            "validation": validation,
            "run_id": run_id,
            "quarantined_at": _now(),
        }
        cast(dict[str, Any], self._state["proposals"])[proposal_id] = metadata
        (final_dir / "metadata.json").write_text(
            json.dumps(metadata, indent=2, sort_keys=True), encoding="utf-8"
        )
        return metadata

    def _scan_files(self) -> dict[str, dict[str, Any]]:
        found: dict[str, dict[str, Any]] = {}
        if not self.skills_root.exists():
            return found
        for path in self.skills_root.rglob("SKILL.md"):
            if path.is_symlink() or not path.is_file():
                continue
            target = path.resolve()
            if self.skills_root not in target.parents:
                continue
            with path.open("rb") as skill_file:
                # One extra byte preserves an oversized result for validation
                # without allowing an untrusted skill to exhaust worker memory.
                content = skill_file.read(MAX_SKILL_BYTES + 1)
            relative = path.relative_to(self.skills_root).as_posix()
            found[relative] = {"sha256": _sha256(content), "content": content}
        return found

    def _safe_target(self, relative_path: str) -> Path:
        if not relative_path or Path(relative_path).is_absolute() or ".." in Path(relative_path).parts:
            raise SkillControlError("skill path is invalid")
        target = (self.skills_root / relative_path).resolve(strict=False)
        if self.skills_root not in target.parents:
            raise SkillControlError("skill path escapes the active root")
        return target

    def _snapshot(self, content: bytes) -> str:
        digest = _sha256(content)
        path = self.snapshots / f"{digest}.md"
        if not path.exists():
            _atomic_write(path, content)
        return digest

    def _post_rust(self, path: str, body: dict[str, object]) -> dict[str, object]:
        request = Request(
            f"{self.rust_url}{path}",
            data=json.dumps(body).encode("utf-8"),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urlopen(request, timeout=10) as response:
                result = json.loads(response.read())
        except (HTTPError, URLError, TimeoutError, json.JSONDecodeError) as error:
            raise SkillControlError(f"Rust skill import failed: {error}") from error
        if not isinstance(result, dict):
            raise SkillControlError("Rust skill import returned malformed JSON")
        return result

    def _load_state(self) -> dict[str, object]:
        if not self.state_path.exists():
            return {"initialized": False, "files": {}, "proposals": {}}
        try:
            value = json.loads(self.state_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise SkillControlError(f"skill state is unreadable: {error}") from error
        if not isinstance(value, dict):
            raise SkillControlError("skill state is malformed")
        value.setdefault("initialized", False)
        value.setdefault("files", {})
        value.setdefault("proposals", {})
        return cast(dict[str, object], value)

    def _save_state(self) -> None:
        _atomic_write(
            self.state_path,
            json.dumps(self._state, indent=2, sort_keys=True).encode("utf-8"),
        )


def validate_skill(content: bytes) -> dict[str, object]:
    errors: list[str] = []
    if len(content) > MAX_SKILL_BYTES:
        errors.append("skill_exceeds_size_limit")
    text = content.decode("utf-8", errors="replace")
    if "\x00" in text:
        errors.append("null_byte_detected")
    name = None
    description = None
    if not text.startswith("---\n"):
        errors.append("yaml_frontmatter_required")
    else:
        end = text.find("\n---", 4)
        if end < 0:
            errors.append("yaml_frontmatter_unterminated")
        else:
            frontmatter = text[4:end]
            name_match = re.search(r"(?m)^name:\s*[\"']?([^\n\"']+)", frontmatter)
            description_match = re.search(
                r"(?m)^description:\s*[\"']?([^\n\"']+)", frontmatter
            )
            name = name_match.group(1).strip() if name_match else None
            description = description_match.group(1).strip() if description_match else None
            if not name:
                errors.append("frontmatter_name_required")
            if not description:
                errors.append("frontmatter_description_required")
    return {"passed": not errors, "errors": errors, "name": name, "description": description}


def infer_capabilities(text: str) -> list[str]:
    lowered = text.casefold()
    checks = {
        "terminal": ("shell", "terminal", "subprocess", "bash", "powershell"),
        "filesystem.write": ("write_file", "patch", "delete", "move file"),
        "network": ("http://", "https://", "curl", "wget", "api request"),
        "browser": ("browser", "playwright", "crawl"),
        "git": ("git ", "github", "pull request"),
        "containers": ("docker", "podman", "container"),
        "credentials": ("api key", "token", "credential", "secret"),
        "system.service": ("systemctl", "service restart", "sudo"),
    }
    return sorted(name for name, terms in checks.items() if any(term in lowered for term in terms))


class SkillWorkerServer(ThreadingHTTPServer):
    def __init__(self, address: tuple[str, int], controller: SkillController, token: str) -> None:
        self.controller = controller
        self.token = token
        super().__init__(address, SkillWorkerHandler)


class SkillWorkerHandler(BaseHTTPRequestHandler):
    @property
    def worker(self) -> SkillWorkerServer:
        return cast(SkillWorkerServer, self.server)

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/v1/health":
            self._json(HTTPStatus.OK, {"status": "ok", "worker": "hermes-skill-control"})
            return
        self._json(HTTPStatus.NOT_FOUND, {"error": "not_found"})

    def do_POST(self) -> None:  # noqa: N802
        if not secrets.compare_digest(
            self.headers.get("Authorization", ""), f"Bearer {self.worker.token}"
        ):
            self._json(HTTPStatus.UNAUTHORIZED, {"error": "authentication_required"})
            return
        payload = self._read_json()
        if payload is None:
            return
        try:
            if self.path == "/v1/scan":
                run_id = str(payload.get("run_id", "scheduled-scan"))[:160]
                self._json(HTTPStatus.OK, {"proposals": self.worker.controller.scan(run_id)})
                return
            match = re.fullmatch(r"/v1/proposals/([^/]+)/(decision|rollback)", self.path)
            if match:
                proposal_id, operation = match.groups()
                if operation == "rollback":
                    result = self.worker.controller.rollback(proposal_id)
                else:
                    decision = payload.get("decision")
                    if decision not in {"approve", "reject"}:
                        self._json(HTTPStatus.BAD_REQUEST, {"error": "invalid_decision"})
                        return
                    result = self.worker.controller.decide(proposal_id, decision == "approve")
                self._json(HTTPStatus.OK, {"skill": result})
                return
            self._json(HTTPStatus.NOT_FOUND, {"error": "not_found"})
        except SkillControlError as error:
            self._json(HTTPStatus.CONFLICT, {"error": str(error)})
        except Exception as error:
            self._json(HTTPStatus.INTERNAL_SERVER_ERROR, {"error": str(error)[:500]})

    def _read_json(self) -> dict[str, object] | None:
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if length <= 0 or length > 64 * 1024:
                raise ValueError
            value = json.loads(self.rfile.read(length))
        except (ValueError, json.JSONDecodeError):
            self._json(HTTPStatus.BAD_REQUEST, {"error": "invalid_json"})
            return None
        if not isinstance(value, dict):
            self._json(HTTPStatus.BAD_REQUEST, {"error": "object_required"})
            return None
        return value

    def _json(self, status: HTTPStatus, payload: dict[str, object]) -> None:
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format: str, *_args: object) -> None:
        return


def _atomic_write(path: Path, content: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    temporary.write_bytes(content)
    os.chmod(temporary, 0o640)
    os.replace(temporary, path)


def _sha256(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


def _now() -> str:
    from datetime import UTC, datetime

    return datetime.now(UTC).isoformat()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8794)
    args = parser.parse_args()
    token_file = Path(os.environ.get("VOICEOS_SKILL_WORKER_TOKEN_FILE", "/etc/voiceos/hermes-skill-worker.key"))
    token = token_file.read_text(encoding="utf-8").strip()
    if not token:
        raise SystemExit("skill worker token is empty")
    controller = SkillController(
        Path(os.environ.get("HERMES_SKILLS_ROOT", "/var/lib/voiceos/hermes/skills")),
        Path(os.environ.get("VOICEOS_SKILL_CONTROL_ROOT", "/var/lib/voiceos/hermes-skill-control")),
        os.environ.get("VOICEOS_MEMORY_URL", "http://127.0.0.1:8790"),
    )
    def scheduled_scan() -> None:
        while True:
            time.sleep(5)
            try:
                controller.scan("scheduled-scan")
            except Exception:
                # A failed Rust import still leaves the changed file quarantined.
                continue

    threading.Thread(target=scheduled_scan, daemon=True).start()
    server = SkillWorkerServer((args.host, args.port), controller, token)
    server.serve_forever()


if __name__ == "__main__":
    main()
