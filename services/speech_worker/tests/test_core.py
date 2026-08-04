from __future__ import annotations

import unittest

from services.speech_worker.core import SessionRegistry, SpeechSessionRejected


class SpeechSessionTest(unittest.TestCase):
    def test_sessions_are_device_and_conversation_bound(self) -> None:
        registry = SessionRegistry(ttl_seconds=60, max_sessions=1)
        session = registry.create(device_id="pixel", conversation_id="shared-owner-conversation")
        self.assertEqual("pixel", session.device_id)
        self.assertEqual("shared-owner-conversation", session.conversation_id)
        self.assertEqual(24_000, session.input_sample_rate_hz)
        self.assertEqual("ogg_opus_mono", session.codec)
        self.assertEqual("moshi-websocket-v0", session.transport_protocol)
        self.assertEqual(0, session.protocol_version)

    def test_capacity_is_bounded(self) -> None:
        registry = SessionRegistry(max_sessions=1)
        registry.create(device_id="pixel", conversation_id="one")
        with self.assertRaisesRegex(SpeechSessionRejected, "capacity"):
            registry.create(device_id="wall", conversation_id="two")

    def test_blank_identity_is_rejected(self) -> None:
        registry = SessionRegistry()
        with self.assertRaisesRegex(SpeechSessionRejected, "required"):
            registry.create(device_id="", conversation_id="one")

    def test_session_can_be_removed_when_a_gpu_lease_fails(self) -> None:
        registry = SessionRegistry()
        session = registry.create(device_id="pixel", conversation_id="one")
        registry.remove(session.session_id)
        self.assertIsNone(registry.get(session.session_id))

    def test_pending_session_expires_after_connection_grace(self) -> None:
        registry = SessionRegistry(connection_grace_seconds=0)
        session = registry.create(device_id="pixel", conversation_id="one")
        with self.assertRaisesRegex(SpeechSessionRejected, "not_found"):
            registry.claim(session.session_id, "pixel")

    def test_session_can_only_be_claimed_once_by_its_device(self) -> None:
        registry = SessionRegistry(connection_grace_seconds=60)
        session = registry.create(device_id="pixel", conversation_id="one")
        self.assertEqual(session, registry.claim(session.session_id, "pixel"))
        with self.assertRaisesRegex(SpeechSessionRejected, "already_connected"):
            registry.claim(session.session_id, "pixel")

    def test_connected_session_renewal_extends_its_expiry(self) -> None:
        registry = SessionRegistry(ttl_seconds=60)
        session = registry.create(device_id="pixel", conversation_id="one")
        registry.claim(session.session_id, "pixel")
        renewed = registry.renew(session.session_id)
        self.assertIsNotNone(renewed)
        self.assertGreaterEqual(renewed.expires_at_unix, session.expires_at_unix)


if __name__ == "__main__":
    unittest.main()
