# Context Integrity and Session Isolation Implementation Plan

> **For Hermes:** Execute selected tasks incrementally with tests and verification after each task.

**Goal:** Complete the durable, provenance-aware context-integrity system so rejected claims are inspectable and unrelated memories or summaries cannot enter the active conversation.

**Architecture:** Keep enforcement in the existing VoiceOS Rust core and ConversationStore. Persist quarantine records and retrieval metadata alongside conversation/memory data, then enforce owner and conversation scope at every compression, recovery, and provider-context assembly boundary. Preserve existing attachment, gateway, and normal text-only behavior.

**Tech Stack:** Rust workspace, SQLite-backed ConversationStore, existing context-claim/integrity types, Rust integration tests.

---

## Parent task

Build and verify the complete context-integrity and session-isolation subsystem, including durable quarantine, retrieval metadata, old-summary repair, enforcement at all injection paths, migration safety, and regression tests.

## Individual tasks

### Task 1: Inventory current integrity and storage behavior

Objective: Establish the exact current schema, APIs, and injection paths before changing behavior.

Files to inspect:
- `services/voiceos-core/src/integrity.rs`
- `services/voiceos-core/src/model.rs`
- `services/voiceos-core/src/schema.rs`
- `services/voiceos-core/src/store.rs`
- `services/voiceos-core/src/engine.rs`
- `services/voiceos-core/tests/conversation_memory.rs`

Verification:
- Map every source of summaries, memories, compression/recovery context, and provider-context assembly.
- Identify existing durable quarantine APIs and the missing retrieval metadata/session checks.

### Task 2: Define durable retrieval metadata

Objective: Add explicit metadata identifying claim origin, owner, conversation scope, source record, confidence, relevance, and creation time.

Likely files:
- `services/voiceos-core/src/model.rs`
- `services/voiceos-core/src/integrity.rs`
- `services/voiceos-core/src/schema.rs`
- `services/voiceos-core/src/store.rs`

Tests:
- Add round-trip tests for metadata persistence and validation.
- Add rejection tests for missing, malformed, or cross-owner metadata.

### Task 3: Add schema migration and backward-compatible loading

Objective: Persist the new metadata without breaking existing VoiceOS databases.

Likely files:
- `services/voiceos-core/src/store.rs`
- `services/voiceos-core/src/schema.rs`
- `services/voiceos-core/tests/conversation_memory.rs`

Tests:
- Open a pre-metadata database fixture or equivalent legacy schema.
- Verify migration, reopening, and safe handling of records with incomplete legacy metadata.

### Task 4: Enforce owner and conversation isolation during retrieval

Objective: Ensure memories and summaries are eligible only when their owner and conversation scope match the active request.

Likely files:
- `services/voiceos-core/src/store.rs`
- `services/voiceos-core/src/engine.rs`
- `services/voiceos-core/src/integrity.rs`

Tests:
- Same-conversation claim is accepted.
- Other-conversation claim is quarantined/rejected.
- Other-owner claim is quarantined/rejected.
- Unscoped legacy claim fails closed rather than being injected.

### Task 5: Repair compression and recovery paths

Objective: Prevent contaminated summaries from being created or reintroduced during compression, restart, and recovery.

Likely files:
- `services/voiceos-core/src/engine.rs`
- `services/voiceos-core/src/store.rs`
- `services/voiceos-core/tests/conversation_memory.rs`

Tests:
- Compression preserves conversation scope and metadata.
- Recovery does not load unrelated summaries.
- Reopened stores retain isolation behavior.

### Task 6: Expose safe quarantine inspection and retrieval metadata

Objective: Provide an internal/API-level read path for diagnosing rejected claims without allowing quarantined content into provider context.

Likely files:
- `services/voiceos-core/src/store.rs`
- `services/voiceos-gateway-rs/src/api/` if an external endpoint is warranted
- relevant API tests

Tests:
- Query quarantine by conversation, owner, source, and rejection reason.
- Confirm inspection results are metadata/audit data and are never treated as eligible context.

### Task 7: Add end-to-end regression coverage

Objective: Verify the full provider-context assembly path fails closed for contaminated or invalid context.

Likely files:
- `services/voiceos-core/tests/conversation_memory.rs`
- existing gateway/provider tests as needed

Tests:
- Dave Johnson-style cross-conversation contamination case.
- Invalid confidence/relevance case.
- Durable quarantine retrieval case.
- Normal text-only conversation case remains unchanged.

### Task 8: Run workspace validation and document the completed contract

Objective: Verify the implementation and record the invariants future changes must preserve.

Verification commands:
- `cargo fmt --all -- --check`
- Focused VoiceOS core integrity and conversation-memory tests.
- Full Rust workspace test suite.
- Relevant Python gateway tests if integration boundaries changed.

Documentation:
- Update the relevant VoiceOS architecture or integrity documentation with the isolation invariants, migration behavior, and quarantine semantics.

## Risks and decisions

- Existing working-tree changes are unrelated in several clients and gateway files; do not reset or overwrite them.
- Legacy records must fail closed when required provenance cannot be established; silent promotion of ambiguous data is unsafe.
- A public quarantine endpoint should be added only if there is a concrete UI/operator need; an internal store API may be sufficient for the first slice.
- Keep the implementation scoped to integrity and isolation; avoid unrelated UI or attachment changes.
