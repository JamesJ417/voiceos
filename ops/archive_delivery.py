#!/usr/bin/env python3
"""Durable artifact archive and AgentMail delivery adapter.

Local archive writes are authoritative. AgentMail is an optional delivery adapter.
Sending is explicit: call send_pending() only after approval and credentials exist.
"""
from __future__ import annotations

import base64
import hashlib
import json
import mimetypes
import os
import sqlite3
import urllib.error
import urllib.request
import uuid
from pathlib import Path
from typing import Any


class Archive:
    def __init__(self, root: str | Path):
        self.root = Path(root).expanduser()
        self.files = self.root / "files"
        self.db_path = self.root / "archive.sqlite3"
        self.files.mkdir(parents=True, exist_ok=True)
        self.db = sqlite3.connect(self.db_path)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("""CREATE TABLE IF NOT EXISTS artifacts (
            artifact_id TEXT PRIMARY KEY, filename TEXT NOT NULL, path TEXT NOT NULL,
            media_type TEXT NOT NULL, size INTEGER NOT NULL, sha256 TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )""")
        self.db.execute("""CREATE TABLE IF NOT EXISTS outbox (
            delivery_id TEXT PRIMARY KEY, artifact_id TEXT NOT NULL, inbox_id TEXT NOT NULL,
            recipient TEXT NOT NULL, subject TEXT NOT NULL, body TEXT NOT NULL,
            idempotency_key TEXT NOT NULL UNIQUE, status TEXT NOT NULL DEFAULT 'pending',
            provider_message_id TEXT, attempts INTEGER NOT NULL DEFAULT 0,
            last_error TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            sent_at TEXT
        )""")
        self.db.commit()

    def add_file(self, source: str | Path, media_type: str | None = None) -> dict[str, Any]:
        src = Path(source)
        data = src.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        artifact_id = str(uuid.uuid4())
        dest = self.files / f"{digest[:16]}-{src.name}"
        dest.write_bytes(data)
        media = media_type or mimetypes.guess_type(src.name)[0] or "application/octet-stream"
        self.db.execute(
            "INSERT INTO artifacts VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)",
            (artifact_id, src.name, str(dest), media, len(data), digest),
        )
        self.db.commit()
        return {"artifact_id": artifact_id, "filename": src.name, "path": str(dest), "sha256": digest, "size": len(data), "media_type": media}

    def queue_delivery(self, artifact_id: str, inbox_id: str, recipient: str, subject: str, body: str) -> str:
        delivery_id = str(uuid.uuid4())
        key = f"voiceos-{delivery_id}"
        self.db.execute(
            "INSERT INTO outbox(delivery_id, artifact_id, inbox_id, recipient, subject, body, idempotency_key) VALUES (?, ?, ?, ?, ?, ?, ?)",
            (delivery_id, artifact_id, inbox_id, recipient, subject, body, key),
        )
        self.db.commit()
        return delivery_id

    def pending(self) -> list[sqlite3.Row]:
        self.db.row_factory = sqlite3.Row
        return self.db.execute("SELECT o.*, a.path, a.filename, a.media_type FROM outbox o JOIN artifacts a ON a.artifact_id=o.artifact_id WHERE o.status='pending' ORDER BY o.created_at").fetchall()

    def send_pending(self, dry_run: bool = True) -> list[dict[str, Any]]:
        api_key = os.environ.get("AGENTMAIL_API_KEY")
        results = []
        for row in self.pending():
            if dry_run or not api_key:
                results.append({"delivery_id": row["delivery_id"], "status": "dry_run", "recipient": row["recipient"], "attachment": row["filename"]})
                continue
            payload = {"to": [row["recipient"]], "subject": row["subject"], "text": row["body"], "attachments": [{"content": base64.b64encode(Path(row["path"]).read_bytes()).decode(), "filename": row["filename"], "content_type": row["media_type"]}]}
            request = urllib.request.Request(f"https://api.agentmail.to/v0/inboxes/{row['inbox_id']}/messages/send", data=json.dumps(payload).encode(), method="POST", headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json", "Idempotency-Key": row["idempotency_key"]})
            try:
                with urllib.request.urlopen(request, timeout=30) as response:
                    result = json.loads(response.read())
                message_id = result.get("message_id")
                self.db.execute("UPDATE outbox SET status='sent', provider_message_id=?, attempts=attempts+1, sent_at=CURRENT_TIMESTAMP WHERE delivery_id=?", (message_id, row["delivery_id"]))
                self.db.commit()
                results.append({"delivery_id": row["delivery_id"], "status": "sent", "message_id": message_id})
            except (urllib.error.URLError, urllib.error.HTTPError, json.JSONDecodeError) as error:
                self.db.execute("UPDATE outbox SET attempts=attempts+1, last_error=? WHERE delivery_id=?", (str(error), row["delivery_id"]))
                self.db.commit()
                results.append({"delivery_id": row["delivery_id"], "status": "failed", "error": str(error)})
        return results


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", default=os.environ.get("VOICEOS_ARCHIVE_ROOT", "./var/archive"))
    parser.add_argument("--dry-run", action="store_true", default=True)
    args = parser.parse_args()
    archive = Archive(args.archive)
    print(json.dumps({"archive": str(archive.root), "pending": archive.send_pending(dry_run=True)}, indent=2))
