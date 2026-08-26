"""Durable, local audit history for VoiceOS turns and tool activity."""

from __future__ import annotations

import json
import hashlib
import secrets
import sqlite3
import threading
import time
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

CLIENT_EVENT_RETENTION = 20_000


class AuditStore:
    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        self.path = path
        self._lock = threading.Lock()
        self._connection = sqlite3.connect(path, check_same_thread=False)
        self._connection.row_factory = sqlite3.Row
        with self._connection:
            self._connection.executescript(
                """
                CREATE TABLE IF NOT EXISTS turns (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    request_id TEXT,
                    request_fingerprint TEXT,
                    session_id TEXT NOT NULL,
                    transcript TEXT NOT NULL,
                    response_text TEXT NOT NULL,
                    provider TEXT NOT NULL,
                    tool_requests_json TEXT NOT NULL,
                    approvals_json TEXT NOT NULL,
                    results_json TEXT NOT NULL,
                    errors_json TEXT NOT NULL,
                    processing_ms INTEGER NOT NULL,
                    input_tokens INTEGER,
                    output_tokens INTEGER,
                    cost_usd REAL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS turns_session_id_idx ON turns(session_id, id);

                CREATE TABLE IF NOT EXISTS audit_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT,
                    event_type TEXT NOT NULL,
                    actor TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS audit_events_session_id_idx
                    ON audit_events(session_id, id);

                CREATE TABLE IF NOT EXISTS enrollment_codes (
                    code_hash TEXT PRIMARY KEY,
                    expires_at INTEGER NOT NULL,
                    used_at TEXT
                );

                CREATE TABLE IF NOT EXISTS devices (
                    device_id TEXT PRIMARY KEY,
                    device_name TEXT NOT NULL,
                    token_hash TEXT NOT NULL UNIQUE,
                    created_at TEXT NOT NULL,
                    last_seen_at TEXT,
                    disabled_at TEXT
                );

                CREATE TABLE IF NOT EXISTS pending_approvals (
                    request_id TEXT PRIMARY KEY,
                    session_id TEXT,
                    tool_name TEXT NOT NULL,
                    arguments_json TEXT NOT NULL,
                    status TEXT NOT NULL,
                    expires_at INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    decided_at TEXT
                );

                CREATE TABLE IF NOT EXISTS daily_checkin_sessions (
                    checkin_date TEXT PRIMARY KEY,
                    status TEXT NOT NULL CHECK(status IN ('active', 'paused', 'completed')),
                    next_question INTEGER NOT NULL,
                    started_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    completed_at TEXT
                );
                CREATE TABLE IF NOT EXISTS daily_checkin_answers (
                    checkin_date TEXT NOT NULL,
                    question_index INTEGER NOT NULL,
                    question TEXT NOT NULL,
                    answer TEXT NOT NULL,
                    device_id TEXT,
                    answered_at TEXT NOT NULL,
                    PRIMARY KEY(checkin_date, question_index),
                    FOREIGN KEY(checkin_date) REFERENCES daily_checkin_sessions(checkin_date)
                );
                CREATE TABLE IF NOT EXISTS daily_action_plans (
                    plan_date TEXT PRIMARY KEY,
                    status TEXT NOT NULL CHECK(status IN ('proposed', 'accepted', 'rejected')),
                    plan_json TEXT NOT NULL,
                    generated_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS client_events (
                    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event_type TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS client_events_created_idx
                    ON client_events(event_id, created_at);
                CREATE TABLE IF NOT EXISTS hermes_completion_imports (
                    session_id TEXT NOT NULL,
                    message_id INTEGER NOT NULL,
                    imported_at TEXT NOT NULL,
                    PRIMARY KEY(session_id, message_id)
                );
                """
            )
            columns = {
                str(row[1])
                for row in self._connection.execute("PRAGMA table_info(pending_approvals)")
            }
            if "provider" not in columns:
                self._connection.execute("ALTER TABLE pending_approvals ADD COLUMN provider TEXT")
            if "provider_run_id" not in columns:
                self._connection.execute(
                    "ALTER TABLE pending_approvals ADD COLUMN provider_run_id TEXT"
                )
            if "evidence_json" not in columns:
                self._connection.execute(
                    "ALTER TABLE pending_approvals ADD COLUMN evidence_json TEXT NOT NULL DEFAULT '{}'"
                )
            turn_columns = {
                str(row[1]) for row in self._connection.execute("PRAGMA table_info(turns)")
            }
            if "request_id" not in turn_columns:
                self._connection.execute("ALTER TABLE turns ADD COLUMN request_id TEXT")
            if "request_fingerprint" not in turn_columns:
                self._connection.execute("ALTER TABLE turns ADD COLUMN request_fingerprint TEXT")
            self._connection.execute(
                "CREATE UNIQUE INDEX IF NOT EXISTS turns_request_id_idx ON turns(request_id) "
                "WHERE request_id IS NOT NULL"
            )

    def create_enrollment_code(self, ttl_seconds: int = 600) -> tuple[str, int]:
        ttl = min(max(ttl_seconds, 60), 3_600)
        code = secrets.token_urlsafe(18)
        expires_at = int(time.time()) + ttl
        with self._lock, self._connection:
            self._connection.execute(
                "INSERT INTO enrollment_codes (code_hash, expires_at) VALUES (?, ?)",
                (_secret_hash(code), expires_at),
            )
        return code, expires_at

    def exchange_enrollment_code(self, code: str, device_name: str) -> dict[str, str] | None:
        code_hash = _secret_hash(code)
        now = int(time.time())
        created_at = _now()
        with self._lock, self._connection:
            row = self._connection.execute(
                """
                SELECT code_hash FROM enrollment_codes
                WHERE code_hash = ? AND expires_at >= ? AND used_at IS NULL
                """,
                (code_hash, now),
            ).fetchone()
            if row is None:
                return None
            device_id = str(uuid.uuid4())
            token = secrets.token_urlsafe(32)
            self._connection.execute(
                """
                INSERT INTO devices (device_id, device_name, token_hash, created_at)
                VALUES (?, ?, ?, ?)
                """,
                (device_id, device_name, _secret_hash(token), created_at),
            )
            self._connection.execute(
                "UPDATE enrollment_codes SET used_at = ? WHERE code_hash = ?",
                (created_at, code_hash),
            )
        return {"device_id": device_id, "device_token": token}

    def authenticate_device(self, token: str) -> str | None:
        token_hash = _secret_hash(token)
        seen_at = _now()
        with self._lock, self._connection:
            row = self._connection.execute(
                """
                SELECT device_id FROM devices
                WHERE token_hash = ? AND disabled_at IS NULL
                """,
                (token_hash,),
            ).fetchone()
            if row is None:
                return None
            device_id = str(row["device_id"])
            self._connection.execute(
                "UPDATE devices SET last_seen_at = ? WHERE device_id = ?",
                (seen_at, device_id),
            )
            self._connection.commit()
            return device_id

    def create_pending_approval(
        self,
        *,
        request_id: str,
        session_id: str | None,
        tool_name: str,
        arguments: dict[str, object],
        provider: str | None = None,
        provider_run_id: str | None = None,
        evidence: dict[str, object] | None = None,
        ttl_seconds: int = 300,
    ) -> dict[str, object]:
        ttl = min(max(ttl_seconds, 30), 900)
        expires_at = int(time.time()) + ttl
        created_at = _now()
        with self._lock, self._connection:
            self._connection.execute(
                """
                INSERT INTO pending_approvals (
                    request_id, session_id, tool_name, arguments_json,
                    status, expires_at, created_at, provider, provider_run_id, evidence_json
                ) VALUES (?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?)
                """,
                (
                    request_id,
                    session_id,
                    tool_name,
                    _json(arguments),
                    expires_at,
                    created_at,
                    provider,
                    provider_run_id,
                    _json(evidence or {}),
                ),
            )
            self._append_client_event_locked(
                "approval.proposed",
                {
                    "request_id": request_id,
                    "tool": tool_name,
                    "arguments": arguments,
                    "expires_at_unix": expires_at,
                    "provider": provider,
                    "evidence": evidence or {},
                },
            )
        return {
            "request_id": request_id,
            "tool": tool_name,
            "arguments": arguments,
            "status": "pending",
            "expires_at_unix": expires_at,
            "provider": provider,
            "evidence": evidence or {},
        }

    def decide_pending_approval(
        self, request_id: str, decision: str
    ) -> dict[str, object] | None:
        now_unix = int(time.time())
        decided_at = _now()
        with self._lock, self._connection:
            row = self._connection.execute(
                "SELECT * FROM pending_approvals WHERE request_id = ?",
                (request_id,),
            ).fetchone()
            if row is None:
                return None
            current_status = str(row["status"])
            if current_status != "pending":
                return {
                    "request_id": request_id,
                    "status": "already_decided",
                    "previous_decision": current_status,
                }
            if int(row["expires_at"]) < now_unix:
                self._connection.execute(
                    """
                    UPDATE pending_approvals SET status = 'expired', decided_at = ?
                    WHERE request_id = ? AND status = 'pending'
                    """,
                    (decided_at, request_id),
                )
                return {"request_id": request_id, "status": "expired"}
            updated = self._connection.execute(
                """
                UPDATE pending_approvals SET status = ?, decided_at = ?
                WHERE request_id = ? AND status = 'pending'
                """,
                (decision, decided_at, request_id),
            )
            if updated.rowcount != 1:
                return {"request_id": request_id, "status": "already_decided"}
            self._append_client_event_locked(
                "approval.decided",
                {"request_id": request_id, "decision": decision, "tool": row["tool_name"]},
            )
            return {
                "request_id": request_id,
                "session_id": row["session_id"],
                "tool": row["tool_name"],
                "arguments": json.loads(row["arguments_json"]),
                "provider": row["provider"],
                "provider_run_id": row["provider_run_id"],
                "evidence": json.loads(row["evidence_json"] or "{}"),
                "status": decision,
            }

    def record_turn(
        self,
        *,
        session_id: str,
        transcript: str,
        response_text: str,
        provider: str,
        tool_requests: list[dict[str, Any]],
        approvals: list[dict[str, Any]],
        results: list[dict[str, Any]],
        errors: list[dict[str, Any]],
        processing_ms: int,
        input_tokens: int | None = None,
        output_tokens: int | None = None,
        cost_usd: float | None = None,
        request_id: str | None = None,
        request_fingerprint: str | None = None,
    ) -> int:
        with self._lock, self._connection:
            cursor = self._connection.execute(
                """
                INSERT INTO turns (
                    request_id, request_fingerprint, session_id, transcript, response_text, provider,
                    tool_requests_json, approvals_json, results_json, errors_json,
                    processing_ms, input_tokens, output_tokens, cost_usd, created_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                (
                    request_id,
                    request_fingerprint,
                    session_id,
                    transcript,
                    response_text,
                    provider,
                    _json(tool_requests),
                    _json(approvals),
                    _json(results),
                    _json(errors),
                    processing_ms,
                    input_tokens,
                    output_tokens,
                    cost_usd,
                    _now(),
                ),
            )
            self._append_client_event_locked(
                "conversation.turn",
                {
                    "turn_id": int(cursor.lastrowid),
                    "request_id": request_id,
                    "session_id": session_id,
                    "transcript": transcript,
                    "response_text": response_text,
                    "provider": provider,
                    "processing_ms": processing_ms,
                },
            )
            return int(cursor.lastrowid)

    def completed_turn(self, request_id: str) -> dict[str, Any] | None:
        if not request_id.strip():
            return None
        with self._lock:
            row = self._connection.execute(
                "SELECT * FROM turns WHERE request_id = ?",
                (request_id.strip(),),
            ).fetchone()
        if row is None:
            return None
        return {
            "session_id": row["session_id"],
            "transcript": row["transcript"],
            "response_text": row["response_text"],
            "processing_ms": row["processing_ms"],
            "provider": row["provider"],
            "tool_calls": json.loads(row["tool_requests_json"]),
            "approvals": json.loads(row["approvals_json"]),
            "results": json.loads(row["results_json"]),
            "errors": json.loads(row["errors_json"]),
            "evidence": None,
            "usage": {
                "input_tokens": row["input_tokens"],
                "output_tokens": row["output_tokens"],
                "cost_usd": row["cost_usd"],
            },
            "reply_audio_url": None,
            "replayed": True,
            "_request_fingerprint": row["request_fingerprint"],
        }

    def import_hermes_completion(
        self, *, session_id: str, message_id: int, report: str
    ) -> bool:
        """Store one Hermes background report exactly once in VIC's durable thread."""
        with self._lock, self._connection:
            inserted = self._connection.execute(
                "INSERT OR IGNORE INTO hermes_completion_imports "
                "(session_id, message_id, imported_at) VALUES (?, ?, ?)",
                (session_id, message_id, _now()),
            )
            if inserted.rowcount != 1:
                return False
            cursor = self._connection.execute(
                """
                INSERT INTO turns (
                    session_id, transcript, response_text, provider,
                    tool_requests_json, approvals_json, results_json, errors_json,
                    processing_ms, input_tokens, output_tokens, cost_usd, created_at
                ) VALUES (?, ?, ?, ?, '[]', '[]', '[]', '[]', 0, NULL, NULL, 0, ?)
                """,
                (session_id, "Subagent report", report,
                 "hermes-subagent", _now()),
            )
            self._append_client_event_locked(
                "conversation.turn",
                {
                    "turn_id": int(cursor.lastrowid),
                    "session_id": session_id,
                    "transcript": "Subagent report",
                    "response_text": report,
                    "provider": "hermes-subagent",
                    "processing_ms": 0,
                },
            )
            return True

    def record_event(
        self,
        event_type: str,
        payload: dict[str, Any],
        *,
        actor: str,
        session_id: str | None = None,
    ) -> int:
        with self._lock, self._connection:
            cursor = self._connection.execute(
                """
                INSERT INTO audit_events (session_id, event_type, actor, payload_json, created_at)
                VALUES (?, ?, ?, ?, ?)
                """,
                (session_id, event_type, actor, _json(payload), _now()),
            )
            return int(cursor.lastrowid)

    def list_turns(self, limit: int = 50) -> list[dict[str, Any]]:
        safe_limit = min(max(limit, 1), 200)
        with self._lock:
            rows = self._connection.execute(
                "SELECT * FROM turns ORDER BY id DESC LIMIT ?", (safe_limit,)
            ).fetchall()
        return [_turn_row(row) for row in rows]

    def list_session_turns(self, session_id: str, limit: int = 12) -> list[dict[str, Any]]:
        """Return a bounded session transcript in chronological order."""

        safe_limit = min(max(limit, 1), 24)
        with self._lock:
            rows = self._connection.execute(
                """
                SELECT * FROM (
                    SELECT * FROM turns WHERE session_id = ? ORDER BY id DESC LIMIT ?
                ) ORDER BY id ASC
                """,
                (session_id, safe_limit),
            ).fetchall()
        return [_turn_row(row) for row in rows]

    def list_events(self, limit: int = 100) -> list[dict[str, Any]]:
        safe_limit = min(max(limit, 1), 500)
        with self._lock:
            rows = self._connection.execute(
                "SELECT * FROM audit_events ORDER BY id DESC LIMIT ?", (safe_limit,)
            ).fetchall()
        return [_event_row(row) for row in rows]

    def publish_client_event(self, event_type: str, payload: dict[str, Any]) -> int:
        with self._lock, self._connection:
            return self._append_client_event_locked(event_type, payload)

    def list_client_events(self, after: int = 0, limit: int = 100) -> list[dict[str, Any]]:
        safe_after = max(0, int(after))
        safe_limit = min(max(int(limit), 1), 500)
        with self._lock:
            rows = self._connection.execute(
                """
                SELECT event_id, event_type, payload_json, created_at
                FROM client_events WHERE event_id > ? ORDER BY event_id LIMIT ?
                """,
                (safe_after, safe_limit),
            ).fetchall()
        return [
            {
                "id": int(row["event_id"]),
                "type": str(row["event_type"]),
                "payload": json.loads(str(row["payload_json"])),
                "created_at": str(row["created_at"]),
            }
            for row in rows
        ]

    def latest_client_event_id(self) -> int:
        with self._lock:
            row = self._connection.execute(
                "SELECT COALESCE(MAX(event_id), 0) FROM client_events"
            ).fetchone()
        return int(row[0]) if row is not None else 0

    def _append_client_event_locked(self, event_type: str, payload: dict[str, Any]) -> int:
        cursor = self._connection.execute(
            "INSERT INTO client_events(event_type, payload_json, created_at) VALUES (?, ?, ?)",
            (event_type, _json(payload), _now()),
        )
        newest_id = int(cursor.lastrowid)
        if newest_id % 100 == 0:
            self._connection.execute(
                "DELETE FROM client_events WHERE event_id <= ?",
                (newest_id - CLIENT_EVENT_RETENTION,),
            )
        return newest_id

    def daily_checkin_status(
        self, checkin_date: str, questions: tuple[str, ...]
    ) -> dict[str, Any]:
        with self._lock:
            session = self._connection.execute(
                "SELECT * FROM daily_checkin_sessions WHERE checkin_date = ?",
                (checkin_date,),
            ).fetchone()
            answers = self._connection.execute(
                """
                SELECT question_index, question, answer, device_id, answered_at
                FROM daily_checkin_answers
                WHERE checkin_date = ? ORDER BY question_index
                """,
                (checkin_date,),
            ).fetchall()
        next_index = int(session["next_question"]) if session is not None else 0
        status = str(session["status"]) if session is not None else "not_started"
        return {
            "date": checkin_date,
            "status": status,
            "answered": len(answers),
            "total": len(questions),
            "next_question": (
                {"index": next_index + 1, "text": questions[next_index]}
                if status != "completed" and next_index < len(questions)
                else None
            ),
            "answers": [dict(row) for row in answers],
            "plan": self.daily_action_plan(checkin_date),
        }

    def daily_action_plan(self, plan_date: str) -> dict[str, Any] | None:
        with self._lock:
            row = self._connection.execute(
                "SELECT status, plan_json, generated_at, updated_at FROM daily_action_plans WHERE plan_date=?",
                (plan_date,),
            ).fetchone()
        if row is None:
            return None
        plan = json.loads(str(row["plan_json"]))
        return {
            **plan,
            "date": plan_date,
            "status": str(row["status"]),
            "generated_at": str(row["generated_at"]),
            "updated_at": str(row["updated_at"]),
        }

    def handle_daily_checkin_turn(
        self,
        checkin_date: str,
        device_id: str,
        text: str,
        questions: tuple[str, ...],
    ) -> dict[str, Any] | None:
        normalized = " ".join(text.casefold().replace("-", " ").split())
        start = normalized in {
            "start daily check in", "start my daily check in", "daily check in",
            "start daily questions", "ask me my daily questions", "resume daily check in",
        }
        pause = normalized in {"pause daily check in", "stop daily check in", "pause questions"}
        now = _now()
        with self._lock, self._connection:
            session = self._connection.execute(
                "SELECT * FROM daily_checkin_sessions WHERE checkin_date = ?",
                (checkin_date,),
            ).fetchone()
            if session is None:
                if not start:
                    return None
                self._connection.execute(
                    """
                    INSERT INTO daily_checkin_sessions
                    (checkin_date, status, next_question, started_at, updated_at)
                    VALUES (?, 'active', 0, ?, ?)
                    """,
                    (checkin_date, now, now),
                )
                return _checkin_response(checkin_date, 0, questions, "started")

            status = str(session["status"])
            next_index = int(session["next_question"])
            if pause and status == "active":
                self._connection.execute(
                    "UPDATE daily_checkin_sessions SET status='paused', updated_at=? WHERE checkin_date=?",
                    (now, checkin_date),
                )
                return _checkin_response(checkin_date, next_index, questions, "paused")
            if status == "completed":
                if not start:
                    return None
                return _checkin_response(checkin_date, len(questions), questions, "completed")
            if status == "paused":
                if not start:
                    return None
                self._connection.execute(
                    "UPDATE daily_checkin_sessions SET status='active', updated_at=? WHERE checkin_date=?",
                    (now, checkin_date),
                )
                return _checkin_response(checkin_date, next_index, questions, "resumed")
            if start:
                return _checkin_response(checkin_date, next_index, questions, "active")

            answer = text.strip() if normalized != "skip" else "[skipped]"
            self._connection.execute(
                """
                INSERT OR IGNORE INTO daily_checkin_answers
                (checkin_date, question_index, question, answer, device_id, answered_at)
                VALUES (?, ?, ?, ?, ?, ?)
                """,
                (checkin_date, next_index, questions[next_index], answer, device_id, now),
            )
            next_index += 1
            completed = next_index >= len(questions)
            self._connection.execute(
                """
                UPDATE daily_checkin_sessions
                SET status=?, next_question=?, updated_at=?, completed_at=?
                WHERE checkin_date=?
                """,
                ("completed" if completed else "active", next_index, now, now if completed else None, checkin_date),
            )
            plan = None
            if completed:
                answer_rows = self._connection.execute(
                    """
                    SELECT question_index, answer FROM daily_checkin_answers
                    WHERE checkin_date=? ORDER BY question_index
                    """,
                    (checkin_date,),
                ).fetchall()
                plan = _build_daily_action_plan(answer_rows)
                self._connection.execute(
                    """
                    INSERT INTO daily_action_plans
                    (plan_date, status, plan_json, generated_at, updated_at)
                    VALUES (?, 'proposed', ?, ?, ?)
                    ON CONFLICT(plan_date) DO UPDATE SET
                        status='proposed', plan_json=excluded.plan_json,
                        generated_at=excluded.generated_at, updated_at=excluded.updated_at
                    """,
                    (checkin_date, _json(plan), now, now),
                )
                self._append_client_event_locked(
                    "daily_plan.proposed", {"date": checkin_date, "plan": plan}
                )
            response = _checkin_response(
                checkin_date, next_index, questions, "completed" if completed else "answered"
            )
            if plan is not None:
                response["plan"] = plan
                titles = [str(item["title"]) for item in plan["priorities"]]
                response["response_text"] = (
                    "Daily planning is complete. I have all twelve answers. Your proposed top priorities are: "
                    + "; ".join(f"{index + 1}, {title}" for index, title in enumerate(titles))
                    + ". I also identified twenty-minute next actions, blockers, and work VoiceOS can propose doing."
                )
            return response

    def close(self) -> None:
        with self._lock:
            self._connection.close()


def _now() -> str:
    return datetime.now(UTC).isoformat()


def _json(value: object) -> str:
    return json.dumps(value, separators=(",", ":"), sort_keys=True)


def _secret_hash(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def _turn_row(row: sqlite3.Row) -> dict[str, Any]:
    result = dict(row)
    for key in ("tool_requests_json", "approvals_json", "results_json", "errors_json"):
        result[key.removesuffix("_json")] = json.loads(result.pop(key))
    return result


def _event_row(row: sqlite3.Row) -> dict[str, Any]:
    result = dict(row)
    result["payload"] = json.loads(result.pop("payload_json"))
    return result


def _checkin_response(
    checkin_date: str, next_index: int, questions: tuple[str, ...], state: str
) -> dict[str, Any]:
    if state == "paused":
        response = "Daily planning is paused. Say resume daily check-in when you are ready."
    elif next_index >= len(questions):
        response = "Daily planning is complete. I have all twelve answers and can now help turn them into an ordered task plan."
    else:
        prefix = "Daily planning started. " if state == "started" else ""
        response = f"{prefix}Question {next_index + 1} of {len(questions)}: {questions[next_index]}"
    return {
        "handled": True,
        "state": state,
        "date": checkin_date,
        "answered": min(next_index, len(questions)),
        "total": len(questions),
        "response_text": response,
        "next_question": (
            {"index": next_index + 1, "text": questions[next_index]}
            if next_index < len(questions) and state != "paused"
            else None
        ),
    }


def _build_daily_action_plan(rows: list[sqlite3.Row]) -> dict[str, Any]:
    answers = {int(row["question_index"]): str(row["answer"]).strip() for row in rows}
    priority_candidates = _split_priorities(answers.get(11, ""))
    for index in (0, 3, 4):
        candidate = answers.get(index, "")
        if candidate and candidate != "[skipped]" and candidate not in priority_candidates:
            priority_candidates.append(candidate)
    while len(priority_candidates) < 3:
        priority_candidates.append("Choose the next meaningful unfinished commitment")
    next_actions = [
        answers.get(6, "Define the smallest concrete next action"),
        answers.get(7, "Complete one useful twenty-minute task"),
        "Spend twenty focused minutes moving this priority toward its observable outcome",
    ]
    blocker = _meaningful(answers.get(5))
    dependency = _meaningful(answers.get(9))
    priorities = []
    for rank, title in enumerate(priority_candidates[:3], start=1):
        priorities.append(
            {
                "rank": rank,
                "title": _bounded_text(title, 240),
                "next_action": _bounded_text(next_actions[rank - 1], 320),
                "estimated_minutes": 20,
                "dependencies": [dependency] if dependency else [],
                "blockers": [blocker] if blocker else [],
                "executor": "user",
                "approval_required": False,
            }
        )
    voiceos_request = _meaningful(answers.get(10))
    voiceos_actions = []
    if voiceos_request:
        voiceos_actions.append(
            {
                "title": _bounded_text(voiceos_request, 320),
                "next_action": "VoiceOS will propose a typed capability and evidence before execution.",
                "approval_required": True,
                "status": "proposed",
            }
        )
    return {
        "priorities": priorities,
        "voiceos_actions": voiceos_actions,
        "approval_items": [item for item in voiceos_actions if item["approval_required"]],
        "source": "daily_checkin_v1",
    }


def _split_priorities(value: str) -> list[str]:
    normalized = value.replace(";", ",").replace(" then ", ",")
    results = []
    for item in normalized.split(","):
        cleaned = item.strip().lstrip("0123456789.-) ").strip()
        if cleaned and cleaned != "[skipped]" and cleaned not in results:
            results.append(cleaned)
    return results


def _meaningful(value: str | None) -> str | None:
    if value is None or not value.strip() or value.strip() == "[skipped]":
        return None
    return _bounded_text(value.strip(), 320)


def _bounded_text(value: str, limit: int) -> str:
    return " ".join(value.split())[:limit]
