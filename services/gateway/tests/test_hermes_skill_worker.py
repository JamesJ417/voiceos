from __future__ import annotations

import json
import tempfile
import threading
import unittest
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from services.gateway.hermes_skill_worker import SkillControlError, SkillController


VALID_V1 = b"""---
name: rig-health
description: Check the VoiceOS rig health
---
Use the terminal to inspect disk space.
"""

VALID_V2 = VALID_V1.replace(b"disk space", b"disk space and systemctl status")


class FakeRustHandler(BaseHTTPRequestHandler):
    imports: list[dict[str, object]] = []

    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        payload = json.loads(self.rfile.read(length))
        type(self).imports.append(payload)
        proposal_id = str(uuid.uuid4())
        body = json.dumps({"proposal": {"id": proposal_id}}).encode()
        self.send_response(201)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, message_format: str, *args: object) -> None:
        del message_format, args


class HermesSkillWorkerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), FakeRustHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=2)

    def setUp(self) -> None:
        FakeRustHandler.imports.clear()
        self.temp = tempfile.TemporaryDirectory()
        root = Path(self.temp.name)
        self.skills = root / "skills"
        self.skills.mkdir()
        self.state = root / "state"
        self.controller = SkillController(
            self.skills,
            self.state,
            f"http://127.0.0.1:{self.server.server_port}",
        )

    def tearDown(self) -> None:
        self.temp.cleanup()

    def test_new_skill_is_quarantined_approved_and_rolled_back(self) -> None:
        skill = self.skills / "rig-health" / "SKILL.md"
        skill.parent.mkdir()
        skill.write_bytes(VALID_V1)

        proposal = self.controller.scan("hermes-run-1")[0]
        proposal_id = str(proposal["proposal_id"])
        self.assertFalse(skill.exists())
        self.assertIn("terminal", proposal["required_capabilities"])
        evidence = FakeRustHandler.imports[0]["evidence"][0]
        self.assertEqual("hermes-run-1", evidence["run_id"])
        self.assertEqual("remove_new_skill", evidence["rollback"])

        self.controller.decide(proposal_id, approve=True)
        self.assertEqual(VALID_V1, skill.read_bytes())
        self.controller.rollback(proposal_id)
        self.assertFalse(skill.exists())

    def test_changed_skill_restores_previous_until_approved_and_rollback_restores_it(self) -> None:
        skill = self.skills / "rig-health" / "SKILL.md"
        skill.parent.mkdir()
        skill.write_bytes(VALID_V1)
        self.controller = SkillController(
            self.skills,
            self.state / "second",
            f"http://127.0.0.1:{self.server.server_port}",
        )
        skill.write_bytes(VALID_V2)

        proposal = self.controller.scan("hermes-run-2")[0]
        proposal_id = str(proposal["proposal_id"])
        self.assertEqual(VALID_V1, skill.read_bytes())
        self.controller.decide(proposal_id, approve=True)
        self.assertEqual(VALID_V2, skill.read_bytes())
        self.controller.rollback(proposal_id)
        self.assertEqual(VALID_V1, skill.read_bytes())

    def test_invalid_skill_cannot_be_approved(self) -> None:
        skill = self.skills / "unsafe" / "SKILL.md"
        skill.parent.mkdir()
        skill.write_text("no frontmatter", encoding="utf-8")
        proposal = self.controller.scan("hermes-run-3")[0]
        with self.assertRaisesRegex(SkillControlError, "validation must pass"):
            self.controller.decide(str(proposal["proposal_id"]), approve=True)


if __name__ == "__main__":
    unittest.main()
