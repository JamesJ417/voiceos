"""Approval-bound root command broker with signed, single-use requests."""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import os
import socket
import socketserver
import sqlite3
import subprocess
import time
import uuid
from pathlib import Path
from typing import Any

MAX_REQUEST_BYTES = 128 * 1024
MAX_OUTPUT_BYTES = 64 * 1024


class BrokerRejected(ValueError):
    pass


def canonical_request(payload: dict[str, Any]) -> bytes:
    unsigned = {key: value for key, value in payload.items() if key != "signature"}
    return json.dumps(unsigned, separators=(",", ":"), sort_keys=True).encode("utf-8")


def sign_request(payload: dict[str, Any], key: bytes) -> str:
    return hmac.new(key, canonical_request(payload), hashlib.sha256).hexdigest()


class RootBroker:
    def __init__(self, key: bytes, state_path: Path) -> None:
        if len(key) < 32:
            raise ValueError("root broker key must contain at least 32 bytes")
        state_path.parent.mkdir(parents=True, exist_ok=True)
        self.key = key
        self.connection = sqlite3.connect(state_path, check_same_thread=False)
        with self.connection:
            self.connection.execute(
                """
                CREATE TABLE IF NOT EXISTS consumed_requests (
                    nonce TEXT PRIMARY KEY,
                    request_id TEXT NOT NULL,
                    operation TEXT NOT NULL,
                    consumed_at INTEGER NOT NULL,
                    result_sha256 TEXT
                )
                """
            )

    def execute(self, payload: dict[str, Any]) -> dict[str, Any]:
        request_id, nonce, operation, arguments = self._validate(payload)
        with self.connection:
            try:
                self.connection.execute(
                    "INSERT INTO consumed_requests(nonce, request_id, operation, consumed_at) VALUES (?, ?, ?, ?)",
                    (nonce, request_id, operation, int(time.time())),
                )
            except sqlite3.IntegrityError as error:
                raise BrokerRejected("request_nonce_already_consumed") from error
        result = self._execute_command(arguments)
        digest = hashlib.sha256(
            json.dumps(result, separators=(",", ":"), sort_keys=True).encode("utf-8")
        ).hexdigest()
        with self.connection:
            self.connection.execute(
                "UPDATE consumed_requests SET result_sha256=? WHERE nonce=?", (digest, nonce)
            )
        return {**result, "request_id": request_id, "nonce": nonce, "result_sha256": digest}

    def _validate(self, payload: dict[str, Any]) -> tuple[str, str, str, dict[str, Any]]:
        signature = payload.get("signature")
        if not isinstance(signature, str) or not hmac.compare_digest(
            signature, sign_request(payload, self.key)
        ):
            raise BrokerRejected("invalid_request_signature")
        request_id = str(payload.get("request_id", ""))
        nonce = str(payload.get("nonce", ""))
        try:
            uuid.UUID(request_id)
            uuid.UUID(nonce)
        except ValueError as error:
            raise BrokerRejected("valid_request_and_nonce_required") from error
        expires = payload.get("expires_at_unix")
        if not isinstance(expires, int) or expires < int(time.time()):
            raise BrokerRejected("request_expired")
        if expires > int(time.time()) + 120:
            raise BrokerRejected("request_expiry_too_distant")
        operation = str(payload.get("operation", ""))
        if operation != "command.exec":
            raise BrokerRejected("operation_not_supported")
        arguments = payload.get("arguments")
        if not isinstance(arguments, dict):
            raise BrokerRejected("arguments_object_required")
        return request_id, nonce, operation, arguments

    @staticmethod
    def _execute_command(arguments: dict[str, Any]) -> dict[str, Any]:
        if set(arguments) - {"argv", "cwd", "timeout_seconds", "rollback"}:
            raise BrokerRejected("arguments_not_allowed")
        argv = arguments.get("argv")
        if (
            not isinstance(argv, list)
            or not argv
            or len(argv) > 128
            or any(not isinstance(item, str) or not item or len(item) > 4096 for item in argv)
        ):
            raise BrokerRejected("valid_argv_required")
        executable = Path(argv[0])
        if not executable.is_absolute():
            raise BrokerRejected("absolute_executable_required")
        cwd_value = arguments.get("cwd", "/")
        if not isinstance(cwd_value, str) or not Path(cwd_value).is_absolute():
            raise BrokerRejected("absolute_cwd_required")
        timeout = arguments.get("timeout_seconds", 60)
        if not isinstance(timeout, int) or not 1 <= timeout <= 300:
            raise BrokerRejected("timeout_out_of_range")
        rollback = arguments.get("rollback")
        if not isinstance(rollback, str) or not rollback.strip() or len(rollback) > 4_000:
            raise BrokerRejected("rollback_information_required")
        completed = subprocess.run(
            argv,
            cwd=cwd_value,
            env={"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LANG": "C.UTF-8"},
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
        stdout = completed.stdout[-MAX_OUTPUT_BYTES:]
        stderr = completed.stderr[-MAX_OUTPUT_BYTES:]
        return {
            "operation": "command.exec",
            "argv": argv,
            "cwd": cwd_value,
            "exit_code": completed.returncode,
            "stdout": stdout,
            "stderr": stderr,
            "completed": completed.returncode == 0,
            "truncated": len(completed.stdout) > MAX_OUTPUT_BYTES or len(completed.stderr) > MAX_OUTPUT_BYTES,
            "rollback": rollback,
        }


class RootBrokerClient:
    def __init__(self, socket_path: Path, key_path: Path) -> None:
        self.socket_path = socket_path
        self.key_path = key_path

    def execute(self, request_id: str, arguments: dict[str, Any]) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "request_id": request_id,
            "nonce": str(uuid.uuid4()),
            "expires_at_unix": int(time.time()) + 60,
            "operation": "command.exec",
            "arguments": arguments,
        }
        payload["signature"] = sign_request(payload, self.key_path.read_bytes())
        encoded = json.dumps(payload, separators=(",", ":")).encode("utf-8") + b"\n"
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(310)
            connection.connect(str(self.socket_path))
            connection.sendall(encoded)
            response = b""
            while not response.endswith(b"\n"):
                chunk = connection.recv(65_536)
                if not chunk:
                    break
                response += chunk
                if len(response) > MAX_OUTPUT_BYTES * 3:
                    raise RuntimeError("root_broker_response_too_large")
        result = json.loads(response)
        if not isinstance(result, dict):
            raise RuntimeError("root_broker_invalid_response")
        if result.get("ok") is not True:
            raise RuntimeError(str(result.get("error", "root_broker_rejected")))
        return dict(result["result"])


