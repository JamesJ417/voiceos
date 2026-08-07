from __future__ import annotations

import ast
import json
import re
import unittest
from pathlib import Path

from services.gateway.proxy_routes import PROXY_ROUTES


REPOSITORY = Path(__file__).resolve().parents[2]
OPENAPI = REPOSITORY / "contracts" / "openapi.yaml"
OWNERSHIP = REPOSITORY / "contracts" / "route-ownership.json"
PYTHON_GATEWAY = REPOSITORY / "services" / "gateway" / "server.py"
RUST_ROUTES = REPOSITORY / "services" / "voiceos-gateway-rs" / "src" / "api" / "mod.rs"
RUST_TASKS = REPOSITORY / "services" / "voiceos-gateway-rs" / "src" / "api" / "tasks.rs"
RUST_ARTIFACTS = REPOSITORY / "services" / "voiceos-gateway-rs" / "src" / "api" / "artifacts.rs"
HTTP_METHODS = {"get", "post", "put", "patch", "delete"}


def openapi_operations() -> dict[tuple[str, str], str]:
    operations: dict[tuple[str, str], str] = {}
    current_path: str | None = None
    current_operation: tuple[str, str] | None = None
    in_paths = False
    for line in OPENAPI.read_text(encoding="utf-8").splitlines():
        if line == "paths:":
            in_paths = True
            continue
        if in_paths and line == "components:":
            break
        if not in_paths:
            continue
        path_match = re.fullmatch(r"  (/[^:]+):", line)
        if path_match:
            current_path = path_match.group(1)
            current_operation = None
            continue
        method_match = re.fullmatch(r"    (get|post|put|patch|delete):", line)
        if method_match and current_path:
            current_operation = (method_match.group(1).upper(), current_path)
            operations[current_operation] = ""
            continue
        operation_match = re.fullmatch(r"      operationId: ([A-Za-z][A-Za-z0-9]*)", line)
        if operation_match and current_operation:
            operations[current_operation] = operation_match.group(1)
    return operations


def ownership_operations() -> dict[tuple[str, str], dict[str, object]]:
    payload = json.loads(OWNERSHIP.read_text(encoding="utf-8"))
    return {
        (str(route["method"]).upper(), str(route["path"])): route
        for route in payload["routes"]
    }


def handler_route_literals(method: str) -> set[str]:
    tree = ast.parse(PYTHON_GATEWAY.read_text(encoding="utf-8"))
    function_name = f"do_{method}"
    handler = next(
        node
        for node in ast.walk(tree)
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
        and node.name == function_name
    )
    route_conditions = [node.test for node in ast.walk(handler) if isinstance(node, ast.If)]
    native = {
        node.value
        for condition in route_conditions
        for node in ast.walk(condition)
        if isinstance(node, ast.Constant) and isinstance(node.value, str)
    }
    proxied = {route.template for route in PROXY_ROUTES if route.method == method}
    return native | proxied


def route_is_covered(path: str, literals: set[str]) -> bool:
    if path in literals:
        return True
    parameter = re.search(r"\{[^}]+\}", path)
    if not parameter:
        return False
    prefix = path[: parameter.start()]
    suffix = path[parameter.end() :]
    return prefix in literals and (not suffix or suffix in literals)


