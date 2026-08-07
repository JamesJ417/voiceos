# VIC sleep and reconstructive memory v1 execution plan

- Status: Implemented and locally verified; live rig rollout remains feature-flagged
- Owner: VoiceOS control plane
- Feature flag: `VOICEOS_SLEEP_MEMORY_ENABLED`

## Current architecture

VoiceOS uses a transitional strangler architecture. The Python gateway on port
8787 is the public compatibility ingress and currently owns legacy enrollment,
tool approvals, and provider/capability adapters. The Rust gateway on port 8790
and `voiceos-core` own canonical owner-scoped conversations, messages, rolling
summaries, explicit memories, documents, tasks, artifacts, plans, automations,
skills, provider-run metrics, and append-only execution events. Python already
proxies migrated routes to Rust and uses Rust prepare/commit endpoints around
each production turn.

The Rust provider router registers Gemma as the ordinary Ollama provider,
GPT-OSS as the deep provider, and the restricted Codex bridge only for explicit
exceptional requests. The Python changed-only `ReviewScheduler` is the existing
scheduled-maintenance and attention loop. Android and the kiosk use the public
gateway contract; they do not connect to model runtimes or databases directly.

Existing memory behavior rolls old messages into summaries and extracts only
explicit `remember that ...` statements. There is no existing embedding index,
knowledge graph, contradiction ledger, reconstructive consolidation cycle, or
dream quarantine. Existing skill replay proposes inert skills from legacy audit
evidence and provides the correct review boundary for procedural candidates.

## Integration decision

Extend `voiceos-core` and its existing SQLite database. Do not add a second
database. Rust will own immutable raw-event normalization, cycle state,
proposals, provenance, links, contradictions, retrieval safeguards, atomic
commit, reports, and rollback. Models may only return typed proposal batches.

The Rust gateway will expose authenticated public inspection/action routes and
a loopback-only scheduler route. A provider-neutral proposer will use the
existing `ProviderRouter`: Gemma handles bounded event classification and
episodic/semantic proposals; GPT-OSS handles contradiction criticism,
procedural abstraction, and quarantined dream associations. A deterministic
fixture proposer provides tests without live inference. Codex is not used by
routine cycles.

The Python review scheduler may trigger one cycle during a configured inactive
window. It will not own sleep state or write memory. Android and the kiosk gain
a compact morning-report/status surface rather than a new application.

## Affected modules

- `services/voiceos-core/src/schema.rs`: additive same-database tables and
  indexes.
- `services/voiceos-core/src/sleep_memory.rs`: domain types, deterministic
  authority, state machine, salience, validation, commit, retrieval, and
  rollback.
- `services/voiceos-core/src/provider.rs`: router-backed proposal adapter only;
  no new direct model transport.
- `services/voiceos-gateway-rs/src/api/sleep_memory.rs`: authenticated public
  API and loopback scheduler endpoint.
- `services/voiceos-gateway-rs/src/bootstrap.rs`: feature/configuration wiring.
- `services/gateway/server.py`: compatibility proxy routes.
- `services/gateway/review_scheduler.py`: optional scheduled trigger.
- `contracts/openapi.yaml` and `contracts/route-ownership.json`: public contract
  and authority ledger.
- `apps/android/.../GatewayClient.kt` and `MainActivity.kt`: compact system-page
  status, morning report, dry-run, commit, and rollback controls.
- `apps/kiosk/app/page.tsx`: matching compact system view.

## Storage changes

Additive tables in `memory.sqlite3`:

- `raw_memory_events`: immutable content hash, canonical source reference,
  occurred time, source type, and sanitized payload.
- `sleep_cycles` and `sleep_cycle_events`: resumable state machine, configuration
  snapshot, phase timing/metrics, error, and rollback reason.
- `sleep_event_selection`: salience score and explainable score components.
- `memory_proposals`: typed staged payload, provider/model/prompt version,
  confidence, cognitive status, protection and approval state.
- `cognitive_memories`: derived episodic, semantic, procedural, protected, and
  quarantined dream records with active/quarantine flags and revision lineage.
- `memory_provenance`, `memory_links`, and `memory_contradictions`: explicit
  evidence, graph relations, and non-destructive conflict records.
- `sleep_snapshots`: checksum and prior active-memory identifiers.
- `morning_reports` and `retrieval_quality_results`: inspectable report and
  pre-commit safeguard evidence.

All changes are `CREATE TABLE/INDEX IF NOT EXISTS` migrations. Existing tables
and data are untouched. Raw events have no update/delete API.

## State machine and transaction boundary

