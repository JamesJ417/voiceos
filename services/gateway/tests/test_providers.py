from __future__ import annotations

import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest.mock import MagicMock, patch

from services.gateway.providers import (
    CodexBridgeProvider,
    HermesProvider,
    OllamaProvider,
    ProviderResponse,
    ProviderRouter,
    _should_use_deep_reasoning,
    _should_use_codex,
)


class FakeOllamaHandler(BaseHTTPRequestHandler):
    request_payload: dict[str, object] | None = None

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers["Content-Length"])
        type(self).request_payload = json.loads(self.rfile.read(length))
        body = json.dumps(
            {
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "type": "function",
                            "function": {"name": "disk_space", "arguments": {}},
                        }
                    ],
                },
                "prompt_eval_count": 21,
                "eval_count": 7,
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, message_format: str, *args: object) -> None:
        del message_format, args


class OllamaProviderTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), FakeOllamaHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)

    def test_chat_tool_call_contract(self) -> None:
        provider = OllamaProvider(
            f"http://127.0.0.1:{self.server.server_port}", "test-model"
        )
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "disk.space",
                    "description": "Check disk space",
                    "parameters": {"type": "object", "properties": {}},
                },
            }
        ]
        response = provider.respond("How much storage is available?", tools)
        self.assertEqual("ollama", response.provider)
        self.assertEqual("disk_space", response.tool_calls[0].name)
        self.assertEqual(21, response.input_tokens)
        self.assertEqual(0.0, response.cost_usd)
        payload = FakeOllamaHandler.request_payload
        assert payload is not None
        self.assertEqual("test-model", payload["model"])
        self.assertFalse(payload["stream"])
        self.assertFalse(payload["think"])
        self.assertEqual(0.0, payload["options"]["temperature"])
        self.assertEqual(tools, payload["tools"])
        self.assertIn("Omarchy Voice Master Charter", payload["messages"][0]["content"])
        self.assertEqual("user", payload["messages"][1]["role"])

    def test_deep_provider_enables_thinking_and_bounded_residency(self) -> None:
        provider = OllamaProvider(
            f"http://127.0.0.1:{self.server.server_port}",
            "deep-model",
            name="ollama-deep",
            think=True,
            keep_alive="5m",
            timeout_seconds=300,
            temperature=0.2,
        )
        response = provider.respond("Think deeply about this.")
        self.assertEqual("ollama-deep", response.provider)
        payload = FakeOllamaHandler.request_payload
        assert payload is not None
        self.assertTrue(payload["think"])
        self.assertEqual("5m", payload["keep_alive"])
        self.assertEqual(0.2, payload["options"]["temperature"])


class FakeHermesHandler(BaseHTTPRequestHandler):
    request_payload: dict[str, object] | None = None
    request_headers: dict[str, str] | None = None

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers["Content-Length"])
        type(self).request_payload = json.loads(self.rfile.read(length))
        type(self).request_headers = dict(self.headers)
        body = json.dumps(
            {
                "choices": [
                    {"message": {"role": "assistant", "content": "Hermes answer"}}
                ],
                "usage": {"prompt_tokens": 31, "completion_tokens": 9},
            }
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, message_format: str, *args: object) -> None:
        del message_format, args


class HermesProviderTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), FakeHermesHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)

    def test_agent_contract_and_memory_scope(self) -> None:
        provider = HermesProvider(
            f"http://127.0.0.1:{self.server.server_port}",
            api_key="test-secret",
            use_async_runs=False,
        )
        response = provider.respond(
            "Continue our work", context="Recent VoiceOS turns", conversation_id="owner-1"
        )
        self.assertEqual("Hermes answer", response.text)
        self.assertEqual("hermes", response.provider)
        self.assertEqual(31, response.input_tokens)
        headers = FakeHermesHandler.request_headers
        payload = FakeHermesHandler.request_payload
        assert headers is not None and payload is not None
        self.assertEqual("Bearer test-secret", headers["Authorization"])
        self.assertEqual("voiceos:owner-1", headers["X-Hermes-Session-Key"])
        self.assertIn("You are VIC", payload["messages"][0]["content"])
        self.assertIn("Hermes is your runtime, not your public name", payload["messages"][0]["content"])
        self.assertIn("answer directly without inspecting or creating skills", payload["messages"][0]["content"])
        self.assertIn("Recent VoiceOS turns", payload["messages"][0]["content"])


