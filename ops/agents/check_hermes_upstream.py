#!/usr/bin/env python3
"""Discover Hermes updates and create inert VoiceOS proposals; never mutates production."""
from __future__ import annotations

import argparse, hashlib, json, subprocess
from datetime import UTC, datetime
from pathlib import Path
from urllib.request import Request, urlopen


def run(command: list[str], *, cwd: Path | None = None) -> str:
    return subprocess.run(command, cwd=cwd, check=True, text=True, capture_output=True).stdout.strip()

def optional_check(command: list[str]) -> dict[str, object]:
    if not Path(command[0]).exists(): return {"available": False}
    result=subprocess.run(command,text=True,capture_output=True,check=False,timeout=120)
    return {"available":True,"exit_code":result.returncode,"output":(result.stdout or result.stderr).strip()[:20000]}


def skill_manifest(root: Path) -> dict[str, str]:
    result: dict[str, str] = {}
    for path in sorted(root.glob("**/SKILL.md")) if root.exists() else []:
        result[path.relative_to(root).as_posix()] = hashlib.sha256(path.read_bytes()).hexdigest()
    return result


def changes(current: dict[str, str], proposed: dict[str, str]) -> dict[str, object]:
    return {
        "added": sorted(set(proposed) - set(current)),
        "removed": sorted(set(current) - set(proposed)),
        "changed": sorted(name for name in set(current) & set(proposed) if current[name] != proposed[name]),
        "hashes": proposed,
        "activation": "quarantined_pending_voiceos_approval",
    }


def discover(lock_path: Path, installed_root: Path, candidate_root: Path) -> dict[str, object] | None:
    lock = json.loads(lock_path.read_text(encoding="utf-8"))["dependencies"]["hermes-agent"]
    current = str(lock["commit"])
    repository = str(lock["repository"])
    proposed = run(["git", "ls-remote", repository, "HEAD"]).split()[0]
    official_check=optional_check([str(installed_root/".venv"/"bin"/"hermes"),"update","--check"])
    if proposed == current:
        return None
    destination = candidate_root / proposed
    repo = destination / "repo"
    destination.mkdir(parents=True, exist_ok=True)
    if not (repo / ".git").exists():
        run(["git", "clone", "--filter=blob:none", "--no-checkout", repository, str(repo)])
    run(["git", "-C", str(repo), "fetch", "--depth", "1", "origin", current])
    run(["git", "-C", str(repo), "fetch", "--depth", "1", "origin", proposed])
    run(["git", "-C", str(repo), "checkout", "--detach", proposed])
    changed_files = run(["git", "-C", str(repo), "diff", "--name-only", current, proposed]).splitlines()
    dependency_files = [name for name in changed_files if Path(name).name in {"pyproject.toml", "uv.lock", "requirements.txt"}]
    api_files = [name for name in changed_files if any(part in name.casefold() for part in ("gateway", "api", "sse", "event"))]
    config_files = [name for name in changed_files if any(part in name.casefold() for part in ("config", "migration", "schema"))]
    security_files = [name for name in changed_files if any(part in name.casefold() for part in ("security", "auth", "permission", "advisory"))]
    current_skills = skill_manifest(installed_root / "skills")
    proposed_skills = skill_manifest(repo / "skills")
    skill_delta = changes(current_skills, proposed_skills)
    report = {
        "component": "hermes-agent", "current_version": current, "proposed_version": proposed,
        "release_notes": f"Upstream HEAD changed from {current[:12]} to {proposed[:12]}.",
        "dependency_changes": dependency_files, "api_changes": api_files,
        "configuration_changes": config_files, "skill_changes": skill_delta,
        "security_changes": security_files,
        "affected_components": ["hermes-provider", "gateway", "skill-control", "provider-routing"],
        "rollback_version": current, "candidate_path": str(destination),
        "evidence": {"repository": repository, "hermes_update_check":official_check, "changed_files": changed_files, "checked_at": datetime.now(UTC).isoformat(), "production_changed": False},
    }
    (destination / "proposal.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    return report


def post(url: str, report: dict[str, object]) -> None:
    request = Request(url.rstrip("/") + "/internal/v1/updates/discover", data=json.dumps(report).encode(), headers={"Content-Type": "application/json"}, method="POST")
    with urlopen(request, timeout=15) as response:
        response.read()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--lock", type=Path, default=Path("/opt/voiceos/ops/agents/vendor-lock.json"))
    parser.add_argument("--installed-root", type=Path, default=Path("/opt/voiceos/hermes"))
    parser.add_argument("--candidate-root", type=Path, default=Path("/var/lib/voiceos/update-candidates/hermes"))
    parser.add_argument("--gateway", default="http://127.0.0.1:8788")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = discover(args.lock, args.installed_root, args.candidate_root)
    if report is None:
        print(json.dumps({"status": "current"}))
        return 0
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2), encoding="utf-8")
    post(args.gateway, report)
    print(json.dumps({"status": "proposal_created", "proposed_version": report["proposed_version"]}))
    return 0


if __name__ == "__main__": raise SystemExit(main())
