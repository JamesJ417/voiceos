# VIC sleep-memory adversarial review v1

Date: 2026-08-05  
Scope: existing `vic-sleep-memory-v1` only  
Assessment: **pass for limited manual dry-run and scheduled-shadow rollout; fail for unattended commits, automatic dream promotion, and doctrine modification**

## System boundaries reviewed

The review covered the Rust authority and SQLite schema, conversation-event import, provider router and prompts, Rust HTTP routes and authentication, Python scheduler, skill-review handoff, Android and kiosk projections, deployment environment examples, rollback/recovery behavior, and every ordinary sleep-memory retrieval path. It also checked the legacy conversation-memory boundary: sleep memories are stored separately and are not currently injected into normal provider context.

Authoritative state is the raw events, cycles, proposals, cognitive memories, provenance, links, contradictions, retrieval checks, and morning reports in the Rust SQLite store. Models are proposal generators. The Python scheduler is an untrusted caller. Android and kiosk are projections. Approved skills remain separately controlled records; sleep output cannot execute a skill directly.

The database file, service environment, and host administrator are trusted operational boundaries. An administrator able to replace the binary, drop integrity triggers and rewrite the database, or read service secrets can defeat application controls.

## Vulnerabilities found and fixes applied

| ID | Severity | Proof / impact | Resolution |
|---|---:|---|---|
| SM-01 | Critical | The internal run route accepted unauthenticated requests, allowing a local process or exposed listener to request model work and commit mode. | Fixed: exact `VOICEOS_INTERNAL_TOKEN` authentication is mandatory and fails closed when unconfigured. |
| SM-02 | High | Two cycles could overlap between database calls and snapshot the same active view. | Fixed: authority check plus a partial unique database index permits one running/staged/paused cycle per owner. |
| SM-03 | High | A crash after commit but before reporting could leave committed state marked failed and resume could replay the model. | Fixed: commit durably advances to `reporting`; failures preserve that phase; resume rebuilds the report without model execution. |
| SM-04 | High | Dream promotion directly activated data without rechecking proposal, cycle, provenance, or source hashes. | Fixed: transactional eligibility and provenance digest checks; promotion is only to working hypothesis. |
| SM-05 | High | Generic approval plus commit could authorize identity/doctrine. | Fixed: v1 generic approval refuses doctrine and generic commit excludes it unconditionally. |
| SM-06 | High | Unknown JSON fields were ignored and response/config/content/payload/list sizes were unbounded. | Fixed: strict schemas, finite ranges, and explicit byte/count ceilings. |
| SM-07 | High | Supporting/contradicting provenance could reference unselected events; provider provenance could be empty. | Fixed: every evidence role must cite selected immutable events and provider identity is required. |
| SM-08 | High | Malformed raw payloads became JSON null and hashes were not revalidated. | Fixed: parse or digest failure aborts the cycle; append-only triggers remain. |
| SM-09 | Medium | Ignored duplicate inserts were counted and could receive provenance for a nonexistent ID on retry. | Fixed: ignored inserts add no provenance, links, or count. |
| SM-10 | Medium | Scheduling defaulted to commit and ignored GPU use. | Fixed: scheduled mode defaults to dry-run; automatic commit has a separate off-by-default flag; measured GPU utilization at or above 20% skips the cycle. |
| SM-11 | Medium | Rollback could deactivate a cycle with later explicit memory-link dependents. | Partially fixed: known active cross-cycle dependencies refuse rollback. Latent semantic dependencies remain. |
| SM-12 | Low | Reports counted considered rather than selected events and queried a nonexistent rejected validation state. | Fixed. |
| SM-13 | Low | Disabling the feature did not gate most mutation actions. | Fixed: only rollback and cancel remain available for recovery. |
| SM-14 | Medium | A configured remote Ollama URL could receive private events. | Fixed for this feature: routed sleep memory requires loopback Ollama; only local Gemma and gpt-oss aliases are selected. |

## Attack paths tested

