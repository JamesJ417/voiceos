from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from services.gateway.hermes_skill_worker import validate_skill
from services.gateway.master_prompt import master_system_prompt


ROOT = Path(__file__).resolve().parents[3]


class VicIdentityTest(unittest.TestCase):
    def tearDown(self) -> None:
        master_system_prompt.cache_clear()

    def test_master_charter_names_vic_and_preserves_voiceos_scope(self) -> None:
        prompt = master_system_prompt()
        self.assertIn("You are VIC", prompt)
        self.assertIn("Voice Interface Controller", prompt)
        self.assertIn("voice interface to VoiceOS", prompt)
        self.assertIn("Model output is reasoning, not authority", prompt)

    def test_deployment_context_is_appended_without_replacing_charter(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            context_path = Path(directory) / "deployment-context.md"
            context_path.write_text(
                "VIC is the voice interface for DOM at Brick and Copper.",
                encoding="utf-8",
            )
            with patch.dict(
                os.environ,
                {"VOICEOS_DEPLOYMENT_CONTEXT_PATH": str(context_path)},
                clear=False,
            ):
                master_system_prompt.cache_clear()
                prompt = master_system_prompt()

        self.assertIn("VoiceOS Master Charter", prompt)
        self.assertIn("Active deployment context", prompt)
        self.assertIn("voice interface for DOM at Brick and Copper", prompt)

    def test_soul_is_identity_focused_and_names_runtime_boundary(self) -> None:
        soul = (ROOT / "contracts" / "VIC-SOUL.md").read_text(encoding="utf-8")
        self.assertIn("VIC, the Voice Interface Controller", soul)
        self.assertIn("Hermes is your agent runtime", soul)
        self.assertNotIn("127.0.0.1", soul)
        self.assertNotIn("systemctl", soul)

    def test_vic_dom_profile_preserves_dom_and_data_boundaries(self) -> None:
        profile = ROOT / "ops" / "omarchy" / "profiles" / "vic-dom"
        context = (profile / "deployment-context.md").read_text(encoding="utf-8")
        soul = (profile / "SOUL.md").read_text(encoding="utf-8")
        gateway = (profile / "gateway.env.example").read_text(encoding="utf-8")
        core = (profile / "core.env.example").read_text(encoding="utf-8")

        self.assertIn("DOM", context)
        self.assertIn("Digital Operations Manager", context)
        self.assertIn("Brick and Copper", context)
        self.assertIn("Do not assume or request access to the personal VIC", context)
        self.assertIn("VIC, the Voice Interface Controller for the DOM system", soul)
        self.assertIn("VOICEOS_DEPLOYMENT_ID=vic-dom", gateway)
        self.assertIn("VOICEOS_COMPUTER_ACCESS=0", gateway)
        self.assertIn("VOICEOS_PRIMARY_OWNER_ID=brick-and-copper-dom-owner", core)

    def test_vic_coordination_skill_passes_quarantine_validator(self) -> None:
        content = (
            ROOT / "ops" / "agents" / "vic-voiceos-skill" / "SKILL.md"
        ).read_bytes()
        validation = validate_skill(content)
        self.assertTrue(validation["passed"], validation["errors"])
        self.assertEqual("vic-voiceos-coordination", validation["name"])


if __name__ == "__main__":
    unittest.main()