Cycles use: `preparing`, `snapshotting`, `selecting_events`, `replaying`,
`extracting_memories`, `forming_connections`, `detecting_contradictions`,
`dreaming`, `validating`, `staging`, `committing`, `reporting`, `completed`,
`failed`, and `rolled_back`.

Each transition is persisted before work begins. Proposal generation is
restartable and idempotent by cycle/evidence hash. Staging validates schemas,
provenance, cognitive status, protection rules, capabilities, duplicate keys,
and retrieval quality. Commit is one SQLite transaction. V1 never edits an
existing cognitive memory during consolidation; it inserts new revisions only.
Rollback therefore deactivates only derived rows created by the target cycle,
restoring its snapshot without affecting subsequent cycles, raw events, or
audit history.

## Model-provider changes

No provider is replaced. The proposer sends strict JSON requests through
`ProviderRouter::select(..., Some("gemma"))` and
`ProviderRouter::select(..., Some("gpt-oss"))`. Provider and model labels,
operation version, usage, and failures are persisted. Invalid JSON or a schema
violation fails closed. Fixture mode is deterministic and opt-in outside tests.

## Scheduler and configuration

The existing changed-only review loop triggers the internal sleep endpoint once
per local inactive window when enabled. Configuration covers enablement, maximum
events, minimum salience, quiet/idle window, model-call budget, and dry-run
default. The cycle declines or pauses when deterministic health/resource input
marks the rig busy. Manual run, dry run, cancel, resume, commit, dream promotion,
and rollback remain public authenticated actions.

Dream promotion is narrowly defined as `dream_association -> working_hypothesis`.
The database rejects direct promotion to `supported_inference` or `verified_fact`.
Those later states require additional evidence and separate validation decisions.

## Application and widget impact

Add one compact “Sleep memory” section to the existing System page and kiosk:
current/last status, counts, errors, morning report, dry-run control, commit and
rollback. Existing navigation, conversation, task, files, and widget APIs remain
unchanged. Ordinary widget rendering is not blocked on sleep status.

## Testing strategy

- Rust fixture tests cover immutable evidence, provenance, dream quarantine,
  protected doctrine, dry-run immutability, interruption, rejection, rollback,
  idempotency, schema rejection, capability rejection, contradiction retention,
  report accuracy, routed Gemma/GPT-OSS calls, and retrieval quality.
- Rust API tests cover auth, route payloads, actions, and internal scheduling.
- Python contract tests cover compatibility proxy and scheduler idempotency.
- OpenAPI/route-ledger tests enforce public contract parity.
- Android unit/build and kiosk lint/build/render tests protect existing clients.
- An ignored/opt-in rig test exercises installed Gemma and GPT-OSS.

## Rollout and rollback

1. Ship disabled and run fixture dry runs against a copied database.
2. Run live-model dry runs on the rig with a bounded event set.
3. Review morning reports and retrieval-quality evidence.
4. Enable manual commit for non-protected proposals.
5. Enable scheduled dry runs, then scheduled commits after an observation window.

Disabling the feature stops new cycles. Cycle rollback is available through the
API. Service rollback uses the existing release snapshot. Additive tables can
remain dormant across a binary rollback; no down migration deletes user data.

## Major risks and mitigations

- False generalization: confidence threshold, provenance, contradiction checks,
  and human-review states.
- Memory poisoning/prompt injection: raw content is untrusted data, model output
  is schema-validated, and tools/capabilities are forbidden in proposals.
- Retrieval regression: compare deterministic representative queries before
  commit and reject duplicate/speculation-promoting staged views.
- GPU contention: bounded calls, GPT-OSS only for selected deep phases, scheduler
  resource gate, and cancel/resume checkpoints.
- Partial commit: single transaction plus snapshot/checksum.
- Privacy leakage: local router only by default, provider audit metadata, prompt
  redaction, and no full private content in operational logs.

## Assumptions requiring live verification

- The rig environment names for Gemma and GPT-OSS match the checked-in examples.
- Port 8790 is the deployed Rust service after the pending release rollout.
- GPU load evidence is available to the scheduler before scheduled commit is
  enabled.
- Existing databases have sufficient free space for immutable raw evidence.

## Implementation result

The vertical slice is implemented across Rust storage and authority, both
existing local-model routes, authenticated gateway APIs, the Python
compatibility proxy, the quiet-hours scheduler, Android, kiosk, OpenAPI,
environment examples, and operations documentation. Seventeen Rust safety
invariants, the full Rust workspace, 90 Python gateway tests, 12 contract tests,
kiosk production builds, and Android unit/build tasks pass locally. Production
enablement and live-model quality evaluation on the rig remain deliberate
rollout steps, not assumptions made by this change.
