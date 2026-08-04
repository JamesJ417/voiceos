#!/usr/bin/env bash
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "Run with sudo." >&2
  exit 1
fi

install -d -o voiceos -g voiceos /var/lib/voiceos/speech /var/lib/voiceos/crawl4ai
UV_PYTHON_INSTALL_DIR=/opt/voiceos/python /usr/local/bin/uv python install 3.12
UV_PYTHON_INSTALL_DIR=/opt/voiceos/python /usr/local/bin/uv venv --python 3.12 /opt/voiceos/venvs/speech
UV_PYTHON_INSTALL_DIR=/opt/voiceos/python /usr/local/bin/uv venv --python 3.12 /opt/voiceos/venvs/crawl4ai
/usr/local/bin/uv pip install --python /opt/voiceos/venvs/speech/bin/python --requirement /opt/voiceos/services/speech_worker/requirements.txt
/usr/local/bin/uv pip install --python /opt/voiceos/venvs/crawl4ai/bin/python --requirement /opt/voiceos/services/crawl4ai_adapter/requirements.txt
HOME=/var/lib/voiceos/crawl4ai /opt/voiceos/venvs/crawl4ai/bin/crawl4ai-setup
chown -R voiceos:voiceos /var/lib/voiceos/crawl4ai /var/lib/voiceos/speech
install -m 0644 /opt/voiceos/ops/systemd/voiceos-speech-worker.service /etc/systemd/system/voiceos-speech-worker.service
install -m 0644 /opt/voiceos/ops/systemd/voiceos-crawl4ai.service /etc/systemd/system/voiceos-crawl4ai.service
systemctl daemon-reload
systemctl enable --now voiceos-speech-worker.service voiceos-crawl4ai.service