class FakeHermesAsyncHandler(BaseHTTPRequestHandler):
    approval_choice: str | None = None
    run_id = "run_0123456789abcdef0123456789abcdef"

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        payload = json.loads(self.rfile.read(length) or b"{}")
        if self.path == "/v1/runs":
            self._json(202, {"run_id": self.run_id, "status": "started"})
            return
        if self.path == f"/v1/runs/{self.run_id}/approval":
            type(self).approval_choice = payload.get("choice")
            self._json(200, {"status": "running"})
            return
        self._json(404, {"error": "not_found"})

    def do_GET(self) -> None:  # noqa: N802
        if self.path == f"/v1/runs/{self.run_id}/events":
            events = [
                {
                    "type": "approval.request",
                    "command": "systemctl restart voiceos",
                    "description": "Restart VoiceOS",
                    "pattern_key": "systemctl",
                    "allow_permanent": True,
                    "allow_session": True,
                }
            ]
            body = "".join(f"data: {json.dumps(event)}\n\n" for event in events).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path == f"/v1/runs/{self.run_id}":
            self._json(
                200,
                {
                    "run_id": self.run_id,
                    "status": "completed",
                    "output": "Restart decision recorded",
                    "usage": {"prompt_tokens": 4, "completion_tokens": 3},
                },
            )
            return
        self._json(404, {"error": "not_found"})

    def _json(self, status: int, payload: dict[str, object]) -> None:
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, message_format: str, *args: object) -> None:
        del message_format, args


class HermesAsyncProviderTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), FakeHermesAsyncHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)

    def test_async_approval_is_bounded_to_once_or_deny(self) -> None:
        provider = HermesProvider(
            f"http://127.0.0.1:{self.server.server_port}", api_key="test-secret"
        )
        response = provider.respond("Restart VoiceOS", conversation_id="owner-1")
        self.assertEqual(FakeHermesAsyncHandler.run_id, response.approvals[0].provider_run_id)
        self.assertEqual("hermes.systemctl", response.approvals[0].tool)
        self.assertIn("VIC wants to run", response.text)
        self.assertEqual("systemctl restart voiceos", response.approvals[0].arguments["command"])
        completed = provider.complete_approval(FakeHermesAsyncHandler.run_id, approve=True)
        self.assertEqual("once", FakeHermesAsyncHandler.approval_choice)
        self.assertEqual("Restart decision recorded", completed.text)
        self.assertEqual(4, completed.input_tokens)

class CodexBridgeProviderTest(unittest.TestCase):
    @patch("services.gateway.providers.AF_UNIX", 1)
    @patch("services.gateway.providers.socket.socket")
    def test_answer_only_bridge_contract(self, socket_factory: MagicMock) -> None:
        connection = socket_factory.return_value.__enter__.return_value
        connection.recv.side_effect = [
            json.dumps({"ok": True, "text": "Verified answer"}).encode(),
            b"",
        ]
        provider = CodexBridgeProvider("/run/test/codex.sock", enabled=True)
        response = provider.respond("Ask Codex to verify this", tools=[{"ignored": True}])
        self.assertEqual("codex-sol", response.provider)
        self.assertEqual("Verified answer", response.text)
        connection.connect.assert_called_once_with("/run/test/codex.sock")
        sent = connection.sendall.call_args.args[0]
        bridged_prompt = json.loads(sent)["text"]
        self.assertIn("Omarchy Voice Master Charter", bridged_prompt)
        self.assertTrue(bridged_prompt.endswith("User request: Ask Codex to verify this"))


class RecordingProvider:
    def __init__(self, name: str, model: str = "configured") -> None:
        self.name = name
        self.model = model
        self.prompts: list[str] = []
        self.contexts: list[str | None] = []
        self.conversation_ids: list[str | None] = []

    def respond(
        self,
        text: str,
        tools: list[dict[str, object]] | None = None,
        context: str | None = None,
        conversation_id: str | None = None,
    ) -> ProviderResponse:
        del tools
        self.prompts.append(text)
        self.contexts.append(context)
        self.conversation_ids.append(conversation_id)
        return ProviderResponse(text=self.name, provider=self.name)


