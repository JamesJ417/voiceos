"""Provider-neutral reasoning interfaces for Omarchy Voice."""

from __future__ import annotations

import json
import os
import re
import socket
import threading
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Callable, Protocol
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from services.gateway.master_prompt import master_system_prompt

AF_UNIX = getattr(socket, "AF_UNIX", None)


@dataclass(frozen=True)
class ProviderToolCall:
    name: str
    arguments: dict[str, object]


@dataclass(frozen=True)
class ProviderApproval:
    request_id: str
    tool: str
    arguments: dict[str, object]
    provider_run_id: str
    evidence: dict[str, object] = field(default_factory=dict)


@dataclass(frozen=True)
class ProviderResponse:
    text: str
    provider: str
    input_tokens: int | None = None
    output_tokens: int | None = None
    cost_usd: float | None = None
    tool_calls: list[ProviderToolCall] = field(default_factory=list)
    approvals: list[ProviderApproval] = field(default_factory=list)
    events: list[dict[str, object]] = field(default_factory=list)


@dataclass(frozen=True)
class ProviderDescriptor:
    name: str
    kind: str
    configured: bool
    requires_credentials: bool
    role: str

    def as_dict(self) -> dict[str, object]:
        return asdict(self)


class ProviderUnavailable(RuntimeError):
    pass


class ReasoningProvider(Protocol):
    name: str

    def respond(
        self,
        text: str,
        tools: list[dict[str, object]] | None = None,
        context: str | None = None,
        conversation_id: str | None = None,
    ) -> ProviderResponse: ...


class MockProvider:
    name = "mock"

    def respond(
        self,
        text: str,
        tools: list[dict[str, object]] | None = None,
        context: str | None = None,
        conversation_id: str | None = None,
    ) -> ProviderResponse:
        del tools
        del context
        del conversation_id
        return ProviderResponse(
            text=f"I heard: {text}. The real model provider is not connected yet.",
            provider=self.name,
        )


