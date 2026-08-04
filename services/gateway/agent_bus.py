"""Optional agent-to-agent event transport for VoiceOS.

Buzz is deliberately not used for ordinary private voice transcripts. It is an
explicit, asynchronous collaboration surface for agent messages and evidence.
"""

from __future__ import annotations

import json
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path


class AgentBusUnavailable(RuntimeError):
    pass


@dataclass(frozen=True)
class BuzzReceipt:
    event_id: str | None
    payload: dict[str, object]


class BuzzCliAgentBus:
    """Allowlisted JSON adapter around the upstream Rust ``buzz`` CLI."""

    def __init__(
        self,
        *,
        executable: str = "buzz",
        relay_url: str = "http://127.0.0.1:3000",
        private_key_file: str = "",
        timeout_seconds: int = 15,
    ) -> None:
        self.executable = executable
        self.relay_url = relay_url.rstrip("/")
        self.private_key_file = private_key_file
        self.timeout_seconds = timeout_seconds

    @classmethod
    def from_environment(cls) -> "BuzzCliAgentBus":
        return cls(
            executable=os.environ.get("VOICEOS_BUZZ_CLI", "buzz"),
            relay_url=os.environ.get("VOICEOS_BUZZ_RELAY_URL", "http://127.0.0.1:3000"),
            private_key_file=os.environ.get("VOICEOS_BUZZ_PRIVATE_KEY_FILE", ""),
        )

    @property
    def configured(self) -> bool:
        return bool(self.executable and self.relay_url and self.private_key_file)

    def publish(self, channel_id: str, content: str) -> BuzzReceipt:
        if not self.configured:
            raise AgentBusUnavailable("Buzz agent bus is not configured")
        safe_channel = channel_id.strip()
        safe_content = content.strip()
        if not safe_channel or len(safe_channel) > 128:
            raise ValueError("Buzz channel ID is empty or invalid")
        if not safe_content or len(safe_content) > 16_000:
            raise ValueError("Buzz message is empty or exceeds 16,000 characters")
        private_key = self._read_private_key()
        env = os.environ.copy()
        env["BUZZ_RELAY_URL"] = self.relay_url
        env["BUZZ_PRIVATE_KEY"] = private_key
        command = [
            self.executable,
            "--format",
            "json",
            "messages",
            "send",
            "--channel",
            safe_channel,
            "--content",
            safe_content,
        ]
        try:
            completed = subprocess.run(
                command,
                capture_output=True,
                check=False,
                env=env,
                shell=False,
                text=True,
                timeout=self.timeout_seconds,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise AgentBusUnavailable(f"Buzz CLI invocation failed: {error}") from error
        if completed.returncode != 0:
            detail = completed.stderr.strip()[:500] or f"exit code {completed.returncode}"
            raise AgentBusUnavailable(f"Buzz rejected the event: {detail}")
        try:
            payload = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise AgentBusUnavailable("Buzz returned malformed JSON") from error
        if not isinstance(payload, dict):
            raise AgentBusUnavailable("Buzz returned an unexpected response")
        event_id = payload.get("id")
        return BuzzReceipt(event_id if isinstance(event_id, str) else None, payload)

    def _read_private_key(self) -> str:
        try:
            key = Path(self.private_key_file).read_text(encoding="utf-8").strip()
        except OSError as error:
            raise AgentBusUnavailable(f"Buzz private key file is unavailable: {error}") from error
        if not key or len(key) > 512:
            raise AgentBusUnavailable("Buzz private key file is empty or invalid")
        return key
