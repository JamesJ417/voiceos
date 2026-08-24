from __future__ import annotations

import json
import tempfile
import threading
import unittest
from http.client import HTTPConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from services.gateway.audit import AuditStore
from services.gateway.enrollment_qr import build_enrollment_uri
from services.gateway.providers import ProviderResponse
from services.gateway.tools import ToolBroker
from services.gateway.server import (
    _clean_hermes_completion_report,
    _render_conversation_context,
    create_server,
)


class GatewayTest(unittest.TestCase):
    def test_cleans_hermes_completion_transport_envelope(self) -> None:
        raw = """[ASYNC DELEGATION BATCH COMPLETE — deleg_1234]
Transport explanation.

Dispatched: yesterday
Context you provided: internal prompt
Role: leaf   Model: ?   Total duration: 2s

--- ✓ TASK 1/1: Research something.  (status=completed, api_calls=2, 1.8s) ---
## Outcome
The useful finding is here.
"""
        self.assertEqual(
            "Hermes subagent report\n\n"
            "Worker 1 — Completed\n\n"
            "Outcome\nThe useful finding is here.",
            _clean_hermes_completion_report(raw),
        )

    def test_cleans_multiple_hermes_worker_answers(self) -> None:
        raw = """[ASYNC DELEGATION BATCH COMPLETE — deleg_1234]
--- ✓ TASK 1/2: First.  (status=completed, api_calls=1) ---
First result.
--- ✗ TASK 2/2: Second.  (status=failed, api_calls=1) ---
Second result.
"""
        cleaned = _clean_hermes_completion_report(raw)
        self.assertIn("Hermes subagent reports", cleaned)
        self.assertIn("Worker 1 — Completed\n\nFirst result.", cleaned)
        self.assertIn("Worker 2 — Failed\n\nSecond result.", cleaned)
        self.assertNotIn("ASYNC DELEGATION", cleaned)
        self.assertNotIn("api_calls", cleaned)

    @classmethod
    def setUpClass(cls) -> None:
        cls.temporary_directory = tempfile.TemporaryDirectory()
        cls.audit_store = AuditStore(Path(cls.temporary_directory.name) / "audit.sqlite3")
        cls.server = create_server(
            "127.0.0.1", 0, audit_store=cls.audit_store, admin_token="test-admin"
        )
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)
        cls.audit_store.close()
        cls.temporary_directory.cleanup()

    def request(
        self,
        method: str,
        path: str,
        body: bytes | None = None,
        content_type: str = "application/octet-stream",
        headers: dict[str, str] | None = None,
    ):
        connection = HTTPConnection("127.0.0.1", self.server.server_port, timeout=2)
        request_headers = dict(headers or {})
        if body:
            request_headers.setdefault("Content-Type", content_type)
        connection.request(method, path, body=body, headers=request_headers)
        response = connection.getresponse()
        payload = json.loads(response.read())
        connection.close()
        return response.status, payload

    def test_health(self) -> None:
        status, payload = self.request("GET", "/v1/health")
        self.assertEqual(200, status)
        self.assertEqual("ok", payload["gateway"])
        self.assertEqual("mock", payload["language_model"])

    def test_browser_preflight_allows_local_and_rejects_unknown_origins(self) -> None:
        connection = HTTPConnection("127.0.0.1", self.server.server_port, timeout=2)
        connection.request(
            "OPTIONS",
            "/v1/turns/text",
            headers={
                "Origin": "http://localhost:3000",
                "Access-Control-Request-Method": "POST",
                "Access-Control-Request-Headers": "authorization,content-type",
            },
        )
        response = connection.getresponse()
        response.read()
        self.assertEqual(204, response.status)
        self.assertEqual(
            "http://localhost:3000",
            response.getheader("Access-Control-Allow-Origin"),
        )
        self.assertIn("Authorization", response.getheader("Access-Control-Allow-Headers"))
        connection.close()

        connection = HTTPConnection("127.0.0.1", self.server.server_port, timeout=2)
        connection.request(
            "OPTIONS",
            "/v1/turns/text",
            headers={
                "Origin": "https://voiceos-web.example"
            },
        )
        response = connection.getresponse()
        response.read()
        self.assertEqual(204, response.status)
        self.assertEqual(
            "https://voiceos-web.example",
            response.getheader("Access-Control-Allow-Origin"),
        )
        connection.close()

        connection = HTTPConnection("127.0.0.1", self.server.server_port, timeout=2)
        connection.request(
            "OPTIONS",
            "/v1/turns/text",
            headers={"Origin": "https://untrusted.example"},
        )
        response = connection.getresponse()
        payload = json.loads(response.read())
        self.assertEqual(403, response.status)
        self.assertEqual("web_origin_not_allowed", payload["error"])
        self.assertIsNone(response.getheader("Access-Control-Allow-Origin"))
        connection.close()

    def test_rust_memory_context_is_rendered_for_provider_continuity(self) -> None:
        rendered = _render_conversation_context(
            {
                "summary": "The user is building VoiceOS.",
                "memories": [{"content": "The GPU rig runs inference."}],
                "recent_messages": [
                    {"role": "user", "content": "We discussed the wall terminal."},
                    {"role": "assistant", "content": "It will use the shared conversation."},
                ],
                "document_context": "[Source: profile.md] prefers concise replies",
            }
        )
        self.assertIn("Durable user memories", rendered)
        self.assertIn("Rolling conversation summary", rendered)
        self.assertIn("Recent conversation", rendered)
        self.assertIn("Private uploaded document excerpts", rendered)

    def test_system_health_tool_returns_evidence(self) -> None:
        status, payload = self.request("GET", "/v1/tools/system.health")
        self.assertEqual(200, status)
        self.assertIn(payload["status"], {"healthy", "degraded"})
        self.assertGreater(payload["logical_cpu_count"], 0)
        self.assertGreater(payload["disk_total_bytes"], 0)

    def test_provider_registry_includes_future_slots(self) -> None:
        status, payload = self.request("GET", "/v1/providers")
        self.assertEqual(200, status)
        providers = {provider["name"]: provider for provider in payload["providers"]}
        self.assertTrue(providers["mock"]["configured"])
        self.assertIn("ollama", providers)
        self.assertIn("ollama-deep", providers)
        self.assertEqual("complex-escalation", providers["ollama-deep"]["role"])
        self.assertIn("codex-sol", providers)
        self.assertEqual(
            "explicit-highest-confidence", providers["codex-sol"]["role"]
        )
        self.assertTrue(providers["openai"]["requires_credentials"])
        self.assertEqual("optional-review", providers["claude-review"]["role"])

    def test_skill_proposal_routes_fail_closed_without_rust_authority(self) -> None:
        status, payload = self.request("GET", "/v1/skills/proposals?status=proposed")
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

    def test_task_routes_fail_closed_without_rust_authority(self) -> None:
        status, payload = self.request("GET", "/v1/tasks?limit=3")
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        capture = json.dumps({"title": "Build a greenhouse", "importance": "normal"}).encode()
        status, payload = self.request(
            "POST", "/v1/focus/captures", capture, content_type="application/json"
        )
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        switch = json.dumps({"task_id": "example", "planned_minutes": 5}).encode()
        status, payload = self.request(
            "POST", "/v1/focus/switch", switch, content_type="application/json"
        )
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        attention = json.dumps({"due_at": None, "importance": "high"}).encode()
        status, payload = self.request(
            "POST",
            "/v1/tasks/example/attention",
            attention,
            content_type="application/json",
        )
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        status, payload = self.request("GET", "/v1/projects?limit=20")
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        status, payload = self.request("GET", "/v1/focus?mode=low_energy")
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        focus = json.dumps({"mode": "five_minute", "planned_minutes": 5}).encode()
        status, payload = self.request(
            "POST", "/v1/focus/sessions", focus, content_type="application/json"
        )
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        focus_action = json.dumps({"action": "interrupt"}).encode()
        status, payload = self.request(
            "POST",
            "/v1/focus/sessions/example/actions",
            focus_action,
            content_type="application/json",
        )
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        project = json.dumps({"title": "VIC touch panel"}).encode()
        status, payload = self.request(
            "POST", "/v1/projects", project, content_type="application/json"
        )
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        assignment = json.dumps({"project_id": None}).encode()
        status, payload = self.request(
            "POST",
            "/v1/tasks/example/project",
            assignment,
            content_type="application/json",
        )
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        create = json.dumps(
            {
                "title": "Build widget",
                "observable_outcome": "Widget displays tasks",
                "estimated_minutes": 20,
            }
        ).encode()
        status, payload = self.request(
            "POST", "/v1/tasks", create, content_type="application/json"
        )
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        update = json.dumps({"status": "completed"}).encode()
        status, payload = self.request(
            "POST",
            "/v1/tasks/example/status",
            update,
            content_type="application/json",
        )
        self.assertEqual(503, status)
        self.assertEqual("file_memory_unavailable", payload["error"])

        decision = json.dumps({"decision": "approve"}).encode()
        status, payload = self.request(
            "POST",
            "/v1/skills/proposals/example/decision",
            decision,
            content_type="application/json",
        )
        self.assertEqual(503, status)
        self.assertEqual("skill_memory_unavailable", payload["error"])

    def test_isolated_capability_routes_fail_closed_when_workers_are_absent(self) -> None:
        status, payload = self.request("GET", "/v1/capabilities/speech/health")
        self.assertEqual(503, status)
        self.assertEqual("speech_worker_unavailable", payload["error"])

        speech_request = json.dumps({"conversation_id": "owner-conversation"}).encode()
        status, payload = self.request(
            "POST", "/v1/speech/sessions", speech_request, content_type="application/json"
        )
        self.assertEqual(503, status)
        self.assertEqual("speech_worker_unavailable", payload["error"])

        crawl_request = json.dumps({"url": "https://example.com"}).encode()
        status, payload = self.request(
            "POST", "/v1/retrieval/web", crawl_request, content_type="application/json"
        )
        self.assertEqual(503, status)
        self.assertEqual("crawl4ai_unavailable", payload["error"])

    def test_tool_catalog_exposes_approval_policy(self) -> None:
        status, payload = self.request("GET", "/v1/tools")
        self.assertEqual(200, status)
        tools = {tool["name"]: tool for tool in payload["tools"]}
        self.assertEqual("none", tools["disk.space"]["approval"])
        self.assertEqual("confirm", tools["project.tests"]["approval"])
        self.assertFalse(tools["project.tests"]["read_only"])

    def test_model_tool_alias_resolves_to_canonical_allowlisted_tool(self) -> None:
        schemas = self.server.coordinator.tools.model_schemas()
        names = {schema["function"]["name"] for schema in schemas}
        self.assertIn("disk_space", names)
        outcome = self.server.coordinator.tools.execute("disk_space", {})
        self.assertEqual("completed", outcome.status)
        self.assertEqual("disk.space", outcome.name)

        service_outcome = self.server.coordinator.tools.execute(
            "service_status", {"service": "Ollama"}
        )
        self.assertEqual("completed", service_outcome.status)
        self.assertEqual("ollama", service_outcome.result["service"])

    def test_console_tools_are_narrow_and_need_no_shell_arguments(self) -> None:
        broker = ToolBroker()
        delivered: list[str] = []
        broker.register_console_tools(
            lambda arguments: delivered.append(str(arguments["tool"]))
            or {"status": "completed"}
        )
        outcome = broker.execute("console_show_weather", {})
        self.assertEqual("completed", outcome.status)
        self.assertEqual(["console.show_weather"], delivered)
        denied = broker.execute(
            "console.show_weather", {"script": "alert('no')"}
        )
        self.assertEqual("denied", denied.status)
        self.assertEqual("arguments_not_allowed", denied.error)

    def test_project_tests_require_approval_without_execution(self) -> None:
        body = json.dumps(
            {"name": "project.tests", "arguments": {"suite": "gateway"}}
        ).encode()
        status, payload = self.request(
            "POST", "/v1/tools/execute", body, content_type="application/json"
        )
        self.assertEqual(200, status)
        self.assertEqual("approval_required", payload["status"])
        self.assertTrue(payload["approval_required"])
        self.assertIsNone(payload["result"])

    def test_direct_approval_flag_cannot_bypass_pending_request(self) -> None:
        body = json.dumps(
            {
                "name": "project.tests",
                "arguments": {"suite": "gateway"},
                "approved": True,
            }
        ).encode()
        status, payload = self.request(
            "POST", "/v1/tools/execute", body, content_type="application/json"
        )
        self.assertEqual(400, status)
        self.assertEqual("use_approval_decision_endpoint", payload["error"])

    def test_unknown_tool_is_denied(self) -> None:
        body = json.dumps({"name": "shell", "arguments": {"command": "whoami"}}).encode()
        status, payload = self.request(
            "POST", "/v1/tools/execute", body, content_type="application/json"
        )
        self.assertEqual(403, status)
        self.assertEqual("denied", payload["status"])
        self.assertEqual("tool_not_allowlisted", payload["error"])

    def test_create_session(self) -> None:
        status, payload = self.request("POST", "/v1/sessions", b"")
        self.assertEqual(201, status)
        self.assertIn("session_id", payload)

    def test_one_time_device_enrollment(self) -> None:
        body = json.dumps(
            {"gateway_url": "https://voiceos-rig.example.ts.net", "ttl_seconds": 600}
        ).encode()
        status, payload = self.request(
            "POST",
            "/v1/enrollment/sessions",
            body,
            content_type="application/json",
            headers={"X-VoiceOS-Admin-Token": "test-admin"},
        )
        self.assertEqual(201, status)
        self.assertIn("voiceos://enroll?", payload["enrollment_uri"])
        code = payload["enrollment_uri"].split("code=", 1)[1]

        exchange = json.dumps({"code": code, "device_name": "Pixel test"}).encode()
        status, credential = self.request(
            "POST", "/v1/enrollment/exchange", exchange, content_type="application/json"
        )
        self.assertEqual(201, status)
        self.assertIn("device_id", credential)
        self.assertIsNotNone(
            self.audit_store.authenticate_device(credential["device_token"])
        )

        status, payload = self.request(
            "POST", "/v1/enrollment/exchange", exchange, content_type="application/json"
        )
        self.assertEqual(401, status)
        self.assertEqual("invalid_or_expired_enrollment", payload["error"])

    def test_enrollment_uri_encodes_gateway_and_code(self) -> None:
        uri = build_enrollment_uri("https://host.example/path", "one_time-code")
        self.assertEqual(
            "voiceos://enroll?gateway=https%3A%2F%2Fhost.example%2Fpath&code=one_time-code",
            uri,
        )

    def test_device_authentication_can_be_enforced(self) -> None:
        code, _ = self.audit_store.create_enrollment_code()
        credential = self.audit_store.exchange_enrollment_code(code, "Authenticated Pixel")
        self.assertIsNotNone(credential)
        assert credential is not None
        self.server.require_device_auth = True
        try:
            status, payload = self.request("GET", "/v1/providers")
            self.assertEqual(401, status)
            self.assertEqual("device_authentication_required", payload["error"])
            status, payload = self.request(
                "GET",
                "/v1/providers",
                headers={"Authorization": f"Bearer {credential['device_token']}"},
            )
            self.assertEqual(200, status)
            self.assertIn("providers", payload)
        finally:
            self.server.require_device_auth = False

    def test_audio_turn(self) -> None:
        status, payload = self.request("POST", "/v1/turns/audio", b"\x00\x01" * 400)
        self.assertEqual(200, status)
        self.assertIn("response_text", payload)
        self.assertGreaterEqual(payload["processing_ms"], 1)

    def test_empty_audio_is_rejected(self) -> None:
        status, payload = self.request("POST", "/v1/turns/audio", b"")
        self.assertEqual(400, status)
        self.assertEqual("empty_audio", payload["error"])

    def test_text_turn_preserves_transcript_and_session(self) -> None:
        body = json.dumps({"session_id": "test-session", "text": "Hello gateway"}).encode()
        status, payload = self.request(
            "POST",
            "/v1/turns/text",
            body,
            content_type="application/json",
        )
        self.assertEqual(200, status)
        self.assertEqual("test-session", payload["session_id"])
        self.assertEqual("Hello gateway", payload["transcript"])
        self.assertEqual("mock", payload["provider"])

    def test_text_turn_replays_local_session_history_without_memory_service(self) -> None:
        class ContinuityRouter:
            default_name = "hermes"
            contexts: list[str | None] = []
            conversation_ids: list[str | None] = []

            def respond(self, text: str, provider=None, tools=None, context=None, conversation_id=None):
                del text, provider, tools
                self.contexts.append(context)
                self.conversation_ids.append(conversation_id)
                return ProviderResponse(text="VIC reply", provider="hermes")

        original_memory_url = self.server.memory_url
        original_router = self.server.coordinator.router
        router = ContinuityRouter()
        self.server.memory_url = ""
        self.server.coordinator.router = router  # type: ignore[assignment]
        try:
            first = json.dumps(
                {"session_id": "local-continuity-test", "text": "My favorite color is cobalt."}
            ).encode()
            second = json.dumps(
                {"session_id": "local-continuity-test", "text": "What color did I just name?"}
            ).encode()
            self.assertEqual(
                200,
                self.request("POST", "/v1/turns/text", first, content_type="application/json")[0],
            )
            self.assertEqual(
                200,
                self.request("POST", "/v1/turns/text", second, content_type="application/json")[0],
            )
            self.assertIsNone(router.contexts[0])
            self.assertIn("My favorite color is cobalt.", router.contexts[1] or "")
            self.assertIn("VIC reply", router.contexts[1] or "")
            self.assertEqual(
                ["local-continuity-test", "local-continuity-test"],
                router.conversation_ids,
            )
        finally:
            self.server.memory_url = original_memory_url
            self.server.coordinator.router = original_router

    def test_text_turn_rejects_non_string_provider_hint(self) -> None:
        body = json.dumps({"text": "Hello gateway", "provider": 42}).encode()
        status, payload = self.request(
            "POST", "/v1/turns/text", body, content_type="application/json"
        )
        self.assertEqual(400, status)
        self.assertEqual("invalid_provider", payload["error"])

    def test_fast_provider_turn_keeps_prepare_context_and_commits_reply(self) -> None:
        class MemoryCoreStub(BaseHTTPRequestHandler):
            prepare: dict[str, object] | None = None
            commit: dict[str, object] | None = None

            def do_GET(self) -> None:  # noqa: N802
                if self.path != "/v1/attachments/attachment-1":
                    self.send_error(404)
                    return
                body = b"\x89PNG\r\n\x1a\nfixture"
                self.send_response(200)
                self.send_header("Content-Type", "image/png")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def do_POST(self) -> None:  # noqa: N802
                length = int(self.headers.get("Content-Length", "0"))
                request = json.loads(self.rfile.read(length) or b"{}")
                if self.path == "/internal/v1/conversations/prepare":
                    type(self).prepare = request
                    payload = {
                        "conversation_id": "shared-owner-conversation",
                        "context": {
                            "summary": "We are building VoiceOS.",
                            "memories": [{"content": "The inference host is the GPU rig."}],
                            "recent_messages": [],
                            "document_context": "",
                        },
                    }
                elif self.path == "/internal/v1/conversations/commit":
                    type(self).commit = request
                    payload = {"committed": True}
                else:
                    payload = {}
                body = json.dumps(payload).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        class FastRouterStub:
            default_name = "hermes"
            context: str | None = None
            conversation_id: str | None = None
            provider_hint: str | None = None
            image_data_urls: list[str] | None = None

            def respond(self, text: str, provider=None, tools=None, context=None, conversation_id=None, image_data_urls=None):
                del text, tools
                self.context = context
                self.conversation_id = conversation_id
                self.provider_hint = provider
                self.image_data_urls = image_data_urls
                return ProviderResponse(text="I remember the GPU rig.", provider="ollama")

        core = ThreadingHTTPServer(("127.0.0.1", 0), MemoryCoreStub)
        thread = threading.Thread(target=core.serve_forever, daemon=True)
        thread.start()
        original_memory_url = self.server.memory_url
        original_router = self.server.coordinator.router
        router = FastRouterStub()
        self.server.memory_url = f"http://127.0.0.1:{core.server_port}"
        self.server.coordinator.router = router  # type: ignore[assignment]
        try:
            body = json.dumps(
                {
                    "session_id": "pixel-session",
                    "text": "What were we discussing?",
                    "provider": "ollama",
                    "request_id": "image-turn-1",
                    "attachment_ids": ["attachment-1"],
                }
            ).encode()
            status, payload = self.request(
                "POST",
                "/v1/turns/text",
                body,
                content_type="application/json",
                headers={"X-VoiceOS-Device-ID": "pixel-owner"},
            )
            self.assertEqual(200, status)
            self.assertEqual("ollama", payload["provider"])
            self.assertIn("We are building VoiceOS", router.context or "")
            self.assertEqual("shared-owner-conversation", router.conversation_id)
            self.assertEqual("ollama", router.provider_hint)
            self.assertTrue((router.image_data_urls or [""])[0].startswith("data:image/png;base64,"))
            self.assertEqual("pixel-owner", MemoryCoreStub.prepare["device_id"])  # type: ignore[index]
            self.assertEqual(["attachment-1"], MemoryCoreStub.prepare["attachment_ids"])  # type: ignore[index]
            self.assertEqual("I remember the GPU rig.", MemoryCoreStub.commit["response_text"])  # type: ignore[index]
            self.assertEqual("ollama", MemoryCoreStub.commit["provider"])  # type: ignore[index]
        finally:
            self.server.memory_url = original_memory_url
            self.server.coordinator.router = original_router
            core.shutdown()
            core.server_close()
            thread.join(timeout=2)

    def test_attachment_content_proxy_preserves_binary_bytes(self) -> None:
        image = b"\x89PNG\r\n\x1a\nvoiceos-test-image"

        class AttachmentStub(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802
                if self.path != "/v1/attachments/attachment-1":
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header("Content-Type", "image/png")
                self.send_header("Cache-Control", "private, max-age=300")
                self.send_header("Content-Length", str(len(image)))
                self.end_headers()
                self.wfile.write(image)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        core = ThreadingHTTPServer(("127.0.0.1", 0), AttachmentStub)
        thread = threading.Thread(target=core.serve_forever, daemon=True)
        thread.start()
        original_memory_url = self.server.memory_url
        self.server.memory_url = f"http://127.0.0.1:{core.server_port}"
        try:
            connection = HTTPConnection(
                "127.0.0.1", self.server.server_port, timeout=2
            )
            connection.request("GET", "/v1/attachments/attachment-1")
            response = connection.getresponse()
            body = response.read()
            self.assertEqual(200, response.status)
            self.assertEqual("image/png", response.getheader("Content-Type"))
            self.assertEqual(image, body)
            connection.close()
        finally:
            self.server.memory_url = original_memory_url
            core.shutdown()
            core.server_close()
            thread.join(timeout=2)

    def test_attachment_turn_requires_idempotency_key(self) -> None:
        body = json.dumps({"text": "Describe this", "attachment_ids": ["attachment-1"]}).encode()
        status, payload = self.request("POST", "/v1/turns/text", body, content_type="application/json")
        self.assertEqual(400, status)
        self.assertEqual("request_id_required_for_attachments", payload["error"])

    def test_health_text_turn_uses_deterministic_tool(self) -> None:
        body = json.dumps({"text": "Check system health"}).encode()
        status, payload = self.request(
            "POST",
            "/v1/turns/text",
            body,
            content_type="application/json",
        )
        self.assertEqual(200, status)
        self.assertEqual("deterministic", payload["provider"])
        self.assertEqual("system.health", payload["tool_calls"][0]["name"])
        self.assertIn(payload["evidence"]["status"], {"healthy", "degraded"})

    def test_disk_space_text_turn_uses_deterministic_tool(self) -> None:
        body = json.dumps({"text": "How much disk space is free?"}).encode()
        status, payload = self.request(
            "POST", "/v1/turns/text", body, content_type="application/json"
        )
        self.assertEqual(200, status)
        self.assertEqual("disk.space", payload["tool_calls"][0]["name"])
        self.assertGreater(payload["evidence"]["free_bytes"], 0)

    def test_spoken_task_command_is_dispatched_to_rust_before_the_model(self) -> None:
        class TaskCoreStub(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802
                length = int(self.headers.get("Content-Length", "0"))
                request = json.loads(self.rfile.read(length) or b"{}")
                if self.path == "/internal/v1/tasks/command":
                    self.server.seen = request  # type: ignore[attr-defined]
                    payload = {
                        "handled": True,
                        "response_text": "Added Call the dentist to your task list as a 15 minute task.",
                        "provider": "deterministic-task",
                        "tool_calls": [{"name": "task.create", "status": "completed"}],
                        "approvals": [],
                        "results": [{"task": {"title": "Call the dentist"}}],
                        "errors": [],
                        "evidence": {"tasks_changed": True},
                    }
                else:
                    payload = {}
                body = json.dumps(payload).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        core = ThreadingHTTPServer(("127.0.0.1", 0), TaskCoreStub)
        core.seen = None  # type: ignore[attr-defined]
        thread = threading.Thread(target=core.serve_forever, daemon=True)
        thread.start()
        original_memory_url = self.server.memory_url
        self.server.memory_url = f"http://127.0.0.1:{core.server_port}"
        try:
            body = json.dumps(
                {"text": "Add a task to call the dentist for 15 minutes"}
            ).encode()
            status, payload = self.request(
                "POST", "/v1/turns/text", body, content_type="application/json"
            )
            self.assertEqual(200, status)
            self.assertEqual("deterministic-task", payload["provider"])
            self.assertEqual("task.create", payload["tool_calls"][0]["name"])
            self.assertTrue(payload["evidence"]["tasks_changed"])
            self.assertEqual(
                "Add a task to call the dentist for 15 minutes",
                core.seen["text"],  # type: ignore[index,attr-defined]
            )
        finally:
            self.server.memory_url = original_memory_url
            core.shutdown()
            core.server_close()
            thread.join(timeout=2)

    def test_spoken_console_command_is_dispatched_to_rust_before_the_model(self) -> None:
        class ConsoleCoreStub(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802
                length = int(self.headers.get("Content-Length", "0"))
                request = json.loads(self.rfile.read(length) or b"{}")
                if self.path == "/internal/v1/console/command":
                    self.server.seen = request  # type: ignore[attr-defined]
                    payload = {
                        "handled": True,
                        "response_text": "Showing weather on VIC Console.",
                        "provider": "deterministic-console",
                        "tool_calls": [
                            {"name": "console.show_weather", "status": "completed"}
                        ],
                        "approvals": [],
                        "results": [
                            {"command": "show_weather", "status": "completed"}
                        ],
                        "errors": [],
                        "evidence": {"console_command_delivered": True},
                    }
                else:
                    payload = {}
                body = json.dumps(payload).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        core = ThreadingHTTPServer(("127.0.0.1", 0), ConsoleCoreStub)
        core.seen = None  # type: ignore[attr-defined]
        thread = threading.Thread(target=core.serve_forever, daemon=True)
        thread.start()
        original_memory_url = self.server.memory_url
        self.server.memory_url = f"http://127.0.0.1:{core.server_port}"
        try:
            body = json.dumps({"text": "Show the weather"}).encode()
            status, payload = self.request(
                "POST", "/v1/turns/text", body, content_type="application/json"
            )
            self.assertEqual(200, status)
            self.assertEqual("deterministic-console", payload["provider"])
            self.assertEqual("console.show_weather", payload["tool_calls"][0]["name"])
            self.assertTrue(payload["evidence"]["console_command_delivered"])
            self.assertEqual(
                "Show the weather", core.seen["text"]  # type: ignore[index,attr-defined]
            )
        finally:
            self.server.memory_url = original_memory_url
            core.shutdown()
            core.server_close()
            thread.join(timeout=2)

    def test_spoken_focus_command_is_dispatched_to_rust_before_the_model(self) -> None:
        class FocusCoreStub(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802
                length = int(self.headers.get("Content-Length", "0"))
                request = json.loads(self.rfile.read(length) or b"{}")
                if self.path == "/internal/v1/focus/command":
                    self.server.seen = request  # type: ignore[attr-defined]
                    payload = {
                        "handled": True,
                        "response_text": "Only do this next: put the appointment number in the form.",
                        "provider": "deterministic-focus",
                        "tool_calls": [{"name": "focus.next", "status": "completed"}],
                        "approvals": [],
                        "results": [{"focus": {"priorities": []}}],
                        "errors": [],
                        "evidence": {"focus_state_changed": False},
                    }
                else:
                    payload = {}
                body = json.dumps(payload).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        core = ThreadingHTTPServer(("127.0.0.1", 0), FocusCoreStub)
        core.seen = None  # type: ignore[attr-defined]
        thread = threading.Thread(target=core.serve_forever, daemon=True)
        thread.start()
        original_memory_url = self.server.memory_url
        self.server.memory_url = f"http://127.0.0.1:{core.server_port}"
        try:
            body = json.dumps({"text": "What should I do now?"}).encode()
            status, payload = self.request(
                "POST", "/v1/turns/text", body, content_type="application/json"
            )
            self.assertEqual(200, status)
            self.assertEqual("deterministic-focus", payload["provider"])
            self.assertEqual("focus.next", payload["tool_calls"][0]["name"])
            self.assertFalse(payload["evidence"]["focus_state_changed"])
            self.assertEqual(
                "What should I do now?", core.seen["text"]  # type: ignore[index,attr-defined]
            )
            capture = json.dumps(
                {"text": "Park this idea build a mobile greenhouse"}
            ).encode()
            status, payload = self.request(
                "POST", "/v1/turns/text", capture, content_type="application/json"
            )
            self.assertEqual(200, status)
            self.assertEqual("deterministic-focus", payload["provider"])
            self.assertEqual(
                "Park this idea build a mobile greenhouse",
                core.seen["text"],  # type: ignore[index,attr-defined]
            )
            status, recovery = self.request("GET", "/v1/events/recovery?after=0")
            self.assertEqual(200, status)
            self.assertIn("focus.updated", {event["type"] for event in recovery["events"]})
        finally:
            self.server.memory_url = original_memory_url
            core.shutdown()
            core.server_close()
            thread.join(timeout=2)

    def test_unresolved_task_language_reaches_model_with_authoritative_board(self) -> None:
        class TaskCoreStub(BaseHTTPRequestHandler):
            def do_GET(self) -> None:  # noqa: N802
                payload = {
                    "tasks": [
                        {
                            "id": "task-recipe-cards",
                            "title": "Print and laminate recipe cards",
                            "status": "ready",
                            "estimated_minutes": 20,
                        }
                    ]
                }
                self._reply(payload)

            def do_POST(self) -> None:  # noqa: N802
                length = int(self.headers.get("Content-Length", "0"))
                self.rfile.read(length)
                if self.path == "/internal/v1/tasks/command":
                    self._reply({"handled": False})
                else:
                    self._reply({})

            def _reply(self, payload: dict[str, object]) -> None:
                body = json.dumps(payload).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        class TaskRouterStub:
            default_name = "ollama"
            context: str | None = None

            def respond(self, text: str, provider=None, tools=None, context=None, conversation_id=None):
                del text, provider, tools, conversation_id
                self.context = context
                return ProviderResponse(
                    text="I can help organize, print, and laminate the recipe cards.",
                    provider="ollama",
                )

        core = ThreadingHTTPServer(("127.0.0.1", 0), TaskCoreStub)
        thread = threading.Thread(target=core.serve_forever, daemon=True)
        thread.start()
        original_memory_url = self.server.memory_url
        original_router = self.server.coordinator.router
        router = TaskRouterStub()
        self.server.memory_url = f"http://127.0.0.1:{core.server_port}"
        self.server.coordinator.router = router  # type: ignore[assignment]
        try:
            body = json.dumps(
                {"text": "How can you move the things on my list forward?"}
            ).encode()
            status, payload = self.request(
                "POST", "/v1/turns/text", body, content_type="application/json"
            )

            self.assertEqual(200, status)
            self.assertEqual("ollama", payload["provider"])
            self.assertIn("recipe cards", payload["response_text"])
            self.assertIn("Print and laminate recipe cards", router.context or "")
            self.assertIn("authoritative", (router.context or "").casefold())
        finally:
            self.server.memory_url = original_memory_url
            self.server.coordinator.router = original_router
            core.shutdown()
            core.server_close()
            thread.join(timeout=2)

    def test_daily_checkin_asks_and_persists_twelve_questions(self) -> None:
        start = json.dumps({"text": "Start my daily check-in"}).encode()
        status, payload = self.request(
            "POST", "/v1/turns/text", start, content_type="application/json"
        )
        self.assertEqual(200, status)
        self.assertEqual("deterministic-checkin", payload["provider"])
        self.assertIn("Question 1 of 12", payload["response_text"])

        for index in range(12):
            answer = json.dumps({"text": f"Planning answer {index + 1}"}).encode()
            status, payload = self.request(
                "POST", "/v1/turns/text", answer, content_type="application/json"
            )
            self.assertEqual(200, status)
            self.assertEqual("deterministic-checkin", payload["provider"])
        self.assertIn("all twelve answers", payload["response_text"])

        status, checkin = self.request("GET", "/v1/checkins/daily")
        self.assertEqual(200, status)
        self.assertEqual("completed", checkin["status"])
        self.assertEqual(12, checkin["answered"])
        self.assertEqual(12, len(checkin["answers"]))
        self.assertIsNone(checkin["next_question"])
        self.assertEqual(3, len(checkin["plan"]["priorities"]))
        self.assertTrue(
            all(item["estimated_minutes"] == 20 for item in checkin["plan"]["priorities"])
        )

        status, plan = self.request("GET", "/v1/plans/daily")
        self.assertEqual(200, status)
        self.assertEqual("proposed", plan["plan"]["status"])
        self.assertEqual("daily_checkin_v1", plan["plan"]["source"])

        status, recovery = self.request("GET", "/v1/events/recovery?after=0")
        self.assertEqual(200, status)
        event_types = {event["type"] for event in recovery["events"]}
        self.assertIn("conversation.turn", event_types)
        self.assertIn("daily_plan.proposed", event_types)
        self.assertGreater(recovery["latest_event_id"], 0)

    def test_voice_request_can_propose_but_not_run_tests(self) -> None:
        body = json.dumps({"text": "Run the project tests"}).encode()
        status, payload = self.request(
            "POST", "/v1/turns/text", body, content_type="application/json"
        )
        self.assertEqual(200, status)
        self.assertEqual("approval_required", payload["tool_calls"][0]["status"])
        self.assertEqual("pending", payload["approvals"][0]["status"])
        self.assertEqual([], payload["results"])

    def test_approval_can_be_denied_only_once(self) -> None:
        body = json.dumps({"session_id": "deny-session", "text": "Run the tests"}).encode()
        _, proposal = self.request(
            "POST", "/v1/turns/text", body, content_type="application/json"
        )
        request_id = proposal["approvals"][0]["request_id"]
        decision = json.dumps({"request_id": request_id, "decision": "deny"}).encode()
        status, payload = self.request(
            "POST", "/v1/approvals/decide", decision, content_type="application/json"
        )
        self.assertEqual(200, status)
        self.assertEqual("denied", payload["status"])
        self.assertIn("Nothing ran", payload["response_text"])

        status, payload = self.request(
            "POST", "/v1/approvals/decide", decision, content_type="application/json"
        )
        self.assertEqual(409, status)
        self.assertEqual("already_decided", payload["status"])

    def test_approval_executes_the_stored_arguments(self) -> None:
        functions = self.server.coordinator.tools._functions
        original = functions["project.tests"]
        functions["project.tests"] = lambda arguments: {
            "suite": arguments["suite"],
            "passed": True,
            "exit_code": 0,
            "output": "simulated",
        }
        try:
            body = json.dumps(
                {"session_id": "approve-session", "text": "Run the project tests"}
            ).encode()
            _, proposal = self.request(
                "POST", "/v1/turns/text", body, content_type="application/json"
            )
            request_id = proposal["approvals"][0]["request_id"]
            decision = json.dumps(
                {"request_id": request_id, "decision": "approve"}
            ).encode()
            status, payload = self.request(
                "POST", "/v1/approvals/decide", decision, content_type="application/json"
            )
            self.assertEqual(200, status)
            self.assertEqual("completed", payload["status"])
            self.assertIn("passed", payload["response_text"])
            self.assertEqual("gateway", payload["tool_result"]["arguments"]["suite"])
        finally:
            functions["project.tests"] = original

    def test_turn_and_tool_decisions_are_audited(self) -> None:
        body = json.dumps({"session_id": "audit-session", "text": "Hello audit"}).encode()
        self.request("POST", "/v1/turns/text", body, content_type="application/json")
        status, payload = self.request("GET", "/v1/audit/turns?limit=200")
        self.assertEqual(200, status)
        matching = [
            turn for turn in payload["turns"] if turn["session_id"] == "audit-session"
        ]
        self.assertEqual(1, len(matching))
        self.assertEqual("Hello audit", matching[0]["transcript"])
        self.assertIn("processing_ms", matching[0])

        status, payload = self.request("GET", "/v1/audit/events?limit=200")
        self.assertEqual(200, status)
        event_types = {event["event_type"] for event in payload["events"]}
        self.assertIn("tool.requested", event_types)
        self.assertIn("tool.decided", event_types)

    def test_text_turn_rejects_blank_text(self) -> None:
        body = json.dumps({"text": "   "}).encode()
        status, payload = self.request(
            "POST",
            "/v1/turns/text",
            body,
            content_type="application/json",
        )
        self.assertEqual(400, status)
        self.assertEqual("text_required", payload["error"])

    def test_text_turn_rejects_invalid_json(self) -> None:
        status, payload = self.request(
            "POST",
            "/v1/turns/text",
            b"not-json",
            content_type="application/json",
        )
        self.assertEqual(400, status)
        self.assertEqual("invalid_json", payload["error"])


if __name__ == "__main__":
    unittest.main()