class OllamaProvider:
    """Credential-free local provider using Ollama's native HTTP API."""

    def __init__(
        self,
        base_url: str,
        model: str,
        *,
        name: str = "ollama",
        think: bool = False,
        keep_alive: int | str | None = None,
        timeout_seconds: int = 120,
        temperature: float = 0.0,
        max_output_tokens: int = 384,
        context_window: int = 16_384,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.model = model.strip()
        self.name = name
        self.think = think
        self.keep_alive = keep_alive
        self.timeout_seconds = timeout_seconds
        self.temperature = temperature
        self.max_output_tokens = max(64, min(max_output_tokens, 4_096))
        self.context_window = max(2_048, min(context_window, 65_536))

    def respond(
        self,
        text: str,
        tools: list[dict[str, object]] | None = None,
        context: str | None = None,
        conversation_id: str | None = None,
    ) -> ProviderResponse:
        del conversation_id
        if not self.model:
            raise ProviderUnavailable("VOICEOS_OLLAMA_MODEL is not configured")
        request_body: dict[str, object] = {
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": (
                        master_system_prompt()
                        + "\n\nProvider role: Use typed tools when they can provide verified system evidence."
                        + (f"\n\nOmarchy Voice private continuity context:\n{context}" if context else "")
                    ),
                },
                {"role": "user", "content": text},
            ],
            "stream": False,
            "think": self.think,
            "options": {
                "temperature": self.temperature,
                "num_predict": self.max_output_tokens,
                "num_ctx": self.context_window,
            },
        }
        if self.keep_alive is not None:
            request_body["keep_alive"] = self.keep_alive
        if tools:
            request_body["tools"] = tools
        payload = json.dumps(request_body).encode("utf-8")
        request = Request(
            f"{self.base_url}/api/chat",
            data=payload,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urlopen(request, timeout=self.timeout_seconds) as response:
                result = json.loads(response.read())
        except (HTTPError, URLError, TimeoutError, json.JSONDecodeError) as error:
            raise ProviderUnavailable(f"Ollama request failed: {error}") from error
        message = result.get("message")
        if not isinstance(message, dict):
            raise ProviderUnavailable("Ollama returned no assistant message")
        answer = message.get("content")
        safe_answer = answer.strip() if isinstance(answer, str) else ""
        tool_calls = _parse_tool_calls(message.get("tool_calls"))
        if not safe_answer and not tool_calls:
            raise ProviderUnavailable("Ollama returned neither text nor tool calls")
        return ProviderResponse(
            text=safe_answer,
            provider=self.name,
            input_tokens=_optional_int(result.get("prompt_eval_count")),
            output_tokens=_optional_int(result.get("eval_count")),
            cost_usd=0.0,
            tool_calls=tool_calls,
        )


class CodexBridgeProvider:
    """Explicit cloud escalation through a local, read-only Codex bridge."""

    name = "codex-sol"

    def __init__(
        self,
        socket_path: str,
        *,
        enabled: bool = False,
        timeout_seconds: int = 360,
    ) -> None:
        self.socket_path = socket_path
        self.enabled = enabled
        self.timeout_seconds = timeout_seconds

    @property
    def configured(self) -> bool:
        return self.enabled and bool(self.socket_path)

    def respond(
        self,
        text: str,
        tools: list[dict[str, object]] | None = None,
        context: str | None = None,
        conversation_id: str | None = None,
    ) -> ProviderResponse:
        # Codex may propose typed gateway tools, but the bridge never executes them.
        del conversation_id
        if not self.configured:
            raise ProviderUnavailable("Codex Sol escalation is not enabled")
        if AF_UNIX is None:
            raise ProviderUnavailable("Codex Sol bridge requires Unix-domain sockets")
        prompt = f"{master_system_prompt()}\n\nUser request: {text}"
        if context:
            prompt = (
                f"{master_system_prompt()}\n\nOmarchy Voice private continuity context:\n"
                f"{context}\n\nUser request: {text}"
            )
        request = json.dumps({"text": prompt, "tools": tools or []}, separators=(",", ":")).encode("utf-8") + b"\n"
        if len(request) > 65_536:
            raise ProviderUnavailable("Codex Sol request exceeds the bridge limit")
        try:
            with socket.socket(AF_UNIX, socket.SOCK_STREAM) as connection:
                connection.settimeout(self.timeout_seconds)
                connection.connect(self.socket_path)
                connection.sendall(request)
                response_bytes = _read_bounded_socket(connection, 262_144)
            response = json.loads(response_bytes)
        except (OSError, TimeoutError, json.JSONDecodeError) as error:
            raise ProviderUnavailable(f"Codex Sol bridge request failed: {error}") from error
        if not isinstance(response, dict):
            raise ProviderUnavailable("Codex Sol bridge returned a malformed response")
        if not response.get("ok"):
            message = response.get("error")
            safe_message = message if isinstance(message, str) else "unknown bridge error"
            raise ProviderUnavailable(f"Codex Sol bridge rejected the request: {safe_message}")
        answer = response.get("text")
        if not isinstance(answer, str) or not answer.strip():
            raise ProviderUnavailable("Codex Sol bridge returned no answer")
        tool_calls = _parse_tool_calls(response.get("tool_calls"))
        return ProviderResponse(text=answer.strip(), provider=self.name, tool_calls=tool_calls)


class UnconfiguredCloudProvider:
    """A fail-closed slot for a future credentialed cloud adapter."""

    def __init__(self, name: str, credential_variable: str, model_variable: str) -> None:
        self.name = name
        self.credential_variable = credential_variable
        self.model_variable = model_variable

    @property
    def configured(self) -> bool:
        return bool(
            os.environ.get(self.credential_variable, "").strip()
            and os.environ.get(self.model_variable, "").strip()
        )

    def respond(
        self,
        text: str,
        tools: list[dict[str, object]] | None = None,
        context: str | None = None,
        conversation_id: str | None = None,
    ) -> ProviderResponse:
        del text
        del tools
        del context
        del conversation_id
        if not self.configured:
            raise ProviderUnavailable(
                f"{self.name} is disabled because credentials and a model are not configured"
            )
        raise ProviderUnavailable(
            f"{self.name} credentials are present, but its network adapter is intentionally disabled"
        )


class HermesProvider:
    """Hermes agent runtime behind Omarchy Voice-owned identity, memory, and policy."""

    name = "hermes"

    def __init__(
        self,
        base_url: str,
        *,
        api_key_file: str = "",
        api_key: str = "",
        model: str = "hermes",
        timeout_seconds: int = 360,
        use_async_runs: bool = True,
        skill_worker_url: str = "",
        skill_worker_token_file: str = "",
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key_file = api_key_file.strip()
        self._api_key = api_key.strip()
        self.model = model.strip() or "hermes"
        self.timeout_seconds = timeout_seconds
        self.use_async_runs = use_async_runs
        self.skill_worker_url = skill_worker_url.rstrip("/")
        self.skill_worker_token_file = skill_worker_token_file.strip()
        self.activity_sink: Callable[[str | None, dict[str, object]], None] | None = None
        self.completion_sink: Callable[[str, int, str], None] | None = None
        self._completion_watchers: set[str] = set()
        self._watchers_lock = threading.Lock()

    def set_activity_sink(
        self, sink: Callable[[str | None, dict[str, object]], None] | None
    ) -> None:
        self.activity_sink = sink

    def set_completion_sink(
        self, sink: Callable[[str, int, str], None] | None
    ) -> None:
        self.completion_sink = sink

    @property
    def configured(self) -> bool:
        return bool(self.base_url and (self._api_key or self.api_key_file))

    def respond(
        self,
        text: str,
        tools: list[dict[str, object]] | None = None,
        context: str | None = None,
        conversation_id: str | None = None,
    ) -> ProviderResponse:
        # Hermes owns its skill/tool surface. Omarchy Voice deterministic commands run
        # before this provider, and privileged actions remain approval-gated.
        del tools
        api_key = self._read_api_key()
        system_prompt = (
            master_system_prompt()
            + "\n\nRuntime role: You are VIC, the Voice Interface Controller and core agent inside Omarchy Voice. Hermes is your runtime, not your public name. "
            "For agentic requests, use installed skills when their trigger matches and create reusable skills only when appropriate. "
            "For ordinary conversation explicitly routed to Hermes, answer directly without inspecting or creating skills. "
            "For substantial research or multi-step work that does not need the user's immediate input, use delegate_task with background=true. "
            "After dispatching, immediately tell the user what the worker is doing and keep the foreground conversation available. "
            "Never use a vague holding phrase such as 'one moment' or 'I'm working on that'; report a concrete goal or a real result instead. "
            "Do not claim that a host mutation succeeded without verified evidence. "
            "Omarchy Voice remains the authority for device identity, approvals, and its canonical memory."
        )
        if context:
            system_prompt += f"\n\nOmarchy Voice private continuity context:\n{context}"
        if self.use_async_runs:
            return self._respond_async(text, system_prompt, conversation_id, api_key)
        request_body = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": text},
            ],
            "stream": False,
        }
        headers = {
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        }
        if conversation_id:
            headers["X-Hermes-Session-Key"] = f"voiceos:{conversation_id}"
        request = Request(
            f"{self.base_url}/v1/chat/completions",
            data=json.dumps(request_body).encode("utf-8"),
            headers=headers,
            method="POST",
        )
        try:
            with urlopen(request, timeout=self.timeout_seconds) as response:
                result = json.loads(response.read())
        except (HTTPError, URLError, TimeoutError, json.JSONDecodeError) as error:
            raise ProviderUnavailable(f"Hermes request failed: {error}") from error
        choices = result.get("choices") if isinstance(result, dict) else None
        if not isinstance(choices, list) or not choices or not isinstance(choices[0], dict):
            raise ProviderUnavailable("Hermes returned no completion choice")
        message = choices[0].get("message")
        answer = message.get("content") if isinstance(message, dict) else None
        if not isinstance(answer, str) or not answer.strip():
            raise ProviderUnavailable("Hermes returned no assistant message")
        usage = result.get("usage") if isinstance(result.get("usage"), dict) else {}
        return ProviderResponse(
            text=answer.strip(),
            provider=self.name,
            input_tokens=_optional_int(usage.get("prompt_tokens")),
            output_tokens=_optional_int(usage.get("completion_tokens")),
            cost_usd=0.0,
        )

    def complete_approval(self, run_id: str, approve: bool) -> ProviderResponse:
        if not re.fullmatch(r"run_[0-9a-f]{32}", run_id):
            raise ProviderUnavailable("Hermes run ID is invalid")
        api_key = self._read_api_key()
        self._request_json(
            f"/v1/runs/{run_id}/approval",
            api_key,
            method="POST",
            body={"choice": "once" if approve else "deny"},
        )
        return self._poll_run(run_id, api_key)

    def _respond_async(
        self,
        text: str,
        system_prompt: str,
        conversation_id: str | None,
        api_key: str,
    ) -> ProviderResponse:
        body: dict[str, object] = {
            "input": text,
            "instructions": system_prompt,
            "model_options": {},
        }
        agent_runtime = _should_use_agent_runtime(text)
        body["model_options"] = {"reasoning_effort": "medium" if agent_runtime else "low"}
        if self.model.casefold() == "hermes" and not agent_runtime:
            body["model"] = os.environ.get("VOICEOS_FAST_CHAT_MODEL", "gpt-5.6-luna")
        # "hermes" is our provider slot name, not necessarily an Ollama model.
        # Omitting that sentinel lets Hermes use its own configured default.
        if self.model.casefold() != "hermes":
            body["model"] = self.model
        if conversation_id:
            body["session_id"] = conversation_id
        baseline_message_id = self._latest_message_id(conversation_id, api_key)
        result = self._request_json(
            "/v1/runs",
            api_key,
            method="POST",
            body=body,
            conversation_id=conversation_id,
        )
        run_id = result.get("run_id")
        if not isinstance(run_id, str) or not re.fullmatch(r"run_[0-9a-f]{32}", run_id):
            raise ProviderUnavailable("Hermes returned an invalid run ID")
        events: list[dict[str, object]] = []
        draft_preview = ""
        last_draft_emit = 0.0
        request = Request(
            f"{self.base_url}/v1/runs/{run_id}/events",
            headers={"Authorization": f"Bearer {api_key}", "Accept": "text/event-stream"},
            method="GET",
        )
        try:
            with urlopen(request, timeout=self.timeout_seconds) as response:
                for raw_line in response:
                    line = raw_line.decode("utf-8", errors="replace").strip()
                    if not line.startswith("data:"):
                        continue
                    event = json.loads(line[5:].strip())
                    if not isinstance(event, dict):
                        continue
                    safe_event = _bounded_event(event)
                    events.append(safe_event)
                    event_name = safe_event.get("event", safe_event.get("type"))
                    if event_name == "message.delta" and self.activity_sink:
                        delta = safe_event.get("delta")
                        if isinstance(delta, str):
                            draft_preview = (draft_preview + delta)[-500:]
                            now = time.monotonic()
                            if now - last_draft_emit >= 0.45:
                                self.activity_sink(
                                    conversation_id,
                                    {"event": "response.drafting", "summary": draft_preview.strip()},
                                )
                                last_draft_emit = now
                    elif event_name == "reasoning.available" and self.activity_sink and draft_preview.strip():
                        self.activity_sink(
                            conversation_id,
                            {"event": "response.drafting", "summary": draft_preview.strip()},
                        )
                    if self.activity_sink and event_name in {
                        "reasoning.available", "tool.started", "tool.completed",
                        "subagent.start", "subagent.complete",
                    }:
                        self.activity_sink(conversation_id, safe_event)
                    if event_name == "subagent.start" and conversation_id:
                        self._start_completion_watcher(
                            conversation_id, api_key, baseline_message_id
                        )
                    if event_name == "approval.request":
                        self._notify_skill_scan(run_id)
                        command = str(safe_event.get("command", "")).strip()
                        description = str(safe_event.get("description", "")).strip()
                        pattern = str(safe_event.get("pattern_key", "hermes.command")).strip()
                        return ProviderResponse(
                            text=(
                                f"VIC wants to run {description or pattern}. "
                                "Your approval is required before anything executes."
                            ),
                            provider=self.name,
                            approvals=[
                                ProviderApproval(
                                    request_id=f"hermes:{run_id}",
                                    tool=f"hermes.{pattern}"[:160],
                                    arguments={
                                        "command": command[:4_000],
                                        "description": description[:1_000],
                                    },
                                    provider_run_id=run_id,
                                    evidence={
                                        "source": "hermes-sse",
                                        "choices": safe_event.get("choices", []),
                                    },
                                )
                            ],
                            events=events[-50:],
                        )
                    if event_name == "run.completed":
                        self._notify_skill_scan(run_id)
                        return _provider_response_from_run_event(safe_event, events)
                    if event_name in {"run.failed", "run.cancelled"}:
                        raise ProviderUnavailable(
                            f"Hermes run {event_name.split('.')[-1]}: "
                            f"{str(safe_event.get('error', 'no result'))[:500]}"
                        )
        except (HTTPError, URLError, TimeoutError, OSError, json.JSONDecodeError) as error:
            # Polling is the recovery path when an intermediary interrupts SSE.
            try:
                return self._poll_run(run_id, api_key)
            except ProviderUnavailable:
                raise ProviderUnavailable(f"Hermes event stream failed: {error}") from error
        return self._poll_run(run_id, api_key)

    def _latest_message_id(self, session_id: str | None, api_key: str) -> int:
        if not session_id:
            return 0
        try:
            result = self._request_json(
                f"/api/sessions/{session_id}/messages?order=latest&limit=1", api_key
            )
        except ProviderUnavailable:
            return 0
        messages = result.get("data") if isinstance(result, dict) else None
        if not isinstance(messages, list) or not messages:
            return 0
        return max((_optional_int(item.get("id")) or 0 for item in messages if isinstance(item, dict)), default=0)

    def _start_completion_watcher(
        self, session_id: str, api_key: str, after_message_id: int
    ) -> None:
        with self._watchers_lock:
            if session_id in self._completion_watchers:
                return
            self._completion_watchers.add(session_id)
        threading.Thread(
            target=self._watch_completions,
            args=(session_id, api_key, after_message_id),
            name=f"hermes-completions-{session_id[:8]}",
            daemon=True,
        ).start()

    def _watch_completions(
        self, session_id: str, api_key: str, cursor: int
    ) -> None:
        idle_polls = 0
        try:
            while idle_polls < 360:
                try:
                    result = self._request_json(
                        f"/api/sessions/{session_id}/messages?order=latest&limit=100",
                        api_key,
                    )
                    messages = result.get("data") if isinstance(result, dict) else []
                    newest = cursor
                    found = False
                    for message in messages if isinstance(messages, list) else []:
                        if not isinstance(message, dict):
                            continue
                        message_id = _optional_int(message.get("id")) or 0
                        newest = max(newest, message_id)
                        content = message.get("content")
                        if (
                            message_id > cursor
                            and message.get("role") == "user"
                            and isinstance(content, str)
                            and re.match(r"^\[ASYNC DELEGATION .+ COMPLETE", content.strip())
                        ):
                            found = True
                            if self.completion_sink:
                                self.completion_sink(session_id, message_id, content.strip())
                    cursor = newest
                    idle_polls = 0 if found else idle_polls + 1
                except ProviderUnavailable:
                    idle_polls += 1
                time.sleep(5)
        finally:
            with self._watchers_lock:
                self._completion_watchers.discard(session_id)

    def _poll_run(self, run_id: str, api_key: str) -> ProviderResponse:
        import time

        deadline = time.monotonic() + self.timeout_seconds
        while time.monotonic() < deadline:
            status = self._request_json(f"/v1/runs/{run_id}", api_key)
            state = status.get("status")
            if state == "completed":
                self._notify_skill_scan(run_id)
                usage = status.get("usage") if isinstance(status.get("usage"), dict) else {}
                output = status.get("output")
                if not isinstance(output, str) or not output.strip():
                    raise ProviderUnavailable("Hermes completed without output")
                return ProviderResponse(
                    text=output.strip(),
                    provider=self.name,
                    input_tokens=_optional_int(
                        usage.get("input_tokens", usage.get("prompt_tokens"))
                    ),
                    output_tokens=_optional_int(
                        usage.get("output_tokens", usage.get("completion_tokens"))
                    ),
                    cost_usd=0.0,
                    events=[{"event": "run.completed", "run_id": run_id}],
                )
            if state in {"failed", "cancelled"}:
                self._notify_skill_scan(run_id)
                raise ProviderUnavailable(
                    f"Hermes run {state}: {str(status.get('error', 'no result'))[:500]}"
                )
            if state == "waiting_for_approval":
                raise ProviderUnavailable("Hermes is waiting for an approval event")
            time.sleep(0.2)
        raise ProviderUnavailable("Hermes run timed out")

    def _request_json(
        self,
        path: str,
        api_key: str,
        *,
        method: str = "GET",
        body: dict[str, object] | None = None,
        conversation_id: str | None = None,
    ) -> dict[str, object]:
        headers = {"Authorization": f"Bearer {api_key}", "Accept": "application/json"}
        data = None
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            headers["Content-Type"] = "application/json"
        if conversation_id:
            headers["X-Hermes-Session-Key"] = f"voiceos:{conversation_id}"
        request = Request(f"{self.base_url}{path}", data=data, headers=headers, method=method)
        try:
            with urlopen(request, timeout=self.timeout_seconds) as response:
                result = json.loads(response.read())
        except (HTTPError, URLError, TimeoutError, OSError, json.JSONDecodeError) as error:
            raise ProviderUnavailable(f"Hermes request failed: {error}") from error
        if not isinstance(result, dict):
            raise ProviderUnavailable("Hermes returned malformed JSON")
        return result

    def _notify_skill_scan(self, run_id: str) -> None:
        if not self.skill_worker_url or not self.skill_worker_token_file:
            return
        try:
            token = Path(self.skill_worker_token_file).read_text(encoding="utf-8").strip()
            request = Request(
                f"{self.skill_worker_url}/v1/scan",
                data=json.dumps({"run_id": run_id}).encode("utf-8"),
                headers={
                    "Authorization": f"Bearer {token}",
                    "Content-Type": "application/json",
                },
                method="POST",
            )
            with urlopen(request, timeout=10) as response:
                response.read()
        except (OSError, HTTPError, URLError, TimeoutError):
            # Skill quarantine is independently scheduled by the worker; a
            # notification failure must not discard an otherwise valid answer.
            return

    def _read_api_key(self) -> str:
        if self._api_key:
            return self._api_key
        if not self.api_key_file:
            raise ProviderUnavailable("VOICEOS_HERMES_API_KEY_FILE is not configured")
        try:
            key = Path(self.api_key_file).read_text(encoding="utf-8").strip()
        except OSError as error:
            raise ProviderUnavailable(f"Hermes API key file is unavailable: {error}") from error
        if not key or len(key) > 4096:
            raise ProviderUnavailable("Hermes API key file is empty or invalid")
        return key


