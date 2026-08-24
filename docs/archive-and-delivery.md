# VoiceOS Archive and Agent Mail Delivery

Status: foundation prepared; credentials and provider connection intentionally not configured.

## Goals

1. Preserve every generated file before delivery.
2. Make each archived item content-addressed and verifiable with SHA-256.
3. Keep delivery retryable and separate from file creation.
4. Require explicit approval immediately before sending to an external recipient.

## Existing VoiceOS integration

VoiceOS already has canonical `artifacts` records, task-artifact attachments, job IDs,
execution events, and SHA-256 fields in `services/voiceos-core`. The archive adapter
should create a durable local object first, then record its `voiceos://archive/<id>` URI
through the existing artifact record path. The task attachment can point to the same URI.

## Proposed local layout

`$VOICEOS_ARCHIVE_ROOT/objects/<sha256>` — immutable file bytes
`$VOICEOS_ARCHIVE_ROOT/manifests/<artifact-id>.json` — metadata and delivery history
`$VOICEOS_ARCHIVE_ROOT/outbox/` — approved-but-not-yet-accepted delivery attempts

The archive root must be outside build output and backed up by the operator. Never put
credentials in manifests or logs.

## Delivery adapter

The first adapter will be Agent Mail, isolated behind a provider interface. It will:

- accept an archive artifact ID, recipient, subject, and body;
- verify the object hash before sending;
- create an idempotency key from artifact ID plus recipient plus message revision;
- retry transient failures without recreating the file;
- record accepted, failed, and retrying states;
- expose a read-back/status operation for verification.

No provider account, recipient, API key, or send operation is configured in this slice.
Those are deferred until the computer is available and the connection details are supplied.

## Connection checklist

- Confirm Agent Mail endpoint and authentication method.
- Choose the durable archive root and backup location.
- Connect credentials through the approved secret store; do not place them in `.env` committed files.
- Configure a verified sender identity and a test recipient.
- Send one approved test message with a non-sensitive test artifact.
- Read back the provider status and archive manifest before declaring success.

## Failure behavior

If delivery is unavailable, the archive remains authoritative and the item stays in the
outbox for retry. If the provider accepts a message but status cannot be read back, report
`accepted_unverified`, never `delivered`. Duplicate sends are prevented by the idempotency
key and must be checked again after restart.
