from __future__ import annotations

import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from services.wake_bridge.listener import (
    CACHED_SPEECH,
    GatewayClient,
    command_after_wake_phrase,
    ends_conversation,
    usable_transcript,
)


class _GatewayHandler(BaseHTTPRequestHandler):
    requests: list[dict[str, object]] = []
    paths: list[str] = []

    def do_POST(self) -> None:  # noqa: N802
        size = int(self.headers["Content-Length"])
        self.__class__.requests.append(json.loads(self.rfile.read(size)))
        self.__class__.paths.append(self.path)
        if self.path.endswith("/floor"):
            body = json.dumps({"floor": {"active": True}}).encode()
        else:
            body = json.dumps(
                {
                    "session_id": "voice-session-2",
                    "response_text": "Brick & Copper opens at eleven.",
                }
            ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_: object) -> None:
        pass


class GatewayClientTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), _GatewayHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)

    def setUp(self) -> None:
        _GatewayHandler.requests.clear()
        _GatewayHandler.paths.clear()

    def test_submit_text_uses_and_preserves_the_voice_session(self) -> None:
        client = GatewayClient(f"http://127.0.0.1:{self.server.server_port}", "voice-session-1")

        reply = client.submit_text("Hey Vic, when does Brick and Copper open?")

        self.assertEqual("Brick & Copper opens at eleven.", reply)
        self.assertEqual("voice-session-2", client.session_id)
        self.assertEqual(
            [{
                "session_id": "voice-session-1",
                "text": "Hey Vic, when does Brick and Copper open?",
            }],
            _GatewayHandler.requests,
        )

    def test_change_floor_publishes_shared_window_state(self) -> None:
        client = GatewayClient(f"http://127.0.0.1:{self.server.server_port}", "voice-session-1")

        client.change_floor("claim", "listening", partial_transcript="Hey VIC")

        self.assertEqual(["/v1/conversations/active/floor"], _GatewayHandler.paths)
        self.assertEqual("VIC desktop microphone", _GatewayHandler.requests[0]["display_name"])
        self.assertEqual("Hey VIC", _GatewayHandler.requests[0]["partial_transcript"])


class WakePhraseTest(unittest.TestCase):
    def test_speech_cache_has_no_canned_working_message(self) -> None:
        self.assertNotIn("progress", CACHED_SPEECH)
        self.assertFalse(any("working on that" in text.casefold() for text in CACHED_SPEECH.values()))

    def test_removes_wake_phrase_from_command(self) -> None:
        self.assertEqual(
            "what is on my task list",
            command_after_wake_phrase("Hey Vic, what is on my task list", "hey vic"),
        )

    def test_leaves_command_without_wake_phrase_alone(self) -> None:
        self.assertEqual("open the browser", command_after_wake_phrase("open the browser", "hey vic"))

    def test_recognizes_conversation_stop_phrases(self) -> None:
        self.assertTrue(ends_conversation("Stop listening, VIC."))
        self.assertTrue(ends_conversation("Goodbye"))
        self.assertFalse(ends_conversation("Stop the music"))

    def test_rejects_whisper_background_audio_markers(self) -> None:
        self.assertFalse(usable_transcript("[BLANK_AUDIO]"))
        self.assertFalse(usable_transcript("(dramatic music)"))
        self.assertTrue(usable_transcript("What is the weather?"))
