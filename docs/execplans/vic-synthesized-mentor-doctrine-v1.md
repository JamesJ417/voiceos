# VIC synthesized mentor doctrine v1 execution plan

Status: production-gated foundation implemented; conversational recommendation integration deferred  
Owner: Rust VoiceOS authority  
Default rollout state: disabled

## Current architecture

VoiceOS has one persistent VIC identity, an owner-scoped Rust SQLite authority, immutable memory events, provenance-bearing cognitive memory, protected doctrine gates, contradiction and dream quarantine, local Gemma and gpt-oss providers behind the existing router, Python public ingress, authenticated Android/kiosk clients, reviewed skills, and OpenAPI/ownership parity checks. There is no embedding index, doctrine ledger, belief ledger, or knowledge graph. Doctrine sources require stricter authorization and retrieval separation than ordinary documents.

## Proposed architecture

Extend the Rust store with one private doctrine namespace: source profiles and authorized records; immutable hashed source passages; structured local-model extraction; deterministic identity/style decontamination; normalized candidates with evidence, dissent, contradictions, and revisions; explicit review followed by separate activation; public-safe active-doctrine/lens retrieval; private provenance behind a separate gate; and evaluations for leakage, injection, sycophancy, fidelity, revision, contradiction handling, and removal.

No source text or candidate can write identity, activate doctrine, call tools, enable skills, create tasks, or perform external actions.

## Boundaries and ingestion

Trusted boundaries are Rust authority code, schema constraints, authenticated decisions, protected configuration, and deliberately supplied authorization metadata. Source content, metadata, API bodies, scheduler calls, UI state, and all model output are untrusted.

V1 accepts only explicit user-supplied or separately authorized local content. It performs no crawling, downloading, transcription, or network ingestion. Registration requires an approved profile, approved authorization status and basis, a bounded source type, and content bytes. Hash dedupe, UTF-8 validation, immutable boundaries, revocation, and private provenance are mandatory. The supplied requirement file ends at “no automatic”; this plan applies the stricter interpretation: no automatic ingestion, approval, activation, identity change, tool use, or external action.

## Data model and migrations

- doctrine_source_profiles: private names, visibility/identity policies, uses, domains, authorization/review/ingestion state, version, counts, and timestamps.
- doctrine_source_records and doctrine_source_passages: private origin metadata, authorization, content hashes, managed location, quality, dedupe, immutable chunks, active/revoked/extraction state.
- doctrine_candidates: structured proposition, domain/type/rule/rationale, conditions, exceptions, counterexamples, risk/time/ethics, diversity, model/prompt, confidence, abstraction/style/identity scores, protected lifecycle, cycle, and revision lineage.
- doctrine_candidate_sources: supporting and contradicting private provenance.
- doctrine_contradictions: unresolved tensions without false consensus.
- doctrine_lenses: stable public-safe lens identifiers and domain weights.
- doctrine_evaluations and doctrine_runs: evaluation evidence and consolidation counts.

Recommendation-consistency storage is intentionally deferred until runtime doctrine retrieval is enabled. The existing conversation and execution-event authorities remain the only recommendation record in this slice; the plan does not claim a second ledger that has not been implemented.

Raw source/passages and provenance are private. Only active doctrine is available to runtime retrieval.

## Affected modules and APIs

VoiceOS core owns schema, doctrine authority, validation, decontamination, lifecycle, retrieval, revocation, and tests. The Rust gateway owns feature flags and authenticated doctrine routes using the existing provider router. Python remains a compatibility proxy. OpenAPI and route ownership change together. Android/kiosk receive only a compact System section. Sleep may optionally stage candidates and count results, never approve or activate them.

Authenticated doctrine routes cover status, private profiles and records, processing, candidate review/decision/status, contradictions, active doctrine, safe lenses, evaluations, and private provenance. Private identity routes fail unavailable unless source-audit mode is enabled. Mutations fail unavailable unless their specific flag is enabled.

## Model strategy

Gemma performs bounded topic/domain/candidate extraction. GPT-OSS performs bounded critique, exception, causal, contradiction, and contamination review. Both are selected through the existing provider router. Source text is a user-role JSON value explicitly marked untrusted; prompts contain no secrets and tools are empty. Codex is not used for routine extraction.

## Review, sleep, and runtime

Candidates progress through extracted, decontamination_failed, normalized or disputed, awaiting_review, approved, active, superseded, rejected, or archived. Review and activation are separate authenticated transitions. Constitutional candidates are protected and never automatic. Request-revision creates history rather than rewriting evidence.

The sleep path consumes doctrine only when doctrine, extraction, and sleep-integration flags are all enabled and the sleep cycle is an explicit commit. It selects at most five active, authorized pending records, atomically claims each record, routes extraction through Gemma and gpt-oss, stages protected candidates, and reports counts only. Dry runs never process doctrine, and sleep never approves or activates candidates. Runtime retrieval is implemented behind its own disabled flag and returns active abstract doctrine and safe lens names only. Private provenance requires explicit audit mode.

## Evaluation and risks

Fixtures cover identity/name/quote/style leakage, hostile source instructions, unauthorized ingestion, lifecycle bypass, retrieval leakage, revocation, contradiction preservation, weighting, recommendation stability, and evidence-based revision. Major risks are copyright ingestion, prompt injection, source-name/style leakage, false consensus, model-generated doctrine, and revocation. Controls are no network ingestion, explicit authorization, hashing, strict schemas, no tools, decontamination, dissent preservation, separate review/activation, feature flags, and retained provenance.

## UI, configuration, and rollout

The existing System view gains a compact Doctrine card with active/review/contamination/contradiction counts, last run, evaluation state, processed records, and authorization warnings. Review shows structured fields and approve/reject/revise, with private provenance hidden unless authorized.

Independent off-by-default flags control doctrine availability, extraction, sleep integration, runtime retrieval, and private source audit.

Rollout order: schema/core tests; private authorized fixtures; manual extraction dry runs; review without activation; explicit test activation; UI; scheduled shadow extraction; separately reviewed runtime use. No automatic doctrine activation is planned.

## Rollback

Disable all doctrine flags, deactivate affected doctrine while retaining candidates/provenance, revoke records without deleting audit history, and revert application binaries. Runtime retrieval reads active doctrine only, so disabling runtime use immediately removes doctrine influence without rewriting conversation memory.