class ProviderRouter:
    """Selects interchangeable providers without coupling tools to a model vendor."""

    def __init__(self, default_provider: ReasoningProvider | None = None) -> None:
        ollama_url = os.environ.get("VOICEOS_OLLAMA_URL", "http://127.0.0.1:11434")
        self._ollama = OllamaProvider(
            ollama_url,
            os.environ.get("VOICEOS_OLLAMA_MODEL", ""),
            think=False,
            keep_alive=-1,
        )
        self._ollama_deep = OllamaProvider(
            ollama_url,
            os.environ.get("VOICEOS_OLLAMA_DEEP_MODEL", ""),
            name="ollama-deep",
            think=True,
            keep_alive="5m",
            timeout_seconds=300,
            temperature=0.2,
            max_output_tokens=1_024,
            context_window=32_768,
        )
        self._codex = CodexBridgeProvider(
            os.environ.get("VOICEOS_CODEX_SOCKET", "/run/voiceos-codex/codex.sock"),
            enabled=os.environ.get("VOICEOS_CODEX_ENABLED", "0").strip() == "1",
        )
        self._hermes = HermesProvider(
            os.environ.get("VOICEOS_HERMES_URL", "http://127.0.0.1:8642"),
            api_key_file=os.environ.get("VOICEOS_HERMES_API_KEY_FILE", ""),
            api_key=os.environ.get("VOICEOS_HERMES_API_KEY", ""),
            model=os.environ.get("VOICEOS_HERMES_MODEL", "hermes"),
            use_async_runs=os.environ.get("VOICEOS_HERMES_ASYNC_RUNS", "1").strip() == "1",
            skill_worker_url=os.environ.get("VOICEOS_HERMES_SKILL_WORKER_URL", ""),
            skill_worker_token_file=os.environ.get("VOICEOS_HERMES_SKILL_WORKER_TOKEN_FILE", ""),
        )
        self._openai = UnconfiguredCloudProvider(
            "openai", "OPENAI_API_KEY", "VOICEOS_OPENAI_MODEL"
        )
        self._claude = UnconfiguredCloudProvider(
            "claude-review", "ANTHROPIC_API_KEY", "VOICEOS_CLAUDE_MODEL"
        )
        self._providers: dict[str, ReasoningProvider] = {
            "mock": MockProvider(),
            "ollama": self._ollama,
            "ollama-deep": self._ollama_deep,
            "codex-sol": self._codex,
            "hermes": self._hermes,
            "openai": self._openai,
            "claude-review": self._claude,
        }
        self.default_provider = default_provider or self._provider_from_environment()

    @property
    def default_name(self) -> str:
        return self.default_provider.name

    def set_activity_sink(
        self, sink: Callable[[str | None, dict[str, object]], None] | None
    ) -> None:
        self._hermes.set_activity_sink(sink)

    def set_completion_sink(
        self, sink: Callable[[str, int, str], None] | None
    ) -> None:
        self._hermes.set_completion_sink(sink)

    def respond(
        self,
        text: str,
        provider: str | None = None,
        tools: list[dict[str, object]] | None = None,
        context: str | None = None,
        conversation_id: str | None = None,
    ) -> ProviderResponse:
        selected = self.default_provider if provider is None else self._providers.get(provider)
        if provider is None:
            if self._codex.configured and _should_use_codex(text):
                selected = self._codex
            elif _explicitly_requests_hermes(text) and self._hermes.configured:
                selected = self._hermes
            elif self._ollama_deep.model and _should_use_deep_reasoning(text):
                selected = self._ollama_deep
            elif (
                selected is self._hermes
                and self._ollama.model
                and not _should_use_agent_runtime(text)
            ):
                # Ordinary conversation still receives canonical Omarchy Voice memory,
                # but it does not pay the latency cost of a full Hermes agent run.
                selected = self._ollama
        if selected is None:
            raise ProviderUnavailable(f"Unknown provider: {provider}")
        return selected.respond(text, tools, context, conversation_id)

    def describe(self) -> list[dict[str, object]]:
        return [
            ProviderDescriptor("mock", "built-in", True, False, "fallback").as_dict(),
            ProviderDescriptor(
                "ollama", "local", bool(self._ollama.model), False, "primary"
            ).as_dict(),
            ProviderDescriptor(
                "ollama-deep",
                "local",
                bool(self._ollama_deep.model),
                False,
                "complex-escalation",
            ).as_dict(),
            ProviderDescriptor(
                "hermes",
                "local-agent-runtime",
                self._hermes.configured,
                True,
                "core-agent",
            ).as_dict(),
            ProviderDescriptor(
                "codex-sol",
                "cloud-via-codex-cli",
                self._codex.configured,
                True,
                "explicit-highest-confidence",
            ).as_dict(),
            ProviderDescriptor(
                "openai", "cloud", self._openai.configured, True, "optional-escalation"
            ).as_dict(),
            ProviderDescriptor(
                "claude-review", "cloud", self._claude.configured, True, "optional-review"
            ).as_dict(),
            ProviderDescriptor(
                "deterministic", "tools", True, False, "verified-commands"
            ).as_dict(),
        ]

    def complete_provider_approval(self, provider: str, run_id: str, approve: bool) -> ProviderResponse:
        if provider != "hermes":
            raise ProviderUnavailable(f"Provider approval is unsupported for {provider}")
        return self._hermes.complete_approval(run_id, approve)

    def _provider_from_environment(self) -> ReasoningProvider:
        configured = os.environ.get("VOICEOS_PROVIDER", "mock").strip().casefold()
        provider = self._providers.get(configured)
        if provider is None:
            raise RuntimeError(
                f"VOICEOS_PROVIDER={configured!r} is unknown. "
                "Supported values: mock, ollama, ollama-deep, hermes, codex-sol, "
                "openai, claude-review."
            )
        return provider


