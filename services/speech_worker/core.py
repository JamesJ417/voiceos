from __future__ import annotations

import time
import uuid
from dataclasses import asdict, dataclass, replace


class SpeechSessionRejected(ValueError):
    pass


@dataclass(frozen=True)
class SpeechSession:
    session_id: str
    device_id: str
    conversation_id: str
    input_sample_rate_hz: int
    output_sample_rate_hz: int
    codec: str
    transport_protocol: str
    protocol_version: int
    backend: str
    created_at_unix: int
    connection_deadline_unix: int
    expires_at_unix: int

    def as_dict(self) -> dict[str, object]:
        return asdict(self)


class SessionRegistry:
    def __init__(
        self,
        *,
        backend: str = "moshi-candle-q8",
        ttl_seconds: int = 900,
        connection_grace_seconds: int = 30,
        max_sessions: int = 8,
    ) -> None:
        self.backend = backend
        self.ttl_seconds = ttl_seconds
        self.connection_grace_seconds = connection_grace_seconds
        self.max_sessions = max_sessions
        self._sessions: dict[str, SpeechSession] = {}
        self._connected: set[str] = set()

    def create(self, *, device_id: str, conversation_id: str) -> SpeechSession:
        self.expire()
        if not device_id.strip() or not conversation_id.strip():
            raise SpeechSessionRejected("device_and_conversation_are_required")
        if len(self._sessions) >= self.max_sessions:
            raise SpeechSessionRejected("speech_capacity_reached")
        now = int(time.time())
        session = SpeechSession(
            session_id=str(uuid.uuid4()),
            device_id=device_id.strip(),
            conversation_id=conversation_id.strip(),
            input_sample_rate_hz=24_000,
            output_sample_rate_hz=24_000,
            codec="ogg_opus_mono",
            transport_protocol="moshi-websocket-v0",
            protocol_version=0,
            backend=self.backend,
            created_at_unix=now,
            connection_deadline_unix=now + self.connection_grace_seconds,
            expires_at_unix=now + self.ttl_seconds,
        )
        self._sessions[session.session_id] = session
        return session

    def get(self, session_id: str) -> SpeechSession | None:
        self.expire()
        return self._sessions.get(session_id)

    def claim(self, session_id: str, device_id: str) -> SpeechSession:
        self.expire()
        session = self._sessions.get(session_id)
        if session is None or session.device_id != device_id:
            raise SpeechSessionRejected("speech_session_not_found")
        if session_id in self._connected:
            raise SpeechSessionRejected("speech_session_already_connected")
        self._connected.add(session_id)
        return session

    def renew(self, session_id: str) -> SpeechSession | None:
        session = self._sessions.get(session_id)
        if session is None or session_id not in self._connected:
            return None
        renewed = replace(session, expires_at_unix=int(time.time()) + self.ttl_seconds)
        self._sessions[session_id] = renewed
        return renewed

    def remove(self, session_id: str) -> None:
        self._sessions.pop(session_id, None)
        self._connected.discard(session_id)

    def expire(self) -> list[str]:
        now = int(time.time())
        expired = [
            key
            for key, value in self._sessions.items()
            if value.expires_at_unix <= now
            or (key not in self._connected and value.connection_deadline_unix <= now)
        ]
        for key in expired:
            self.remove(key)
        return expired

    @property
    def active_count(self) -> int:
        self.expire()
        return len(self._connected)

    @property
    def pending_count(self) -> int:
        self.expire()
        return len(self._sessions) - len(self._connected)
