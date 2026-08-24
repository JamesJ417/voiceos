from __future__ import annotations

import unittest
from pathlib import Path

from services.gateway.hermes_skill_worker import validate_skill
from services.gateway.master_prompt import master_system_prompt


ROOT = Path(__file__).resolve().parents[3]


class VicIdentityTest(unittest.TestCase):
    def test_master_charter_names_vic_and_preserves_voiceos_scope(self) -> None:
        prompt = master_system_prompt()
        self.assertIn("You are VIC", prompt)
        self.assertIn("Voice Interface Controller", prompt)
        self.assertIn("voice interface to VoiceOS", prompt)
        self.assertIn("Model output is reasoning, not authority", prompt)

    def test_soul_is_identity_focused_and_names_runtime_boundary(self) -> None:
        soul = (ROOT / "contracts" / "VIC-SOUL.md").read_text(encoding="utf-8")
        self.assertIn("VIC, the Voice Interface Controller", soul)
        self.assertIn("Hermes is your agent runtime", soul)
        self.assertNotIn("127.0.0.1", soul)
        self.assertNotIn("systemctl", soul)

    def test_vic_coordination_skill_passes_quarantine_validator(self) -> None:
        content = (
            ROOT / "ops" / "agents" / "vic-voiceos-skill" / "SKILL.md"
        ).read_bytes()
        validation = validate_skill(content)
        self.assertTrue(validation["passed"], validation["errors"])
        self.assertEqual("vic-voiceos-coordination", validation["name"])


if __name__ == "__main__":
    unittest.main()
