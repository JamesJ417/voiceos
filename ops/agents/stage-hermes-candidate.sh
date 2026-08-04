#!/usr/bin/env bash
set -euo pipefail

proposal_file="${1:?proposal.json is required}"
candidate_root="$(dirname "$(realpath "$proposal_file")")"
repo="$candidate_root/repo"
[[ "$candidate_root" == /var/lib/voiceos/update-candidates/hermes/* ]] || { echo "candidate outside managed root" >&2; exit 2; }
[[ -f "$proposal_file" && -d "$repo/.git" ]] || { echo "invalid candidate" >&2; exit 2; }
proposed="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["proposed_version"])' "$proposal_file")"
[[ "$(git -C "$repo" rev-parse HEAD)" == "$proposed" ]] || { echo "candidate commit mismatch" >&2; exit 2; }
python3 - "$repo" "$candidate_root/skills-manifest.json" <<'PY'
import hashlib,json,pathlib,sys
root=pathlib.Path(sys.argv[1]); out={}
for path in sorted((root/'skills').glob('**/SKILL.md')):
    out[path.relative_to(root/'skills').as_posix()]=hashlib.sha256(path.read_bytes()).hexdigest()
pathlib.Path(sys.argv[2]).write_text(json.dumps(out,indent=2))
PY
export UV_PYTHON_INSTALL_DIR="$candidate_root/python"
uv python install 3.12
python_interpreter="$(uv python find --python-preference only-managed 3.12)"
uv venv --clear --python "$python_interpreter" "$candidate_root/.venv"
uv sync --locked --project "$repo" --python "$candidate_root/.venv/bin/python" --no-dev --extra messaging
"$candidate_root/.venv/bin/python" -m compileall -q "$repo"
skill_token="$(tr -d '\r\n' </etc/voiceos/hermes-skill-worker.key)"
curl -fsS --max-time 120 \
  -H "Authorization: Bearer $skill_token" -H 'Content-Type: application/json' \
  --data "$(python3 -c 'import json,sys; print(json.dumps({"run_id":"hermes-update:"+sys.argv[1],"candidate_skills_root":sys.argv[2]}))' "$proposed" "$repo/skills")" \
  http://127.0.0.1:8794/v1/scan-candidate >"$candidate_root/skill-proposals.json"
touch "$candidate_root/CANDIDATE_READY"
echo "Candidate staged without changing production: $candidate_root"
