from __future__ import annotations

import json
import socket
import tempfile
import threading
import unittest
from pathlib import Path

from services.speech_worker.gpu_scheduler import GpuSchedulerClient, GpuSchedulerError


class FakeScheduler:
    def __init__(self, path: Path, response: dict[str, object]) -> None:
        self.path = path
        self.response = response
        self.request: dict[str, object] | None = None
        self.ready = threading.Event()

    def serve_once(self) -> None:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
            server.bind(str(self.path))
            server.listen(1)
            self.ready.set()
            connection, _ = server.accept()
            with connection:
                stream = connection.makefile("rwb")
                self.request = json.loads(stream.readline())
                stream.write(json.dumps(self.response).encode() + b"\n")
                stream.flush()


@unittest.skipUnless(hasattr(socket, "AF_UNIX"), "Unix-domain sockets are required")
class GpuSchedulerClientTest(unittest.TestCase):
    def test_acquire_sends_a_bounded_lease_request(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "scheduler.sock"
            fake = FakeScheduler(path, {"ok": True, "mode": "speech", "speech_leases": 1})
            thread = threading.Thread(target=fake.serve_once)
            thread.start()
            self.assertTrue(fake.ready.wait(2))

            response = GpuSchedulerClient(str(path), timeout_seconds=2).acquire("session-1", 900)
            thread.join(2)

            self.assertEqual("speech", response["mode"])
            self.assertEqual(
                {"action": "acquire", "lease_id": "session-1", "ttl_seconds": 900},
                fake.request,
            )

    def test_scheduler_rejection_becomes_a_typed_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "scheduler.sock"
            fake = FakeScheduler(path, {"ok": False, "detail": "capacity_unavailable"})
            thread = threading.Thread(target=fake.serve_once)
            thread.start()
            self.assertTrue(fake.ready.wait(2))

            with self.assertRaisesRegex(GpuSchedulerError, "capacity_unavailable"):
                GpuSchedulerClient(str(path), timeout_seconds=2).status()
            thread.join(2)

    def test_renew_sends_the_existing_lease_and_ttl(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "scheduler.sock"
            fake = FakeScheduler(path, {"ok": True, "state": "speech"})
            thread = threading.Thread(target=fake.serve_once)
            thread.start()
            self.assertTrue(fake.ready.wait(2))

            GpuSchedulerClient(str(path), timeout_seconds=2).renew("session-1", 600)
            thread.join(2)
            self.assertEqual(
                {"action": "renew", "lease_id": "session-1", "ttl_seconds": 600},
                fake.request,
            )


if __name__ == "__main__":
    unittest.main()
