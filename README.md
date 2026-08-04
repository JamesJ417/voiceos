# VoiceOS

VoiceOS is a phone-first, voice-controlled personal AI system. The Android phone is the microphone, speaker, display, and approval surface. A private GPU server performs speech recognition, model inference, tool execution, and system health checks.

This repository starts with a deliberately small vertical slice:

1. Tap **Talk** on an Android phone.
2. Record 16 kHz mono PCM audio.
3. Send it to the gateway.
4. Receive a transcript and response.
5. Speak the response on the phone.

The gateway includes two local Ollama reasoning tiers, an explicit GPT-5.6 Sol
escalation through an authenticated Codex CLI, fail-closed cloud-provider slots,
permissioned tools, one-time device enrollment, and SQLite audit history.

## Repository layout

```text
apps/android/       Native Android client and home-screen widget
apps/kiosk/         Carbon Command touchscreen console
contracts/          HTTP API contract
docs/               Architecture and security decisions
services/gateway/   Mock inference gateway
services/voiceos-core/ Provider-neutral Rust conversation and memory core
services/voiceos-ontology/ Canonical speech meaning, validation, aliases, and audit
services/voiceos-gateway-rs/ Android-compatible Rust transition gateway
```

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

## Carbon Command interfaces

The Pixel client and the touchscreen console share the Carbon Command visual
system: near-black carbon surfaces, restrained hex geometry, cyan-teal primary
actions, and explicit state labels. The phone remains voice-first and displays
secondary controls only when they apply. The responsive client in `apps/kiosk`
provides live Command, History, and System views for normal browsers and the
future full-screen HP touchscreen kiosk. It supports browser voice recognition
and playback, shared audit history, provider and system status, exact-response
copying, private file upload, and explicit approval decisions.

The web client enrolls as a separate VoiceOS device. Local browser origins are
accepted for development. Production origins must be explicitly allowlisted in
the gateway with `VOICEOS_WEB_ORIGINS`; wildcards are intentionally unsupported.

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
