from __future__ import annotations

import unittest
from unittest.mock import patch

from ops.rig.gpu_scheduler import GpuState, Scheduler


class SchedulerStateMachineTest(unittest.TestCase):
    def test_successful_acquire_and_release_transition_through_stable_states(self) -> None:
        ready = [False]
        with (
            patch("ops.rig.gpu_scheduler._port_ready", side_effect=lambda *_: ready[0]),
            patch("ops.rig.gpu_scheduler._service_active", return_value=False),
            patch("ops.rig.gpu_scheduler._run") as run,
            patch("ops.rig.gpu_scheduler._unload_ollama"),
        ):
            scheduler = Scheduler()
            ready[0] = True
            acquired = scheduler.acquire("session-1", 900)
            self.assertEqual(GpuState.SPEECH.value, acquired["state"])
            self.assertEqual(1, acquired["speech_leases"])

            ready[0] = False
            released = scheduler.release("session-1")
            self.assertEqual(GpuState.CHAT.value, released["state"])
            commands = [call.args for call in run.call_args_list]
            self.assertIn(("systemctl", "start", "voiceos-moshi.service"), commands)
            self.assertIn(("systemctl", "restart", "voiceos-model-warm.service"), commands)

    def test_failed_speech_transition_rolls_back_and_enters_failed_state(self) -> None:
        with (
            patch("ops.rig.gpu_scheduler._port_ready", return_value=False),
            patch("ops.rig.gpu_scheduler._service_active", return_value=False),
            patch("ops.rig.gpu_scheduler._run"),
        ):
            scheduler = Scheduler()
            with self.assertRaisesRegex(RuntimeError, "speech_transition_failed"):
                scheduler.acquire("session-1", 900)
            status = scheduler.dispatch({"action": "status"})
            self.assertEqual(GpuState.FAILED.value, status["state"])
            self.assertFalse(status["ok"])
            self.assertEqual(0, status["speech_leases"])

            scheduler.maintain_once()
            self.assertEqual(GpuState.CHAT.value, scheduler.dispatch({"action": "status"})["state"])

    def test_renew_extends_only_an_existing_speech_lease(self) -> None:
        ready = [False]
        with (
            patch("ops.rig.gpu_scheduler._port_ready", side_effect=lambda *_: ready[0]),
            patch("ops.rig.gpu_scheduler._service_active", return_value=False),
            patch("ops.rig.gpu_scheduler._run"),
            patch("ops.rig.gpu_scheduler._unload_ollama"),
        ):
            scheduler = Scheduler()
            ready[0] = True
            scheduler.acquire("session-1", 30)
            renewed = scheduler.renew("session-1", 600)
            self.assertEqual(1, renewed["speech_leases"])
            with self.assertRaisesRegex(ValueError, "not_found"):
                scheduler.renew("missing", 600)


if __name__ == "__main__":
    unittest.main()
