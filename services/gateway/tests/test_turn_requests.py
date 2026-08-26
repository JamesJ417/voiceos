from __future__ import annotations

import unittest

from services.gateway.turn_requests import (
    InvalidTurnRequestId,
    resolve_turn_request_identity,
)


class TurnRequestIdentityTest(unittest.TestCase):
    def resolve(
        self,
        *,
        idempotency_key: str | None = "header-id",
        payload_request_id: object = "payload-id",
        text: str = "hello",
    ) -> tuple[str | None, str | None]:
        return resolve_turn_request_identity(
            idempotency_key=idempotency_key,
            payload_request_id=payload_request_id,
            session_id="session-1",
            text=text,
            provider="hermes",
            attachment_ids=["attachment-1"],
        )

    def test_header_takes_precedence_and_is_trimmed(self) -> None:
        request_id, fingerprint = self.resolve(idempotency_key="  header-id  ")
        self.assertEqual("header-id", request_id)
        self.assertEqual(64, len(fingerprint or ""))

    def test_payload_id_is_the_compatibility_fallback(self) -> None:
        request_id, fingerprint = self.resolve(idempotency_key=None)
        self.assertEqual("payload-id", request_id)
        self.assertIsNotNone(fingerprint)

    def test_missing_id_has_no_fingerprint(self) -> None:
        self.assertEqual(
            (None, None),
            self.resolve(idempotency_key=None, payload_request_id=None),
        )

    def test_rejects_blank_and_oversized_ids(self) -> None:
        with self.assertRaises(InvalidTurnRequestId):
            self.resolve(idempotency_key="   ")
        with self.assertRaises(InvalidTurnRequestId):
            self.resolve(idempotency_key="x" * 201)

    def test_fingerprint_changes_with_request_content(self) -> None:
        self.assertNotEqual(self.resolve(text="first")[1], self.resolve(text="second")[1])


if __name__ == "__main__":
    unittest.main()
