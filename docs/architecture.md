# MVP architecture

## Trust boundaries

The phone is a client, not a privileged administrator. The gateway authenticates the device and delegates every executable request to a policy-controlled tool broker. Models can propose tool calls; they cannot obtain an unrestricted shell through the voice interface.

Codex Sol is reserved for explicit, highest-confidence reasoning and deliberate
development review. VoiceOS invokes it through a separate answer-only process;
it does not receive the gateway's tool schemas and cannot approve or execute a
VoiceOS action. Claude can independently review collected evidence and proposed
repairs when that adapter is later configured. No model alone determines whether
a system is healthy: deterministic probes and tests produce the health result.

## Initial request flow

```text
Pixel microphone
  -> Android foreground recording service
  -> POST /v1/turns/audio
  -> gateway
  -> mock response (initially)
  -> Android TextToSpeech
```

The first protocol uses buffered PCM because it is easy to inspect and test. Once the vertical slice works on the Pixel and mining rig, it will be replaced with streaming Opus over WebRTC.

The primary phone path now performs discrete on-device transcription and sends text turns. The buffered PCM route remains available for future server-side transcription.

## Server evolution

The gateway will eventually coordinate four independent layers:

1. Media: voice activity detection, speech recognition, and speech synthesis.
2. Reasoning: local model by default, with explicit cloud escalation.
3. Tools: typed, permissioned operations with previews and undo metadata.
4. Verification: deterministic health checks plus optional independent model review.

The current `system.health` tool collects CPU, memory, disk, and operating-system evidence directly from the gateway host. Its health classification is deterministic; the provider router is not involved.

The local router supports two Ollama tiers. `VOICEOS_OLLAMA_MODEL` is the
resident, non-thinking model for routine spoken interaction.
`VOICEOS_OLLAMA_DEEP_MODEL` is loaded on demand with thinking enabled when the
request explicitly asks for deep analysis, matches a small allowlist of complex
review phrases, or exceeds 600 normalized characters. Audit records distinguish
these as `ollama` and `ollama-deep`.

The optional third tier is `codex-sol`. It is selected only by an explicit
spoken phrase such as “ask Codex” or “use Sol,” so long or complex requests do
not silently consume ChatGPT subscription capacity. A Unix-domain socket
connects the unprivileged gateway to a bridge running as the separately
authenticated `llm` user. The bridge pins `gpt-5.6-sol`, high reasoning,
ephemeral sessions, ignored user configuration and rules, and the read-only
sandbox. Shell, unified execution, web search, apps, hooks, and multi-agent
tools are disabled by fixed command-line options. The bridge exposes no
arbitrary command, tool, or model-selection parameters.

## Current security controls

- Tailscale Serve terminates private HTTPS while the gateway remains on loopback.
- Enrollment codes are random, short-lived, and single-use.
- Device tokens are stored as hashes on the server and encrypted by Android Keystore on the Pixel.
- Device authentication is staged behind `VOICEOS_REQUIRE_DEVICE_AUTH=1`.
- The tool broker exposes typed functions only and has no arbitrary-shell endpoint.
- `project.tests` runs a fixed command and requires explicit approval.
- Approval records bind one request ID to exact validated arguments, expire after five minutes, and reject replay.
- Ollama receives tool schemas but never executes tools itself; proposed calls return to the broker for validation.
- Codex authentication remains readable only by the `llm` account; the gateway
  can access only the group-protected answer bridge socket.
- Codex Sol runs answer-only with agent tools disabled in a fixed read-only
  sandbox and receives no permissioned-tool definitions.
- Turns and security decisions are stored in a local SQLite audit database.

## Canonical meaning layer

`voiceos-ontology` sits between recognized speech and future command dispatch.
It defines provider-neutral intent, entity, alias, unit, argument, confidence,
validation, correction, and final-decision contracts. Its seed catalog covers
playback speed, provider selection, memory, documents, health, disk, network,
services, project tests, and approvals.

The resolver uses deterministic rules and owner-approved aliases first. An
optional local-model fallback must return one structured candidate, which is
validated against exactly the same allowlisted intent and argument schemas. A
model cannot add an intent, entity, argument, unit, or permission. Invalid model
output is rejected and audited rather than passed to tools.

The Rust gateway currently records deterministic ontology decisions in shadow
mode for every text turn. The explicit ontology interpretation endpoint can use
the model fallback when enabled. This separation avoids adding a second model
call to ordinary conversation while shadow data is evaluated. Until the shared
owner migration lands, the authenticated device ID is the temporary ontology
owner key.

Audit endpoints are private-tailnet administration surfaces. They must not be exposed through Tailscale Funnel and need stronger administrator authorization before a multi-user deployment.

## Non-goals for the first slice

- Always-on background microphone access.
- Public internet exposure.
- Autonomous root access.
- Automatic inference of long-term memories without an explicit user request.
- Multi-user support.
- App-store distribution.
