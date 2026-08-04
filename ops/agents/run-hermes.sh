#!/usr/bin/env bash
set -euo pipefail

key_file="${VOICEOS_HERMES_API_KEY_FILE:-/etc/voiceos/hermes-api.key}"
if [[ ! -r "$key_file" ]]; then
  echo "Hermes API key file is not readable: $key_file" >&2
  exit 1
fi
export API_SERVER_KEY
API_SERVER_KEY="$(tr -d '\r\n' < "$key_file")"
if [[ -z "$API_SERVER_KEY" ]]; then
  echo "Hermes API key file is empty" >&2
  exit 1
fi

exec /opt/voiceos/hermes/.venv/bin/hermes gateway run --external-supervisor
