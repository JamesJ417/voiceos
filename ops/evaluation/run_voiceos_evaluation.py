#!/usr/bin/env python3
"""Run VoiceOS security/reliability checks and emit an auditable JSON report."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.request
from datetime import UTC, datetime
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--project-root", type=Path, default=Path("/opt/voiceos"))
    parser.add_argument("--database", type=Path, default=Path("/var/lib/voiceos/gateway/audit.sqlite3"))
    parser.add_argument("--health-url", default="http://127.0.0.1:8787/v1/health")
    parser.add_argument("--output", type=Path, default=Path("/var/lib/voiceos/evaluations/latest.json"))
    args = parser.parse_args()

    started = time.perf_counter()
    checks: dict[str, object] = {}
    suite = subprocess.run(
        [sys.executable, "-m", "unittest", "discover", "-s", "services/gateway/tests", "-q"],
        cwd=args.project_root,
        capture_output=True,
        text=True,
        timeout=300,
        check=False,
        env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
    )
    checks["security_and_reliability_suite"] = {
        "passed": suite.returncode == 0,
        "covers": [
            "prompt_injection", "memory_poisoning", "tool_approval_bypass",
            "model_routing", "conversation_recall", "root_grant_replay_and_tampering",
        ],
        "evidence": (suite.stdout + suite.stderr)[-8_000:],
    }

    health_started = time.perf_counter()
    try:
        with urllib.request.urlopen(args.health_url, timeout=10) as response:
            health = json.load(response)
        checks["response_latency_and_service_health"] = {
            "passed": response.status == 200 and health.get("status") in {"ok", "degraded"},
            "latency_ms": round((time.perf_counter() - health_started) * 1000),
            "health": health,
        }
    except Exception as error:  # evaluation must record failures, not hide them
        checks["response_latency_and_service_health"] = {"passed": False, "error": str(error)}

    checks["backup_restoration"] = verify_backup_copy(args.database)
    checks["service_recovery_policy"] = systemd_recovery_policy()
    passed = all(bool(item.get("passed")) for item in checks.values() if isinstance(item, dict))
    report = {
        "schema": "voiceos.evaluation.v1",
        "generated_at": datetime.now(UTC).isoformat(),
        "passed": passed,
        "duration_ms": round((time.perf_counter() - started) * 1000),
        "checks": checks,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0 if passed else 1


def verify_backup_copy(database: Path) -> dict[str, object]:
    if not database.exists():
        return {"passed": False, "error": f"database_not_found:{database}"}
    with tempfile.TemporaryDirectory() as directory:
        backup = Path(directory) / "restore-test.sqlite3"
        source = sqlite3.connect(database)
        destination = sqlite3.connect(backup)
        try:
            source.backup(destination)
            integrity = destination.execute("PRAGMA integrity_check").fetchone()[0]
            tables = destination.execute(
                "SELECT count(*) FROM sqlite_master WHERE type='table'"
            ).fetchone()[0]
        finally:
            source.close()
            destination.close()
        return {"passed": integrity == "ok" and tables > 0, "integrity": integrity, "tables": tables}


def systemd_recovery_policy() -> dict[str, object]:
    completed = subprocess.run(
        ["systemctl", "show", "voiceos-gateway", "--property=Restart,RestartUSec,ActiveState"],
        capture_output=True,
        text=True,
        timeout=10,
        check=False,
    )
    evidence = completed.stdout.strip()
    return {
        "passed": completed.returncode == 0 and "Restart=" in evidence and "Restart=no" not in evidence,
        "evidence": evidence or completed.stderr.strip(),
    }


if __name__ == "__main__":
    raise SystemExit(main())
