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
for existing in \
  /opt/voiceos/bin/voiceos-gateway \
  /etc/systemd/system/voiceos-rust.service \
  /etc/systemd/system/voiceos-gateway.service \
  /etc/systemd/system/voiceos-connectors.service; do
  if sudo test -f "$existing"; then
    sudo cp --preserve=mode,timestamps "$existing" "$rollback_root/$(basename "$existing")"
  fi
done

rollback_release() {
  local failed_line="$1"
  trap - ERR
  set +e
  echo "Release failed near line $failed_line; restoring snapshot $rollback_root" >&2
  sudo test -f "$rollback_root/voiceos-gateway" \
    && sudo install -o root -g root -m 0755 "$rollback_root/voiceos-gateway" /opt/voiceos/bin/voiceos-gateway
  for unit in voiceos-rust.service voiceos-gateway.service voiceos-connectors.service; do
    sudo test -f "$rollback_root/$unit" \
      && sudo install -o root -g root -m 0644 "$rollback_root/$unit" "/etc/systemd/system/$unit"
  done
  sudo systemctl daemon-reload
  sudo systemctl restart voiceos-rust.service voiceos-connectors.service voiceos-gateway.service
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

"$repo_root/ops/rig/install-gateway-service.sh" "$repo_root"
sudo install -d -o voiceos -g voiceos -m 0750 /var/lib/voiceos-rust /var/lib/voiceos/artifacts
sudo install -d -o voiceos -g voiceos -m 0750 /var/lib/voiceos/update-candidates
sudo install -d -o root -g root -m 0755 /opt/voiceos/bin
sudo install -o root -g root -m 0755 \
  "$repo_root/target/release/voiceos-gateway" /opt/voiceos/bin/voiceos-gateway
if [[ ! -f /etc/voiceos/rust.env ]]; then
  sudo install -o root -g voiceos -m 0640 \
    "$repo_root/ops/rig/voiceos-rust.env.example" /etc/voiceos/rust.env
fi
sed "s|@@REPO_ROOT@@|$repo_root|g" "$repo_root/ops/rig/voiceos-rust.service.template" \
  | sudo tee /etc/systemd/system/voiceos-rust.service >/dev/null

# Refresh only the safe upstream-monitor executables and units. This does not
# update the live Hermes runtime or activate any changed upstream skill.
sudo install -o root -g root -m 0755 "$repo_root/ops/agents/check_hermes_upstream.py" /opt/voiceos/bin/check-hermes-upstream
sudo install -o root -g root -m 0755 "$repo_root/ops/agents/stage-hermes-candidate.sh" /opt/voiceos/bin/stage-hermes-candidate
sudo install -o root -g root -m 0755 "$repo_root/ops/agents/deploy-hermes-candidate.sh" /opt/voiceos/bin/deploy-hermes-candidate
sudo install -o root -g root -m 0644 "$repo_root/ops/systemd/voiceos-hermes-update-check.service" /etc/systemd/system/
sudo install -o root -g root -m 0644 "$repo_root/ops/systemd/voiceos-hermes-update-check.timer" /etc/systemd/system/

sudo systemctl daemon-reload
sudo systemctl enable --now voiceos-rust.service voiceos-connectors.service voiceos-hermes-update-check.timer
sudo systemctl restart voiceos-rust.service voiceos-connectors.service voiceos-gateway.service

curl --fail --silent --show-error http://127.0.0.1:8790/v1/health >/dev/null
curl --fail --silent --show-error http://127.0.0.1:8793/v1/health >/dev/null
curl --fail --silent --show-error http://127.0.0.1:8787/v1/health >/dev/null
binary_sha="$(sha256sum /opt/voiceos/bin/voiceos-gateway | awk '{print $1}')"
printf '{"release_id":"%s","status":"healthy","binary_sha256":"%s","rollback_root":"%s"}\n' \
  "$release_id" "$binary_sha" "$rollback_root" \
  | sudo tee "$rollback_root/result.json" >/dev/null
trap - ERR
echo "VoiceOS release applied and all local health checks passed."