def _should_use_codex(text: str) -> bool:
    """Escalate to subscription-backed Sol only when the speaker asks for it."""

    normalized = " ".join(text.casefold().split())
    return any(
        phrase in normalized
        for phrase in (
            "ask codex",
            "use codex",
            "use sol",
            "ask sol",
            "codex review",
            "highest confidence",
            "final verification",
        )
    )


def _explicitly_requests_hermes(text: str) -> bool:
    normalized = " ".join(text.casefold().split())
    return any(
        phrase in normalized
        for phrase in (
            "ask hermes",
            "use hermes",
            "hermes agent",
            "agent mode",
        )
    )


def _should_use_agent_runtime(text: str) -> bool:
    """Reserve Hermes for requests that can require skills, tools, or external state."""

    normalized = " ".join(text.casefold().split())
    if _explicitly_requests_hermes(normalized):
        return True
    if any(
        phrase in normalized
        for phrase in (
            "go ahead and get started",
            "go ahead and do that",
            "let's get started",
            "let’s get started",
            "get to work on",
            "make that happen",
            "create a skill",
            "update a skill",
            "change a skill",
            "use a skill",
            "skill proposal",
            "root broker",
            "root access",
            "sudo ",
            "systemctl",
        )
    ):
        return True

    action = (
        r"install|deploy|restart|reboot|start|stop|enable|disable|configure|"
        r"implement|build|fix|patch|edit|change|update|delete|remove|move|copy|"
        r"run|execute|test|check|inspect|verify|diagnose|crawl|browse|research|"
        r"search|send|create|schedule|"
        r"approve|reject"
    )
    prefixes = (
        r"(?:please\s+)?",
        r"(?:please\s+)?go ahead and\s+",
        r"(?:can|could|would|will) you\s+",
        r"i want you to\s+",
        r"let(?:'|’)s\s+",
        r"vic[, ]+",
    )
    return any(re.match(rf"^{prefix}(?:{action})\b", normalized) for prefix in prefixes)


