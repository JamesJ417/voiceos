from __future__ import annotations

import tempfile
import unittest
from datetime import UTC, datetime
from pathlib import Path

from services.gateway.audit import AuditStore
from services.gateway.review_scheduler import ReviewScheduler


NOW = datetime(2026, 8, 4, 14, 0, tzinfo=UTC)


def policy(**changes: object) -> dict[str, object]:
    value: dict[str, object] = {
        "enabled": True,
        "quiet_hours_start": "22:00",
        "quiet_hours_end": "08:00",
        "timezone": "UTC",
        "max_checkins_per_day": 6,
        "cooldown_minutes": 30,
        "driving_mode": False,
        "daily_digest_enabled": True,
        "do_not_disturb": False,
        "current_location": "home",
        "daily_planning_time": "23:00",
        "morning_digest_time": "08:00",
        "evening_digest_time": "18:00",
        "scan_interval_minutes": 20,
    }
    value.update(changes)
    return {"policy": value}


def automations() -> dict[str, object]:
    actions = {
        "review": ["review.scan"],
        "tasks": ["notify.needs_you", "digest.add"],
        "system": ["notify.needs_you", "digest.add"],
        "approval": ["notify.needs_you"],
        "email": ["notify.needs_you", "digest.add", "model.classify"],
        "calendar": ["notify.needs_you", "digest.add", "model.classify"],
        "message": ["notify.needs_you", "digest.add", "model.classify"],
        "document": ["digest.add"],
        "question": ["digest.add"],
        "planning": ["planning.prompt"],
        "digest": ["digest.deliver"],
    }
    return {"automations": [
        {
            "id": f"rule-{source}",
            "owner_id": "owner",
            "enabled": True,
            "trigger": {"kind": "change_or_schedule", "source": source},
            "conditions": {"respect_attention_policy": True},
            "permitted_actions": permitted,
            "frequency_limit": {"max_runs": 500, "window_minutes": 1440},
            "evidence": {"origin": "test"},
        }
        for source, permitted in actions.items()
    ]}


class ReviewSchedulerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.audit = AuditStore(Path(self.directory.name) / "audit.sqlite3")
        self.created: list[dict[str, object]] = []
        self.posted: list[tuple[str, dict[str, object]]] = []
        self.responses: dict[str, dict[str, object]] = {
            "/v1/outreach/policy": policy(),
            "/v1/automations?include_disabled=false&limit=500": automations(),
            "/v1/tasks?include_completed=true&limit=500": {"tasks": [], "details": []},
            "/v1/artifacts?limit=500": {"artifacts": []},
            "/v1/files": {"files": []},
            "/v1/outreach?limit=200": {"outreach": []},
        }

    def tearDown(self) -> None:
        self.audit.close()
        self.directory.cleanup()

    def scheduler(self) -> ReviewScheduler:
        def create(payload: dict[str, object]) -> dict[str, object]:
            self.created.append(payload)
            return {"outreach": {"id": f"outreach-{len(self.created)}", **payload}}

        return ReviewScheduler(
            "http://memory",
            self.audit,
            fetch_json=lambda path: self.responses.get(path),
            create_outreach=create,
            post_json=lambda path, payload: self.posted.append((path, payload)) or {"ok": True},
            health_probe=lambda: {"status": "healthy", "issues": [], "checked_at": NOW.isoformat()},
            now=lambda: NOW,
        )

    def test_scan_is_changed_only_and_routine_changes_become_one_silent_digest(self) -> None:
        scheduler = self.scheduler()
        first = scheduler.run_once()
        self.assertEqual([], self.created)
        self.assertIn("tasks", first["changed_sources"])

        self.responses["/v1/tasks?include_completed=true&limit=500"] = {
            "tasks": [{"id": "task-1", "title": "Recipe cards", "updated_at": "later"}],
            "details": [{
                "task": {"id": "task-1", "title": "Recipe cards", "updated_at": "later"},
                "blockers": [],
                "initiative": {"status": "completed", "updated_at": "later"},
            }],
        }
        second = scheduler.run_once()

        self.assertEqual(1, second["delivered"])
        self.assertEqual("digest", self.created[0]["kind"])
        self.assertEqual("quiet", self.created[0]["priority"])
        self.assertEqual([], self.audit.pending_review_digest())
        self.assertTrue(any(path == "/v1/attention" for path, _ in self.posted))

    def test_pending_approval_uses_needs_you_priority(self) -> None:
        self.audit.create_pending_approval(
            request_id="approval-1",
            session_id="session-1",
            tool_name="artifact.pdf.create",
            arguments={"title": "Recipe cards"},
        )

        scheduler = self.scheduler()
        report = scheduler.run_once()

        self.assertEqual(1, report["delivered"])
        self.assertEqual("needs_you", self.created[0]["priority"])
        self.assertIn("approval", str(self.created[0]["title"]).lower())

    def test_deadline_inside_twenty_four_hours_is_needs_you(self) -> None:
        self.responses["/v1/tasks?include_completed=true&limit=500"] = {
            "tasks": [{"id": "task-urgent"}],
            "details": [{
                "task": {
                    "id": "task-urgent",
                    "title": "Submit order",
                    "status": "ready",
                    "due_at": "2026-08-05T10:00:00+00:00",
                },
                "blockers": [],
                "initiative": None,
            }],
        }

        self.scheduler().run_once()

        self.assertEqual("needs_you", self.created[0]["priority"])
        self.assertIn("Deadline", str(self.created[0]["title"]))

    def test_do_not_disturb_holds_even_needs_you_events(self) -> None:
        self.responses["/v1/outreach/policy"] = policy(do_not_disturb=True)
        self.audit.create_pending_approval(
            request_id="approval-2",
            session_id="session-1",
            tool_name="project.tests",
            arguments={},
        )

        scheduler = self.scheduler()
        report = scheduler.run_once()

        self.assertEqual(0, report["delivered"])
        self.assertEqual(1, report["held_by_policy"])
        self.assertEqual([], self.created)
        self.assertEqual(1, len(self.audit.pending_review_notices()))

        self.responses["/v1/outreach/policy"] = policy(do_not_disturb=False)
        released = scheduler.run_once()
        self.assertEqual(1, released["delivered"])
        self.assertEqual("needs_you", self.created[0]["priority"])
        self.assertEqual([], self.audit.pending_review_notices())

    def test_email_model_fallback_runs_only_for_marked_signals(self) -> None:
        interpreted: list[dict[str, object]] = []
        self.responses["https://email/signals"] = {
            "signals": [
                {"id": "routine", "title": "Newsletter", "summary": "Weekly news"},
                {"id": "ambiguous", "requires_interpretation": True, "title": "Can you decide?"},
            ]
        }
        scheduler = ReviewScheduler(
            "http://memory",
            self.audit,
            email_signals_url="https://email/signals",
            fetch_json=lambda path: self.responses.get(path),
            create_outreach=lambda payload: {"outreach": {"id": "one", **payload}},
            post_json=lambda path, payload: self.posted.append((path, payload)) or {"ok": True},
            interpret_signal=lambda signal: interpreted.append(signal) or {**signal, "urgent": True, "summary": "Decision needed"},
            health_probe=lambda: {"status": "healthy", "issues": []},
            now=lambda: NOW,
        )

        scheduler.run_once()

        self.assertEqual(["ambiguous"], [item["id"] for item in interpreted])
        email_items = [payload for path, payload in self.posted if path == "/v1/attention"]
        self.assertTrue(any(item["category"] == "email" and item["approval_required"] for item in email_items))

    def test_calendar_signal_is_normalized_for_planning_and_attention(self) -> None:
        self.responses["https://calendar/signals"] = {
            "signals": [{
                "id": "invite-1",
                "title": "Site visit",
                "summary": "Invitation needs a response",
                "start_at": "2026-08-05T13:00:00+00:00",
                "end_at": "2026-08-05T14:00:00+00:00",
                "response_status": "needs_action",
                "location": "work",
            }]
        }
        scheduler = ReviewScheduler(
            "http://memory",
            self.audit,
            calendar_signals_url="https://calendar/signals",
            fetch_json=lambda path: self.responses.get(path),
            create_outreach=lambda payload: {"outreach": {"id": "one", **payload}},
            post_json=lambda path, payload: self.posted.append((path, payload)) or {"ok": True},
            health_probe=lambda: {"status": "healthy", "issues": []},
            now=lambda: NOW,
        )
        scheduler.run_once()
        paths = [path for path, _ in self.posted]
        self.assertIn("/v1/calendar/events", paths)
        attention = next(payload for path, payload in self.posted if path == "/v1/attention")
        self.assertEqual("calendar", attention["category"])
        self.assertTrue(attention["approval_required"])

    def test_disabled_automation_is_a_real_off_switch(self) -> None:
        rules = automations()
        approval = next(
            rule
            for rule in rules["automations"]
            if rule["trigger"]["source"] == "approval"
        )
        approval["enabled"] = False
        self.responses["/v1/automations?include_disabled=false&limit=500"] = rules
        self.audit.create_pending_approval(
            request_id="approval-disabled",
            session_id="session-1",
            tool_name="project.tests",
            arguments={},
        )

        report = self.scheduler().run_once()

        self.assertEqual(0, report["delivered"])
        self.assertEqual([], self.created)

    def test_automation_frequency_limit_prevents_a_second_execution(self) -> None:
        rules = automations()
        approval = next(
            rule
            for rule in rules["automations"]
            if rule["trigger"]["source"] == "approval"
        )
        approval["frequency_limit"] = {"max_runs": 1, "window_minutes": 1440}
        self.responses["/v1/automations?include_disabled=false&limit=500"] = rules
        scheduler = self.scheduler()
        self.audit.create_pending_approval(
            request_id="approval-first",
            session_id="session-1",
            tool_name="project.tests",
            arguments={},
        )
        scheduler.run_once()
        self.audit.set_review_state("last_interruption_at", "2026-08-03T00:00:00+00:00")
        self.audit.create_pending_approval(
            request_id="approval-second",
            session_id="session-1",
            tool_name="artifact.pdf.create",
            arguments={"title": "Plan"},
        )

        scheduler.run_once()

        self.assertEqual(1, len(self.created))

    def test_sleep_cycle_runs_once_in_quiet_hours_when_system_is_healthy(self) -> None:
        quiet_now = datetime(2026, 8, 4, 23, 0, tzinfo=UTC)
        self.responses["/v1/outreach/policy"] = policy()
        scheduler = ReviewScheduler(
            "http://memory",
            self.audit,
            fetch_json=lambda path: self.responses.get(path),
            create_outreach=lambda payload: {"outreach": {"id": "one", **payload}},
            post_json=lambda path, payload: self.posted.append((path, payload)) or {"cycle": {"id": "cycle-1"}},
            health_probe=lambda: {"status": "healthy", "issues": [], "checked_at": quiet_now.isoformat()},
            now=lambda: quiet_now,
            sleep_memory_enabled=True,
        )

        first = scheduler.run_once()
        second = scheduler.run_once()

        calls = [item for item in self.posted if item[0] == "/internal/v1/memory/sleep/run"]
        self.assertEqual(1, len(calls))
        self.assertEqual("completed", first["sleep_cycle"]["status"])
        self.assertEqual("already_ran", second["sleep_cycle"]["status"])


if __name__ == "__main__":
    unittest.main()
