# Hermes and Buzz integration

## Decision

Hermes is VoiceOS's default agent runtime, not its security or persistence
authority. It runs from a pinned upstream commit as an isolated Python service.
VoiceOS keeps the Rust control-plane contracts for device identity, canonical
conversation memory, tasks, ontology, approvals, audit, and scheduling.

Rewriting Hermes in Rust is intentionally deferred. Its value is its existing
skill, plugin, session, curator, delegation, and gateway ecosystem. Python
dispatch overhead is small compared with local model inference and external
tool latency. A rewrite would create behavior drift and delay useful features.

Buzz is an optional signed collaboration transport. The upstream Rust relay is
kept separate, and VoiceOS calls its JSON CLI through an allowlisted adapter.
Private voice transcripts are not published automatically. Agent messages,
workflow events, approval evidence, and selected artifacts may be published by
an explicit policy-controlled action.

## Request path

1. Android, web, and wall clients authenticate to VoiceOS.
2. Rust memory prepares the owner-scoped conversation context.
3. Deterministic VoiceOS commands run before any model.
4. The Python compatibility gateway starts a Hermes asynchronous run over
   loopback and consumes its server-sent event stream. A stable VoiceOS
   conversation ID scopes the Hermes run.
5. Tool lifecycle events are retained as bounded evidence. A Hermes
   `approval.request` becomes the existing VoiceOS approval card; VoiceOS sends
   only `once` or `deny`, never session-wide or permanent permission.
6. Hermes uses its installed skills and local Ollama model.
7. VoiceOS commits the answer and audit metadata to its canonical stores.
8. Selected agent collaboration events can later be sent to Buzz asynchronously.

## Skill lifecycle

Hermes can read, create, and improve skills in its own persistent
`HERMES_HOME/skills` directory. It does not receive root access, the Docker
socket, Codex credentials, Android device credentials, or direct write access
to VoiceOS databases. Host-changing tools stay behind the VoiceOS approval
broker.

A dedicated worker owned by the Hermes service account snapshots the active
skill tree. When Hermes creates or changes `SKILL.md`, the worker immediately
quarantines that revision and restores the last approved snapshot before the
next run. It validates frontmatter and size, infers required capabilities, and
submits content plus hash, run ID, prior hash, validation evidence, and rollback
method to the Rust proposal store. Approval atomically activates the exact
hashed revision; rejection leaves it quarantined; rollback restores the prior
snapshot or removes a newly created skill. Invalid revisions cannot be
approved.

## Performance rules

- Keep Hermes, Ollama, and VoiceOS on the same host and use loopback HTTP.
- Keep Hermes resident; do not start a Python process per voice turn.
- Reuse the already loaded Gemma model so Hermes consumes no second model copy.
- Consume Hermes SSE events now; expose client token/audio streaming as a
  separate latency optimization without changing the approval contract.
- Keep Buzz writes asynchronous and outside the spoken-response critical path.
- Pin upstream revisions in `ops/agents/vendor-lock.json`; upgrade deliberately
  after upstream tests and VoiceOS contract tests pass.

## Rollout gates

1. Install Hermes under its own service account and configure its local Ollama
   model; verify `/health`, `/v1/skills`, and one isolated chat turn.
2. Set `VOICEOS_PROVIDER=hermes`, restart the gateway, and replay the VoiceOS
   integration/audit tests.
3. Completed: async Hermes run/SSE support maps approval requests into existing
   approve/reject/evidence cards.
4. Completed: Hermes-created and changed skills are mirrored through validation,
   quarantine, evidence, explicit approval, provenance, and rollback.
5. Deferred by design: build and deploy the pinned Buzz relay only when
   multi-agent collaboration is enabled; generate one keypair per agent and
   store private keys outside Git.