class OpenApiContractTests(unittest.TestCase):
    def test_openapi_and_ownership_inventory_have_identical_operations(self) -> None:
        documented = openapi_operations()
        owned = ownership_operations()
        self.assertEqual(set(documented), set(owned))
        self.assertTrue(all(documented.values()), "Every operation needs a stable operationId")
        operation_ids = list(documented.values())
        self.assertEqual(len(operation_ids), len(set(operation_ids)))

    def test_every_operation_has_one_target_owner(self) -> None:
        for key, route in ownership_operations().items():
            with self.subTest(operation=key):
                self.assertEqual(route.get("target_owner"), "rust-control-plane")
                self.assertTrue(route.get("current_implementation"))

    def test_python_ingress_covers_every_public_operation(self) -> None:
        by_method = {
            method: handler_route_literals(method)
            for method in {operation[0] for operation in openapi_operations()}
        }
        for method, path in openapi_operations():
            with self.subTest(method=method, path=path):
                self.assertTrue(
                    route_is_covered(path, by_method[method]),
                    f"Python ingress does not implement or proxy {method} {path}",
                )

    def test_python_ingress_has_no_undocumented_static_routes(self) -> None:
        documented_paths = {path for _, path in openapi_operations()}
        for method in {operation[0] for operation in openapi_operations()}:
            for literal in handler_route_literals(method):
                if not literal.startswith("/v1/"):
                    continue
                if literal.endswith("/"):
                    self.assertTrue(
                        any(path.startswith(literal) for path in documented_paths),
                        f"Undocumented dynamic route prefix: {method} {literal}",
                    )
                else:
                    self.assertIn(literal, documented_paths)

    def test_rust_proxy_routes_exist_in_the_rust_router(self) -> None:
        rust_source = RUST_ROUTES.read_text(encoding="utf-8")
        for (method, path), route in ownership_operations().items():
            if route["current_implementation"] != "python-proxy-rust":
                continue
            with self.subTest(method=method, path=path):
                self.assertIn(path, rust_source)

    def test_schema_references_resolve(self) -> None:
        source = OPENAPI.read_text(encoding="utf-8")
        schema_section = source.split("  schemas:\n", 1)[1]
        definitions = set(re.findall(r"^    ([A-Za-z][A-Za-z0-9]*):$", schema_section, re.MULTILINE))
        references = set(
            re.findall(r'\$ref: "#/components/schemas/([A-Za-z][A-Za-z0-9]*)"', source)
        )
        self.assertEqual(references - definitions, set())

    def test_device_security_is_defined_with_public_enrollment_exceptions(self) -> None:
        source = OPENAPI.read_text(encoding="utf-8")
        self.assertIn("security:\n  - deviceBearer: []", source)
        self.assertIn("deviceBearer:\n      type: http\n      scheme: bearer", source)
        for operation_id in ["getHealth", "createEnrollment", "exchangeEnrollment"]:
            block = source.split(f"operationId: {operation_id}", 1)[1].split("responses:", 1)[0]
            self.assertIn("security: []", block)

    def test_task_attachment_uses_the_managed_artifact_tool_only(self) -> None:
        source = OPENAPI.read_text(encoding="utf-8")
        task_action = source.split("    TaskAction:\n", 1)[1].split("    VicOutreach:\n", 1)[0]
        self.assertNotIn("artifact.attach", task_action)
        self.assertNotIn("uri:", task_action)
        self.assertNotIn('"artifact.attach" =>', RUST_TASKS.read_text(encoding="utf-8"))
        self.assertIn('"artifact.attach" =>', RUST_ARTIFACTS.read_text(encoding="utf-8"))

    def test_ontology_contract_is_versioned_and_has_explicit_validator_outcomes(self) -> None:
        source = OPENAPI.read_text(encoding="utf-8")
        catalog = source.split("    OntologyCatalog:\n", 1)[1].split("    Artifact:\n", 1)[0]
        decision = source.split("    InterpretationDecision:\n", 1)[1]
        self.assertIn("minimum_compatible_version", catalog)
        for entity_kind in ["artifact", "task", "person", "project", "skill", "email", "location"]:
            self.assertIn(entity_kind, catalog)
        for disposition in ["execute", "ask_for_confirmation", "ask_clarifying_question", "reject"]:
            self.assertIn(disposition, decision)

    def test_automation_contract_exposes_controls_and_off_switch(self) -> None:
        source = OPENAPI.read_text(encoding="utf-8")
        rule = source.split("    AutomationRule:\n", 1)[1].split("    CreateAutomationRule:\n", 1)[0]
        for field in [
            "owner_id",
            "enabled",
            "trigger",
            "conditions",
            "permitted_actions",
            "frequency_limit",
            "evidence",
        ]:
            self.assertIn(field, rule)
        self.assertIn("/v1/automations/{automation_id}/enabled:", source)

    def test_attention_and_planning_contracts_keep_external_actions_approval_controlled(self) -> None:
        source = OPENAPI.read_text(encoding="utf-8")
        inbox = source.split("    AttentionItem:\n", 1)[1].split("    UpsertAttentionItem:\n", 1)[0]
        for category in ["email", "calendar", "question", "approval", "document", "system", "message", "agent_work"]:
            self.assertIn(category, inbox)
        action = source.split("    AttentionAction:\n", 1)[1].split("    CalendarEventInput:\n", 1)[0]
        self.assertIn("request_send_approval", action)
        self.assertIn("request_invitation_approval", action)
        self.assertIn("preparation_minutes", source)
        self.assertIn("travel_minutes", source)
        self.assertIn("recurrence_rule", source)
        self.assertIn("unscheduled_task_ids", source)

    def test_update_and_control_contracts_are_proposal_first(self) -> None:
        source = OPENAPI.read_text(encoding="utf-8")
        update = source.split("    UpdateProposal:\n", 1)[1].split("    AttentionItem:\n", 1)[0]
        for field in ["current_version", "proposed_version", "skill_changes", "security_changes", "affected_components", "rollback_version", "evidence"]:
            self.assertIn(field, update)
        self.assertIn("Single-use root-broker approval card", source)
        self.assertIn("never executes directly", source)
        self.assertIn("/v1/activity:", source)
        self.assertIn("/v1/admin/status:", source)


if __name__ == "__main__":
    unittest.main()
