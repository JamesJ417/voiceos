# VoiceOS context-integrity inventory

**Scope:** `services/voiceos-core` as inspected on 2026-08-23. This is an implementation inventory, not a future design.

## Provider-bound context flow

1. `ConversationEngine::prepare_owner_turn_with_attachments` resolves an **active owner conversation**, stores explicit `remember that …` memories with that conversation id, appends the user message, rolls a summary, then calls `ConversationStore::context_for_owner` (`src/engine.rs:221-258`).
2. `context_for_owner` now rejects a missing, foreign, or archived conversation before reading any context. For an eligible active conversation it loads:
   - the owner- and active-conversation-scoped rolling summary;
   - memories where both `owner_id` and `conversation_id` match;
   - owner-scoped document retrieval; and
   - recent messages filtered by that already-validated conversation id (`src/store.rs:594-626`).
3. `provider_messages` converts the summary, each memory, document text, and each recent message into separate `ContextClaim`s. `validate_context` rejects missing/mismatched conversation scope, empty content, and confidence/relevance outside `[0,1]` (`src/engine.rs:296-346`, `src/integrity.rs:68-95`).
4. If any claim is rejected, `context_quarantine` records are persisted and `EngineError::Integrity` is returned *before* `Provider::complete`. Otherwise the system prompt, memories, document context, summary, and recent messages are rendered as provider messages (`src/engine.rs:348-376`; `ProviderRequest` is defined in `src/model.rs:116-121`).

## Durable sources and isolation metadata

| Source | Durable representation / writer | Retrieval and enforcement | Current metadata / gap |
|---|---|---|---|
| Conversation messages | `messages`; `append_message_from` and `claim_attachments_for_owner_turn` | `recent_messages` and `messages_through` select by conversation id. Owner context validates that id belongs to the active owner first. | Message rows have conversation id, origin device, request id, timestamp; no per-row durable claim provenance/confidence/relevance. |
| Rolling summaries | `conversation_summaries`; compression in `roll_summary` reads only `messages_through(conversation_id, through_id)` and writes `save_summary_for_owner` | `summary_for_owner` requires matching summary owner id, active owned conversation, and non-empty provenance. Legacy rows lacking those fields are not returned to owner/provider context. | `conversation_id`, `owner_id`, `through_message_id`, `updated_at`, and `conversation-summary://<conversation_id>` provenance are durable. The schema migration adds nullable owner/provenance; null legacy metadata fails closed. |
| Explicit memories | `memories`; extracted in the engine and written by `remember_for_owner_in_conversation` | `memories_for_owner_conversation` requires both ids. Null `conversation_id` legacy rows do not match and therefore fail closed. | Durable owner, conversation, source, created/updated timestamps. There is no stable source-record provenance or confidence/relevance column. |
| Documents | `documents` and `document_chunks`; owner writer is `ingest_text_document_for_owner` | `relevant_document_context_for_owner` queries only `documents.owner_id`; profile chunks are always candidates and reference chunks are lexical-overlap candidates. | Owner-scoped, but intentionally not conversation-scoped. Its assembled string includes filename/mode/passage, not claim-level source ids or quality metadata. |
| Attachments | `attachments` plus `message_attachments`; claimed transactionally by `claim_attachments_for_owner_turn` | Attachment ownership/device/status are checked when attached; attachment records are exposed with conversation messages. | Not currently rendered into `provider_messages`; attachment bytes remain outside this text-context path. |
| Legacy audit recovery | `import_legacy_audit` imports audit turns to the device's resolved conversation and deduplicates with `legacy_imports` | Imported messages enter only their resolved conversation; current owner context eligibility prevents recovering an archived/foreign conversation through `context_for_owner`. | Import is device/session-based legacy recovery, not per-claim provenance metadata. |

## Quarantine and rejection APIs

- `ContextClaim` carries id, conversation scope, source enum, provenance, confidence, relevance, and content. New runtime claims default to `provenance = "runtime"` and confidence/relevance `1.0` (`src/integrity.rs:15-53`).
- `ConversationStore::quarantine_claims` transactionally appends a UUID, original claim fields, reason, and RFC3339 timestamp to `context_quarantine`.
- `quarantined_claims_for_owner` reads records only through the conversation/owner join. The table and `(conversation_id, created_at)` index are migrated by `src/schema.rs:49-63`.
- Provider assembly fails closed on rejected constructed claims. Store-level ineligible recovery fails with `StoreError::InvalidInput`; it does not query or copy foreign/archived content, so it cannot quarantine it under an untrusted scope.

## Intentional/non-owner paths to keep out of provider assembly

`ConversationStore::context`, `summary`, `save_summary`, `memories`, and device-scoped document methods remain public compatibility APIs. They do not apply the owner-active conversation gate or durable summary metadata requirement. Current engine provider assembly uses `context_for_owner`/`summary_for_owner`, not these methods. New provider paths must preserve that rule.

## Evidence and regression coverage

- `tests/conversation_memory.rs::compression_persists_owner_scoped_summary_metadata_across_reopen` proves compression writes owner id and summary provenance, then a reopened SQLite store retrieves only the scoped summary.
- `tests/conversation_memory.rs::reopened_store_rejects_recovery_of_an_archived_summary` proves an archived summary cannot be recovered after reopening; a new active conversation receives a different id.
- `tests/conversation_memory.rs::owner_context_retrieval_is_conversation_scoped_for_summaries_and_memories` covers absent/untrusted scope rejection and no scoped memory/summary retrieval.
- `tests/conversation_memory.rs::rejected_claims_are_durable_and_retrievable_with_provenance_reason_and_timestamp` verifies durable quarantine readback.
- `src/integrity.rs` unit tests cover cross-conversation, unscoped/empty, and invalid confidence claim rejection.
- `tests/conversation_memory.rs::explicit_memories_and_rolling_summaries_are_durable` and `legacy_python_audit_can_be_replayed_idempotently` cover ordinary SQLite reopen and legacy-audit recovery.
