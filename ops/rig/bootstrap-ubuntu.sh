#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "This bootstrap supports Linux only." >&2
  exit 2
fi

if [[ ! -r /etc/os-release ]]; then
  echo "Cannot identify the Linux distribution." >&2
  exit 2
fi

source /etc/os-release
if [[ "${ID:-}" != "ubuntu" ]]; then
  echo "This conservative bootstrap expects Ubuntu; detected ${ID:-unknown}." >&2
  exit 2
fi

SUDO=()
if [[ "$(id -u)" -ne 0 ]]; then
  command -v sudo >/dev/null || { echo "sudo is required." >&2; exit 2; }
  SUDO=(sudo)
fi

echo "Installing base VoiceOS runtime packages on Ubuntu ${VERSION_ID:-unknown}..."
"${SUDO[@]}" apt-get update
"${SUDO[@]}" apt-get install -y ca-certificates curl git jq python3 python3-venv

if ! command -v nvidia-smi >/dev/null; then
  cat >&2 <<'EOF'
NVIDIA is not ready. This script will not guess at or replace GPU drivers.
Install the current NVIDIA open driver using NVIDIA's Ubuntu instructions,
reboot, and confirm `nvidia-smi` before rerunning this bootstrap.
EOF
  exit 20
fi
nvidia-smi >/dev/null

temporary_dir="$(mktemp -d)"
trap 'rm -rf -- "$temporary_dir"' EXIT

if ! command -v ollama >/dev/null; then
  echo "Downloading the official Ollama installer..."
  curl --fail --silent --show-error --location https://ollama.com/install.sh \
    --output "$temporary_dir/ollama-install.sh"
  "${SUDO[@]}" sh "$temporary_dir/ollama-install.sh"
fi

if ! command -v tailscale >/dev/null; then
  echo "Downloading the official Tailscale installer..."
  curl --fail --silent --show-error --location https://tailscale.com/install.sh \
    --output "$temporary_dir/tailscale-install.sh"
  "${SUDO[@]}" sh "$temporary_dir/tailscale-install.sh"
fi

"${SUDO[@]}" systemctl enable --now ollama
"${SUDO[@]}" systemctl enable --now tailscaled

echo
echo "Runtime installation complete. Remaining interactive steps:"
echo "  1. Run: sudo tailscale up"
echo "  2. Pull a tool-capable model with: ollama pull MODEL_NAME"
echo "  3. Run: python3 ops/rig/diagnose.py --json"
