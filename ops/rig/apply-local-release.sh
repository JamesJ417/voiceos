#!/usr/bin/env bash
set -euo pipefail

repo_root="${1:-}"
mode="${2:-apply}"
if [[ -z "$repo_root" ]]; then
  echo "Usage: $0 /absolute/path/to/voiceos-repository" >&2
  exit 2
fi
repo_root="$(realpath "$repo_root")"
[[ -f "$repo_root/Cargo.toml" && -f "$repo_root/services/gateway/server.py" ]] || {
  echo "Not a VoiceOS repository: $repo_root" >&2
  exit 2
}
for command in cargo curl python3 sudo systemctl; do
  command -v "$command" >/dev/null || { echo "Missing required command: $command" >&2; exit 2; }
done

preflight() {
  local failed=0
  echo "VoiceOS release preflight for $repo_root"
  [[ "$(uname -s)" == "Linux" ]] || { echo "FAIL: Linux is required"; failed=1; }
  [[ -r /etc/os-release ]] || { echo "FAIL: /etc/os-release is unavailable"; failed=1; }
  command -v nvidia-smi >/dev/null && nvidia-smi >/dev/null \
    && echo "PASS: NVIDIA driver" || { echo "FAIL: NVIDIA driver"; failed=1; }
  command -v tailscale >/dev/null && tailscale status --json >/dev/null \
    && echo "PASS: Tailscale client" || { echo "FAIL: Tailscale client"; failed=1; }
  command -v ollama >/dev/null && echo "PASS: Ollama executable" \
    || { echo "FAIL: Ollama executable"; failed=1; }
  local free_kib
  free_kib="$(df -Pk "$repo_root" | awk 'NR==2 {print $4}')"
  [[ "$free_kib" =~ ^[0-9]+$ && "$free_kib" -ge 5242880 ]] \
    && echo "PASS: at least 5 GiB free" || { echo "FAIL: less than 5 GiB free"; failed=1; }
  [[ ! -L "$repo_root" ]] && echo "PASS: repository root is not a symlink" \
    || { echo "FAIL: repository root cannot be a symlink"; failed=1; }
  return "$failed"
}

preflight
if [[ "$mode" == "--preflight" ]]; then
  echo "Preflight completed without changing the installation."
  exit 0
fi
[[ "$mode" == "apply" ]] || { echo "Second argument must be --preflight when supplied." >&2; exit 2; }

release_id="$(date -u +%Y%m%dT%H%M%SZ)"
rollback_root="/var/lib/voiceos/releases/$release_id"
sudo useradd --system --home-dir /var/lib/voiceos --create-home --shell /usr/sbin/nologin voiceos 2>/dev/null || true
sudo install -d -o voiceos -g voiceos -m 0750 /var/lib/voiceos /var/lib/voiceos/releases
sudo install -d -o root -g voiceos -m 0750 "$rollback_root"
managed_files=(
  /opt/voiceos/bin/voiceos-gateway \
  /opt/voiceos/bin/check-hermes-upstream \
  /opt/voiceos/bin/stage-hermes-candidate \
  /opt/voiceos/bin/deploy-hermes-candidate \
  /etc/systemd/system/voiceos-rust.service \
  /etc/systemd/system/voiceos-gateway.service \
  /etc/systemd/system/voiceos-connectors.service \
  /etc/systemd/system/voiceos-codex-supervisor.service \
  /etc/systemd/system/voiceos-model-warm.service \
  /etc/systemd/system/voiceos-codex.service \
  /etc/systemd/system/voiceos-hermes-update-check.service \
  /etc/systemd/system/voiceos-hermes-update-check.timer
)
for existing in "${managed_files[@]}"; do
  if sudo test -f "$existing"; then
    sudo cp --archive "$existing" "$rollback_root/$(basename "$existing")"
  else
    sudo touch "$rollback_root/$(basename "$existing").absent"
  fi
done

