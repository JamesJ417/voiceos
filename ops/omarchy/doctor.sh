#!/usr/bin/env bash
set -uo pipefail

failed=0
check_command() {
  if command -v "$1" >/dev/null; then
    printf 'PASS  %-18s %s\n' "$1" "$(command -v "$1")"
  else
    printf 'FAIL  %-18s missing\n' "$1"
    failed=1
  fi
}

for command_name in omarchy python3 node npm hermes codex google-chrome-stable ffmpeg whisper-cli; do
  check_command "$command_name"
done

if codex login status >/dev/null 2>&1; then
  printf 'PASS  %-18s authenticated\n' "Codex"
else
  printf 'FAIL  %-18s run: codex login\n' "Codex"
  failed=1
fi

if [[ -f "$HOME/.hermes/config.yaml" ]]; then
  printf 'PASS  %-18s configured\n' "Hermes"
else
  printf 'FAIL  %-18s run: hermes setup\n' "Hermes"
  failed=1
fi

model_path="${XDG_DATA_HOME:-$HOME/.local/share}/voiceos/models/ggml-base.en.bin"
if [[ -s "$model_path" ]]; then
  printf 'PASS  %-18s %s\n' "speech model" "$model_path"
else
  printf 'FAIL  %-18s missing\n' "speech model"
  failed=1
fi

for unit in voiceos-core voiceos-hermes voiceos-codex voiceos-gateway voiceos-ui; do
  if systemctl --user is-active --quiet "$unit.service"; then
    printf 'PASS  %-18s active\n' "$unit"
  else
    printf 'FAIL  %-18s inactive\n' "$unit"
    failed=1
  fi
done

if curl --fail --silent http://127.0.0.1:8790/v1/health >/dev/null; then
  printf 'PASS  %-18s healthy\n' "task authority"
else
  printf 'FAIL  %-18s unavailable\n' "task authority"
  failed=1
fi

if curl --fail --silent http://127.0.0.1:8787/v1/tasks?limit=1 >/dev/null; then
  printf 'PASS  %-18s available to VIC\n' "task board"
else
  printf 'FAIL  %-18s unavailable to VIC\n' "task board"
  failed=1
fi

if curl --fail --silent http://127.0.0.1:8787/v1/health >/dev/null; then
  printf 'PASS  %-18s healthy\n' "gateway"
else
  printf 'FAIL  %-18s unavailable\n' "gateway"
  failed=1
fi

if [[ -f "${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/extensions/omarchy-menu.jsonc" ]] &&
   grep -q '"omarchy-voice"' "${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/extensions/omarchy-menu.jsonc"; then
  printf 'PASS  %-18s installed\n' "Omarchy menu"
else
  printf 'FAIL  %-18s missing\n' "Omarchy menu"
  failed=1
fi

if command -v wpctl >/dev/null && wpctl status 2>/dev/null | grep -A20 'Sources:' | grep -q '[0-9]'; then
  printf 'PASS  %-18s source detected\n' "microphone"
else
  printf 'WARN  %-18s connect or enable an input source\n' "microphone"
fi

if command -v tailscale >/dev/null && tailscale status >/dev/null 2>&1; then
  printf 'PASS  %-18s connected\n' "Tailscale"
else
  printf 'WARN  %-18s phone access is not configured\n' "Tailscale"
fi

exit "$failed"