1. Dream association promoted to fact: **blocked**. Dreams are inactive/quarantined; explicit promotion produces only a semantic working hypothesis after provenance validation. Database transition guards reject both a direct jump and a two-step `dream -> working hypothesis -> inference/fact` bypass. Dream-origin hypotheses cannot advance further until a separate evidence-backed validation workflow exists; promotion itself never supplies that authority.
2. Indirect identity/doctrine change: **blocked** by approval and commit gates.
3. Tool, capability, permission, or executable smuggling: **blocked from execution**. Requests have no tools, returned tool calls fail the cycle, capability requests invalidate proposals, and skill candidates remain inert.
4. Commit without complete provenance: **blocked**, including all evidence roles and provider provenance.
5. Raw-event alteration/deletion/semantic replacement: **blocked by triggers and checked hashes**.
6. Duplicate/retried cycle delivery: **idempotent** through proposal uniqueness, active dedupe, and insert-result handling.
7. Concurrent cycles: **blocked** at application and database layers.
8. Pause/cancel/termination/power/database failure: memory, provenance, and links commit atomically; post-commit reporting is recoverable without model replay.
9. Rollback removing raw evidence: **blocked**; raw events and provenance are retained.
10. Stale indexes/caches after rollback: **not present in v1**. Retrieval reads SQLite directly and rollback-search tests pass.
11. Malformed model response: unknown fields/enums, optional ambiguity, nested evidence smuggling, non-finite/range violations, oversize, and count exhaustion are **blocked**. Unicode confusables can evade semantic dedupe but cannot bypass authority gates.
12. Silent contradiction overwrite: **blocked**; contradictions remain open review records.
13. Doctrine approved through wrong route: **blocked**.
14. Disabled skill becomes executable: **blocked in the reviewed path**. Sleep creates a separate proposed skill with no capabilities; sleep-proposal approval does not activate it.
15. Dream triggers task/notification/tool/skill/action: **blocked**; no dispatcher consumes quarantined dreams.
16. Private memory sent externally: **blocked** by fixed provider selection and loopback URL validation.
17. Content leaked in logs/reports: **no direct leak found**. Events/reports contain counts, IDs, phases, and bounded errors, not memory content. Database backups remain sensitive.
18. Quiet-hours overlap/GPU use: **improved** by one-cycle and GPU-utilization gates. Unavailable GPU telemetry is a residual risk.
19. UI falsely shows failed transaction committed: **not reproduced**. Android/kiosk show counts after a successful response and refresh authoritative state; failure renders an error.
20. Unauthorized network invocation: **blocked when production device auth is enabled**; internal scheduling has independent auth. Disabling device auth remains unsafe operator configuration.
21. Rollback without dependency understanding: **partially blocked** for explicit links; semantic impact preview remains absent.
22. Dream-promotion validation bypass: **blocked** by type/status/proposal/cycle/provenance/digest checks.
23. Quarantined/rejected data affecting ordinary retrieval: **blocked in all current paths**. Normal search requires active data; legacy conversation context does not read sleep tables.
24. Delayed prompt injection in ordinary memory: **contained, not eliminated**. Stored text has no execution path. Future conversational retrieval must preserve untrusted role separation.
25. Prompt injection in historic events: **tested**. Injection remains inside the user-data JSON message, the system prompt labels it untrusted, tools are empty, and output passes deterministic validation.

## Tests added

The Rust adversarial suite covers append-only evidence, digest mismatch, malformed/oversized proposals, unknown fields, bounded config, nested unselected provenance, capability/tool smuggling, prompt injection, dream quarantine/promotion, doctrine blocking, skill inertness, contradiction non-overwrite, duplicate staging/delivery, concurrency, forced provenance-insert failure, rollback, restart/recovery, and retrieval after rollback. Gateway tests cover internal-token authorization and strict bodies. Scheduler tests cover dry-run default, once-per-day execution, and GPU-busy suppression.

The forced transaction test uses a SQLite trigger to abort provenance insertion after memory insertion begins and proves the transaction rolls back. Restart recovery closes/reopens the database, simulates failure in durable reporting, and proves recovery completes without calling a deliberately failing model fixture.

## Commands run

```text
cargo fmt --all
cargo test --workspace --all-targets
python -m unittest discover -s services/gateway/tests -p "test_*.py"
git diff --check
```

Results: all Rust workspace tests passed, including 27 sleep-memory adversarial tests and 10 Rust gateway tests. All 91 Python gateway tests passed. `git diff --check` passed.

## Residual risks

- Deterministic validation cannot prove every accepted model statement true. Manual review, confidence, and provenance remain necessary.
- Unicode confusables/paraphrases may create semantic duplicates without corrupting structure.
- Rollback understands explicit links, not latent semantic dependence in later model text.
- No embedding/external sleep-memory index exists. Any future index requires quarantine and rollback invalidation tests first.
- Scheduler authentication is a shared service secret, not hardware-backed identity. Protect and rotate environment files.
- Unavailable GPU telemetry does not alone block shadow work; keep rig GPU scheduling enabled.
- The current UI exposes no dream-promotion control, which is safer. Do not add automatic promotion.
- Database administrators remain inside the trusted boundary.

## Final rollout assessment

| Mode | Decision | Conditions |
|---|---|---|
| 1. Manual dry-run only | **Safe / pass** | Local models, device auth, internal token, and output treated as untrusted. |
| 2. Manual routed-model commits | **Conditional pass** | Limited supervised rollout; inspect proposals/provenance, keep backups, never approve doctrine. |
| 3. Scheduled shadow operation | **Safe / pass** | Keep `VOICEOS_SLEEP_AUTOMATIC_COMMITS=0` and retain quiet-hours, health, GPU, and overlap gates. |
| 4. Scheduled automatic low-risk commits | **Not approved / fail** | Needs soak evidence and rollback-impact UX; the capability stays behind an explicit off-by-default flag. |
| 5. Automatic dream promotion | **Not permitted / fail** | Promotion must remain explicit and provenance-validated, and can produce only a working hypothesis—not an inference or fact. |
| 6. Automatic doctrine modification | **Not permitted / fail** | V1 intentionally has no executable doctrine approval or commit path. |

Final assessment: **suitable for limited production dry-run and shadow evaluation, and conditionally for carefully supervised manual commits. Not suitable for unattended authoritative consolidation.**
