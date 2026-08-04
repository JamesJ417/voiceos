from __future__ import annotations

import unittest
from unittest.mock import patch

from services.gateway.coordinator import CoordinatedResponse, TurnCoordinator
from services.gateway.server import VoiceOSServer


class _Coordinator:
    def __init__(self) -> None:
        self.prompt = ""

    def respond(self, text: str, **_: object) -> CoordinatedResponse:
        self.prompt = text
        return CoordinatedResponse(
            text="I prepared the first three next actions.",
            provider="hermes",
            results=[{"artifact": "next-actions"}],
        )


class _Audit:
    def __init__(self) -> None:
        self.turns: list[dict[str, object]] = []
        self.events: list[tuple[str, dict[str, object]]] = []

    def record_turn(self, **values: object) -> None:
        self.turns.append(values)

    def publish_client_event(self, kind: str, payload: dict[str, object]) -> None:
        self.events.append((kind, payload))

    def create_pending_approval(self, **values: object) -> dict[str, object]:
        return dict(values)


class _Gateway:
    memory_url = "http://rust-core"

    def __init__(self) -> None:
        self.coordinator = _Coordinator()
        self.audit_store = _Audit()


class TaskInitiativeWorkerTest(unittest.TestCase):
    def test_proactive_scope_blocks_even_safe_tools_not_explicitly_granted(self) -> None:
        response = TurnCoordinator().respond(
            "Check system health",
            allowed_tools=set(),
        )
        self.assertEqual("policy", response.provider)
        self.assertEqual("capability_scope_denied", response.errors[0]["type"])

    def test_claims_runs_and_records_safe_hermes_work(self) -> None:
        gateway = _Gateway()
        calls: list[tuple[str, dict[str, object]]] = []

        def post(url: str, payload: dict[str, object]) -> dict[str, object]:
            calls.append((url, payload))
            return {"claimed": True} if url.endswith("/claim") else {"recorded": True}

        task = {
            "id": "task-1",
            "title": "Print recipe cards",
            "observable_outcome": "Cards are ready",
        }
        initiative = {"job_id": "job-1", "capabilities": ["task.next_actions"]}
        with patch("services.gateway.server._post_json", side_effect=post):
            VoiceOSServer._run_task_initiative(
                gateway, task, initiative, "pixel"  # type: ignore[arg-type]
            )

        self.assertEqual(2, len(calls))
        self.assertIn("untrusted user data", gateway.coordinator.prompt)
        self.assertEqual("completed", calls[1][1]["status"])
        self.assertEqual(1, len(gateway.audit_store.turns))
        self.assertEqual("task.initiative.updated", gateway.audit_store.events[0][0])

    def test_does_nothing_when_another_worker_already_claimed_job(self) -> None:
        gateway = _Gateway()
        with patch(
            "services.gateway.server._post_json", return_value={"claimed": False}
        ):
            VoiceOSServer._run_task_initiative(
                gateway,
                {"id": "task-1", "title": "A task"},
                {"job_id": "job-1", "capabilities": []},
                "pixel",
            )
        self.assertEqual("", gateway.coordinator.prompt)
        self.assertEqual([], gateway.audit_store.turns)


if __name__ == "__main__":
    unittest.main()
