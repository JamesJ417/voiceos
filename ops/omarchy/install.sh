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
chmod 0644 "$unit_dir/voiceos-gateway.service"
chmod 0644 "$unit_dir/voiceos-hermes.service"
chmod 0644 "$unit_dir/voiceos-codex.service"
chmod 0644 "$unit_dir/voiceos-ui.service"

install -m 0755 "$script_dir/voiceosctl" "$bin_dir/voiceosctl"
install -m 0755 "$script_dir/voiceos-talk" "$bin_dir/voiceos-talk"
systemctl --user daemon-reload

if ((enable)); then
  systemctl --user enable --now voiceos-hermes.service voiceos-codex.service voiceos-gateway.service voiceos-ui.service
  "$bin_dir/voiceosctl" wait
else
  echo "Omarchy Voice integration installed but not started."
  echo "Review $config_dir/gateway.env, then run:"
  echo "  systemctl --user enable --now voiceos-gateway.service"
fi
