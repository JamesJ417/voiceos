"""Deterministic changed-only review loop for proactive VIC outreach."""

from __future__ import annotations

import hashlib
import json
import threading
from dataclasses import asdict, dataclass
from datetime import UTC, datetime
from typing import Callable
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from services.gateway.audit import AuditStore
from services.gateway.system_health import collect_system_health


JsonObject = dict[str, object]
FetchJson = Callable[[str], JsonObject | None]
CreateOutreach = Callable[[JsonObject], JsonObject | None]
InterpretSignal = Callable[[JsonObject], JsonObject | None]
PostJson = Callable[[str, JsonObject], JsonObject | None]


@dataclass(frozen=True)
class ReviewNotice:
    dedupe_key: str
    category: str
    kind: str
    priority: str
    title: str
    body: str
    reason: str
    task_id: str | None = None


class ReviewScheduler:
    """Scans changed records and converts only actionable changes into outreach."""

    def __init__(
        self,
        memory_url: str,
        audit_store: AuditStore,
        *,
        email_signals_url: str | None = None,
        calendar_signals_url: str | None = None,
        communication_signals_url: str | None = None,
        fetch_json: FetchJson | None = None,
        create_outreach: CreateOutreach | None = None,
        interpret_signal: InterpretSignal | None = None,
        post_json: PostJson | None = None,
        health_probe: Callable[[], JsonObject] | None = None,
        now: Callable[[], datetime] | None = None,
        sleep_memory_enabled: bool = False,
    ) -> None:
        self.memory_url = memory_url.rstrip("/")
        self.audit = audit_store
        self.email_signals_url = email_signals_url
        self.calendar_signals_url = calendar_signals_url
        self.communication_signals_url = communication_signals_url
        self._fetch_json = fetch_json or self._fetch
        self._create_outreach = create_outreach or self._create
        self._interpret_signal = interpret_signal
        self._post_json = post_json or self._safe_post
        self._health_probe = health_probe or collect_system_health
        self._now = now or (lambda: datetime.now(UTC))
        self.sleep_memory_enabled = sleep_memory_enabled
        self._stop = threading.Event()
        self._scan_lock = threading.Lock()
        self._thread: threading.Thread | None = None
        self._active_rules: list[JsonObject] = []
        self._active_policy: JsonObject = {}
        self._current_local_now = self._now()

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._thread = threading.Thread(
            target=self._run,
            daemon=True,
            name="vic-review-scheduler",
        )
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=2)

    def trigger(self) -> None:
        """Run an immediate changed-only scan after an authoritative event."""
        threading.Thread(
            target=self._run_triggered,
            daemon=True,
            name="vic-review-event",
        ).start()

    def _run_triggered(self) -> None:
        if not self._scan_lock.acquire(blocking=False):
            return
        try:
            self.run_once()
        except (OSError, RuntimeError, ValueError, HTTPError, URLError):
            return
        finally:
            self._scan_lock.release()

    def run_once(self) -> JsonObject:
        policy = self._policy()
        self._active_policy = policy
        local_now = self._local_now(policy)
        self._current_local_now = local_now
        rules_payload = self._safe_fetch("/v1/automations?include_disabled=false&limit=500") or {}
        rules = rules_payload.get("automations")
        self._active_rules = [rule for rule in rules if isinstance(rule, dict)] if isinstance(rules, list) else []
        scan_rule = self._automation_rule("review", "review.scan", local_now)
        if scan_rule is None:
            return {
                "checked_at": self._now().isoformat(),
                "status": "disabled_or_rate_limited",
                "changed_sources": [],
                "notices_considered": 0,
                "delivered": 0,
                "held_by_policy": 0,
                "next_scan_minutes": int(policy.get("scan_interval_minutes", 20)),
            }
        notices: list[ReviewNotice] = []
        changed_sources: list[str] = []
        delivered = self._release_held_notices(policy, local_now)

        snapshots = {
            "tasks": self._safe_fetch("/v1/tasks?include_completed=true&limit=500"),
            "files": self._safe_fetch("/v1/artifacts?limit=500"),
            "knowledge": self._safe_fetch("/v1/files"),
            "system": self._stable_health(),
            "approvals": {"approvals": self.audit.pending_approvals()},
        }
        outreach_payload = self._safe_fetch("/v1/outreach?limit=200")
        if isinstance(outreach_payload, dict):
            outreach = outreach_payload.get("outreach")
            snapshots["unanswered_questions"] = {
                "questions": [
                    item
                    for item in outreach
                    if isinstance(item, dict)
                    and item.get("kind") == "question"
                    and item.get("status") in {"queued", "delivered", "snoozed"}
                ]
                if isinstance(outreach, list)
                else []
            }
        if self.email_signals_url:
            snapshots["emails"] = self._safe_fetch(self.email_signals_url)
        if self.calendar_signals_url:
            snapshots["calendar"] = self._safe_fetch(self.calendar_signals_url)
        if self.communication_signals_url:
            snapshots["messages"] = self._safe_fetch(self.communication_signals_url)

        for source, snapshot in snapshots.items():
            if snapshot is None:
                continue
            fingerprint = _fingerprint(snapshot)
            key = f"snapshot:{source}"
            previous = self.audit.review_state(key)
            if previous == fingerprint:
                continue
            self.audit.set_review_state(key, fingerprint)
            changed_sources.append(source)
            notices.extend(self._notices_for(source, snapshot, first_scan=previous is None))

        held = 0
        for notice in notices:
            self._upsert_attention(notice)
            if notice.priority == "quiet":
                rule = self._automation_rule(notice.category, "digest.add", local_now)
                if rule is None:
                    continue
                added = self.audit.add_review_digest_item(
                    notice.dedupe_key,
                    notice.category,
                    notice.title,
                    notice.body,
                )
                if added:
                    self._record_automation(rule, "digest.add", notice.dedupe_key)
                continue
            rule = self._automation_rule(notice.category, "notify.needs_you", local_now)
            if rule is None:
                continue
            if not self._may_interrupt(policy, local_now):
                self.audit.hold_review_notice(notice.dedupe_key, asdict(notice))
                held += 1
                continue
            if self._deliver(notice):
                delivered += 1
                self._record_interruption(local_now)
                self._record_automation(rule, "notify.needs_you", notice.dedupe_key)
            else:
                self.audit.hold_review_notice(notice.dedupe_key, asdict(notice))
                held += 1

        delivered += self._maybe_daily_planning(policy, local_now)
        delivered += self._maybe_digest(policy, local_now)
        sleep_cycle = self._maybe_sleep_cycle(policy, local_now, snapshots.get("system"))
        report: JsonObject = {
            "checked_at": self._now().isoformat(),
            "changed_sources": changed_sources,
            "notices_considered": len(notices),
            "delivered": delivered,
            "held_by_policy": held,
            "next_scan_minutes": int(policy.get("scan_interval_minutes", 20)),
            "sleep_cycle": sleep_cycle,
        }
        self.audit.set_review_state("last_report", report)
        self._record_automation(scan_rule, "review.scan", _fingerprint(report))
        self.audit.publish_client_event("vic.review.completed", report)
        return report

    def _maybe_sleep_cycle(
        self,
        policy: JsonObject,
        local_now: datetime,
        system_snapshot: object,
    ) -> JsonObject | None:
        """Run at most one bounded cycle per local day, only during quiet hours."""
        if not self.sleep_memory_enabled or bool(policy.get("do_not_disturb", False)):
            return None
        if not _inside_window(
            local_now.strftime("%H:%M"),
            str(policy.get("quiet_hours_start", "22:00")),
            str(policy.get("quiet_hours_end", "08:00")),
        ):
            return None
        if not isinstance(system_snapshot, dict) or system_snapshot.get("status") != "healthy":
            return {"status": "skipped", "reason": "system_not_healthy"}
        state_key = f"sleep-memory:{local_now.date().isoformat()}"
        if self.audit.review_state(state_key):
            return {"status": "already_ran"}
        result = self._post_json(
            "/internal/v1/memory/sleep/run",
            {
                "mode": "commit",
                "trigger_kind": "scheduled",
                "config": {"max_events": 96, "model_call_budget": 2},
            },
        )
        if not isinstance(result, dict):
            return {"status": "failed"}
        cycle = result.get("cycle")
        cycle_id = cycle.get("id") if isinstance(cycle, dict) else None
        self.audit.set_review_state(state_key, {"cycle_id": cycle_id, "completed_at": self._now().isoformat()})
        return {"status": "completed", "cycle_id": cycle_id}

    def _run(self) -> None:
        if self._stop.wait(5):
            return
        while not self._stop.is_set():
            try:
                with self._scan_lock:
                    report = self.run_once()
                minutes = int(report.get("next_scan_minutes", 20))
            except (OSError, RuntimeError, ValueError, HTTPError, URLError):
                minutes = 20
            self._stop.wait(max(15, min(30, minutes)) * 60)

    def _policy(self) -> JsonObject:
        payload = self._safe_fetch("/v1/outreach/policy") or {}
        policy = payload.get("policy")
        return policy if isinstance(policy, dict) else {
            "enabled": True,
            "quiet_hours_start": "22:00",
            "quiet_hours_end": "08:00",
            "timezone": "America/New_York",
            "max_checkins_per_day": 6,
            "cooldown_minutes": 30,
            "daily_digest_enabled": True,
            "do_not_disturb": False,
            "current_location": "unknown",
            "daily_planning_time": "08:30",
            "morning_digest_time": "08:00",
            "evening_digest_time": "18:00",
            "scan_interval_minutes": 20,
        }

    def _local_now(self, policy: JsonObject) -> datetime:
        try:
            timezone = ZoneInfo(str(policy.get("timezone", "America/New_York")))
        except ZoneInfoNotFoundError:
            timezone = UTC
        return self._now().astimezone(timezone)

    def _stable_health(self) -> JsonObject:
        health = self._health_probe()
        health.pop("checked_at", None)
        return health

    def _notices_for(
        self, source: str, snapshot: JsonObject, *, first_scan: bool
    ) -> list[ReviewNotice]:
        if source == "system":
            if snapshot.get("status") == "degraded":
                issues = ", ".join(str(item) for item in snapshot.get("issues", []))
                return [ReviewNotice(
                    f"system:{_fingerprint(snapshot)}", "system", "blocker", "needs_you",
                    "VIC detected a system problem", f"System health is degraded: {issues}.",
                    "A deterministic health check failed.",
                )]
            if not first_scan:
                return [ReviewNotice(
                    f"system-recovered:{_fingerprint(snapshot)}", "system", "status_update", "quiet",
                    "System health recovered", "The latest deterministic health check is healthy.",
                    "A previously changed system condition is now healthy.",
                )]
            return []
        if source == "approvals":
            approvals = snapshot.get("approvals")
            if not isinstance(approvals, list):
                return []
            return [ReviewNotice(
                f"approval:{item.get('request_id')}", "approval", "review", "needs_you",
                "VIC needs your approval", f"Review the proposed {item.get('tool_name', 'action')} action.",
                "A structured tool is waiting at an approval boundary.",
            ) for item in approvals if isinstance(item, dict)]
        if source == "tasks":
            return self._task_notices(snapshot, first_scan)
        if source == "emails":
            return self._external_notices("email", snapshot)
        if source == "calendar":
            return self._external_notices("calendar", snapshot)
        if source == "messages":
            return self._external_notices("message", snapshot)
        if source == "unanswered_questions" and not first_scan:
            questions = snapshot.get("questions")
            count = len(questions) if isinstance(questions, list) else 0
            if count:
                return [ReviewNotice(
                    f"questions:{_fingerprint(snapshot)}", "question", "digest", "quiet",
                    "VIC has unanswered questions", f"There are {count} unanswered VIC questions.",
                    "The changed-only unanswered-question scan found pending items.",
                )]
            return []
        if source in {"files", "knowledge"} and not first_scan:
            label = "VIC-created files" if source == "files" else "private knowledge files"
            return [ReviewNotice(
                f"{source}:{_fingerprint(snapshot)}", "document", "status_update", "quiet",
                f"Changes in {label}", f"VIC found updates in {label}.",
                "The changed-only catalog fingerprint was updated.",
            )]
        return []

    def _task_notices(self, snapshot: JsonObject, first_scan: bool) -> list[ReviewNotice]:
        details = snapshot.get("details")
        if not isinstance(details, list):
            return []
        notices: list[ReviewNotice] = []
        for detail in details:
            if not isinstance(detail, dict):
                continue
            task = detail.get("task")
            if not isinstance(task, dict):
                continue
            task_id = str(task.get("id", ""))
            title = str(task.get("title", "Task"))
            blockers = detail.get("blockers")
            open_blockers = [item for item in blockers if isinstance(item, dict) and item.get("status") == "open"] if isinstance(blockers, list) else []
            initiative = detail.get("initiative")
            due_at = task.get("due_at")
            deadline_urgent = False
            if isinstance(due_at, str) and task.get("status") not in {"completed", "cancelled"}:
                try:
                    deadline_urgent = (datetime.fromisoformat(due_at).astimezone(UTC) - self._now()).total_seconds() <= 86_400
                except ValueError:
                    deadline_urgent = False
            if deadline_urgent:
                notices.append(ReviewNotice(
                    f"task-deadline:{task_id}:{due_at}", "task", "question", "needs_you",
                    f"Deadline is urgent: {title}", f"This task is due at {due_at}.",
                    "A task deadline is overdue or less than 24 hours away.", task_id,
                ))
            elif open_blockers:
                notices.append(ReviewNotice(
                    f"task-blocker:{task_id}:{_fingerprint(open_blockers)}", "task", "blocker", "needs_you",
                    f"Blocked: {title}", str(open_blockers[0].get("description", "A task blocker needs attention.")),
                    "A task has an unresolved blocker.", task_id,
                ))
            elif isinstance(initiative, dict) and initiative.get("status") == "failed":
                notices.append(ReviewNotice(
                    f"task-failed:{task_id}:{initiative.get('updated_at')}", "task", "blocker", "needs_you",
                    f"VIC could not finish: {title}", "The task initiative failed and needs review.",
                    "A VIC background job failed.", task_id,
                ))
            elif isinstance(initiative, dict) and initiative.get("status") == "completed" and not first_scan:
                notices.append(ReviewNotice(
                    f"task-complete:{task_id}:{initiative.get('updated_at')}", "task", "status_update", "quiet",
                    f"VIC completed work on {title}", "Open the task to review VIC's progress and evidence.",
                    "A VIC background job completed.", task_id,
                ))
            elif not first_scan:
                notices.append(ReviewNotice(
                    f"task-change:{task_id}:{task.get('updated_at')}", "task", "status_update", "quiet",
                    f"Task updated: {title}", "The task or its progress details changed.",
                    "The changed-only task scan found an update.", task_id,
                ))
        return notices

    def _external_notices(self, source: str, snapshot: JsonObject) -> list[ReviewNotice]:
        signals = snapshot.get("signals")
        if not isinstance(signals, list):
            return []
        notices: list[ReviewNotice] = []
        for signal in signals:
            if not isinstance(signal, dict):
                continue
            interpreted = signal
            signal_id = str(signal.get("id", _fingerprint(signal)))
            if signal.get("requires_interpretation") and self._interpret_signal:
                rule = self._automation_rule(source, "model.classify", self._current_local_now)
                if rule is not None:
                    interpreted = self._interpret_signal({**signal, "signal_source": source}) or signal
                    self._record_automation(rule, "model.classify", signal_id)
            identifier = str(interpreted.get("id", _fingerprint(interpreted)))
            if source == "calendar":
                self._ingest_calendar_signal(interpreted, identifier)
            urgent = bool(interpreted.get("urgent") or interpreted.get("needs_you"))
            if source == "calendar" and isinstance(interpreted.get("start_at"), str):
                try:
                    seconds_until = (
                        datetime.fromisoformat(str(interpreted["start_at"])).astimezone(UTC)
                        - self._now()
                    ).total_seconds()
                    urgent = urgent or seconds_until <= 86_400
                except ValueError:
                    pass
            notices.append(ReviewNotice(
                f"{source}:{identifier}", source, "question" if urgent else "status_update",
                "needs_you" if urgent else "quiet",
                str(interpreted.get("title", f"{source.title()} update")),
                str(interpreted.get("summary", f"A {source} signal changed.")),
                f"The {source} adapter marked this signal actionable." if urgent else f"Routine {source} activity is held for a digest.",
            ))
        return notices

    def _upsert_attention(self, notice: ReviewNotice) -> None:
        category = {
            "task": "agent_work",
            "planning": "question",
            "digest": "agent_work",
        }.get(notice.category, notice.category)
        actions = ["summarize", "review", "create_task", "resolve", "dismiss", "snooze"]
        approval_required = category in {"approval", "email", "calendar"}
        if category == "email":
            actions.extend(["prepare_reply", "request_send_approval"])
        if category == "calendar":
            actions.append("request_invitation_approval")
        payload: JsonObject = {
            "category": category,
            "source_id": notice.dedupe_key,
            "title": notice.title,
            "summary": notice.body,
            "urgency": "urgent" if notice.priority == "needs_you" else "routine",
            "task_id": notice.task_id,
            "occurred_at": self._now().isoformat(),
            "approval_required": approval_required,
            "available_actions": actions,
            "evidence": {"reason": notice.reason, "dedupe_key": notice.dedupe_key},
        }
        self._post_json("/v1/attention", payload)

    def _ingest_calendar_signal(self, signal: JsonObject, identifier: str) -> None:
        start_at = signal.get("start_at")
        end_at = signal.get("end_at")
        if not isinstance(start_at, str) or not isinstance(end_at, str):
            return
        self._post_json(
            "/v1/calendar/events",
            {
                "source_id": identifier,
                "title": str(signal.get("title", "Calendar event")),
                "start_at": start_at,
                "end_at": end_at,
                "location": signal.get("location"),
                "status": str(signal.get("status", "confirmed")),
                "response_status": str(signal.get("response_status", "none")),
                "preparation_minutes": int(signal.get("preparation_minutes", 0)),
                "travel_minutes": int(signal.get("travel_minutes", 0)),
                "metadata": {"adapter_signal": signal},
            },
        )

    def _safe_post(self, path: str, payload: JsonObject) -> JsonObject | None:
        try:
            request = Request(
                f"{self.memory_url}{path}",
                data=json.dumps(payload, separators=(",", ":")).encode("utf-8"),
                headers={"Content-Type": "application/json", "Accept": "application/json"},
                method="POST",
            )
            with urlopen(request, timeout=10) as response:
                result = json.loads(response.read(256 * 1024))
            return result if isinstance(result, dict) else None
        except (OSError, RuntimeError, ValueError, HTTPError, URLError):
            return None

    def _may_interrupt(self, policy: JsonObject, local_now: datetime) -> bool:
        if not bool(policy.get("enabled", True)) or bool(policy.get("do_not_disturb", False)):
            return False
        if bool(policy.get("driving_mode", False)) or policy.get("current_location") == "driving":
            return False
        if _inside_window(
            local_now.strftime("%H:%M"),
            str(policy.get("quiet_hours_start", "22:00")),
            str(policy.get("quiet_hours_end", "08:00")),
        ):
            return False
        date_key = local_now.date().isoformat()
        count = int(self.audit.review_state(f"interruptions:{date_key}", 0))
        if count >= int(policy.get("max_checkins_per_day", 6)):
            return False
        last_value = self.audit.review_state("last_interruption_at")
        if isinstance(last_value, str):
            last = datetime.fromisoformat(last_value)
            elapsed = (self._now() - last.astimezone(UTC)).total_seconds() / 60
            if elapsed < int(policy.get("cooldown_minutes", 30)):
                return False
        return True

    def _release_held_notices(self, policy: JsonObject, local_now: datetime) -> int:
        delivered = 0
        for payload in self.audit.pending_review_notices():
            if not self._may_interrupt(policy, local_now):
                break
            try:
                notice = ReviewNotice(**payload)
            except TypeError:
                continue
            rule = self._automation_rule(notice.category, "notify.needs_you", local_now)
            if rule is None:
                continue
            if not self._deliver(notice):
                break
            self.audit.release_review_notice(notice.dedupe_key)
            self._record_interruption(local_now)
            self._record_automation(rule, "notify.needs_you", notice.dedupe_key)
            delivered += 1
        return delivered

    def _record_interruption(self, local_now: datetime) -> None:
        date_key = local_now.date().isoformat()
        key = f"interruptions:{date_key}"
        self.audit.set_review_state(key, int(self.audit.review_state(key, 0)) + 1)
        self.audit.set_review_state("last_interruption_at", self._now().isoformat())

    def _maybe_daily_planning(self, policy: JsonObject, local_now: datetime) -> int:
        if not self._may_deliver_silent(policy, local_now):
            return 0
        rule = self._automation_rule("planning", "planning.prompt", local_now)
        if rule is None:
            return 0
        date_key = local_now.date().isoformat()
        state_key = f"daily-planning:{date_key}"
        if self.audit.review_state(state_key) or local_now.strftime("%H:%M") < str(policy.get("daily_planning_time", "08:30")):
            return 0
        notice = ReviewNotice(
            state_key, "planning", "check_in", "quiet", "Ready for today’s twelve questions?",
            "Open VIC when you are ready to build today’s priorities and next actions.",
            "The configured daily planning time has arrived.",
        )
        delivered = self._deliver(notice)
        if delivered:
            self.audit.set_review_state(state_key, True)
            self._record_automation(rule, "planning.prompt", state_key)
        return int(delivered)

    def _maybe_digest(self, policy: JsonObject, local_now: datetime) -> int:
        if not bool(policy.get("daily_digest_enabled", True)) or not self._may_deliver_silent(policy, local_now):
            return 0
        rule = self._automation_rule("digest", "digest.deliver", local_now)
        if rule is None:
            return 0
        current = local_now.strftime("%H:%M")
        slots = [str(policy.get("morning_digest_time", "08:00")), str(policy.get("evening_digest_time", "18:00"))]
        eligible = [slot for slot in slots if current >= slot]
        if not eligible:
            return 0
        slot = eligible[-1]
        state_key = f"digest:{local_now.date().isoformat()}:{slot}"
        if self.audit.review_state(state_key):
            return 0
        items = self.audit.pending_review_digest()
        if not items:
            return 0
        grouped: dict[str, int] = {}
        for item in items:
            category = str(item["category"])
            grouped[category] = grouped.get(category, 0) + 1
        body = "Routine updates: " + ", ".join(f"{count} {category}" for category, count in sorted(grouped.items())) + "."
        notice = ReviewNotice(
            state_key, "digest", "digest", "quiet", "VIC routine update", body,
            "Routine changes were combined to prevent repeated interruptions.",
        )
        if not self._deliver(notice):
            return 0
        self.audit.set_review_state(state_key, True)
        self.audit.consume_review_digest([int(item["item_id"]) for item in items])
        self._record_automation(rule, "digest.deliver", state_key)
        return 1

    def _may_deliver_silent(self, policy: JsonObject, local_now: datetime) -> bool:
        if not bool(policy.get("enabled", True)) or bool(policy.get("do_not_disturb", False)):
            return False
        if bool(policy.get("driving_mode", False)) or policy.get("current_location") in {"driving", "away"}:
            return False
        return not _inside_window(
            local_now.strftime("%H:%M"),
            str(policy.get("quiet_hours_start", "22:00")),
            str(policy.get("quiet_hours_end", "08:00")),
        )

    def _automation_rule(
        self, source: str, action: str, local_now: datetime
    ) -> JsonObject | None:
        del local_now  # Frequency windows use UTC epoch time; policy evaluates local time separately.
        normalized_source = {
            "task": "tasks",
            "files": "document",
            "knowledge": "document",
        }.get(source, source)
        for rule in self._active_rules:
            if not bool(rule.get("enabled", False)):
                continue
            trigger = rule.get("trigger")
            conditions = rule.get("conditions")
            actions = rule.get("permitted_actions")
            if not isinstance(trigger, dict) or trigger.get("source") != normalized_source:
                continue
            if not isinstance(actions, list) or action not in actions:
                continue
            if not self._conditions_match(conditions if isinstance(conditions, dict) else {}):
                continue
            frequency = rule.get("frequency_limit")
            if not isinstance(frequency, dict):
                continue
            max_runs = int(frequency.get("max_runs", 0))
            window_minutes = int(frequency.get("window_minutes", 0))
            state_key = f"automation-runs:{rule.get('id')}"
            history = self.audit.review_state(state_key, [])
            if not isinstance(history, list):
                history = []
            cutoff = self._now().timestamp() - window_minutes * 60
            recent = [
                value
                for value in history
                if isinstance(value, (int, float)) and value >= cutoff
            ]
            self.audit.set_review_state(state_key, recent)
            if max_runs < 1 or window_minutes < 1 or len(recent) >= max_runs:
                continue
            return rule
        return None

    def _conditions_match(self, conditions: JsonObject) -> bool:
        location = str(self._active_policy.get("current_location", "unknown"))
        allowed = conditions.get("locations")
        if isinstance(allowed, list) and location not in allowed:
            return False
        excluded = conditions.get("excluded_locations")
        if isinstance(excluded, list) and location in excluded:
            return False
        if bool(conditions.get("requires_available", False)) and (
            bool(self._active_policy.get("do_not_disturb", False))
            or bool(self._active_policy.get("driving_mode", False))
        ):
            return False
        return True

    def _record_automation(self, rule: JsonObject, action: str, evidence_key: str) -> None:
        rule_id = str(rule.get("id", ""))
        state_key = f"automation-runs:{rule_id}"
        history = self.audit.review_state(state_key, [])
        values = list(history) if isinstance(history, list) else []
        values.append(self._now().timestamp())
        self.audit.set_review_state(state_key, values[-1_000:])
        self.audit.publish_client_event(
            "automation.executed",
            {
                "automation_id": rule_id,
                "owner_id": rule.get("owner_id"),
                "action": action,
                "evidence_key": evidence_key,
                "rule_evidence": rule.get("evidence"),
            },
        )

    def _deliver(self, notice: ReviewNotice) -> bool:
        payload: JsonObject = {
            "kind": notice.kind,
            "priority": notice.priority,
            "title": notice.title,
            "body": notice.body,
            "reason": notice.reason,
            "task_id": notice.task_id,
            "dedupe_key": notice.dedupe_key,
            "actions": ["talk_now", "show_progress", "later", "dismiss"],
        }
        try:
            result = self._create_outreach(payload)
        except (OSError, RuntimeError, ValueError, HTTPError, URLError):
            return False
        outreach = result.get("outreach") if isinstance(result, dict) else None
        if not isinstance(outreach, dict):
            return False
        self.audit.publish_client_event("vic.outreach.created", outreach)
        return True

    def _safe_fetch(self, path_or_url: str) -> JsonObject | None:
        try:
            return self._fetch_json(path_or_url)
        except (OSError, RuntimeError, ValueError, HTTPError, URLError):
            return None

    def _fetch(self, path_or_url: str) -> JsonObject | None:
        url = path_or_url if path_or_url.startswith(("http://", "https://")) else f"{self.memory_url}{path_or_url}"
        with urlopen(Request(url, headers={"Accept": "application/json"}), timeout=10) as response:
            payload = json.loads(response.read(3 * 1024 * 1024))
        return payload if isinstance(payload, dict) else None

    def _create(self, payload: JsonObject) -> JsonObject | None:
        request = Request(
            f"{self.memory_url}/v1/outreach",
            data=json.dumps(payload, separators=(",", ":")).encode("utf-8"),
            headers={"Content-Type": "application/json", "Accept": "application/json"},
            method="POST",
        )
        with urlopen(request, timeout=10) as response:
            result = json.loads(response.read(256 * 1024))
        return result if isinstance(result, dict) else None


def _fingerprint(value: object) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), default=str).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def _inside_window(current: str, start: str, end: str) -> bool:
    if start == end:
        return False
    if start < end:
        return start <= current < end
    return current >= start or current < end
