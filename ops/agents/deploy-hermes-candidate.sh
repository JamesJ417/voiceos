#!/usr/bin/env bash
set -euo pipefail

action="${1:?deploy, health-check, or rollback required}"
candidate_root="${2:?managed candidate directory required}"
[[ "$candidate_root" == /var/lib/voiceos/update-candidates/hermes/* ]] || { echo "candidate outside managed root" >&2; exit 2; }
[[ -f "$candidate_root/proposal.json" ]] || { echo "proposal evidence missing" >&2; exit 2; }
install_root=/opt/voiceos/hermes
rollback_root=/var/lib/voiceos/hermes-rollbacks
version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["proposed_version"])' "$candidate_root/proposal.json")"
case "$action" in
  deploy)
    [[ -f "$candidate_root/CANDIDATE_READY" ]] || { echo "candidate was not staged" >&2; exit 2; }
    install -d -m 0750 "$rollback_root"
    rollback="$rollback_root/$(date -u +%Y%m%dT%H%M%SZ)"
    cp -a "$install_root" "$rollback"
    systemctl stop voiceos-hermes
    trap 'systemctl start voiceos-hermes || true' EXIT
    git -C "$install_root" fetch --depth 1 origin "$version"
    git -C "$install_root" checkout --detach "$version"
    uv sync --locked --project "$install_root" --python "$install_root/.venv/bin/python" --no-dev --extra messaging
    # Skills are intentionally not copied. The skill worker must quarantine and approve them separately.
    systemctl start voiceos-hermes
    curl -fsS --max-time 20 http://127.0.0.1:8642/health >/dev/null
    trap - EXIT
    ;;
  health-check) curl -fsS --max-time 20 http://127.0.0.1:8642/health ;;
  rollback)
    rollback="$(find "$rollback_root" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2-)"
    [[ -n "$rollback" ]] || { echo "no rollback snapshot" >&2; exit 2; }
    systemctl stop voiceos-hermes
    mv "$install_root" "$install_root.failed.$(date -u +%s)"
    cp -a "$rollback" "$install_root"
    systemctl start voiceos-hermes
    curl -fsS --max-time 20 http://127.0.0.1:8642/health >/dev/null
    ;;
  *) echo "unsupported action" >&2; exit 2 ;;
esac
