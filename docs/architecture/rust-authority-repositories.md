# Rust authority repository boundaries

## Purpose

Doctrine and reconstructive memory have different policy rules, but both persist through the
server-owned `ConversationStore`. Their authority services must not duplicate row decoding,
retrieval filters, provenance joins, or report lookup logic.

## Current boundary

- `doctrine.rs` owns authorization, extraction, validation, review, activation, and revocation
  policy.
- `doctrine_repository.rs` owns doctrine queries, row decoding, active-doctrine filtering,
  reasoning-lens lookup, status aggregation, and provenance projections.
- `sleep_memory.rs` owns cycle orchestration, proposal validation, commit transactions,
  promotion rules, rollback rules, and phase transitions.
- `sleep_memory_repository.rs` owns raw-event verification, ordinary/dream retrieval filtering,
  provenance hydration, cycle lookup, cycle-event projection, and morning-report lookup.

The repository modules are crate-private. API handlers continue to call the authority services,
so no endpoint can bypass policy by calling persistence directly.

## Safety rules

1. Dream visibility remains an explicit repository argument and defaults to false at API call
   sites.
2. Raw event hashes are verified inside the repository before events leave the persistence
   boundary.
3. Doctrine row decoding and active filtering have one implementation.
4. Write-side cycle commit and rollback transactions remain together until their repository
   extraction can preserve the existing atomicity and forced-failure tests.
5. Repository extraction must not make repository types public or injectable from API code.

## Next extraction

Move write-side SQL into narrow repository commands one transaction at a time. Start with
doctrine source registration and cycle metadata transitions. Move sleep-memory commit and
rollback last because they span provenance, cognitive memories, links, reports, and lifecycle
events in a single authoritative operation.
