from __future__ import annotations

import tempfile
import threading
import time
import unittest
from pathlib import Path

from services.gateway.audit import AuditStore


class ClientEventWaitTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.store = AuditStore(Path(self.temporary_directory.name) / "audit.sqlite3")

    def tearDown(self) -> None:
        self.store.close()
        self.temporary_directory.cleanup()

    def test_publish_wakes_waiter_without_polling_delay(self) -> None:
        received: list[dict[str, object]] = []
        started = threading.Event()

        def wait_for_event() -> None:
            started.set()
            received.extend(self.store.wait_for_client_events(timeout=1.0))

        waiter = threading.Thread(target=wait_for_event)
        waiter.start()
        self.assertTrue(started.wait(0.2))
        time.sleep(0.02)
        published_at = time.monotonic()
        self.store.publish_client_event("status.changed", {"status": "ok"})
        waiter.join(timeout=0.25)

        self.assertFalse(waiter.is_alive())
        self.assertLess(time.monotonic() - published_at, 0.25)
        self.assertEqual("status.changed", received[0]["type"])

    def test_wait_returns_at_heartbeat_deadline_when_idle(self) -> None:
        started_at = time.monotonic()
        events = self.store.wait_for_client_events(timeout=0.05)

        self.assertEqual([], events)
        self.assertGreaterEqual(time.monotonic() - started_at, 0.04)


if __name__ == "__main__":
    unittest.main()
