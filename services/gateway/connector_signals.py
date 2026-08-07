"""Credential-driven, localhost-only email/calendar/communication signal adapter.

OAuth access tokens are read from protected files on every request so a token can
be rotated without restarting VoiceOS. The service never sends mail or accepts an
invitation; those remain approval-controlled gateway operations.
"""
from __future__ import annotations

import argparse
import json
import os
import threading
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlencode, urlsplit
from urllib.request import Request, urlopen

MAX_RESPONSE = 2 * 1024 * 1024


def _secret(path_name: str) -> str | None:
    path = os.environ.get(path_name, "").strip()
    if not path:
        return None
    try:
        value = Path(path).read_text(encoding="utf-8").strip()
    except OSError:
        return None
    return value or None


def _google(path: str, token_file_env: str) -> dict[str, object] | None:
    token = _oauth_token(token_file_env)
    if not token:
        return None
    request = Request(
        f"https://www.googleapis.com{path}",
        headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
    )
    with urlopen(request, timeout=15) as response:
        return json.loads(response.read(MAX_RESPONSE))


def _oauth_token(token_file_env: str) -> str | None:
    """Accept a short-lived access token or refreshable protected OAuth JSON."""
    secret = _secret(token_file_env)
    if not secret:
        return None
    if not secret.startswith("{"):
        return secret
    value = json.loads(secret)
    if not isinstance(value, dict):
        raise ValueError("oauth_credential_must_be_an_object")
    refresh_token = value.get("refresh_token")
    client_id = value.get("client_id")
    client_secret = value.get("client_secret")
    if refresh_token and client_id and client_secret:
        body = urlencode({
            "grant_type": "refresh_token", "refresh_token": str(refresh_token),
            "client_id": str(client_id), "client_secret": str(client_secret),
        }).encode()
        request = Request(
            "https://oauth2.googleapis.com/token", data=body,
            headers={"Content-Type": "application/x-www-form-urlencoded", "Accept": "application/json"},
        )
        with urlopen(request, timeout=15) as response:
            refreshed = json.loads(response.read(MAX_RESPONSE))
        access_token = refreshed.get("access_token") if isinstance(refreshed, dict) else None
        if not access_token:
            raise ValueError("oauth_refresh_did_not_return_access_token")
        return str(access_token)
    access_token = value.get("access_token")
    return str(access_token) if access_token else None


def email_signals() -> dict[str, object]:
    query = urlencode({"maxResults": 25, "q": "in:inbox newer_than:7d"})
    listing = _google(f"/gmail/v1/users/me/messages?{query}", "VOICEOS_GMAIL_TOKEN_FILE")
    if listing is None:
        return {"signals": [], "connector": "gmail", "status": "credentials_required"}
    signals: list[dict[str, object]] = []
    for reference in listing.get("messages", []) if isinstance(listing, dict) else []:
        if not isinstance(reference, dict) or not reference.get("id"):
            continue
        message_id = str(reference["id"])
        details = _google(
            f"/gmail/v1/users/me/messages/{message_id}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date",
            "VOICEOS_GMAIL_TOKEN_FILE",
        ) or {}
        headers = {
            str(item.get("name", "")).casefold(): str(item.get("value", ""))
            for item in (details.get("payload", {}).get("headers", []) if isinstance(details.get("payload"), dict) else [])
            if isinstance(item, dict)
        }
        labels = details.get("labelIds", [])
        signals.append({
            "id": message_id,
            "title": headers.get("subject", "Email message"),
            "summary": f"From {headers.get('from', 'unknown sender')}. {str(details.get('snippet', ''))[:500]}",
            "received_at": headers.get("date"),
            "unread": isinstance(labels, list) and "UNREAD" in labels,
            "needs_you": isinstance(labels, list) and "IMPORTANT" in labels,
            "requires_interpretation": True,
            "provider": "gmail",
        })
    return {"signals": signals, "connector": "gmail", "status": "connected"}


def calendar_signals() -> dict[str, object]:
    query = urlencode({"maxResults": 50, "singleEvents": "true", "orderBy": "startTime", "timeMin": _utc_now()})
    listing = _google(f"/calendar/v3/calendars/primary/events?{query}", "VOICEOS_CALENDAR_TOKEN_FILE")
    if listing is None:
        return {"signals": [], "connector": "google-calendar", "status": "credentials_required"}
    signals = []
    for event in listing.get("items", []) if isinstance(listing, dict) else []:
        if not isinstance(event, dict):
            continue
        start = event.get("start") if isinstance(event.get("start"), dict) else {}
        signals.append({
            "id": str(event.get("id", "")),
            "title": str(event.get("summary", "Calendar event")),
            "summary": str(event.get("description", event.get("location", "")))[:1000],
            "start_at": start.get("dateTime", start.get("date")),
            "location": event.get("location"),
            "needs_you": event.get("status") == "tentative",
            "requires_interpretation": True,
            "provider": "google-calendar",
        })
    return {"signals": signals, "connector": "google-calendar", "status": "connected"}


