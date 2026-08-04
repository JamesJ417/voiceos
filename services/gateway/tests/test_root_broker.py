from __future__ import annotations

import sys
import tempfile
import time
import unittest
import uuid
from pathlib import Path

from services.gateway.root_broker import BrokerRejected, RootBroker, sign_request


class RootBrokerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.key = b"voiceos-root-broker-test-key-32-bytes-minimum"
        self.broker = RootBroker(
            self.key, Path(self.temporary_directory.name) / "broker.sqlite3"
        )

    def tearDown(self) -> None:
        self.broker.connection.close()
        self.temporary_directory.cleanup()

    def request(self) -> dict[str, object]:
        payload: dict[str, object] = {
            "request_id": str(uuid.uuid4()),
            "nonce": str(uuid.uuid4()),
            "expires_at_unix": int(time.time()) + 60,
            "operation": "command.exec",
            "arguments": {
                "argv": [sys.executable, "-c", "print('broker-ok')"],
                "cwd": str(Path(self.temporary_directory.name).resolve()),
                "timeout_seconds": 5,
                "rollback": "No state change; no rollback required.",
            },
        }
        payload["signature"] = sign_request(payload, self.key)
        return payload

    def test_signed_request_executes_once_and_records_evidence(self) -> None:
        payload = self.request()
        result = self.broker.execute(payload)
        self.assertTrue(result["completed"])
        self.assertEqual("broker-ok", str(result["stdout"]).strip())
        self.assertEqual(64, len(str(result["result_sha256"])))
        with self.assertRaisesRegex(BrokerRejected, "already_consumed"):
            self.broker.execute(payload)

    def test_tampering_after_approval_is_rejected(self) -> None:
        payload = self.request()
        payload["arguments"] = {"argv": [sys.executable, "-c", "print('changed')"], "rollback": "none"}
        with self.assertRaisesRegex(BrokerRejected, "invalid_request_signature"):
            self.broker.execute(payload)

    def test_expired_and_relative_commands_are_rejected(self) -> None:
        expired = self.request()
        expired["expires_at_unix"] = int(time.time()) - 1
        expired["signature"] = sign_request(expired, self.key)
        with self.assertRaisesRegex(BrokerRejected, "request_expired"):
            self.broker.execute(expired)

        relative = self.request()
        relative["arguments"] = {"argv": ["echo", "unsafe-path-resolution"], "rollback": "none"}
        relative["signature"] = sign_request(relative, self.key)
        with self.assertRaisesRegex(BrokerRejected, "absolute_executable_required"):
            self.broker.execute(relative)


if __name__ == "__main__":
    unittest.main()
