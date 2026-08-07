from __future__ import annotations

import json, subprocess, tempfile, unittest
from pathlib import Path

from ops.agents.check_hermes_upstream import changes, discover, skill_manifest


def run(*command: str, cwd: Path) -> str:
    return subprocess.run(command, cwd=cwd, check=True, text=True, capture_output=True).stdout.strip()


class HermesUpdateTest(unittest.TestCase):
    def test_discovery_is_isolated_and_skills_are_quarantined(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); upstream = root / "upstream"; upstream.mkdir()
            run("git", "init", "-b", "main", cwd=upstream)
            run("git", "config", "user.email", "test@voiceos.local", cwd=upstream)
            run("git", "config", "user.name", "VoiceOS Test", cwd=upstream)
            (upstream / "skills" / "one").mkdir(parents=True)
            (upstream / "skills" / "one" / "SKILL.md").write_text("one", encoding="utf-8")
            (upstream / "gateway.py").write_text("v1", encoding="utf-8")
            run("git", "add", ".", cwd=upstream); run("git", "commit", "-m", "current", cwd=upstream)
            current = run("git", "rev-parse", "HEAD", cwd=upstream)
            (upstream / "skills" / "one" / "SKILL.md").write_text("two", encoding="utf-8")
            (upstream / "skills" / "two").mkdir()
            (upstream / "skills" / "two" / "SKILL.md").write_text("new", encoding="utf-8")
            (upstream / "gateway.py").write_text("v2", encoding="utf-8")
            run("git", "add", ".", cwd=upstream); run("git", "commit", "-m", "proposed", cwd=upstream)
            proposed = run("git", "rev-parse", "HEAD", cwd=upstream)
            lock = root / "vendor-lock.json"
            lock.write_text(json.dumps({"dependencies":{"hermes-agent":{"repository":str(upstream),"commit":current}}}), encoding="utf-8")
            installed = root / "installed"; (installed / "skills" / "one").mkdir(parents=True)
            (installed / "skills" / "one" / "SKILL.md").write_text("one", encoding="utf-8")
            report = discover(lock, installed, root / "candidates")
            self.assertIsNotNone(report); assert report is not None
            self.assertEqual(proposed, report["proposed_version"])
            self.assertFalse(report["evidence"]["production_changed"])
            self.assertEqual(["two/SKILL.md"], report["skill_changes"]["added"])
            self.assertEqual(["one/SKILL.md"], report["skill_changes"]["changed"])
            self.assertEqual("quarantined_pending_voiceos_approval", report["skill_changes"]["activation"])
            self.assertTrue((Path(report["candidate_path"]) / "proposal.json").exists())

    def test_hash_manifest_detects_changed_skill(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); (root / "skill").mkdir(); path = root / "skill" / "SKILL.md"
            path.write_text("before", encoding="utf-8"); before = skill_manifest(root)
            path.write_text("after", encoding="utf-8"); after = skill_manifest(root)
            self.assertEqual(["skill/SKILL.md"], changes(before, after)["changed"])


if __name__ == "__main__": unittest.main()