def _should_use_deep_reasoning(text: str) -> bool:
    """Use deterministic, auditable rules for local deep-model escalation."""

    normalized = " ".join(text.casefold().split())
    explicit_phrases = (
        "think deeply",
        "deep reasoning",
        "deep analysis",
        "analyze carefully",
        "reason step by step",
        "use gpt oss",
        "use gpt-oss",
    )
    if any(phrase in normalized for phrase in explicit_phrases):
        return True
    complex_phrases = (
        "root cause",
        "architecture review",
        "security review",
        "threat model",
        "compare the tradeoffs",
        "compare the trade-offs",
    )
    if any(phrase in normalized for phrase in complex_phrases):
        return True
    numbers = re.findall(r"\d+(?:\.\d+)?", normalized)
    number_words = {
        "zero", "one", "two", "three", "four", "five", "six", "seven",
        "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen",
        "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty",
        "hundred", "thousand", "million",
    }
    spoken_numbers = sum(
        token in number_words for token in re.findall(r"[a-z]+", normalized)
    )
    numerical_reasoning_terms = (
        " each ",
        " total ",
        " remaining",
        " percent",
        " percentage",
        " combined",
        " altogether",
        " average",
        " probability",
    )
    padded = f" {normalized} "
    if len(numbers) + spoken_numbers >= 2 and any(
        term in padded for term in numerical_reasoning_terms
    ):
        return True
    return len(normalized) >= 600


