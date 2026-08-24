#!/usr/bin/env bash
set -euo pipefail

readonly REPOSITORY_URL="https://github.com/JamesJ417/voiceos.git"
install_root="${OMARCHY_VOICE_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/omarchy-voice}"
source_dir="$install_root/source"

if [[ $EUID -eq 0 ]]; then
  echo "Run this installer as your normal Omarchy user, not as root." >&2
  exit 1
fi
if ! command -v omarchy >/dev/null || [[ ! -d /usr/share/omarchy ]]; then
  echo "This installer must run from an Omarchy desktop." >&2
  exit 1
fi
command -v git >/dev/null || { echo "Git is required by VoiceOS." >&2; exit 1; }

mkdir -p "$install_root"
if [[ -d "$source_dir/.git" ]]; then
  if [[ -n "$(git -C "$source_dir" status --porcelain)" ]]; then
    echo "Existing VoiceOS source has local changes: $source_dir" >&2
    echo "Commit or remove those changes before reinstalling." >&2
    exit 1
  fi
  git -C "$source_dir" pull --ff-only
else
  git clone "$REPOSITORY_URL" "$source_dir"
fi

chmod +x "$source_dir/ops/omarchy/setup.sh"
if [[ -r /dev/tty ]]; then
  exec "$source_dir/ops/omarchy/setup.sh" "$@" </dev/tty
fi
exec "$source_dir/ops/omarchy/setup.sh" "$@"
