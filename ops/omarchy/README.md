# Omarchy Voice on Omarchy

This integration runs the Omarchy Voice gateway, Hermes Agent, and an answer-only
Codex CLI fallback as user services underneath the Omarchy desktop session.
Hermes is VIC's default agent runtime and uses its configured remote provider;
no local LLM is required. It requires
no root account and does not modify packaged Omarchy files.

## Install

For a clean Omarchy machine, run the complete setup wizard:

```bash
curl -fsSL https://raw.githubusercontent.com/JamesJ417/voiceos/main/install-omarchy.sh | bash
```

It installs the required Arch and AUR packages through `omarchy pkg`, installs
Hermes Agent and Codex CLI from their official installers, guides you through
remote-provider sign-in, downloads a checksum-verified Whisper model, builds the
web interface, registers all user services, adds Omarchy Voice to the Omarchy
menu, enables Tailscale, and runs an end-to-end readiness check.

The upstream installers are downloaded over HTTPS from
`hermes-agent.nousresearch.com/install.sh` and `chatgpt.com/codex/install.sh`.
The speech model is downloaded from the official whisper.cpp Hugging Face
repository and must match the SHA-256 recorded in `setup.sh` before installation.

For automated image preparation, use `--non-interactive`. Authentication is
intentionally not bypassed; after provisioning, run `codex login` and
`hermes setup`, then rerun `ops/omarchy/install.sh --enable`.

The lower-level installer is idempotent and useful after pulling an update:

```bash
ops/omarchy/install.sh --enable
omarchy-voice-doctor
```

The installer creates:

- `~/.config/systemd/user/voiceos-gateway.service`
- `~/.config/systemd/user/voiceos-core.service`
- `~/.config/systemd/user/voiceos-hermes.service`
- `~/.config/systemd/user/voiceos-codex.service`
- `~/.config/systemd/user/voiceos-ui.service`
- `~/.config/systemd/user/voiceos-wake.service`
- `~/.config/voiceos/gateway.env`
- `~/.config/voiceos/hermes-api.key`
- `~/.local/state/voiceos/` for private state
- `~/.local/bin/voiceosctl`
- `~/.local/bin/voiceos-talk`
- `~/.local/bin/omarchy-voice-doctor`
- `~/.local/share/applications/omarchy-voice.desktop`
- `~/.config/omarchy/extensions/omarchy-menu.jsonc` entry

It preserves existing environment and key files. Omarchy Voice talks to Hermes over
an authenticated loopback API, and Hermes publicly identifies itself as VIC.
The authenticated Codex CLI remains available through a private Unix socket. Codex is
ephemeral, answer-only, read-only, and has its command, web, app, hook, and
subagent tools disabled; permissioned system actions remain in the gateway.

`voiceos-core.service` is the loopback-only authority for VIC's task board,
conversation memory, ontology, and private documents. On first start it imports
the legacy gateway audit history idempotently, so installing task support does
not discard earlier conversations.

## Operate

```bash
voiceosctl status
voiceosctl health
voiceosctl logs
voiceosctl restart
voiceos-talk
```

`voiceos-talk` opens VIC Panel full-screen in Google Chrome. Press **Talk**, allow microphone
access the first time, speak, and press **Done**. Chromium performs speech
recognition, the gateway sends the transcript to the Hermes-powered VIC agent, and
the gateway returns the same Ava Neural voice used by the local Hey VIC listener.
Browser speech synthesis remains available as a fallback if neural TTS is offline.

The always-on `voiceos-wake.service` keeps wake-word detection local. Say
**“Hey VIC”**, then speak your command. Only the post-wake utterance is transcribed
and sent to the same VIC/Hermes conversation path; VIC's reply is spoken aloud.
After each reply, continue speaking naturally without repeating the wake phrase.
Say **“goodbye”** or **“stop listening”** to end the conversation, or wait 20
seconds for it to return to wake-word mode automatically.
If you say only **“Hey VIC”**, VIC answers **“Yes, I'm here”** before listening
for your first request.

Wake acknowledgement and error prompts are cached locally for immediate
playback. Ordinary conversation uses low reasoning latency, while action and
tool requests retain medium reasoning. Longer work is surfaced through live
VIC Panel status instead of a repeated spoken holding message. Ordinary chat
uses the low-latency Luna model, and VIC Panel synthesizes Ava Neural speech
one sentence ahead so long answers begin playing sooner.

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