def _optional_int(value: object) -> int | None:
    return value if isinstance(value, int) else None


def _bounded_event(event: dict[str, object]) -> dict[str, object]:
    allowed = {
        "event", "type", "run_id", "timestamp", "tool", "preview", "duration", "error",
        "command", "description", "pattern_key", "pattern_keys", "choices", "delta",
        "output", "usage", "status", "summary", "subagent_id",
    }
    bounded: dict[str, object] = {}
    for key, value in event.items():
        if key not in allowed:
            continue
        if isinstance(value, str):
            bounded[key] = value[:16_000]
        elif isinstance(value, (int, float, bool, list, dict)) or value is None:
            bounded[key] = value
    return bounded


def _provider_response_from_run_event(
    event: dict[str, object], events: list[dict[str, object]]
) -> ProviderResponse:
    output = event.get("output")
    if not isinstance(output, str) or not output.strip():
        raise ProviderUnavailable("Hermes completed without output")
    usage = event.get("usage") if isinstance(event.get("usage"), dict) else {}
    return ProviderResponse(
        text=output.strip(),
        provider="hermes",
        input_tokens=_optional_int(usage.get("input_tokens", usage.get("prompt_tokens"))),
        output_tokens=_optional_int(usage.get("output_tokens", usage.get("completion_tokens"))),
        cost_usd=0.0,
        events=events[-50:],
    )


