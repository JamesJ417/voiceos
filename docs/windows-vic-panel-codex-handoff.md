# Windows Touch — Codex handoff

## Goal

Build an installable Windows desktop client in Rust that connects over Tailscale to the Omarchy-hosted VIC services. The Windows computer is an enrolled client only: it must not host Hermes, provider credentials, task authority, canonical memory, or approval policy.

## What is already working on Omarchy

The shared Rust workspace is at the repository root. Relevant components are:

- `services/voiceos-core`: owner-scoped conversations, durable memory, ordered messages, request-id deduplication, tasks, skills, and documents.
- `services/voiceos-gateway-rs`: Rust conversation/control-plane gateway. It normally listens on loopback port 8790 and authenticates enrolled bearer tokens against the legacy enrollment database during migration.
- `services/gateway`: the production Python compatibility gateway on loopback port 8787. It currently owns enrollment, approvals, deterministic tools, audio, and audit APIs.
- `apps/vic-panel-native`: GTK4 Linux panel. Treat this as UX/API reference only; GTK and PipeWire commands are not the Windows implementation path.

The Rust gateway now exposes `GET /v1/client/bootstrap`. An authenticated client calls it first to confirm the server contract before using the conversation API. Version 1 returns:

```json
{
  "contract_version": 1,
  "device_id": "server-authenticated-device-id",
  "authentication": {"scheme": "bearer"},
  "endpoints": {
    "conversation": "/v1/conversations/active",
    "conversation_events": "/v1/conversations/active/events",
    "turn": "/v1/turns/text"
  },
  "transport": {"private_network_required": true, "tls_required": true}
}
```

The endpoint is implemented in `services/voiceos-gateway-rs/src/api/client.rs` and registered in `services/voiceos-gateway-rs/src/api/mod.rs`. It is deliberately a contract discovery endpoint, not an enrollment endpoint.

## Security and connection model

1. Install Tailscale on Windows and join the same tailnet as Omarchy.
2. The Windows app reaches the Omarchy Tailscale HTTPS name. Do not expose the gateway publicly, use a public tunnel, or enable Tailscale Funnel.
3. Enrollment is a one-time exchange with the production Python gateway:
   - an administrator creates a short-lived enrollment session;
   - Windows exchanges its code with `POST /v1/enrollment/exchange` and a user-visible device name;
   - the returned device token is stored with Windows Credential Manager, never in source code, configuration files, logs, or crash reports.
4. Every server request includes `Authorization: Bearer <device token>`.
5. The client calls `/v1/client/bootstrap` after enrollment or token restore. Refuse unsupported major contract versions.
6. The device that sends a turn is the default speech target. Other devices update silently.

## Recommended Windows implementation

Use Tauri 2 with a Rust backend and the system WebView2 renderer. This keeps the installed desktop application Rust-owned while avoiding a GTK port and preserving a fast UI iteration loop.

Suggested new crate location: `apps/vic-panel-windows/`.

Rust responsibilities:

- configuration and secure token storage through Windows Credential Manager;
- typed HTTP/SSE client and request-id generation;
- Tailscale connection and server-contract health checks;
- microphone permission/state, audio capture, playback, and push-to-talk;
- Windows notifications, system tray, reconnect behavior, and structured logs with secrets redacted.

UI responsibilities:

- match the Touch visual language, but do not copy Linux-only GTK code;
- visible connection/listening/thinking/speaking state;
- conversation history, task status, activity, approvals, and explicit error recovery;
- no background microphone capture in the first milestone. Start with push-to-talk.

## First Windows milestone

Deliver a signed-or-debug-installable Windows app that can:

1. accept the private Omarchy gateway URL and exchange a one-time enrollment code;
2. store its bearer token in Credential Manager;
3. call `/v1/client/bootstrap` and show the authenticated device ID plus contract version;
4. fetch `GET /v1/conversations/active`;
5. send a typed request to `POST /v1/turns/text` with a UUID request ID;
6. subscribe to `GET /v1/conversations/active/events` using SSE and reconnect from the last sequence after sleep/network loss;
7. display a text conversation without synthesizing speech on any other client.

Do not add wake-word detection, always-on listening, remote desktop control, provider credentials, or server-side tool execution to this milestone.

## Later milestones

- M2: push-to-talk using Windows audio APIs; upload audio only after deliberate activation.
- M3: Windows notifications, tray presence, and speech playback on the initiating device.
- M4: approval view and exact-action confirmation against the production compatibility gateway.
- M5: device rename, revocation, credential rotation, and full Rust gateway parity before the Python gateway can be retired.

## Windows workstation prerequisites

Install on Windows before using Codex:

- Git for Windows;
- Rust stable with the `x86_64-pc-windows-msvc` target;
- Visual Studio 2022 Build Tools with the Desktop development with C++ workload and Windows SDK;
- Node.js LTS and the Tauri CLI/tooling selected by the generated Tauri project;
- Microsoft Edge WebView2 Runtime (normally present on Windows 11);
- Tailscale, signed into the same tailnet;
- Codex CLI, authenticated with the user's own Codex account.

Codex should work in a clone of this repository, not in a copied source folder. It must not receive the Omarchy device token, the admin enrollment token, server environment files, Hermes credentials, or any private database.

## Codex prompt for the Windows machine

Use this after the server changes have been committed and pushed to a branch that Windows can clone:

```
You are implementing the Windows Touch interface in this repository. Read docs/windows-vic-panel-codex-handoff.md first, then inspect services/voiceos-gateway-rs/src/api/client.rs, services/voiceos-gateway-rs/src/api/conversations.rs, and apps/vic-panel-native/src/main.rs.

Create apps/vic-panel-windows as a Tauri 2 Windows app with a Rust backend. Implement only milestone 1 from the handoff. Use a typed client contract, bearer-token storage via Windows Credential Manager, UUID request ids, Tailscale HTTPS-only validation, bootstrap version validation, active conversation loading, text turns, and resumable SSE conversation updates. Do not implement enrollment-session creation, wake-word listening, always-on microphone capture, provider credentials, tool execution, or any public-network path. Add focused tests and run the relevant Rust and frontend tests. Do not commit changes.
```

## Verification already performed on Omarchy

- `cargo test -p voiceos-gateway bootstrap_describes_the_windows_client_contract` passed.
- `cargo test --workspace --all-targets` passed.
- A temporary loopback Rust gateway returned the bootstrap payload at `GET /v1/client/bootstrap`.
