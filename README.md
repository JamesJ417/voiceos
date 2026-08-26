# Omarchy Touch

Omarchy Touch is a voice-first, touch-first add-on for
[Omarchy OS](https://omarchy.org). It combines three distinct layers:
**VoiceOS** is the private backend and control plane, **VIC** is the Voice
Interface Controller, and **Touch** is the portrait touchscreen system
interface. Hermes and a remote Codex CLI reasoning tier run behind VoiceOS
directly underneath the Omarchy desktop—no local LLM required.

Talk to VIC from the full-screen Touch interface or the Android companion app. The
Omarchy workstation owns the private gateway, conversation, permissions, tools,
and audit history; Tailscale carries encrypted traffic between devices without
exposing the gateway to the public internet.

## What it does

- Opens Touch as a dedicated full-screen portrait Hyprland workspace.
- Uses a USB microphone for hands-free conversations with VIC.
- Connects VIC to Hermes for agent orchestration and Codex for remote reasoning.
- Extends VIC to a Pixel phone over private Tailscale HTTPS.
- Requires explicit approval for consequential computer actions.
- Parks voice, Touch, and signed Fieldy brain dumps in a temporary review inbox.
- Offers one restart action and a five-minute fallback without silently creating commitments.
- Runs as hardened user services and does not modify packaged Omarchy files.

The core voice flow is deliberately simple:

1. Tap **Talk** on an Android phone.
2. Record 16 kHz mono PCM audio.
3. Send it to the gateway.
4. Receive a transcript and response.
5. Speak the response on the phone.

The gateway supports Hermes as the primary runtime, an authenticated Codex CLI
bridge, optional local Ollama tiers, permissioned tools, one-time device
enrollment, and SQLite audit history.

## Install on Omarchy

From a terminal on a fresh Omarchy desktop:

```bash
curl -fsSL https://raw.githubusercontent.com/JamesJ417/voiceos/main/install-omarchy.sh | bash
```

See [`ops/omarchy/README.md`](ops/omarchy/README.md) for configuration, service
management, Tailscale access, and the security model.

## Repository layout

```text
apps/android/       Native Android client and home-screen widget
apps/kiosk/         Touch web system interface
apps/vic-console/   Native Tauri information console and weather dashboard
contracts/          HTTP API contract
docs/               Architecture and security decisions
services/gateway/   Mock inference gateway
services/voiceos-core/ Provider-neutral Rust conversation and memory core
services/voiceos-ontology/ Canonical speech meaning, validation, aliases, and audit
services/voiceos-gateway-rs/ Android-compatible Rust transition gateway
```

`contracts/component-registry.json` is the canonical integration map. VoiceOS
serves it through the authenticated client bootstrap route so Touch can show
which production, registered, and preview components are tied into the system.
VIC Console uses that typed boundary through an owner-only local Unix socket.
Voice commands are ontology-validated and every acknowledged or failed delivery
is recorded in the VoiceOS execution log.

## Architecture guardrails

Rust is the target public control plane, Python is the temporary compatibility
ingress, and Hermes is an internal asynchronous agent runtime. The ownership,
data-authority, approval, migration, and rollback rules are recorded in
[`ADR-0001`](docs/architecture/ADR-0001-service-ownership-and-migration.md).
The public route ledger is machine-readable in
[`contracts/route-ownership.json`](contracts/route-ownership.json) and must stay
in exact sync with [`contracts/openapi.yaml`](contracts/openapi.yaml).

GitHub Actions and the local verification commands enforce Rust formatting,
Clippy and tests; Python service and contract tests; OpenAPI validation; kiosk
lint/build/render tests; and Android unit/lint/build checks. Public API changes
must update the OpenAPI document and ownership ledger in the same commit.

Hermes can now be selected as the core agent runtime through the authenticated
loopback adapter. The Buzz JSON adapter is experimental and deliberately not
wired into the production gateway until multiple bounded agents are introduced.
See `docs/hermes-buzz-integration.md` for the security boundary, performance
rationale, pinned upstream revisions, and rollout gates.

## Run the mock gateway

Python 3.11 or newer is sufficient.

```powershell
python -m services.gateway.server --host 127.0.0.1 --port 8787
```

Verify it from another terminal:

```powershell
Invoke-RestMethod http://127.0.0.1:8787/v1/health
```

Publish it privately to devices in the same Tailscale network:

```powershell
tailscale serve --bg --yes 8787
```

The gateway remains bound to loopback; Tailscale terminates private HTTPS and proxies requests to it. Do not enable Tailscale Funnel for this service.

Run its tests:

```powershell
python -m unittest discover -s services/gateway/tests -v
```

## Android development

The Android project targets API 36 and uses Android Gradle Plugin 9.3 with its built-in Kotlin support. It requires JDK 17, Android SDK Platform 36, and Build Tools 36.0.0.

The default gateway URL is `http://10.0.2.2:8787`, which reaches this laptop from the Android emulator. For a physical Pixel, build with the mining rig's Tailscale URL or address:

```powershell
./gradlew :app:assembleDebug -PAIOS_SERVER_URL=http://100.x.y.z:8787
```

Cleartext HTTP is allowed only in the debug build for initial LAN testing. The production transport will require TLS over the private Tailscale network.

## Touch and VIC interfaces

The Pixel VIC client and the Touch console share a visual system:
near-black carbon surfaces, restrained hex geometry, cyan-teal primary
actions, and explicit state labels. The phone remains voice-first and displays
secondary controls only when they apply. The responsive client in `apps/kiosk`
provides live Command, History, and System views for normal browsers and the
future full-screen HP touchscreen kiosk. It supports browser voice recognition
and playback, shared audit history, provider and system status, exact-response
copying, private file upload, and explicit approval decisions.

Touch enrolls as a separate VoiceOS device. Local browser origins are
accepted for development. Production origins must be explicitly allowlisted in
the gateway with `VOICEOS_WEB_ORIGINS`; wildcards are intentionally unsupported.

Touch's **Reset** workspace is the personal-support surface. A capture remains
temporary until VIC extracts bounded suggestions and the owner approves an
individual task or private review record. `Capture this …`, `What should I do
next?`, `Help me get unstuck`, and `I'm interrupted` use the same deterministic
workflow through the normal VIC voice path.

Fieldy transcript intake is optional and fail-closed. Configure
`VOICEOS_FIELDY_WEBHOOK_SECRET` in the private gateway environment and send
events to `POST /v1/integrations/fieldy/transcripts`. VoiceOS accepts either an
`X-Fieldy-Signature: sha256=<hex>` HMAC header or, for Fieldy's public webhook
service, the same secret as a `?token=` query parameter. Query tokens are
redacted from gateway logs. Fieldy's documented payload is normalized into the
private intake contract, and retries map to the same conversation chunk. VIC
assembles chunks into a durable conversation while events remain within the
same Fieldy session/recording and five-minute activity window. It preserves
speaker segments and waits 330 seconds of quiet before analysis so partial
transcripts do not become premature tasks. Analysis receives bounded active
project, open-task, relevant-memory, and pending-review context. Repeated
suggestions across conversations collapse into one project-aware review item
with an occurrence count and all supporting capture IDs. By default, completed
Fieldy conversations are analyzed in a retrying background worker and surfaced
as review-only task, appointment, worry, idea, or note proposals; nothing is
committed without approval.
Set `VOICEOS_FIELDY_AUTO_EXTRACT=0` to park transcripts without analysis.

For a local Tailscale deployment, keep the normal VIC gateway tailnet-only on
port 443 and expose only Fieldy's signed route on a dedicated Funnel port:

```bash
tailscale serve --bg --yes http://127.0.0.1:8787
tailscale funnel --https=8443 --bg --yes \
  --set-path=/v1/integrations/fieldy/transcripts \
  http://127.0.0.1:8787/v1/integrations/fieldy/transcripts
```

Enter `https://<tailscale-dns-name>:8443/v1/integrations/fieldy/transcripts?token=<secret>`
in Fieldy Developer Settings. Do not expose the root gateway through Funnel.

## Provider routing

All reasoning tiers receive the same versioned charter from
`contracts/master-system-prompt.md`. Provider-specific instructions can narrow a
model's role but cannot replace the shared identity, evidence, memory, approval,
and self-improvement rules.

The default remains safe and credential-free. Select local Ollama with
`VOICEOS_PROVIDER=ollama`, set `VOICEOS_OLLAMA_MODEL` to the resident fast model,
and set `VOICEOS_OLLAMA_DEEP_MODEL` to the on-demand reasoning model. The adapter
uses Ollama chat tool calling and validates every proposed function through the
gateway allowlist.

When the rig's `llm` account is already signed into Codex with ChatGPT, enable
`VOICEOS_CODEX_ENABLED=1`. Phrases such as “ask Codex,” “use Sol,” “highest
confidence,” and “final verification” route that turn to `gpt-5.6-sol` with high
reasoning. Other requests remain local. The separate bridge fixes Codex to an
ephemeral, read-only sandbox, strips unrelated environment variables, and never
passes gateway tool definitions or credentials to the gateway process. Shell,
unified execution, web search, apps, hooks, and multi-agent tools are disabled
for every bridged turn. The
direct OpenAI API and Claude review slots remain fail-closed until separately
configured.

## Secure enrollment

Generate a one-time QR locally so its secret is never sent to a public QR service:

```powershell
python -m pip install -r services/gateway/requirements-enrollment.txt
python -m services.gateway.enrollment_qr --gateway https://voiceos-rig.example.ts.net --output outputs/voiceos-enrollment.png
```

The Pixel exchanges the code for a random device token and encrypts it using Android Keystore. After enrollment is verified, start the gateway with `VOICEOS_REQUIRE_DEVICE_AUTH=1`.

## Permissioned tools and audit

- `GET /v1/tools` lists typed tools and approval policies.
- `POST /v1/tools/execute` denies unknown tools and never accepts shell text.
- System health, disk space, network status, and allowlisted service status are read-only.
- Project test execution is fixed to the gateway suite and requires explicit approval.
- Approvals preserve exact arguments, expire after five minutes, and can be decided only once.
- Android supports touch approval plus spoken `approve` or `deny`.
- SQLite history defaults to `work/gateway-data/audit.sqlite3`.

## RTX rig package

See `ops/rig/README.md`. It includes a read-only cross-platform diagnostic, a conservative Ubuntu runtime bootstrap, an environment template, and a hardened systemd gateway service. The bootstrap refuses to guess at NVIDIA driver installation and requires `nvidia-smi` to work first.

## Rust conversation core

The migration foundation lives in `services/voiceos-core`. It gives every
enrolled device a server-owned persistent conversation, supplies recent turns
to every provider, rolls older turns into a summary, and stores explicit durable
memories. `services/voiceos-gateway-rs` exposes the existing text-turn response
shape while the Python gateway remains active for tools and enrollment.

The Android app can also select private `.txt`, `.md`, `.json`, and `.csv` files
through Android's system picker. Rust stores the source and hash, chunks text,
pins “About me” profiles, and retrieves relevant reference passages for the
active provider. Files are device-owned, limited to 5 MB, and never executed.

During the Python-to-Rust transition, every authenticated production text turn
uses an internal prepare/commit bridge. Rust supplies prior conversation,
summary, explicit memories, and document context before the selected provider
runs, then stores the provider reply. The Android public turn API is unchanged.

Run the Rust tests with:

```powershell
cargo test --workspace --all-targets
```

See `docs/rust-memory-migration.md` for the provider configuration, rig shadow
deployment, eventual HP database move, and Python retirement gate.

## Omarchy OS add-on integration

Omarchy Touch installs as an optional add-on above Omarchy OS. Its Touch
interface, VIC voice controller, and VoiceOS backend stay in hardened user
services. It does not change packaged Omarchy
files or grant root authority. See
[`ops/omarchy/README.md`](ops/omarchy/README.md) for installation, operation,
and the boundary between the desktop control plane and optional GPU workers.

## Trusted agent kernel and reviewed skills

`voiceos-core` now owns canonical, owner-scoped records for goals, projects,
tasks, jobs, skill proposals, automation proposals, artifacts, provider runs,
and append-only execution events. Tasks require an observable outcome and a
bounded time estimate. Jobs accept typed capability arrays and an idempotency
key instead of arbitrary authority.

Provider runs record the provider, model, input and output tokens, latency,
calculated output tokens per second, cost, outcome, and related job. Each run
also emits an execution event so the phone and wall console can eventually show
live work from the same authoritative stream.

The first reviewed-skill prototype replays the legacy gateway audit database in
read-only mode. It proposes an inert `SKILL.md` only for a repeated successful
tool workflow, records the supporting audit-turn hash, and will not generate a
duplicate proposal from the same evidence. Generated skill text has no
execution path. An automation proposal cannot reference the skill until a user
has explicitly approved it.

Run the agent-kernel contract tests with the rest of the Rust workspace:

```powershell
cargo test --workspace --all-targets
```

## Canonical ontology

`services/voiceos-ontology` converts supported speech into typed canonical
requests without coupling meaning to Gemma, gpt-oss, or Codex. Deterministic
resolution runs first. A configured structured local-model fallback can propose
an interpretation, but the same catalog validates its intent, entities,
arguments, units, ranges, and confidence before anything can use it.

Original phrases, normalized phrases, interpretations, validation issues,
corrections, and final decisions are stored in `ontology.sqlite3`. Learned
aliases are explicit and owner-scoped; approving “the mining box” as `gpu-rig`
changes lookup data and never retrains a model.

## Next milestones

- Cut the Pixel and kiosk over to the owner-scoped conversation snapshot,
  incremental recovery, and SSE stream now deployed on the rig.
- Add bounded capability leases, job cancellation/checkpoints, idempotent resume,
  rollback metadata, and execution telemetry cards.
- Bootstrap and test the HP wall terminal, then add presence and playback transfer.
- Move speech transport to streaming Opus/WebRTC with VAD and barge-in.
- Add device revocation, credential rotation, retention, export/delete, and tested
  backup/restore before moving Rust memory authority to the HP.

## Implemented tools and routing

- `POST /v1/turns/text` accepts recognized phone text.
- `GET /v1/tools/system.health` returns deterministic host evidence.
- Health requests use the `system.health` tool instead of model judgment.
- Ordinary turns use the resident local model; deep requests use the on-demand
  local reasoning model.
- Explicit highest-confidence requests use Codex Sol through the read-only bridge.
- OpenAI and Claude review slots are visible but network-disabled.
