# Rust conversation and memory migration

## Decisions

`voiceos-core` is the provider-neutral authority for conversation state. A model
provider never owns history. Each authenticated device owns exactly one active,
server-generated conversation; an Android `session_id` is retained only as a
compatibility alias. Changing from Gemma to gpt-oss or Codex therefore changes
the inference engine, not the conversation.

Context is assembled in this order:

1. VoiceOS system policy.
2. Explicit durable user memories.
3. A rolling summary of older messages.
4. The most recent user and assistant messages.
5. The current request, which is already the newest recent message.

The initial durable-memory extractor records only explicit phrases such as
“remember that …”. This avoids silently treating guesses from a model as user
facts. Later extraction can propose candidate memories, but promotion should be
audited and confidence-gated.

## Private file knowledge

Enrolled devices can upload UTF-8 text, Markdown, JSON, or CSV files up to 5 MB.
The original bytes, SHA-256 hash, filename, media type, ordered passages, and
mode are stored under that device's identity. `profile` passages are pinned into
the private context budget; `reference` passages are selected by query relevance.
Uploaded text is always labeled as untrusted reference data so instructions
inside a document cannot override VoiceOS policy or authorize tools.

Android uses the system document picker, so VoiceOS receives only the file the
user explicitly selects. PDF and Word files remain rejected until sandboxed
extractors and adversarial parser fixtures are added. Deleting a document
cascades to all extracted passages.

## Components

- `services/voiceos-core`: Rust library containing contracts, SQLite storage,
  context assembly, summary/memory policies, legacy-audit import, and providers.
- `services/voiceos-gateway-rs`: compatibility server. Its text-turn response is
  shaped like the existing `/v1/turns/text` API so Android does not need a new
  public protocol.
- `services/gateway`: remains the production gateway for enrollment,
  deterministic tools, approvals, and full audit endpoints during migration.

The Rust gateway can read the existing Python audit database to authenticate
already-enrolled device bearer tokens. Conversation content is stored in a
separate `memory.sqlite3` database during the shadow phase.

The production Python gateway now uses a prepare/commit bridge during this
transition. Before inference, Rust idempotently imports older Python audit turns,
stores the current user message, and returns durable memories, the rolling
summary, recent turns, and relevant documents. After inference, Python commits
the assistant reply to Rust. The bridge fails open if Rust is unavailable, so
tool and approval behavior remains available during rollback, but a healthy
bridge gives the existing Android app persistent provider-neutral context without
changing its public API.

## Provider environment

```text
VOICEOS_OLLAMA_URL=http://127.0.0.1:11434
VOICEOS_GEMMA_MODEL=gemma4-fast:12b
VOICEOS_GPT_OSS_MODEL=gpt-oss:20b
VOICEOS_CODEX_ENABLED=1
VOICEOS_CODEX_SOCKET=/run/voiceos-codex/codex.sock
```

Normal requests route to Gemma, explicit deep-analysis requests route to
gpt-oss, and explicit “ask Codex”/“use Sol” requests route to the existing
answer-only bridge. All providers receive the same `ProviderRequest` messages.

## Staged rollout

1. **Local tests:** compile the workspace and replay representative Python audit
   turns into a temporary Rust database.
2. **Rig shadow:** run Rust on loopback port 8790 with a copy of the Python audit
   database. Exercise it directly without changing Tailscale Serve.
3. **Android canary:** point a debug build or a temporary Tailscale Serve path at
   Rust. The app continues to call `/v1/health` and `/v1/turns/text` unchanged.
4. **Feature parity:** move enrollment, deterministic tools, approvals, audio,
   audit endpoints, credential rotation, and revocation into Rust. Compare
   responses against captured contract fixtures.
5. **HP migration:** stop the Rust service, copy `memory.sqlite3` plus its WAL/SHM
   files using SQLite backup or a clean shutdown, verify checksums, start Rust on
   the HP, and leave Ollama/Codex inference on the GPU rig over Tailscale.
6. **Retirement:** switch Tailscale Serve only after parity tests pass, retain the
   Python service disabled for one rollback window, then remove it from startup.

## Python retirement gate

Python is not retired until Rust supports and tests all existing public routes,
device enrollment/authentication, pending approvals and replay protection,
every allowlisted deterministic tool, complete audit metadata, and operational
backup/restore. Successful text conversation alone is not feature parity.
