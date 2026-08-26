# Image attachment upload contract

Status: upload, owner-scoped turn association, and Ollama image-byte delivery implemented. Text-only providers fail closed with `vision_not_supported`.

## Design

The client uploads an image through a resumable, authenticated session. The upload is not visible to the model until finalization succeeds. A finalized attachment may then be referenced by a text turn.

All endpoints require the existing device authentication headers.

### 1. Create upload session

`POST /v1/uploads`

Headers:

- `X-VoiceOS-File-Name`: URL-encoded filename, required
- `Content-Type`: image media type, required (`image/jpeg`, `image/png`, or `image/webp`)
- `X-VoiceOS-Upload-Length`: decimal byte length, required
- `X-VoiceOS-Upload-SHA256`: lowercase hexadecimal SHA-256 of the complete file, required

Response `201`:

```json
{
  "upload": {
    "upload_id": "uuid",
    "filename": "photo.png",
    "media_type": "image/png",
    "byte_size": 123456,
    "sha256": "...",
    "chunk_size": 1048576,
    "received_bytes": 0,
    "status": "created"
  }
}
```

The server rejects unsupported media types, missing metadata, zero-byte files, lengths above 25 MiB, and malformed hashes.

### 2. Upload a chunk

`PUT /v1/uploads/{upload_id}/chunks/{offset}`

Headers:

- `Content-Type: application/octet-stream`
- `Content-Length`: exact chunk size

`offset` is the zero-based byte offset. Chunks must be contiguous, no larger than `chunk_size`, and may be retried idempotently with identical bytes. A conflicting retry returns `409`.

Response `200`:

```json
{
  "upload_id": "uuid",
  "received_bytes": 1048576,
  "next_offset": 1048576,
  "status": "uploading"
}
```

### 3. Finalize upload

`POST /v1/uploads/{upload_id}/finalize`

The server requires all bytes, recomputes SHA-256, stores the attachment, and marks the session finalized. Finalization is idempotent.

Response `201` on first success, `200` on a repeated request:

```json
{
  "attachment": {
    "attachment_id": "uuid",
    "filename": "photo.png",
    "media_type": "image/png",
    "byte_size": 123456,
    "sha256": "...",
    "status": "ready"
  }
}
```

Hash mismatch returns `422`; an incomplete upload returns `409`.

### 4. Attach to a text turn

`POST /v1/turns/text` accepts an optional `attachment_ids` array:

```json
{
  "session_id": "uuid",
  "text": "What is in this image?",
  "attachment_ids": ["uuid"],
  "request_id": "uuid"
}
```

`attachment_ids` must contain finalized, owner-scoped attachments only. The server preserves attachment IDs in the idempotent user-turn record. Ollama receives the owned image bytes as base64 image payloads on the final user message; text-only providers return the typed `vision_not_supported` error before an attachment is claimed.

## Invariants

- Upload sessions and attachments are owner-scoped and authenticated.
- Raw chunks are never exposed through the conversation context endpoint.
- Finalization verifies both declared length and SHA-256.
- Repeated create/finalize/chunk requests are safe to retry.
- The existing text-only turn behavior remains unchanged when `attachment_ids` is absent.
