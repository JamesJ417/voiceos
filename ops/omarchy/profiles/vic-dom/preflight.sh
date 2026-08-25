#!/usr/bin/env bash
set -u

failed=0
warned=0

pass() { printf 'PASS  %-22s %s\n' "$1" "$2"; }
warn() { printf 'WARN  %-22s %s\n' "$1" "$2"; warned=1; }
fail() { printf 'FAIL  %-22s %s\n' "$1" "$2"; failed=1; }

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(realpath "$script_dir/../../../..")"

if command -v omarchy >/dev/null 2>&1 && [[ -d /usr/share/omarchy ]]; then
  pass "Omarchy" "installed"
else
  fail "Omarchy" "this profile requires Omarchy OS"
fi

if [[ $EUID -eq 0 ]]; then
  fail "desktop user" "run as the normal Omarchy user, not root"
else
  pass "desktop user" "$(id -un)"
fi

if systemctl --user show-environment >/dev/null 2>&1; then
  pass "user session" "systemd user session is available"
else
  fail "user session" "open an Omarchy desktop session first"
fi

if command -v hermes >/dev/null 2>&1; then
  pass "Hermes" "$(command -v hermes)"
else
  fail "Hermes" "install and configure Hermes before VIC-DOM"
fi

if [[ -f "$HOME/.hermes/config.yaml" ]]; then
  pass "Hermes configuration" "$HOME/.hermes/config.yaml"
else
  warn "Hermes configuration" "run hermes setup before installation"
fi

for command_name in python3 node npm codex cargo; do
  if command -v "$command_name" >/dev/null 2>&1; then
    pass "$command_name" "$(command -v "$command_name")"
  else
    warn "$command_name" "setup.sh will install or configure this dependency"
  fi
done

transfer_failed=0
for relative_path in \
  services/gateway/server.py \
  contracts/master-system-prompt.md \
  ops/omarchy/setup.sh \
  ops/omarchy/profiles/vic-dom/gateway.env.example \
  ops/omarchy/profiles/vic-dom/core.env.example \
  ops/omarchy/profiles/vic-dom/ui-build.env \
  ops/omarchy/profiles/vic-dom/deployment-context.md \
  ops/omarchy/profiles/vic-dom/restaurant-data-sources.yaml.example \
  ops/omarchy/profiles/vic-dom/voice-command-learning.md \
  ops/omarchy/profiles/vic-dom/SOUL.md \
  ops/omarchy/profiles/vic-dom/AGENTS.md; do
  if [[ ! -f "$repo_root/$relative_path" ]]; then
    fail "transfer contents" "missing $relative_path"
    transfer_failed=1
  fi
done
if ((transfer_failed == 0)); then
  pass "transfer contents" "VIC-DOM profile is complete"
fi

config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/voiceos"
hermes_home="${HERMES_HOME:-$HOME/.hermes}"
for target in \
  "$config_dir/gateway.env" \
  "$config_dir/core.env" \
  "$config_dir/deployment-context.md" \
  "$hermes_home/SOUL.md" \
  "$hermes_home/workspace/AGENTS.md"; do
  if [[ -e "$target" ]]; then
    warn "existing file" "$target will be preserved and reviewed"
  fi
done

if [[ -d "$HOME/.local/state/voiceos" ]]; then
  warn "existing VoiceOS state" "will not be imported or overwritten"
else
  pass "VoiceOS state" "destination is clean"
fi

if command -v tailscale >/dev/null 2>&1 && tailscale status >/dev/null 2>&1; then
  warn "Tailscale" "connected; keep port 8787 unserved until device auth is reviewed"
else
  pass "remote exposure" "no active Tailscale gateway detected"
fi

printf '\nPreflight is read-only; it changed no files or services.\n'
if ((failed)); then
  printf 'VIC-DOM is not ready to install. Resolve the FAIL items first.\n'
  exit 1
fi
if ((warned)); then
  printf 'VIC-DOM can proceed after the WARN items are reviewed.\n'
else
  printf 'VIC-DOM is ready for the profile installation.\n'
fi