class BrokerHandler(socketserver.StreamRequestHandler):
    def handle(self) -> None:
        raw = self.rfile.readline(MAX_REQUEST_BYTES + 1)
        if len(raw) > MAX_REQUEST_BYTES:
            response = {"ok": False, "error": "request_too_large"}
        else:
            try:
                payload = json.loads(raw)
                if not isinstance(payload, dict):
                    raise BrokerRejected("request_object_required")
                response = {"ok": True, "result": self.server.broker.execute(payload)}  # type: ignore[attr-defined]
            except (BrokerRejected, OSError, ValueError, subprocess.SubprocessError) as error:
                response = {"ok": False, "error": str(error)}
        self.wfile.write(json.dumps(response, separators=(",", ":")).encode("utf-8") + b"\n")


if hasattr(socketserver, "ThreadingUnixStreamServer"):
    class BrokerServer(socketserver.ThreadingUnixStreamServer):  # type: ignore[attr-defined]
        daemon_threads = True

        def __init__(self, socket_path: str, broker: RootBroker) -> None:
            Path(socket_path).unlink(missing_ok=True)
            self.broker = broker
            super().__init__(socket_path, BrokerHandler)
            os.chmod(socket_path, 0o660)
else:
    class BrokerServer:
        def __init__(self, socket_path: str, broker: RootBroker) -> None:
            raise OSError("unix_domain_sockets_not_supported")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--socket", required=True)
    parser.add_argument("--key-file", required=True)
    parser.add_argument("--state", required=True)
    args = parser.parse_args()
    broker = RootBroker(Path(args.key_file).read_bytes(), Path(args.state))
    with BrokerServer(args.socket, broker) as server:
        server.serve_forever()


if __name__ == "__main__":
    main()
