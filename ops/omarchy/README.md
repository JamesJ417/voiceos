# Omarchy Voice on Omarchy

This integration runs the Omarchy Voice gateway, Hermes Agent, and an answer-only
Codex CLI fallback as user services underneath the Omarchy desktop session.
Hermes is VIC's default agent runtime and uses its configured remote provider;
no local LLM is required. It requires
no root account and does not modify packaged Omarchy files.

## Install

```bash
chmod +x ops/omarchy/install.sh ops/omarchy/voiceosctl
ops/omarchy/install.sh --enable
```

The installer creates:

- `~/.config/systemd/user/voiceos-gateway.service`
- `~/.config/systemd/user/voiceos-hermes.service`
- `~/.config/systemd/user/voiceos-codex.service`
- `~/.config/systemd/user/voiceos-ui.service`
- `~/.config/voiceos/gateway.env`
- `~/.config/voiceos/hermes-api.key`
- `~/.local/state/voiceos/` for private state
- `~/.local/bin/voiceosctl`
- `~/.local/bin/voiceos-talk`

It preserves existing environment and key files. Omarchy Voice talks to Hermes over
an authenticated loopback API, and Hermes publicly identifies itself as VIC.
The authenticated Codex CLI remains available through a private Unix socket. Codex is
ephemeral, answer-only, read-only, and has its command, web, app, hook, and
subagent tools disabled; permissioned system actions remain in the gateway.

## Operate

```bash
voiceosctl status
voiceosctl health
voiceosctl logs
voiceosctl restart
voiceos-talk
```

`voiceos-talk` opens Carbon Command in Google Chrome. Press **Talk**, allow microphone
access the first time, speak, and press **Done**. Chromium performs speech
recognition, the gateway sends the transcript to the Hermes-powered VIC agent, and
browser speech synthesis reads VIC's answer aloud.

The gateway listens only on `127.0.0.1:8787`. Use Tailscale Serve when a phone
needs private HTTPS access; never expose port 8787 through the router or enable
Tailscale Funnel.

## Supervised computer access

The Omarchy profile enables `computer.run`, allowing Codex to propose exact
user-level commands and browser launches. Every proposal is inert until the user
approves its exact argv, working directory, and stated reason in Omarchy Voice. The
gateway has write access to the user's home directory, not the protected system
filesystem. Root operations remain behind the desktop's password prompt.

## Hardware boundary

The Omarchy workstation can host the gateway, audit store, approvals, and user
interface. NVIDIA-specific inference and speech workers remain optional remote
capabilities. This keeps desktop integration independent of GPU hardware.
