# ADR-0003: Rust-owned synthesized VIC doctrine

- Status: accepted for disabled-by-default implementation
- Date: 2026-08-05
- Scope: private source abstractions, doctrine review, activation, and safe reasoning-lens retrieval

## Context

VIC needs coherent decision principles informed by authorized private source material while remaining one independent identity. Source names, text, style, and model output must not become VIC’s persona or bypass protected doctrine review.

## Decision

The existing Rust control plane owns a doctrine namespace in the same owner-scoped SQLite database. Source material is immutable, hashed, private, authorized, and untrusted. Existing Gemma/gpt-oss providers may propose structured candidates but cannot approve or activate them. Deterministic validation and identity/style decontamination precede review. Approval and activation are separate authenticated transitions. Constitutional proposals are protected. Only active doctrine can enter safe runtime lens retrieval, and that projection omits source identities, titles, quotations, raw weighting evidence, and provenance.

VIC does not simulate, quote, imitate, or visibly attribute source individuals. V1 performs no network corpus ingestion.

## Authority hierarchy

1. Existing VIC constitution and explicit protected user values.
2. Authenticated human decisions.
3. Rust lifecycle, validation, and provenance invariants.
4. Active normalized doctrine.
5. Model proposals and source content, which remain untrusted.

Models and schedulers have no doctrine mutation authority.

## Consequences

Doctrine extends existing storage, routing, authentication, ingress, and UI surfaces. New tables do not create a competing memory service. Private audit has a separate feature gate. All flags default off. Revocation preserves provenance and creates review work for materially dependent doctrine.

The initial named profiles are private registry metadata only. No source works or quotations are included or fetched. Retrieval is lexical/domain based in v1; embeddings are not introduced.

## Prohibited transitions

- source content to active doctrine without extraction, decontamination, review, and activation;
- model proposal to approved or active state;
- contaminated candidate to review or activation;
- private source identity to ordinary output;
- extraction to tool, skill, task, notification, or external action;
- sleep cycle to approval, activation, or constitutional modification;
- dream-origin memory to doctrine or verified fact.

## Alternatives rejected

A separate vector database duplicates authority. Prompt-only mentor personas leak identity/style and lack provenance. Automatic activation violates protected doctrine. Direct runtime use of source passages creates copyright, injection, privacy, and attribution risks.
