from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from services.gateway.audit import AuditStore
from services.gateway.server import _revoke_device
from services.gateway.tools import ToolBroker


class DeviceInventoryTests(unittest.TestCase):
    def test_revocation_invalidates_the_actual_bearer_credential(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            store = AuditStore(Path(directory) / "audit.sqlite3")
            code, _ = store.create_enrollment_code()
            enrolled = store.exchange_enrollment_code(code, "Wall panel")
            self.assertIsNotNone(enrolled)
            assert enrolled is not None
            self.assertEqual(enrolled["device_id"], store.authenticate_device(enrolled["device_token"]))
            revoked = _revoke_device(store, {"device_id": enrolled["device_id"], "requesting_device_id": "pixel"})
            self.assertEqual("revoked", revoked["status"])
            self.assertIsNone(store.authenticate_device(enrolled["device_token"]))
            self.assertNotIn("token", str(store.list_devices()).casefold())
            store.close()

    def test_device_tool_requires_approval_and_rejects_self_revocation(self) -> None:
        broker = ToolBroker()
        broker.register_device_tools(lambda arguments: arguments)
        proposal = broker.execute("device.revoke", {"device_id": "panel", "requesting_device_id": "pixel"})
        self.assertEqual("approval_required", proposal.status)
        denied = broker.execute("device.revoke", {"device_id": "pixel", "requesting_device_id": "pixel"})
        self.assertEqual("self_revocation_not_allowed", denied.error)


if __name__ == "__main__":
    unittest.main()
