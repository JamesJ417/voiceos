# ADR-0002: VIC sleep and reconstructive-memory authority

- Status: Accepted for v1 implementation
- Date: 2026-08-05
- Scope: cognitive consolidation, provenance, dream quarantine, retrieval, and rollback

## Context

VoiceOS already has canonical Rust conversation storage and a provider router,
but durable memories are limited to explicit user requests. Reconstructive
consolidation requires model judgment while durable memory, doctrine, and
permissions must remain deterministic and auditable.

## Decision

`voiceos-core` is the sole authority for sleep-cycle state and derived memory.
It stores raw normalized evidence and derived records in the existing
`memory.sqlite3`. Models called through the existing provider router may create
typed proposals only. Rust validates provenance, status, protection, duplicate
identity, contradiction records, retrieval quality, and capability absence
before one atomic commit.

Raw events are immutable. Derived memories never replace evidence. Identity and
doctrine proposals always require explicit human approval. Dream associations
are quarantined, omitted from ordinary retrieval, and cannot trigger tools or
become verified facts without a separate promotion decision. Routine cycles do
not invoke Codex.

Python may schedule or proxy a cycle but may not persist cognitive memory.
Android and kiosk clients only use authenticated public Rust-owned routes through
the compatibility gateway.

## Consequences

The implementation adds tables to the existing database but no new database or
competing memory service. Cycles are resumable and rollback affects only their
derived active view. Model quality can improve independently of the trusted
commit boundary. The first retrieval safeguard is lexical and deterministic;
semantic/embedding evaluation remains a later milestone.
