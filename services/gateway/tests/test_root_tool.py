from __future__ import annotations

import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from services.gateway.tools import ToolBroker


class RootToolTest(unittest.TestCase):
    def test_root_tool_is_not_advertised_by_default(self) -> None:
        with patch.dict(os.environ, {}, clear=True):
            broker = ToolBroker()
        self.assertNotIn("rig.root_command", {item["name"] for item in broker.describe()})

    def test_root_tool_requires_approval_and_exact_absolute_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory, patch.dict(
            os.environ, {"VOICEOS_ROOT_BROKER_ENABLED": "1"}, clear=False
        ):
            broker = ToolBroker(Path(directory))
            arguments = {
                "argv": [str(Path(sys.executable).resolve())],
                "cwd": str(Path(directory).resolve()),
                "timeout_seconds": 10,
                "rollback": "No state change; no rollback required.",
            }
            proposed = broker.execute("rig.root_command", arguments)
            self.assertEqual("approval_required", proposed.status)
            self.assertTrue(proposed.approval_required)

            denied = broker.execute(
                "rig.root_command", {"argv": ["id"], "cwd": "/", "rollback": "none"}
            )
            self.assertEqual("denied", denied.status)
            self.assertEqual("absolute_executable_required", denied.error)

    def test_rust_backed_task_tools_are_typed_and_allowlisted(self) -> None:
        seen: list[dict[str, object]] = []
        broker = ToolBroker()
        broker.register_task_tools(lambda arguments: seen.append(arguments) or {"detail": {"progress": {"lane": "vic_working"}}})

        names = {schema["function"]["name"] for schema in broker.model_schemas()}
        self.assertIn("task_step_create", names)
        result = broker.execute(
            "task_step_create",
            {"task_id": "task-1", "title": "Prepare layout", "owner": "vic"},
        )
        self.assertEqual("completed", result.status)
        self.assertEqual("task.step.create", seen[0]["tool"])
        denied = broker.execute(
            "task_step_create",
            {"task_id": "task-1", "title": "Prepare layout", "owner": "root"},
        )
        self.assertEqual("denied", denied.status)

    def test_rust_backed_artifact_tools_are_typed_and_allowlisted(self) -> None:
        seen: list[dict[str, object]] = []
        broker = ToolBroker()
        broker.register_artifact_tools(lambda payload: seen.append(payload) or {"artifact": {"title": "Recipe cards"}})
        names = {schema["function"]["name"] for schema in broker.model_schemas()}
        self.assertIn("artifact_pdf_create", names)
        result = broker.execute("artifact_pdf_create", {"title": "Recipe cards", "template": "recipe-card"})
        self.assertEqual("completed", result.status)
        self.assertEqual("artifact.pdf.create", seen[0]["tool"])
        denied = broker.execute("artifact_pdf_create", {"title": "Recipe cards", "template": "shell-script"})
        self.assertEqual("denied", denied.status)

    def test_structured_tool_execution_requires_an_executable_ontology_decision(self) -> None:
        invoked: list[dict[str, object]] = []
        broker = ToolBroker(
            ontology_validator=lambda _tool, _arguments: {
                "catalog_version": 2,
                "disposition": "ask_clarifying_question",
                "reason": "missing_task_reference",
                "issues": [],
            }
        )
        broker.register_artifact_tools(
            lambda payload: invoked.append(payload) or {"artifact": {"title": "Recipe cards"}}
        )

        denied = broker.execute("artifact_pdf_create", {"title": "Recipe cards"})

        self.assertEqual("denied", denied.status)
        self.assertEqual("ontology_ask_clarifying_question", denied.error)
        self.assertEqual([], invoked)
        self.assertEqual(2, denied.ontology_decision["catalog_version"])

    def test_ontology_confirmation_is_bound_to_the_existing_approval_path(self) -> None:
        invoked: list[dict[str, object]] = []
        broker = ToolBroker(
            ontology_validator=lambda _tool, _arguments: {
                "catalog_version": 2,
                "disposition": "ask_for_confirmation",
                "reason": "intent_requires_confirmation",
                "issues": [],
            }
        )
        broker.register_artifact_tools(
            lambda payload: invoked.append(payload) or {"artifact": {"title": "Recipe cards"}}
        )

        proposed = broker.execute("artifact_pdf_create", {"title": "Recipe cards"})
        self.assertEqual("approval_required", proposed.status)
        self.assertEqual([], invoked)

        completed = broker.execute(
            "artifact_pdf_create", {"title": "Recipe cards"}, approved=True
        )

        self.assertEqual("completed", completed.status)
        self.assertEqual(1, len(invoked))


if __name__ == "__main__":
    unittest.main()
