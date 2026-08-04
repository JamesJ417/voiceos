from __future__ import annotations

import json
import socket


class GpuSchedulerError(RuntimeError):
    pass


class GpuSchedulerClient:
    def __init__(self, socket_path: str, *, timeout_seconds: int = 1220) -> None:
        self.socket_path = socket_path
        self.timeout_seconds = timeout_seconds

    def request(self, action: str, *, lease_id: str = "", ttl_seconds: int = 900) -> dict[str, object]:
        payload = {"action": action, "lease_id": lease_id, "ttl_seconds": ttl_seconds}
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                client.settimeout(self.timeout_seconds)
                client.connect(self.socket_path)
                client.sendall(json.dumps(payload, separators=(",", ":")).encode() + b"\n")
                response = b""
                while not response.endswith(b"\n") and len(response) <= 8192:
                    chunk = client.recv(8192)
                    if not chunk:
                        break
                    response += chunk
        except OSError as error:
            raise GpuSchedulerError(f"gpu_scheduler_unavailable: {error}") from error
        try:
            result = json.loads(response)
        except (json.JSONDecodeError, UnicodeDecodeError) as error:
            raise GpuSchedulerError("gpu_scheduler_returned_invalid_json") from error
        if not isinstance(result, dict):
            raise GpuSchedulerError("gpu_scheduler_returned_invalid_response")
        if not result.get("ok"):
            raise GpuSchedulerError(str(result.get("detail", "gpu_scheduler_rejected_request")))
        return result

    def acquire(self, lease_id: str, ttl_seconds: int) -> dict[str, object]:
        return self.request("acquire", lease_id=lease_id, ttl_seconds=ttl_seconds)

    def release(self, lease_id: str) -> dict[str, object]:
        return self.request("release", lease_id=lease_id)

    def renew(self, lease_id: str, ttl_seconds: int) -> dict[str, object]:
        return self.request("renew", lease_id=lease_id, ttl_seconds=ttl_seconds)

    def status(self) -> dict[str, object]:
        return self.request("status")
