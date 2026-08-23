#!/usr/bin/env bash
set -euo pipefail

readonly HERMES_INSTALLER_URL="https://hermes-agent.nousresearch.com/install.sh"
readonly CODEX_INSTALLER_URL="https://chatgpt.com/codex/install.sh"
readonly WHISPER_MODEL_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"
readonly WHISPER_MODEL_SHA256="a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002"

usage() {
  cat <<'EOF'
Usage: ops/omarchy/setup.sh [--non-interactive] [--skip-tailscale]

Installs every Omarchy Voice dependency, configures VIC, builds the interface,
and enables the user services. Run this from a cloned Omarchy Voice repository.
EOF
}

non_interactive=0
skip_tailscale=0
while (($#)); do
  case "$1" in
    --non-interactive) non_interactive=1 ;;
    --skip-tailscale) skip_tailscale=1 ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; exit 2 ;;
  esac
  shift
done

if [[ $EUID -eq 0 ]]; then
  echo "Run this installer as your normal Omarchy user, not as root." >&2
  exit 1
fi
if ! command -v omarchy >/dev/null || [[ ! -d /usr/share/omarchy ]]; then
  echo "Omarchy Voice requires an installed Omarchy system." >&2
  exit 1
fi
if ! systemctl --user show-environment >/dev/null 2>&1; then
  echo "A running Omarchy desktop user session is required." >&2
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(realpath "$script_dir/../..")"
temp_dir="$(mktemp -d)"
trap 'rm -rf -- "$temp_dir"' EXIT

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

step "Installing Omarchy Voice system packages"
omarchy pkg add git curl python nodejs npm ffmpeg whisper-cpp tailscale jq
if ! command -v google-chrome-stable >/dev/null; then
  omarchy pkg aur add google-chrome
fi

step "Installing Hermes Agent"
if ! command -v hermes >/dev/null; then
  curl --fail --location --silent --show-error "$HERMES_INSTALLER_URL" \
    --output "$temp_dir/hermes-install.sh"
  bash "$temp_dir/hermes-install.sh" --skip-setup
fi

step "Installing the OpenAI Codex CLI"
if ! command -v codex >/dev/null; then
  curl --fail --location --silent --show-error "$CODEX_INSTALLER_URL" \
    --output "$temp_dir/codex-install.sh"
  bash "$temp_dir/codex-install.sh"
fi

export PATH="$HOME/.local/bin:$HOME/.codex/bin:$PATH"
command -v hermes >/dev/null || { echo "Hermes was installed but is not on PATH." >&2; exit 1; }
command -v codex >/dev/null || { echo "Codex was installed but is not on PATH." >&2; exit 1; }

step "Installing the verified English speech-recognition model"
model_dir="${XDG_DATA_HOME:-$HOME/.local/share}/voiceos/models"
model_path="$model_dir/ggml-base.en.bin"
install -d -m 0755 "$model_dir"
if [[ ! -f "$model_path" ]] || ! echo "$WHISPER_MODEL_SHA256  $model_path" | sha256sum --check --status; then
  curl --fail --location --show-error "$WHISPER_MODEL_URL" --output "$temp_dir/ggml-base.en.bin"
  echo "$WHISPER_MODEL_SHA256  $temp_dir/ggml-base.en.bin" | sha256sum --check
  install -m 0644 "$temp_dir/ggml-base.en.bin" "$model_path"
fi

step "Building the Omarchy Voice interface"
npm ci --prefix "$repo_root/apps/kiosk"
npm run build --prefix "$repo_root/apps/kiosk"

if ((non_interactive == 0)); then
  if ! codex login status >/dev/null 2>&1; then
    step "Sign in to Codex with your ChatGPT account"
    codex login
  fi
  if [[ ! -f "$HOME/.hermes/config.yaml" ]]; then
    step "Choose the remote model provider Hermes will use for VIC"
    hermes setup
  fi
fi

step "Installing desktop integration and services"
"$script_dir/install.sh" --repo "$repo_root" --enable

if ((skip_tailscale == 0)); then
  step "Enabling private phone access through Tailscale"
  sudo systemctl enable --now tailscaled
  if ! tailscale status >/dev/null 2>&1; then
    if ((non_interactive)); then
      echo "Run 'sudo tailscale up', then 'tailscale serve --bg --yes 8787' to connect a phone."
    else
      sudo tailscale up
    fi
  fi
  if tailscale status >/dev/null 2>&1; then
    tailscale serve --bg --yes 8787
  fi
fi

step "Running final checks"
"$script_dir/doctor.sh"
printf '\nOmarchy Voice is installed. Open the Omarchy menu and choose Omarchy Voice.\n'
