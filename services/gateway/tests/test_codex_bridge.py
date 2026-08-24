from __future__ import annotations

import os
import subprocess
import unittest
from unittest.mock import patch

from services.codex_bridge.server import run_codex


class CodexBridgeCommandTest(unittest.TestCase):
    @patch("services.codex_bridge.server.subprocess.run")
    def test_fixed_read_only_ephemeral_invocation(self, run) -> None:
        run.return_value = subprocess.CompletedProcess([], 0, "Sol answer\n", "")
        with patch.dict(os.environ, {"HOME": "/home/llm", "SECRET": "do-not-pass"}, clear=True):
            answer = run_codex(
                "Use Codex to review this.",
                executable="/home/llm/.local/bin/codex",
                model="gpt-5.6-sol",
                reasoning_effort="high",
                workdir="/var/lib/voiceos-codex/work",
                timeout_seconds=330,
            )
        self.assertEqual("Sol answer", answer)
        command = run.call_args.args[0]
        self.assertIn("--ephemeral", command)
        self.assertIn("--ignore-user-config", command)
        self.assertIn("--ignore-rules", command)
        self.assertEqual("read-only", command[command.index("--sandbox") + 1])
        disabled = [
            command[index + 1]
            for index, value in enumerate(command)
            if value == "--disable"
        ]
        self.assertEqual(
            ["shell_tool", "unified_exec", "multi_agent", "apps", "hooks"], disabled
        )
        self.assertIn("web_search=disabled", command)
        self.assertEqual("gpt-5.6-sol", command[command.index("-m") + 1])
        config_values = [
            command[index + 1] for index, value in enumerate(command) if value == "-c"
        ]
        self.assertIn("model_reasoning_effort=high", config_values)
        self.assertEqual({"HOME": "/home/llm"}, run.call_args.kwargs["env"])
        self.assertNotIn("Use Codex to review this.", command)
        self.assertIn("Use Codex to review this.", run.call_args.kwargs["input"])
        self.assertIn("VoiceOS Master Charter", run.call_args.kwargs["input"])


if __name__ == "__main__":
    unittest.main()
