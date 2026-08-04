# ADR-0001: Service ownership and migration to the Rust control plane

- Status: Accepted
- Date: 2026-08-03
- Scope: VoiceOS gateway, domain state, agent execution, providers, and capability workers

## Context

VoiceOS currently exposes a Python HTTP gateway while a Rust gateway owns conversation memory, tasks, documents, ontology, and skill proposals. Hermes runs agent workflows and can create or modify skills. This transitional design allowed rapid delivery, but several responsibilities overlap: both gateways can process turns, Python and Rust write separate SQLite databases, and Hermes has its own run and skill state.

Without explicit ownership, a client request can be interpreted, authorized, recorded, or retried by more than one service. That makes conversation continuity, approval enforcement, event ordering, recovery, and rollback difficult to prove.

## Decision

Rust is the target VoiceOS control plane and the sole long-term owner of the public API contract. Python and Hermes remain bounded internal adapters during migration.

### Rust owns

- Public HTTP and SSE ingress, device enrollment, authentication, and rate limits.
- Canonical conversations, messages, summaries, memories, tasks, plans, documents, ontology decisions, aliases, approvals, events, and audit evidence.
- Provider selection policy, idempotency, permission decisions, capability grants, and response assembly.
- The authoritative database schema, numbered migrations, backup compatibility, and recovery cursors.
- Mirrored Hermes run state and skill proposals used by VoiceOS clients.

### Python owns temporarily

- The current public ingress on port 8787 while clients are migrated without changing their API.
- Compatibility translation for legacy Android and kiosk payloads.
- Adapters for capability workers or provider bridges that do not yet have Rust parity.

Python must not gain new authoritative domain tables. Existing Python audit, enrollment, check-in, event, and approval records move to Rust route by route. Once a route migrates, Python may proxy it but may not transform or persist a second authoritative result.

### Hermes owns

- Asynchronous agent reasoning runs and their transient execution context.
- Agent planning, tool proposals, and skill authoring inside its restricted workspace.
- Hermes-native logs needed to diagnose its runtime.

Hermes is not a public API gateway, identity provider, permission authority, task database, conversation database, or final skill registry. A Hermes tool proposal is inert until Rust validates it and issues any required approval. Created or changed skills are mirrored into Rust with content hash, source run, evidence, validation results, approval state, and rollback provenance.

### Capability workers own

Speech-to-speech and Crawl4AI workers own their isolated runtime mechanics. They accept only device identity and bounded typed requests from the control plane. They do not own conversations, permissions, or durable user memory. Retrieved web content is evidence marked as untrusted data and cannot issue instructions.

## Data ownership

| Data | Current authority | Target authority |
| --- | --- | --- |
| Device enrollment and credentials | Python SQLite | Rust |
| Conversations, messages, summaries, memories | Rust SQLite | Rust |
| Tasks, documents, ontology, skill proposals | Rust SQLite | Rust |
| Legacy turns, approvals, client events, daily check-ins | Python SQLite | Rust |
| Hermes runs and raw runtime logs | Hermes | Hermes, mirrored metadata in Rust |
| Approved skills and provenance | Split Rust/Hermes | Rust |
| Capability lifecycle evidence | Worker logs | Worker evidence indexed by Rust |

## Public-route migration rule

The machine-readable route inventory is [route-ownership.json](../../contracts/route-ownership.json). Every public operation must appear in both that inventory and `openapi.yaml`. The Python ingress must implement or proxy every operation until the Tailscale service is switched to Rust.

A route migrates only when:

1. Rust passes contract tests for the same request, response, authentication, idempotency, and error behavior.
2. Existing Python state is imported idempotently and the import count is auditable.
3. Android and kiosk tests pass against the Rust implementation without client API changes.
4. SSE cursor recovery and duplicate-request behavior are verified.
5. The deployed release has a health-checked rollback artifact.

## Migration sequence

1. Establish the versioned baseline, OpenAPI contract, ownership inventory, and CI gates.
2. Move enrollment/authentication and the public event ledger to Rust.
3. Move approvals, daily check-ins, plans, and remaining Python audit records to Rust.
4. Move provider orchestration and capability proxying behind Rust interfaces.
5. Run Python and Rust contract suites against the same fixtures and compare normalized results.
6. Switch the private Tailscale ingress from Python to Rust while retaining Python as a rollback adapter.
7. Remove Python persistence, then retire the Python gateway after an observed parity period.

## Invariants

- One authoritative writer exists for every record type.
- The original user phrase, canonical interpretation, confidence, corrections, provider, tool requests, approvals, results, errors, timing, and provenance remain auditable.
- Models propose typed operations; they never bypass Rust validation or permission policy.
- Privileged operations require a scoped, expiring, single-use capability and are never executed by Hermes as root.
- Public API changes require OpenAPI and contract-test changes in the same commit.
- Client-visible IDs and event cursors remain stable during migration.

## Consequences

This decision favors an incremental strangler migration over a rewrite. Python remains operational longer, but each migrated route has an explicit owner and rollback boundary. Hermes remains replaceable, and local or cloud models can change without changing conversation, permission, memory, or task semantics.
