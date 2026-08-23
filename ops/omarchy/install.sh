#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 [--enable] [--repo /absolute/path/to/voiceos]" >&2
}

enable=0
repo_root=""
while (($#)); do
  case "$1" in
    --enable) enable=1; shift ;;
    --repo)
      [[ $# -ge 2 ]] || { usage; exit 2; }
      repo_root="$2"
      shift 2
      ;;
    *) usage; exit 2 ;;
  esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
default_repo_root="$(realpath "$script_dir/../..")"
repo_root="${repo_root:-$default_repo_root}"
repo_root="$(realpath "$repo_root")"

if [[ ! -f "$repo_root/services/gateway/server.py" ]]; then
  echo "Not an Omarchy Voice repository: $repo_root" >&2
  exit 2
fi
if [[ "$repo_root" == *'|'* || "$repo_root" == *$'\n'* ]]; then
  echo "Repository path cannot contain a pipe or newline." >&2
  exit 2
fi

python_bin="$(command -v python3 || true)"
if [[ -z "$python_bin" ]]; then
  echo "Python 3.11 or newer is required." >&2
  exit 1
fi
python_version="$($python_bin -c 'import sys; print(sys.version_info.major * 100 + sys.version_info.minor)')"
if ((python_version < 311)); then
  echo "Python 3.11 or newer is required." >&2
  exit 1
fi

codex_bin="$(command -v codex || true)"
if [[ -z "$codex_bin" ]]; then
  echo "The Codex CLI is required for the remote-brain service." >&2
  exit 1
fi
codex_dir="$(dirname "$codex_bin")"

hermes_bin="$(command -v hermes || true)"
if [[ -z "$hermes_bin" ]]; then
  echo "Hermes Agent is required for VIC's agent runtime." >&2
  exit 1
fi
hermes_dir="$(dirname "$hermes_bin")"

npm_bin="$(command -v npm || true)"
if [[ -z "$npm_bin" ]]; then
  echo "Node.js and npm are required for the Omarchy Voice talk interface." >&2
  exit 1
fi
npm_dir="$(dirname "$npm_bin")"

wake_venv="${XDG_DATA_HOME:-$HOME/.local/share}/voiceos/wake-venv"
if [[ ! -x "$wake_venv/bin/python" ]]; then
  "$python_bin" -m venv "$wake_venv"
fi
"$wake_venv/bin/python" -m pip install --disable-pip-version-check \
  --requirement "$repo_root/services/wake_bridge/requirements.txt"

rust_gateway="$repo_root/target/release/voiceos-gateway"
if [[ ! -x "$rust_gateway" ]]; then
  cargo_bin="$(command -v cargo || true)"
  if [[ -z "$cargo_bin" ]]; then
    echo "The task service is not built. Run ops/omarchy/setup.sh first." >&2
    exit 1
  fi
  "$cargo_bin" build --release --locked --manifest-path "$repo_root/Cargo.toml" -p voiceos-gateway
fi

if [[ ! -d "$repo_root/apps/kiosk/node_modules" ]]; then
  "$npm_bin" ci --prefix "$repo_root/apps/kiosk"
fi
if [[ ! -d "$repo_root/apps/kiosk/dist" ]]; then
  "$npm_bin" run build --prefix "$repo_root/apps/kiosk"
fi

config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/voiceos"
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/voiceos"
unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"

install -d -m 0700 "$config_dir" "$state_dir"
install -d -m 0755 "$unit_dir" "$bin_dir"
if [[ ! -f "$config_dir/gateway.env" ]]; then
  sed -e "s|%h|$HOME|g" -e "s|%t|${XDG_RUNTIME_DIR:-/run/user/$(id -u)}|g" \
    "$script_dir/gateway.env.example" >"$config_dir/gateway.env"
  chmod 0600 "$config_dir/gateway.env"
fi
"$python_bin" - "$config_dir/gateway.env" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
lines = path.read_text(encoding="utf-8").splitlines()
key = "VOICEOS_MEMORY_URL"
replacement = f"{key}=http://127.0.0.1:8790"
found = False
for index, line in enumerate(lines):
    if line.startswith(f"{key}="):
        found = True
        if not line.partition("=")[2].strip():
            lines[index] = replacement
if not found:
    lines.append(replacement)
path.write_text("\n".join(lines) + "\n", encoding="utf-8")
PY
if [[ ! -f "$config_dir/hermes-api.key" ]]; then
  umask 077
  python3 -c 'import secrets; print(secrets.token_hex(32))' >"$config_dir/hermes-api.key"
fi
chmod 0600 "$config_dir/hermes-api.key"
if [[ ! -f "$config_dir/hermes.env" ]]; then
  printf 'API_SERVER_KEY=%s\n' "$(tr -d '\r\n' <"$config_dir/hermes-api.key")" >"$config_dir/hermes.env"
fi
chmod 0600 "$config_dir/hermes.env"
sed -e "s|@@REPO_ROOT@@|$repo_root|g" -e "s|@@PYTHON@@|$python_bin|g" \
  "$script_dir/voiceos-gateway.service.template" >"$unit_dir/voiceos-gateway.service"
sed -e "s|@@REPO_ROOT@@|$repo_root|g" -e "s|@@HERMES@@|$hermes_bin|g" \
  -e "s|@@HERMES_PATH@@|$hermes_dir|g" \
  "$script_dir/voiceos-hermes.service.template" >"$unit_dir/voiceos-hermes.service"
sed -e "s|@@REPO_ROOT@@|$repo_root|g" -e "s|@@PYTHON@@|$python_bin|g" \
  -e "s|@@CODEX@@|$codex_bin|g" -e "s|@@CODEX_PATH@@|$codex_dir|g" \
  "$script_dir/voiceos-codex.service.template" >"$unit_dir/voiceos-codex.service"
sed -e "s|@@REPO_ROOT@@|$repo_root|g" -e "s|@@NPM@@|$npm_bin|g" \
  -e "s|@@NODE_PATH@@|$npm_dir|g" \
  "$script_dir/voiceos-ui.service.template" >"$unit_dir/voiceos-ui.service"
sed -e "s|@@REPO_ROOT@@|$repo_root|g" -e "s|@@RUST_GATEWAY@@|$rust_gateway|g" \
  "$script_dir/voiceos-core.service.template" >"$unit_dir/voiceos-core.service"
sed -e "s|@@REPO_ROOT@@|$repo_root|g" -e "s|@@WAKE_PYTHON@@|$wake_venv/bin/python|g" \
  "$script_dir/voiceos-wake.service.template" >"$unit_dir/voiceos-wake.service"
chmod 0644 "$unit_dir/voiceos-gateway.service"
chmod 0644 "$unit_dir/voiceos-hermes.service"
chmod 0644 "$unit_dir/voiceos-codex.service"
chmod 0644 "$unit_dir/voiceos-ui.service"
chmod 0644 "$unit_dir/voiceos-core.service"
chmod 0644 "$unit_dir/voiceos-wake.service"

install -m 0755 "$script_dir/voiceosctl" "$bin_dir/voiceosctl"
install -m 0755 "$script_dir/voiceos-talk" "$bin_dir/voiceos-talk"
install -m 0755 "$script_dir/doctor.sh" "$bin_dir/omarchy-voice-doctor"

applications_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
install -d -m 0755 "$applications_dir"
desktop_file="$applications_dir/omarchy-voice.desktop"
{
  printf '%s\n' '[Desktop Entry]'
  printf '%s\n' 'Type=Application'
  printf '%s\n' 'Name=Omarchy Voice'
  printf '%s\n' 'Comment=Talk to VIC through Hermes and Codex'
  printf 'Exec=%s\n' "$bin_dir/voiceos-talk"
  printf '%s\n' 'Icon=audio-input-microphone'
  printf '%s\n' 'Terminal=false'
  printf '%s\n' 'Categories=Utility;Audio;'
  printf '%s\n' 'Keywords=VIC;voice;assistant;Hermes;Codex;'
} >"$desktop_file"

menu_file="${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/extensions/omarchy-menu.jsonc"
install -d -m 0755 "$(dirname "$menu_file")"
if [[ ! -f "$menu_file" ]]; then
  printf '{}\n' >"$menu_file"
fi
"$python_bin" - "$menu_file" <<'PY'
from pathlib import Path
import re
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
if '"omarchy-voice"' not in text:
    closing = text.rfind("}")
    if closing < 0:
        raise SystemExit(f"Omarchy menu file has no closing object: {path}")
    body = text[:closing]
    without_comments = re.sub(r"//.*", "", body).strip()
    comma = "," if without_comments not in {"", "{"} and not body.rstrip().endswith(",") else ""
    entry = (
        f'{comma}\n  "omarchy-voice": {{\n'
        '    "icon": "󰍬",\n'
        '    "label": "Omarchy Voice",\n'
        '    "action": "voiceos-talk",\n'
        '    "aliases": ["vic", "voice"],\n'
        '    "description": "Talk to VIC"\n'
        '  }\n'
    )
    path.write_text(body.rstrip() + entry + text[closing:], encoding="utf-8")
PY

systemctl --user daemon-reload
omarchy menu refresh >/dev/null

if ((enable)); then
  systemctl --user enable voiceos-core.service voiceos-hermes.service voiceos-codex.service voiceos-gateway.service voiceos-ui.service voiceos-wake.service
  systemctl --user restart voiceos-core.service
  for _ in {1..40}; do
    if curl --fail --silent http://127.0.0.1:8790/v1/health >/dev/null; then
      break
    fi
    sleep 0.25
  done
  curl --fail --silent --show-error http://127.0.0.1:8790/v1/health >/dev/null
  systemctl --user restart voiceos-hermes.service voiceos-codex.service voiceos-gateway.service voiceos-ui.service voiceos-wake.service
  "$bin_dir/voiceosctl" wait
else
  echo "Omarchy Voice integration installed but not started."
  echo "Review $config_dir/gateway.env, then run:"
  echo "  systemctl --user enable --now voiceos-gateway.service"
fi
