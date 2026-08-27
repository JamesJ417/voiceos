# Fieldy → VIC Webhook Backend Roadmap

Status: live foundation and intelligence upgrade implemented. The remaining
items in this document are follow-on privacy, retention, and operator-surface
work rather than prerequisites for the active webhook.

## Implemented intelligence path

- Signed/query-token ingress normalizes Fieldy payload variants and deduplicates event retries.
- Rust assembles nearby chunks by session/recording and activity window, retaining speaker segments and chunk provenance.
- Analysis begins only after the conversation has been quiet for 330 seconds.
- The classifier receives bounded active-project, open-task, relevant-memory, and pending-proposal context.
- Project IDs are owner-scoped and active-state validated by Rust before persistence.
- Exact normalized proposal repeats are durably merged across conversations; semantic repeats are filtered against bounded task/proposal context before insertion.
- Approval closes every supporting capture and carries the project link into the resulting task or review record.
- All extracted items remain review-only until explicit approval.

## Goal

Accept finalized Fieldy transcripts through a dedicated authenticated webhook, normalize and deduplicate them, place them in a private review buffer, and require explicit approval before creating tasks, focus captures, or durable memories.

## Existing integration points

- Rust gateway router: `services/voiceos-gateway-rs/src/api/mod.rs`
- Existing authenticated device routes: `services/voiceos-gateway-rs/src/api/auth.rs`
- Existing conversation processing: `services/voiceos-gateway-rs/src/api/turns.rs`
- Existing owner-scoped persistence: `services/voiceos-core/src/store.rs`
- Existing schema/migrations: `services/voiceos-core/src/schema.rs`
- Existing task/focus APIs: `services/voiceos-gateway-rs/src/api/tasks.rs` and `focus.rs`
- Existing approval authority: Rust gateway/core, not the Python compatibility layer

## Backend work packages

### 1. Dedicated ingress contract

Add a Rust-owned route, separate from `/v1/turns/text`:

`POST /v1/webhooks/fieldy/transcripts`

Initial request contract:

```json
{
  "event_id": "fieldy-event-id",
  "occurred_at": "2026-08-24T12:00:00Z",
  "transcript": "final transcript text",
  "recording_id": "optional-fieldy-recording-id",
  "session_id": "optional-fieldy-session-id",
  "speakers": [],
  "metadata": {}
}
```

The adapter must preserve the original payload, source (`fieldy`), event ID, and timestamps. It must reject empty transcripts, oversized payloads, missing event IDs, and unsupported event types.

### 2. Webhook authentication

Support a dedicated Fieldy credential, not a device bearer token. The preferred first version is:

- `X-Fieldy-Signature: sha256=<hex HMAC-SHA256>`
- `X-Fieldy-Event-Id: <stable event ID>`
- secret supplied only through gateway configuration/environment
- constant-time signature comparison
- bounded request body before parsing

Do not log the secret or full transcript. Return the same failure shape for malformed signatures and unknown credentials.

### 3. Durable intake/review buffer

Add a Rust-owned table, for example `fieldy_transcript_intake`, with:

- internal intake ID
- owner ID
- source
- event ID and unique `(owner_id, source, event_id)` constraint
- received/occurred timestamps
- raw payload JSON, encrypted or access-controlled as appropriate
- normalized transcript text
- status: `received`, `reviewing`, `approved`, `discarded`, `expired`, `failed`
- retention/expiry timestamp
- processing error and review metadata

The first successful duplicate should return an idempotent response. A repeated event must never create another conversation, task, memory, or notification.

### 4. Normalization and provenance

Create a Fieldy adapter that maps provider payload variants into one internal struct. Keep provider-specific parsing out of task/memory logic. Every downstream proposal must carry:

- source: `fieldy`
- intake ID
- original event ID
- conversation scope
- owner scope
- original transcript text
- confidence and extraction timestamp

Cross-owner or cross-conversation claims must fail closed into the existing context quarantine path.

### 5. Review/proposal API

Add owner-authenticated routes:

- `GET /v1/fieldy/intake`
- `GET /v1/fieldy/intake/{intake_id}`
- `POST /v1/fieldy/intake/{intake_id}/analyze`
- `POST /v1/fieldy/intake/{intake_id}/decision`
- `DELETE /v1/fieldy/intake/{intake_id}`

Analysis may propose tasks, commitments, appointments, worries, ideas, or memory candidates. It must not commit any of them. The decision endpoint must use the existing approval/policy authority and idempotency rules.

### 6. Downstream actions

Only after approval:

- explicit action/commitment → bounded task or `/v1/focus/captures`
- interruption/restart signal → focus/recovery event
- durable preference/fact → memory proposal/commit flow
- ordinary conversation → retain only in the configured temporary buffer

Tasks must satisfy the existing observable-outcome and estimated-duration requirements.

### 7. Retention and privacy controls

Implement configurable retention with a safe default, explicit discard, and expiry cleanup. Redact or classify sensitive content before displaying review summaries. Do not automatically place Fieldy transcripts in durable memory. Add audit events for receive, duplicate, analyze, approve, discard, expire, and failure.

### 8. Observability and operations

Add metrics/log fields for source, event ID hash, intake ID, status, latency, and failure category. Never emit full transcript text, signatures, or secrets to ordinary logs. Add a health/config diagnostic that reports whether Fieldy ingress is configured without revealing the secret.

## Build order

1. Contract types and fixture payloads.
2. Signature verifier and bounded-body middleware.
3. Schema migration and store methods with unique-event idempotency.
4. Dedicated authenticated ingress route returning `202 Accepted` only after durable intake write.
5. Intake list/detail/discard routes.
6. Deterministic proposal extraction and approval integration.
7. Fieldy-specific contract tests and end-to-end test with a fake signed request.
8. HTTPS/tunnel deployment configuration and Fieldy Developer Settings hookup.
9. Live test event, then retention and privacy review.

## Acceptance tests

- valid signed event is normalized and stored once
- invalid/missing signature is rejected
- empty, oversized, malformed, and unsupported events are rejected
- duplicate event returns the original intake result and performs no downstream action
- owner authentication cannot read another owner’s intake
- transcript is not written to durable memory without approval
- approved task proposal satisfies task validation and records provenance
- discard and expiry remove or anonymize transcript content according to policy
- logs and public events contain no transcript body or secret
- OpenAPI and route-ownership contracts are updated together

## Current activation status

The reviewed implementation now includes the public signed ingress route, Fieldy payload normalization, durable conversation assembly, quiet-window background extraction, review-only proposal persistence, explicit approval boundaries, OpenAPI, route ownership, and focused tests.

The installed local gateway has the Fieldy webhook configured and is healthy. It remains bound to localhost; Fieldy delivery is not activated until a private signed endpoint is entered in Fieldy Developer Settings and a controlled live event is sent. Do not expose the root gateway. Keep the normal gateway tailnet-only and expose only `/v1/integrations/fieldy/transcripts` through a dedicated Funnel path if remote Fieldy delivery is required.

Follow-on hardening remains: configurable retention cleanup/redaction, a Fieldy-specific operator list/detail/discard surface, and moving public ingress ownership from the Python gateway into the Rust control plane.
