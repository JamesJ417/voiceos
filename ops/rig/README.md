# VoiceOS rig bootstrap

This package prepares an Ubuntu host without assuming a specific CPU, GPU model, disk name, network interface, or installed Ollama model.

## Safe sequence

1. Install Ubuntu Server 24.04 LTS and all updates.
2. Install the current NVIDIA open driver following NVIDIA's Ubuntu guide.
3. Reboot and require `nvidia-smi` to succeed.
4. Clone or copy this repository to the rig.
5. Run `chmod +x ops/rig/*.sh`.
6. Run `ops/rig/bootstrap-ubuntu.sh`.
7. Run `sudo tailscale up` and join the existing tailnet.
8. Run `python3 ops/rig/diagnose.py --json`.
9. Inspect installed models with `ollama list`; choose a tool-capable model.
10. Run `ops/rig/install-gateway-service.sh /absolute/path/to/repository`.
11. Edit `/etc/voiceos/voiceos-gateway.env` before enabling the service.

## Optional Codex Sol tier

If an existing `llm` user has `/home/llm/.local/bin/codex` and `codex login
status` reports a ChatGPT login, the service installer also installs
`voiceos-codex.service`. Set `VOICEOS_CODEX_ENABLED=1` in the gateway environment
and enable the bridge. No API key is copied into VoiceOS.

The service runs Codex as `llm`, pins `gpt-5.6-sol` with high reasoning, uses
ephemeral non-interactive turns, ignores user configuration and project rules,
enforces Codex's read-only sandbox, and disables command, web-search, app, hook,
and multi-agent tools. It communicates with the gateway only
through `/run/voiceos-codex/codex.sock`. VoiceOS does not expose Codex as an
arbitrary shell or allow voice requests to alter these controls.

The installed gateway pulls in `voiceos-model-warm.service` during startup. That
unit loads the configured Ollama model before the gateway starts, avoiding a
large first-request delay after a reboot.

## Exclusive GPU scheduling

On a 16 GB GPU, the Ollama chat model and Moshi Q8 speech model are scheduled as
exclusive workloads. `voiceos-gpu-scheduler.service` exposes only fixed
`acquire`, `release`, and `status` operations over a group-restricted Unix
socket. The speech worker acquires a lease before connecting to Moshi. The
scheduler unloads the configured Ollama model, starts the loopback-only Moshi
service, and restores the warm Ollama model after the final speech lease ends or
expires. It never accepts commands or service names from clients.

The scheduler publishes explicit `chat`, `starting_speech`, `speech`,
`restoring_chat`, and `failed` states. Transition identifiers and failures are
written to the system journal. Failed speech starts perform a best-effort chat
rollback, and the maintenance loop restores a stable chat state after abandoned
or expired leases.

Speech session creation has a short connection grace period and does not reserve
the GPU. The GPU lease begins only after the enrolled device claims its
WebSocket, renews while the stream remains active, and releases when either side
disconnects. Lifecycle and release-failure evidence is appended to
`/var/lib/voiceos/speech/lifecycle.jsonl`.

The installer also creates `/etc/voiceos/voiceos-admin.env` with a random
administrator token and mode `0640`. The token remains on the rig and is used
only to authorize creation of short-lived device-enrollment codes.

The bootstrap deliberately stops when NVIDIA is unavailable instead of installing or replacing a driver automatically. It downloads installers only from the official Ollama and Tailscale HTTPS origins, stores them in a temporary directory, and executes them locally.

The systemd service binds VoiceOS to loopback, applies basic process hardening, and writes only to `/var/lib/voiceos`. Use Tailscale Serve for private HTTPS; do not enable Funnel.
