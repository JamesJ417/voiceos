from __future__ import annotations

import asyncio
import os
import ssl

import websockets
from fastapi import FastAPI, Header, HTTPException, WebSocket, WebSocketDisconnect
from pydantic import BaseModel, Field

from .audit import LifecycleAudit
from .core import SessionRegistry, SpeechSessionRejected
from .gpu_scheduler import GpuSchedulerClient, GpuSchedulerError


class CreateSessionRequest(BaseModel):
    conversation_id: str = Field(min_length=1, max_length=200)


backend_websocket_url = os.environ.get("VOICEOS_MOSHI_WEBSOCKET_URL", "").strip()
backend_ca_file = os.environ.get("VOICEOS_MOSHI_CA_FILE", "").strip()
scheduler_socket = os.environ.get(
    "VOICEOS_GPU_SCHEDULER_SOCKET", "/run/voiceos-gpu-scheduler/control.sock"
).strip()
scheduler = GpuSchedulerClient(scheduler_socket) if scheduler_socket else None
registry = SessionRegistry(
    backend=os.environ.get("VOICEOS_SPEECH_BACKEND", "moshi-candle-q8"),
    max_sessions=int(os.environ.get("VOICEOS_SPEECH_MAX_SESSIONS", "2")),
    ttl_seconds=int(os.environ.get("VOICEOS_SPEECH_LEASE_SECONDS", "900")),
    connection_grace_seconds=int(os.environ.get("VOICEOS_SPEECH_CONNECTION_GRACE_SECONDS", "30")),
)
lease_renewal_seconds = int(os.environ.get("VOICEOS_SPEECH_RENEWAL_SECONDS", "10"))
audit = LifecycleAudit(
    os.environ.get("VOICEOS_SPEECH_AUDIT_PATH", "/var/lib/voiceos/speech/lifecycle.jsonl")
)
app = FastAPI(title="VoiceOS Speech Worker", docs_url=None, redoc_url=None)


async def record(event: str, session_id: str, detail: dict[str, object] | None = None) -> None:
    await asyncio.to_thread(audit.record, event, session_id=session_id, detail=detail)


async def cleanup_expired_sessions() -> None:
    for expired_session_id in registry.expire():
        await record("speech.session_expired", expired_session_id)


@app.get("/v1/health")
async def health() -> dict[str, object]:
    await cleanup_expired_sessions()
    scheduler_status: dict[str, object] | None = None
    if scheduler:
        try:
            scheduler_status = await asyncio.to_thread(scheduler.status)
        except (GpuSchedulerError, OSError, ValueError):
            scheduler_status = {"ok": False, "state": "unknown", "moshi_ready": False}
    return {
        "status": "ok"
        if backend_websocket_url and scheduler_status and scheduler_status.get("ok")
        else "degraded",
        "worker": "speech-to-speech",
        "backend": registry.backend,
        "configured": bool(backend_websocket_url),
        "full_duplex": True,
        "transport_protocol": "moshi-websocket-v0",
        "protocol_version": 0,
        "codec": "ogg_opus_mono",
        "pending_sessions": registry.pending_count,
        "active_sessions": registry.active_count,
        "gpu_scheduler": scheduler_status,
    }


@app.post("/v1/sessions", status_code=201)
async def create_session(
    request: CreateSessionRequest,
    x_voiceos_device_id: str | None = Header(default=None),
) -> dict[str, object]:
    await cleanup_expired_sessions()
    if not x_voiceos_device_id:
        raise HTTPException(status_code=401, detail="voiceos_device_identity_required")
    if not backend_websocket_url:
        raise HTTPException(status_code=503, detail="moshi_backend_not_configured")
    try:
        session = registry.create(
            device_id=x_voiceos_device_id,
            conversation_id=request.conversation_id,
        )
    except SpeechSessionRejected as error:
        raise HTTPException(status_code=429, detail=str(error)) from error
    await record(
        "speech.session_created",
        session.session_id,
        {
            "conversation_id": session.conversation_id,
            "connection_deadline_unix": session.connection_deadline_unix,
        },
    )
    return {**session.as_dict(), "stream_path": f"/v1/stream/{session.session_id}"}


@app.websocket("/v1/stream/{session_id}")
async def stream(websocket: WebSocket, session_id: str) -> None:
    await cleanup_expired_sessions()
    device_id = websocket.headers.get("x-voiceos-device-id", "")
    try:
        session = registry.claim(session_id, device_id)
    except SpeechSessionRejected:
        await websocket.close(code=4401)
        return
    await websocket.accept()
    acquired = False
    tasks: set[asyncio.Task[None]] = set()
    try:
        await record("speech.gpu_acquire_started", session_id)
        if scheduler:
            await asyncio.to_thread(scheduler.acquire, session_id, registry.ttl_seconds)
        acquired = True
        await record("speech.gpu_acquired", session_id)

        async with websockets.connect(
            backend_websocket_url,
            max_size=2 * 1024 * 1024,
            ssl=_backend_ssl_context(),
            open_timeout=30,
        ) as backend:
            await record("speech.backend_connected", session_id)

            async def client_to_backend() -> None:
                while True:
                    message = await websocket.receive()
                    if message.get("bytes") is not None:
                        await backend.send(message["bytes"])
                    elif message.get("text") is not None:
                        await backend.send(message["text"])
                    else:
                        return

            async def backend_to_client() -> None:
                async for message in backend:
                    if isinstance(message, bytes):
                        await websocket.send_bytes(message)
                    else:
                        await websocket.send_text(message)

            async def renew_lease() -> None:
                while True:
                    await asyncio.sleep(lease_renewal_seconds)
                    if registry.renew(session_id) is None:
                        raise GpuSchedulerError("speech_session_expired")
                    if scheduler:
                        await asyncio.to_thread(
                            scheduler.renew, session_id, registry.ttl_seconds
                        )
                    await record("speech.lease_renewed", session_id)

            tasks = {
                asyncio.create_task(client_to_backend()),
                asyncio.create_task(backend_to_client()),
                asyncio.create_task(renew_lease()),
            }
            done, pending = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
            for task in pending:
                task.cancel()
            await asyncio.gather(*pending, return_exceptions=True)
            for task in done:
                error = task.exception()
                if error is not None:
                    raise error
    except (WebSocketDisconnect, websockets.ConnectionClosed):
        await record("speech.client_disconnected", session_id)
    except Exception as error:
        await record(
            "speech.session_error",
            session_id,
            {"error": type(error).__name__, "detail": str(error)[:500]},
        )
        try:
            await websocket.close(code=1013)
        except RuntimeError:
            pass
    finally:
        for task in tasks:
            if not task.done():
                task.cancel()
        if tasks:
            await asyncio.gather(*tasks, return_exceptions=True)
        registry.remove(session_id)
        if acquired and scheduler:
            try:
                await asyncio.to_thread(scheduler.release, session_id)
                await record("speech.gpu_released", session_id)
            except GpuSchedulerError as error:
                await record(
                    "speech.gpu_release_failed",
                    session_id,
                    {"detail": str(error)[:500]},
                )
        await record("speech.session_closed", session_id)


def _backend_ssl_context() -> ssl.SSLContext | None:
    if not backend_websocket_url.startswith("wss://"):
        return None
    context = ssl.create_default_context(cafile=backend_ca_file or None)
    if backend_ca_file:
        context.check_hostname = False
    return context
