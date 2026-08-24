# Project Task: First-Class Image Attachments Across VoiceOS Clients

Status: Active planning; implementation begins after repository audit and contract review.

## Outcome

Enable users to send pictures from the desktop panel apps and phone clients and have VIC receive the image together with the text turn in one conversation message.

## Product behavior

- Desktop: attach-file picker, drag-and-drop, and clipboard image paste.
- Phone: camera capture and photo-library picker.
- Before sending: inline preview, remove, optional crop, and compression/resize controls.
- Upload progress, cancellation, retry, and resumable uploads.
- JPEG, PNG, and WebP in milestone 1.
- Enforce a 15 MB maximum request/object size, with automatic downscaling or compression for oversized source photos where practical.
- Store uploads temporarily and expire unreferenced files.
- Render the attachment inline in the conversation and preserve it on retry without duplicating the turn.

## Shared architecture

1. Client requests an authenticated upload session from the server.
2. Client uploads bytes in resumable chunks over the private authenticated transport.
3. Server validates declared and detected media type, size, dimensions, and content; strips unsafe metadata where supported; stores a temporary object.
4. Client submits a text turn containing an attachment reference and a unique idempotency/request ID.
5. Server associates the attachment with the turn and emits the message/event containing attachment metadata.
6. Clients synchronize the message and recover upload/event state after sleep or network loss.

The server remains authoritative. Clients must not receive provider credentials, server secrets, or direct database access.

## Milestones

### M0 — Verified audit and contract design

Completed audit findings:

- `POST /v1/files` is an authenticated text-document ingestion route, not binary attachment storage. It accepts only `.txt`, `.md`, `.markdown`, `.json`, and `.csv` under a 5 MB limit, then chunks content for retrieval.
- Current Rust text turns accept only text, session/provider fields, and a request ID. Neither the Rust nor Python gateway has `/v1/attachments` or attachment references in message contracts.
- The kiosk/web panel and Android app each have text-document upload support only. The GTK native panel has no file upload path.

Therefore images must use a separate attachment pipeline, not a widened `/v1/files` route. Preserve unrelated working-tree changes. Define the versioned attachment contract and lifecycle states, then add failing contract tests before implementation.

### M1 — Safe Rust attachment foundation

- Add a separate authenticated `POST /v1/attachments` raw-binary route with a conservative 5 MB initial limit; later raise the limit only after resumable storage and client resizing are implemented.
- Return immutable metadata: attachment ID, safe display filename, detected media type, byte size, and SHA-256.
- Store attachment metadata and bytes separately from text documents, owner-scoped and device-provenanced.
- Allow only JPEG, PNG, and WebP after both declared media type and detected file signature match.
- Add attachment IDs to the existing idempotent text-turn request while preserving the current text-only request path.
- Do not forward images to a model or change provider behavior in M1; persist and associate them with the user turn first.
- Add focused backend contract tests plus full relevant workspace tests.

### M2 — Desktop and web/kiosk client

- Picker, drag/drop, clipboard paste, preview, remove, resize/compress, progress, cancel, retry.
- Resumable upload recovery and attachment-aware conversation rendering.
- Verify against a temporary local server and existing client test suites.

### M3 — Android client

- Camera/library picker and Android upload permission handling.
- Same shared contract and retry/resume behavior.
- Device testing for rotation, backgrounding, and network loss.

### M4 — Hardening and release review

- Malware/content scanning integration decision.
- EXIF/privacy policy review and metadata stripping verification.
- Rate limits, quotas, cleanup monitoring, audit events, and abuse cases.
- End-to-end test matrix and documented rollout/rollback plan.

## Explicit exclusions for the first milestone

- Audio capture, speech recognition, playback, and tool execution changes.
- Public-network exposure or unauthenticated upload URLs.
- Permanent general-purpose file storage.
- Video, animated media, arbitrary documents, or provider-specific vision behavior.

## Acceptance criteria

- An authenticated desktop or phone client can upload a supported image, resume after interruption, and attach it to exactly one text turn.
- The server rejects unsupported types, malformed content, oversized payloads, and invalid offsets.
- Retry with the same request ID does not create a duplicate turn.
- Conversation clients display the image and recover it after reconnect.
- Temporary unreferenced uploads expire.
- Focused tests, relevant full tests, and a clean scoped diff pass.

## Current blockers and review gates

- Existing repository has unrelated uncommitted changes; implementation must isolate its diff.
- Exact current route and contract locations must be confirmed by audit.
- Storage backend and Android client readiness need verification.
- No commit, push, deployment, external communication, or destructive cleanup is authorized by this task outline alone.

## Immediate next action

Complete the parallel repository audit and architecture plan, then convert verified findings into the M0 contract and test checklist before writing implementation code.
