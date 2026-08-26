"""Pure request-identity helpers for durable text turns."""

from __future__ import annotations

import hashlib
import json


class InvalidTurnRequestId(ValueError):
    """Raised when a supplied turn request ID violates the gateway contract."""


def resolve_turn_request_identity(
    *,
    idempotency_key: str | None,
    payload_request_id: object,
    session_id: str,
    text: str,
    provider: str | None,
    attachment_ids: list[str],
) -> tuple[str | None, str | None]:
    request_id = idempotency_key
    if not request_id and isinstance(payload_request_id, str):
        request_id = payload_request_id
    request_id = request_id.strip() if request_id else None
    if request_id is not None and (not request_id or len(request_id) > 200):
        raise InvalidTurnRequestId
    if request_id is None:
        return None, None

    fingerprint = hashlib.sha256(
        json.dumps(
            {
                "session_id": session_id,
                "text": text,
                "provider": provider,
                "attachment_ids": attachment_ids,
            },
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    ).hexdigest()
    return request_id, fingerprint
