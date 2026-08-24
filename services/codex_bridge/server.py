"""Unix-socket service that invokes Codex Sol with fixed read-only controls."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import socketserver
import subprocess
from typing import Any

from services.gateway.master_prompt import master_system_prompt

MAX_REQUEST_BYTES = 65_536
MAX_RESPONSE_BYTES = 262_144
_UnixStreamServerBase = getattr(socketserver, "UnixStreamServer", socketserver.TCPServer)


def run_codex(
    prompt: str,
    *,
    executable: str,
    model: str,
    reasoning_effort: str,
    workdir: str,
    timeout_seconds: int,
    tools: list[dict[str, object]] | None = None,
) -> str:
    """Run a single answer-only, ephemeral Codex turn with immutable permissions."""

    command = [
        executable,
        "exec",
        "--ephemeral",
        "--ignore-user-config",
        "--ignore-rules",
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
        "--disable",
        "shell_tool",
        "--disable",
        "unified_exec",
        "--disable",
        "multi_agent",
        "--disable",
        "apps",
        "--disable",
        "hooks",
        "-c",
        "web_search=disabled",
        "-m",
        model,
        "-c",
        f"model_reasoning_effort={reasoning_effort}",
        "-C",
        workdir,
        "-",
    ]
    instruction = (
        f"{master_system_prompt()}\n\n"
        "Provider role: You are the highest-confidence reasoning tier behind VoiceOS. "
        "Answer directly and concisely for spoken playback. Your built-in command, web-search, app, hook, "
        "and subagent tools are disabled. You may propose the typed VoiceOS tools supplied below. "
        "For opening a local browser, propose computer_run with /usr/bin/google-chrome-stable and the URL as argv. "
        "Do not claim that an external action occurred. "
        "If current system evidence "
        "would be required, say that VoiceOS must run its permissioned tools.\n\n"
        f"User request:\n{prompt}"
    )
    if tools:
        instruction += (
            "\n\nAvailable VoiceOS tools (proposal only):\n"
            + json.dumps(tools, separators=(",", ":"))
            + "\nIf an action is needed, return ONLY JSON in this form: "
            '{"text":"brief explanation","tool_calls":[{"function":{"name":"tool_name","arguments":{}}}]}. '
            "Propose at most one tool call. Never claim it ran; VoiceOS will request approval. "
            "Otherwise answer normally without JSON."
        )
    completed = subprocess.run(
        command,
        input=instruction,
        text=True,
        capture_output=True,
        check=False,
        timeout=timeout_seconds,
        env=_codex_environment(),
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip().splitlines()
        message = detail[-1] if detail else f"Codex exited with status {completed.returncode}"
        raise RuntimeError(message[:500])
    answer = completed.stdout.strip()
    if not answer:
        raise RuntimeError("Codex returned an empty final response")
    if len(answer.encode("utf-8")) > MAX_RESPONSE_BYTES:
        raise RuntimeError("Codex response exceeds the bridge limit")
    return answer


def _codex_environment() -> dict[str, str]:
    allowed = ("HOME", "PATH", "LANG", "LC_ALL", "SSL_CERT_FILE", "SSL_CERT_DIR")
    return {key: os.environ[key] for key in allowed if key in os.environ}


class CodexRequestHandler(socketserver.StreamRequestHandler):
    def handle(self) -> None:
        try:
            raw_request = self.rfile.readline(MAX_REQUEST_BYTES + 1)
            if not raw_request or len(raw_request) > MAX_REQUEST_BYTES:
                raise ValueError("request is empty or exceeds the bridge limit")
            request: Any = json.loads(raw_request)
            if not isinstance(request, dict) or set(request) != {"text", "tools"}:
                raise ValueError("request must contain text and tools")
            text = request["text"]
            tools = request["tools"]
            if not isinstance(text, str) or not text.strip():
                raise ValueError("text must be a non-empty string")
            if not isinstance(tools, list):
                raise ValueError("tools must be an array")
            answer = run_codex(
                text.strip(),
                executable=self.server.codex_executable,
                model=self.server.codex_model,
                reasoning_effort=self.server.reasoning_effort,
                workdir=self.server.codex_workdir,
                timeout_seconds=self.server.codex_timeout,
                tools=tools,
            )
            tool_calls: list[dict[str, object]] = []
            response_text = answer
            try:
                structured = json.loads(answer)
                if isinstance(structured, dict) and isinstance(structured.get("tool_calls"), list):
                    response_text = str(structured.get("text", "Action proposed for approval."))
                    tool_calls = structured["tool_calls"][:1]
            except json.JSONDecodeError:
                pass
            response = {"ok": True, "text": response_text, "tool_calls": tool_calls}
        except (ValueError, json.JSONDecodeError, RuntimeError, subprocess.TimeoutExpired) as error:
            response = {"ok": False, "error": str(error)[:500]}
        self.wfile.write(json.dumps(response, separators=(",", ":")).encode("utf-8"))


class CodexBridgeServer(_UnixStreamServerBase):
    codex_executable: str
    codex_model: str
    reasoning_effort: str
    codex_workdir: str
    codex_timeout: int


def main() -> None:
    if not hasattr(socketserver, "UnixStreamServer"):
        raise SystemExit("The Codex bridge requires Unix-domain socket support")
    parser = argparse.ArgumentParser(description="Run the VoiceOS Codex bridge")
    parser.add_argument("--socket", default="/run/voiceos-codex/codex.sock")
    parser.add_argument("--codex", default="/home/llm/.local/bin/codex")
    parser.add_argument("--model", default="gpt-5.6-sol")
    parser.add_argument("--reasoning-effort", default="high")
    parser.add_argument("--workdir", default="/var/lib/voiceos-codex/work")
    parser.add_argument("--timeout", type=int, default=330)
    args = parser.parse_args()

    socket_path = Path(args.socket)
    socket_path.parent.mkdir(parents=True, exist_ok=True)
    socket_path.unlink(missing_ok=True)
    Path(args.workdir).mkdir(parents=True, exist_ok=True)
    server = CodexBridgeServer(str(socket_path), CodexRequestHandler)
    server.codex_executable = args.codex
    server.codex_model = args.model
    server.reasoning_effort = args.reasoning_effort
    server.codex_workdir = args.workdir
    server.codex_timeout = args.timeout
    os.chmod(socket_path, 0o660)
    try:
        server.serve_forever()
    finally:
        server.server_close()
        socket_path.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
