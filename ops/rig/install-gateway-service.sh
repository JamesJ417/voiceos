#!/usr/bin/env bash
set -euo pipefail

repo_root="${1:-}"
if [[ -z "$repo_root" ]]; then
  echo "Usage: $0 /absolute/path/to/voiceos-repository" >&2
  exit 2
fi
repo_root="$(realpath "$repo_root")"
if [[ ! -f "$repo_root/services/gateway/server.py" ]]; then
  echo "Not a VoiceOS repository: $repo_root" >&2
  exit 2
fi
if [[ "$repo_root" == *'|'* ]]; then
  echo "Repository path cannot contain a pipe character." >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sudo useradd --system --home-dir /var/lib/voiceos --create-home --shell /usr/sbin/nologin voiceos 2>/dev/null || true
if ! sudo -u voiceos test -x "$repo_root" \
  || ! sudo -u voiceos test -r "$repo_root/services/gateway/server.py"; then
  echo "The voiceos service account cannot traverse or read the repository: $repo_root" >&2
  echo "Make repository directories executable and source files readable, then retry." >&2
  exit 2
fi
sudo install -d -o voiceos -g voiceos -m 0750 /var/lib/voiceos
sudo install -d -o voiceos -g voiceos -m 0750 /var/lib/voiceos/connectors
sudo install -d -o root -g voiceos -m 0750 /etc/voiceos
sudo install -d -o root -g voiceos -m 0750 /etc/voiceos/secrets
if ! sudo test -f /etc/voiceos/voiceos-gateway.env; then
  sudo install -o root -g voiceos -m 0640 \
    "$script_dir/voiceos-gateway.env.example" /etc/voiceos/voiceos-gateway.env
fi
if ! sudo test -f /etc/voiceos/voiceos-connectors.env; then
  sudo install -o root -g voiceos -m 0640 \
    "$script_dir/voiceos-connectors.env.example" /etc/voiceos/voiceos-connectors.env
fi
if ! sudo test -f /etc/voiceos/secrets/connector-ingest-token; then
  python3 -c 'import secrets; print(secrets.token_urlsafe(48))' \
    | sudo install -o root -g voiceos -m 0640 /dev/stdin /etc/voiceos/secrets/connector-ingest-token
fi
if ! sudo test -f /etc/voiceos/voiceos-admin.env; then
  admin_token="$(python3 -c 'import secrets; print(secrets.token_urlsafe(48))')"
  printf 'VOICEOS_ADMIN_TOKEN=%s\n' "$admin_token" \
    | sudo install -o root -g voiceos -m 0640 /dev/stdin /etc/voiceos/voiceos-admin.env
  unset admin_token
fi
sed "s|@@REPO_ROOT@@|$repo_root|g" "$script_dir/voiceos-gateway.service.template" \
  | sudo tee /etc/systemd/system/voiceos-gateway.service >/dev/null
sed "s|@@REPO_ROOT@@|$repo_root|g" "$script_dir/voiceos-model-warm.service.template" \
  | sudo tee /etc/systemd/system/voiceos-model-warm.service >/dev/null
sed "s|@@REPO_ROOT@@|$repo_root|g" "$repo_root/ops/systemd/voiceos-connectors.service" \
  | sudo tee /etc/systemd/system/voiceos-connectors.service >/dev/null
if id llm >/dev/null 2>&1 && sudo -u llm test -x /home/llm/.local/bin/codex; then
  sed "s|@@REPO_ROOT@@|$repo_root|g" "$script_dir/voiceos-codex.service.template" \
    | sudo tee /etc/systemd/system/voiceos-codex.service >/dev/null
  echo "Codex bridge service installed for the existing llm account."
else
  echo "Codex bridge not installed: llm account or its Codex CLI was not found."
fi
sudo systemctl daemon-reload

echo "Gateway service installed but not started."
echo "Edit /etc/voiceos/voiceos-gateway.env, then run:"
echo "  sudo systemctl enable --now voiceos-gateway"
echo "  sudo systemctl enable --now voiceos-connectors"
