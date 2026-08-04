from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from services.gateway.agent_bus import BuzzCliAgentBus


class BuzzCliAgentBusTest(unittest.TestCase):
    def test_uses_argv_json_contract_without_a_shell(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            key_file = Path(temporary_directory) / "buzz.key"
            key_file.write_text("nsec-test", encoding="utf-8")
            bus = BuzzCliAgentBus(
                executable="/opt/voiceos/bin/buzz",
                relay_url="http://127.0.0.1:3000",
                private_key_file=str(key_file),
            )
            with patch("services.gateway.agent_bus.subprocess.run") as run:
                run.return_value.returncode = 0
                run.return_value.stdout = json.dumps({"id": "event-1"})
                run.return_value.stderr = ""
                receipt = bus.publish("channel-1", "Agent result")
            self.assertEqual("event-1", receipt.event_id)
            command = run.call_args.args[0]
            self.assertEqual(
                [
                    "/opt/voiceos/bin/buzz",
                    "--format",
                    "json",
                    "messages",
                    "send",
                    "--channel",
                    "channel-1",
                    "--content",
                    "Agent result",
                ],
                command,
            )
            self.assertFalse(run.call_args.kwargs["shell"])
            self.assertEqual("nsec-test", run.call_args.kwargs["env"]["BUZZ_PRIVATE_KEY"])


if __name__ == "__main__":
    unittest.main()
