#!/usr/bin/env bash
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "Run this installer with sudo on the HP wall terminal." >&2
  exit 1
fi

source_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if ! id voiceos-kiosk >/dev/null 2>&1; then
  useradd --system --create-home --home-dir /var/lib/voiceos-kiosk --shell /usr/sbin/nologin voiceos-kiosk
fi
for group in audio video render input; do
  if getent group "$group" >/dev/null; then
    usermod -aG "$group" voiceos-kiosk
  fi
done

install -d -m 0755 /opt/voiceos/bin /etc/voiceos
install -d -o voiceos-kiosk -g voiceos-kiosk -m 0700 /var/lib/voiceos-kiosk
install -m 0755 "$source_dir/start-carbon-kiosk" /opt/voiceos/bin/start-carbon-kiosk
install -m 0644 "$source_dir/voiceos-kiosk.service" /etc/systemd/system/voiceos-kiosk.service
if [[ ! -f /etc/voiceos/kiosk.env ]]; then
  install -m 0600 "$source_dir/kiosk.env.example" /etc/voiceos/kiosk.env
fi

if ! command -v cage >/dev/null 2>&1; then
  echo "Install the 'cage' Wayland kiosk compositor before starting VoiceOS." >&2
fi
if ! command -v chromium >/dev/null 2>&1 && ! command -v chromium-browser >/dev/null 2>&1 && [[ ! -x /snap/bin/chromium ]] && ! command -v google-chrome-stable >/dev/null 2>&1; then
  echo "Install Chromium or Google Chrome before starting VoiceOS." >&2
fi

systemctl daemon-reload
systemctl enable voiceos-kiosk.service
echo "Edit /etc/voiceos/kiosk.env, then run: sudo systemctl start voiceos-kiosk"
