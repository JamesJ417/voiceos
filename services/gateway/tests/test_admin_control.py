from __future__ import annotations
import unittest
from services.gateway.admin_control import action_proposal

class AdminControlTest(unittest.TestCase):
    def test_restart_is_exact_allowlisted_and_approval_controlled(self) -> None:
        value=action_proposal("restart_service","voiceos-hermes")
        self.assertEqual("approval_required",value["status"])
        self.assertEqual(["/usr/bin/systemctl","restart","voiceos-hermes"],value["approval"]["arguments"]["argv"])
        self.assertTrue(value["approval"]["single_use"])

    def test_arbitrary_service_or_action_is_rejected(self) -> None:
        with self.assertRaises(ValueError):action_proposal("restart_service","ssh")
        with self.assertRaises(ValueError):action_proposal("shell","voiceos-hermes")

if __name__ == "__main__":unittest.main()