def _read_bounded_socket(connection: socket.socket, limit: int) -> bytes:
    chunks: list[bytes] = []
    total = 0
    while True:
        chunk = connection.recv(min(65_536, limit + 1 - total))
        if not chunk:
            break
        chunks.append(chunk)
        total += len(chunk)
        if total > limit:
            raise OSError("bridge response exceeds the size limit")
    if not chunks:
        raise OSError("bridge closed without a response")
    return b"".join(chunks)


def _parse_tool_calls(value: object) -> list[ProviderToolCall]:
    if value is None:
        return []
    if not isinstance(value, list):
        raise ProviderUnavailable("Ollama tool_calls must be an array")
    if len(value) > 4:
        raise ProviderUnavailable("Ollama proposed too many tool calls")
    parsed: list[ProviderToolCall] = []
    for item in value:
        if not isinstance(item, dict):
            raise ProviderUnavailable("Ollama returned a malformed tool call")
        function = item.get("function")
        if not isinstance(function, dict):
            raise ProviderUnavailable("Ollama tool call is missing function data")
        name = function.get("name")
        arguments: Any = function.get("arguments", {})
        if not isinstance(name, str) or not name.strip() or not isinstance(arguments, dict):
            raise ProviderUnavailable("Ollama returned invalid tool-call arguments")
        parsed.append(ProviderToolCall(name.strip(), arguments))
    return parsed