rollback_release() {
  local failed_line="$1"
  trap - ERR
  set +e
  echo "Release failed near line $failed_line; restoring snapshot $rollback_root" >&2
  for existing in "${managed_files[@]}"; do
    local backup="$rollback_root/$(basename "$existing")"
    if sudo test -f "$backup"; then
      sudo cp --archive "$backup" "$existing"
    elif sudo test -f "$backup.absent"; then
      if [[ "$existing" == /etc/systemd/system/* ]]; then
        sudo systemctl disable --now "$(basename "$existing")" 2>/dev/null || true
      fi
      sudo rm -f -- "$existing"
    fi
  done
  sudo systemctl daemon-reload
  for unit in voiceos-rust.service voiceos-connectors.service voiceos-gateway.service; do
    sudo systemctl cat "$unit" >/dev/null 2>&1 && sudo systemctl restart "$unit" || true
  done
  printf '{"release_id":"%s","status":"rolled_back","failed_line":%s}\n' "$release_id" "$failed_line" \
    | sudo tee "$rollback_root/result.json" >/dev/null
  exit 1
}
trap 'rollback_release "$LINENO"' ERR

echo "Building and testing the Rust control plane..."
cargo test --workspace --manifest-path "$repo_root/Cargo.toml"
cargo build --release --package voiceos-gateway --manifest-path "$repo_root/Cargo.toml"
python3 -m unittest discover -s "$repo_root/services/gateway/tests" -p 'test_*.py'
python3 -m unittest discover -s "$repo_root/contracts/tests" -p 'test_*.py'

bash "$repo_root/ops/rig/install-gateway-service.sh" "$repo_root"
sudo install -d -o voiceos -g voiceos -m 0750 /var/lib/voiceos-rust /var/lib/voiceos/artifacts
sudo install -d -o voiceos -g voiceos -m 0750 /var/lib/voiceos/update-candidates
sudo install -d -o root -g root -m 0755 /opt/voiceos/bin
sudo install -o root -g root -m 0755 \
  "$repo_root/target/release/voiceos-gateway" /opt/voiceos/bin/voiceos-gateway
if ! sudo test -f /etc/voiceos/rust.env; then
  sudo install -o root -g voiceos -m 0640 \
    "$repo_root/ops/rig/voiceos-rust.env.example" /etc/voiceos/rust.env
fi
sed "s|@@REPO_ROOT@@|$repo_root|g" "$repo_root/ops/rig/voiceos-rust.service.template" \
  | sudo tee /etc/systemd/system/voiceos-rust.service >/dev/null
sed "s|@@REPO_ROOT@@|$repo_root|g" "$repo_root/ops/rig/voiceos-codex-supervisor.service.template" \
  | sudo tee /etc/systemd/system/voiceos-codex-supervisor.service >/dev/null

# Refresh only the safe upstream-monitor executables and units. This does not
# update the live Hermes runtime or activate any changed upstream skill.
sudo install -o root -g root -m 0755 "$repo_root/ops/agents/check_hermes_upstream.py" /opt/voiceos/bin/check-hermes-upstream
sudo install -o root -g root -m 0755 "$repo_root/ops/agents/stage-hermes-candidate.sh" /opt/voiceos/bin/stage-hermes-candidate
sudo install -o root -g root -m 0755 "$repo_root/ops/agents/deploy-hermes-candidate.sh" /opt/voiceos/bin/deploy-hermes-candidate
sudo install -o root -g root -m 0644 "$repo_root/ops/systemd/voiceos-hermes-update-check.service" /etc/systemd/system/
sudo install -o root -g root -m 0644 "$repo_root/ops/systemd/voiceos-hermes-update-check.timer" /etc/systemd/system/

sudo systemctl daemon-reload
sudo systemctl enable voiceos-codex-supervisor.service
sudo systemctl enable --now voiceos-rust.service voiceos-connectors.service voiceos-hermes-update-check.timer
sudo systemctl restart voiceos-rust.service voiceos-connectors.service voiceos-gateway.service

wait_for_health() {
  local url="$1"
  local attempt
  for attempt in $(seq 1 30); do
    if curl --fail --silent --max-time 3 "$url" >/dev/null; then
      return 0
    fi
    sleep 2
  done
  curl --fail --silent --show-error --max-time 5 "$url" >/dev/null
}

wait_for_health http://127.0.0.1:8790/v1/health
wait_for_health http://127.0.0.1:8793/v1/health
wait_for_health http://127.0.0.1:8795/v1/health
wait_for_health http://127.0.0.1:8787/v1/health
binary_sha="$(sha256sum /opt/voiceos/bin/voiceos-gateway | awk '{print $1}')"
printf '{"release_id":"%s","status":"healthy","binary_sha256":"%s","rollback_root":"%s"}\n' \
  "$release_id" "$binary_sha" "$rollback_root" \
  | sudo tee "$rollback_root/result.json" >/dev/null
trap - ERR
echo "VoiceOS release applied and all local health checks passed."