def _utc_now() -> str:
    from datetime import UTC, datetime
    return datetime.now(UTC).isoformat().replace("+00:00", "Z")


class SignalStore:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.lock = threading.Lock()

    def list(self) -> list[dict[str, object]]:
        with self.lock:
            try:
                value = json.loads(self.path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                return []
        return value if isinstance(value, list) else []

    def append(self, signal: dict[str, object]) -> None:
        with self.lock:
            values = self.list_unlocked()
            values = [item for item in values if item.get("id") != signal.get("id")]
            values.append(signal)
            self.path.parent.mkdir(parents=True, exist_ok=True)
            temporary = self.path.with_suffix(".tmp")
            temporary.write_text(json.dumps(values[-500:], separators=(",", ":")), encoding="utf-8")
            temporary.replace(self.path)

    def list_unlocked(self) -> list[dict[str, object]]:
        try:
            value = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return []
        return value if isinstance(value, list) else []


class Handler(BaseHTTPRequestHandler):
    server_version = "VoiceOSConnectors/1"

    @property
    def signal_store(self) -> SignalStore:
        return self.server.signal_store  # type: ignore[attr-defined]

    def do_GET(self) -> None:  # noqa: N802
        path = urlsplit(self.path).path
        try:
            if path == "/v1/health":
                self._json(HTTPStatus.OK, {"status": "ok", "email": "configured" if _secret("VOICEOS_GMAIL_TOKEN_FILE") else "credentials_required", "calendar": "configured" if _secret("VOICEOS_CALENDAR_TOKEN_FILE") else "credentials_required"})
            elif path == "/v1/email-signals":
                self._json(HTTPStatus.OK, email_signals())
            elif path == "/v1/calendar-signals":
                self._json(HTTPStatus.OK, calendar_signals())
            elif path == "/v1/communication-signals":
                self._json(HTTPStatus.OK, {"signals": self.signal_store.list(), "connector": "device-ingest", "status": "connected"})
            else:
                self._json(HTTPStatus.NOT_FOUND, {"error": "not_found"})
        except Exception as error:
            self._json(HTTPStatus.BAD_GATEWAY, {"error": "connector_failed", "detail": str(error)[:500]})

    def do_POST(self) -> None:  # noqa: N802
        if urlsplit(self.path).path != "/v1/communication-signals":
            self._json(HTTPStatus.NOT_FOUND, {"error": "not_found"}); return
        expected = _secret("VOICEOS_CONNECTOR_INGEST_TOKEN_FILE")
        supplied = self.headers.get("Authorization", "").removeprefix("Bearer ").strip()
        if not expected or supplied != expected:
            self._json(HTTPStatus.UNAUTHORIZED, {"error": "ingest_authentication_required"}); return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            value = json.loads(self.rfile.read(min(length, 64 * 1024)))
            if not isinstance(value, dict) or not str(value.get("id", "")).strip():
                raise ValueError("signal_id_required")
            self.signal_store.append(value)
            self._json(HTTPStatus.CREATED, {"status": "accepted"})
        except (ValueError, json.JSONDecodeError) as error:
            self._json(HTTPStatus.BAD_REQUEST, {"error": str(error)})

    def _json(self, status: HTTPStatus, value: dict[str, object]) -> None:
        body = json.dumps(value, separators=(",", ":")).encode()
        self.send_response(status); self.send_header("Content-Type", "application/json")
        self.send_header("Cache-Control", "no-store"); self.send_header("Content-Length", str(len(body)))
        self.end_headers(); self.wfile.write(body)

    def log_message(self, format: str, *args: object) -> None:
        return


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1"); parser.add_argument("--port", type=int, default=8795)
    args = parser.parse_args()
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    server.signal_store = SignalStore(Path(os.environ.get("VOICEOS_COMMUNICATION_SIGNAL_FILE", "/var/lib/voiceos/connectors/communication.json")))  # type: ignore[attr-defined]
    server.serve_forever()


if __name__ == "__main__":
    main()
