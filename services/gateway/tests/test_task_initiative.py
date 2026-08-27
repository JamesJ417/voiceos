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

    def enqueue_bridge_notification(self, **values: object) -> dict[str, object]:
        return dict(values)

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
            if url.endswith("/claim"):
                return {"claimed": True}
            if url.endswith("/v1/outreach"):
                return {"outreach": {"id": "outreach-1", **payload}}
            return {"recorded": True}

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

        self.assertEqual(3, len(calls))
        self.assertIn("untrusted user data", gateway.coordinator.prompt)
        self.assertEqual("completed", calls[1][1]["status"])
        self.assertEqual(1, len(gateway.audit_store.turns))
        event_names = [name for name, _ in gateway.audit_store.events]
        self.assertIn("agent.worker.updated", event_names)
        self.assertIn("task.initiative.updated", event_names)
        self.assertIn("vic.outreach.created", event_names)
        self.assertEqual("check_in", calls[2][1]["priority"])

    def test_records_structured_hermes_progress_on_the_task_board(self) -> None:
        class ProgressCoordinator(_Coordinator):
            def respond(self, text: str, **_: object) -> CoordinatedResponse:
                self.prompt = text
                return CoordinatedResponse(
                    text=(
                        "I identified the website URL and approved menu as the next inputs.\n\n"
                        "```voiceos-task-update\n"
                        '{"actions":[{"action":"progress.record","summary":"Identified the website URL and approved menu as the next required inputs.","evidence":{"source":"vic-analysis"}},{"action":"blocker.create","description":"Website URL and approved Sunday brunch menu have not been provided.","owner":"user"}]}\n'
                        "```"
                    ),
                    provider="hermes",
                )

        gateway = _Gateway()
        gateway.coordinator = ProgressCoordinator()
        calls: list[tuple[str, dict[str, object]]] = []

        def post(url: str, payload: dict[str, object]) -> dict[str, object]:
            calls.append((url, payload))
            if url.endswith("/claim"):
                return {"claimed": True}
            if url.endswith("/v1/outreach"):
                return {"outreach": {"id": "outreach-1", **payload}}
            return {"recorded": True}

        task = {"id": "task-1", "title": "Update the website", "observable_outcome": "Website is current"}
        initiative = {"job_id": "job-1", "capabilities": ["task.next_actions"]}
        with patch("services.gateway.server._post_json", side_effect=post):
            VoiceOSServer._run_task_initiative(gateway, task, initiative, "pixel")  # type: ignore[arg-type]

        task_updates = [payload for url, payload in calls if url.endswith("/internal/v1/tasks/actions")]
        self.assertEqual(2, len(task_updates))
        self.assertEqual("task-1", task_updates[0]["task_id"])
        self.assertEqual("progress.record", task_updates[0]["action"])
        self.assertEqual("blocker.create", task_updates[1]["action"])
        self.assertNotIn("voiceos-task-update", gateway.audit_store.turns[0]["response_text"])

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
