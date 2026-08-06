# VIC sleep and reconstructive memory v1

VoiceOS now contains a feature-flagged, end-to-end vertical slice for bounded memory consolidation. Rust is the sole authority for raw events, cycle state, proposals, derived memories, provenance, graph links, contradictions, reports, commits, and rollbacks. The Python gateway only proxies the public contract and schedules eligible runs.

## Safety model

- Source events are append-only. SQLite triggers reject updates and deletes.
- Gemma and GPT-OSS receive bounded event selections through the existing provider router. They can only return typed proposals; sleep requests contain no tools, and any returned tool call fails the cycle.
- Every memory has an explicit kind, cognitive status, confidence, provider, operation version, and raw-event provenance.
- Dream associations are stored inactive and quarantined. Normal retrieval excludes them.
- Identity/doctrine and skill proposals require explicit review. Scheduled cycles cannot activate them.
- Validation and a retrieval-preservation check run before commit.
- Derived changes commit in one SQLite transaction. Failure leaves the pre-cycle active view unchanged.
- Rollback deactivates only memories and links derived by the selected cycle. Raw events remain intact.

## Operations

The feature is disabled by default. On a non-production candidate first, set:

```text
VOICEOS_SLEEP_MEMORY_ENABLED=1
VOICEOS_SLEEP_MODEL_MODE=routed
```

For the once-per-day healthy quiet-hours scheduler, separately set:

```text
VOICEOS_SLEEP_SCHEDULER_ENABLED=1
```

The scheduler uses the existing outreach quiet-hour policy and records one run per local date. Manual controls remain available in Android and the kiosk under System → VIC sleep cycle.

Public endpoints:

- `GET /v1/memory/sleep/cycles/current`
- `POST /v1/memory/sleep/cycles` with `dry_run` or `commit`
- `GET /v1/memory/sleep/cycles/{cycle_id}`
- `POST /v1/memory/sleep/cycles/{cycle_id}/actions`
- `GET /v1/memory/morning-report`
- `GET /v1/memory/search?q=...`

The internal scheduler calls `POST /internal/v1/memory/sleep/run` directly on the Rust control plane.
Cycle actions include pause, resume, cancel, approved dry-run commit, rollback,
proposal decisions, and explicit dream promotion.

## Rollout and rollback

1. Back up `memory.sqlite3` and verify the backup can be opened.
2. Deploy with both flags off; migrations are additive.
3. Enable `VOICEOS_SLEEP_MEMORY_ENABLED=1` on a candidate and run a dry run.
4. Inspect the morning report, provider evidence, rejected proposals, dreams, and contradictions.
5. Run a manual commit and test retrieval.
6. Test the cycle rollback action.
7. Enable the scheduler only after the candidate passes.

To disable immediately, set both flags to `0` and restart the corresponding services. Existing derived records remain auditable. Use cycle rollback for a bad consolidation; restore the database backup only for migration-level recovery.

## Known v1 limits

- Retrieval uses deterministic lexical matching; embeddings and graph traversal are future work.
- Contradictions are surfaced for review but are not automatically resolved.
- Dream promotion is explicit and currently promotes into a working semantic hypothesis.
- The scheduler uses the existing quiet-hours window rather than a separate sleep window.
- The fixture generator exists only for tests and local development. Production defaults to routed Gemma/GPT-OSS when enabled.