class ProviderRouterTest(unittest.TestCase):
    def setUp(self) -> None:
        self.router = ProviderRouter()
        self.fast = RecordingProvider("ollama")
        self.deep = RecordingProvider("ollama-deep")
        self.router._ollama = self.fast
        self.router._ollama_deep = self.deep
        self.codex = RecordingProvider("codex-sol")
        self.codex.configured = True
        self.router._codex = self.codex
        self.router._providers["ollama"] = self.fast
        self.router._providers["ollama-deep"] = self.deep
        self.router._providers["codex-sol"] = self.codex
        self.router.default_provider = self.fast

    def configure_hermes_as_default(self) -> RecordingProvider:
        hermes = RecordingProvider("hermes")
        hermes.configured = True
        self.router._hermes = hermes
        self.router._providers["hermes"] = hermes
        self.router.default_provider = hermes
        return hermes

    def test_routine_request_uses_fast_provider(self) -> None:
        response = self.router.respond("What is ten plus ten?")
        self.assertEqual("ollama", response.provider)

    def test_explicit_deep_request_uses_deep_provider(self) -> None:
        response = self.router.respond("Think deeply about this architecture.")
        self.assertEqual("ollama-deep", response.provider)

    def test_explicit_codex_request_uses_sol_before_local_deep(self) -> None:
        self.assertTrue(_should_use_codex("Use Codex and think deeply about this."))
        response = self.router.respond("Use Codex and think deeply about this.")
        self.assertEqual("codex-sol", response.provider)

    def test_deep_request_does_not_implicitly_use_codex(self) -> None:
        self.assertFalse(_should_use_codex("Think deeply about this architecture."))
        response = self.router.respond("Think deeply about this architecture.")
        self.assertEqual("ollama-deep", response.provider)

    def test_complex_review_phrase_uses_deep_provider(self) -> None:
        self.assertTrue(_should_use_deep_reasoning("Perform a security review."))
        response = self.router.respond("Perform a security review.")
        self.assertEqual("ollama-deep", response.provider)

    def test_long_request_uses_deep_provider(self) -> None:
        response = self.router.respond("x" * 600)
        self.assertEqual("ollama-deep", response.provider)

    def test_multi_step_numerical_request_uses_deep_provider(self) -> None:
        response = self.router.respond(
            "Three workers process 14 jobs each. How many total jobs is that?"
        )
        self.assertEqual("ollama-deep", response.provider)

    def test_simple_arithmetic_stays_fast(self) -> None:
        response = self.router.respond("What is 17 times 23?")
        self.assertEqual("ollama", response.provider)

    def test_unconfigured_deep_model_stays_fast(self) -> None:
        self.deep.model = ""
        response = self.router.respond("Think deeply about this.")
        self.assertEqual("ollama", response.provider)

    def test_routine_conversation_bypasses_default_hermes_and_keeps_memory(self) -> None:
        hermes = self.configure_hermes_as_default()
        response = self.router.respond(
            "What were we talking about earlier?",
            context="Canonical memory context",
            conversation_id="owner-device-conversation",
        )
        self.assertEqual("ollama", response.provider)
        self.assertEqual([], hermes.prompts)
        self.assertEqual(["Canonical memory context"], self.fast.contexts)
        self.assertEqual(["owner-device-conversation"], self.fast.conversation_ids)

    def test_operational_request_uses_default_hermes(self) -> None:
        hermes = self.configure_hermes_as_default()
        response = self.router.respond("Go ahead and deploy the gateway update.")
        self.assertEqual("hermes", response.provider)
        self.assertEqual(1, len(hermes.prompts))

    def test_voice_style_follow_through_request_uses_hermes(self) -> None:
        self.configure_hermes_as_default()
        response = self.router.respond("All right, let's get started on that.")
        self.assertEqual("hermes", response.provider)

    def test_requested_system_check_uses_hermes(self) -> None:
        self.configure_hermes_as_default()
        response = self.router.respond("Can you check the memory service?")
        self.assertEqual("hermes", response.provider)

    def test_brainstorming_about_building_stays_fast(self) -> None:
        self.configure_hermes_as_default()
        response = self.router.respond("What could we build next for VoiceOS?")
        self.assertEqual("ollama", response.provider)

    def test_explicit_hermes_request_overrides_fast_lane(self) -> None:
        self.configure_hermes_as_default()
        response = self.router.respond("Use Hermes to answer this ordinary question.")
        self.assertEqual("hermes", response.provider)

    def test_explicit_provider_hint_always_wins(self) -> None:
        hermes = self.configure_hermes_as_default()
        response = self.router.respond("Hello VIC.", provider="hermes")
        self.assertEqual("hermes", response.provider)
        self.assertEqual(1, len(hermes.prompts))


if __name__ == "__main__":
    unittest.main()
