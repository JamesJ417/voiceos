#!/usr/bin/env bash
set -euo pipefail

repo_url="https://github.com/NousResearch/hermes-agent.git"
commit="a01f979b6e86d24a798968d5341788e008f96caf"
install_root="/opt/voiceos/hermes"
state_root="/var/lib/voiceos/hermes"
key_file="/etc/voiceos/hermes-api.key"
skill_worker_key_file="/etc/voiceos/hermes-skill-worker.key"

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run this installer with sudo." >&2
  exit 1
fi
for command in git uv install openssl systemctl; do
  command -v "$command" >/dev/null || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done

getent group voiceos-agents >/dev/null || groupadd --system voiceos-agents
id voiceos-hermes >/dev/null 2>&1 || useradd \
  --system --gid voiceos-agents --home-dir "$state_root" --shell /usr/sbin/nologin voiceos-hermes
usermod -a -G voiceos-agents voiceos
# The existing /var/lib/voiceos root is intentionally 0750 and group-owned by
# voiceos, so the isolated runtime needs traversal without gaining ownership.
usermod -a -G voiceos voiceos-hermes

install -d -o root -g root -m 0755 /opt/voiceos
if [[ ! -d "$install_root/.git" ]]; then
  git clone --filter=blob:none "$repo_url" "$install_root"
fi
git -C "$install_root" fetch --depth 1 origin "$commit"
git -C "$install_root" checkout --detach "$commit"

export UV_PYTHON_INSTALL_DIR=/opt/voiceos/python
uv python install 3.12
python_interpreter="$(uv python find --python-preference only-managed 3.12)"
if systemctl is-active --quiet voiceos-hermes; then
  systemctl stop voiceos-hermes
fi
uv venv --clear --python "$python_interpreter" "$install_root/.venv"
# The authenticated API-server adapter is packaged in Hermes's messaging extra
# because it uses aiohttp. Other optional tools remain lazy-installed by Hermes.
uv sync --locked --project "$install_root" --python "$python_interpreter" --no-dev --extra messaging

install -d -o voiceos-hermes -g voiceos-agents -m 0750 \
  "$state_root" "$state_root/workspace" "$state_root/skills" "$state_root/optional-skills"
install -o voiceos-hermes -g voiceos-agents -m 0640 \
  /opt/voiceos/contracts/VIC-SOUL.md "$state_root/SOUL.md"
install -o voiceos-hermes -g voiceos-agents -m 0640 \
  /opt/voiceos/ops/agents/hermes-workspace-AGENTS.md "$state_root/workspace/AGENTS.md"
chmod 2770 "$state_root/skills"
install -d -o voiceos-hermes -g voiceos-agents -m 0750 /var/lib/voiceos/hermes-skill-control
if ! find "$state_root/skills" -name SKILL.md -print -quit | grep -q .; then
  cp -a "$install_root/skills/." "$state_root/skills/"
fi
install -d -o voiceos-hermes -g voiceos-agents -m 0750 \
  "$state_root/skills/voiceos/vic-voiceos-coordination"
install -o voiceos-hermes -g voiceos-agents -m 0640 \
  /opt/voiceos/ops/agents/vic-voiceos-skill/SKILL.md \
  "$state_root/skills/voiceos/vic-voiceos-coordination/SKILL.md"
# Keep the upstream optional catalog available for reviewed activation without
# placing high-impact skills such as Docker or security tooling on the live path.
cp -a "$install_root/optional-skills/." "$state_root/optional-skills/"
chown -R voiceos-hermes:voiceos-agents "$state_root/skills" "$state_root/optional-skills"
if [[ ! -f "$state_root/config.yaml" ]]; then
  install -o voiceos-hermes -g voiceos-agents -m 0640 \
    /opt/voiceos/ops/agents/hermes-config.yaml.example "$state_root/config.yaml"
fi
if [[ ! -f "$key_file" ]]; then
  umask 0027
  openssl rand -hex 32 > "$key_file"
fi
chown root:voiceos-agents "$key_file"
chmod 0640 "$key_file"
if [[ ! -f "$skill_worker_key_file" ]]; then
  umask 0027
  openssl rand -hex 32 > "$skill_worker_key_file"
fi
chown root:voiceos-agents "$skill_worker_key_file"
chmod 0640 "$skill_worker_key_file"

install -d -o root -g root -m 0755 /opt/voiceos/bin
install -o root -g root -m 0755 /opt/voiceos/ops/agents/run-hermes.sh /opt/voiceos/bin/run-hermes
install -o root -g root -m 0644 /opt/voiceos/ops/systemd/voiceos-hermes.service \
  /etc/systemd/system/voiceos-hermes.service
install -o root -g root -m 0644 \
  /opt/voiceos/ops/systemd/voiceos-hermes-skill-worker.service \
  /etc/systemd/system/voiceos-hermes-skill-worker.service
systemctl daemon-reload

echo "Hermes is installed but not started."
echo "Edit $state_root/config.yaml and set the installed Ollama model, then run:"
echo "  systemctl enable --now voiceos-hermes"
echo "  systemctl enable --now voiceos-hermes-skill-worker"
echo "  curl -fsS http://127.0.0.1:8642/health"
echo "After health succeeds, set VOICEOS_PROVIDER=hermes and restart voiceos-gateway."
