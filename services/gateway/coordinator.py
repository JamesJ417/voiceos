"""Coordinates deterministic tools and the configured reasoning provider."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from .providers import ProviderRouter, ProviderUnavailable
from .system_health import summarize_system_health
from .tools import ToolBroker, ToolResult


@dataclass(frozen=True)
class CoordinatedResponse:
    text: str
    provider: str
    tool_calls: list[dict[str, Any]] = field(default_factory=list)
    approvals: list[dict[str, Any]] = field(default_factory=list)
    results: list[dict[str, Any]] = field(default_factory=list)
    errors: list[dict[str, Any]] = field(default_factory=list)
    evidence: dict[str, object] | None = None
    input_tokens: int | None = None
    output_tokens: int | None = None
    cost_usd: float | None = None


class TurnCoordinator:
    def __init__(
        self,
        router: ProviderRouter | None = None,
        tool_broker: ToolBroker | None = None,
        project_root: Path | None = None,
    ) -> None:
        self.router = router or ProviderRouter()
        self.tools = tool_broker or ToolBroker(project_root)

    def respond(
        self,
        text: str,
        document_context: str | None = None,
        conversation_id: str | None = None,
        provider: str | None = None,
        allowed_tools: set[str] | None = None,
    ) -> CoordinatedResponse:
        tool_request = self._deterministic_tool_request(text)
        if tool_request is not None:
            name, arguments = tool_request
            if allowed_tools is not None and name not in allowed_tools:
                return CoordinatedResponse(
                    text=f"The {name} tool is outside this task's proactive capability scope.",
                    provider="policy",
                    errors=[{"type": "capability_scope_denied", "tool": name}],
                )
            outcome = self.tools.execute(name, arguments)
            return self._tool_response(outcome)

        model_schemas = self.tools.model_schemas()
        if allowed_tools is not None:
            model_names = {name.replace(".", "_") for name in allowed_tools}
            model_schemas = [
                schema
                for schema in model_schemas
                if isinstance(schema.get("function"), dict)
                and schema["function"].get("name") in model_names
            ]
        try:
            provider_response = self.router.respond(
                text,
                provider=provider,
                tools=model_schemas,
                context=document_context,
                conversation_id=conversation_id,
            )
        except ProviderUnavailable as error:
            return CoordinatedResponse(
                text=f"The selected model provider is unavailable. {error}",
                provider="unavailable",
                errors=[{"type": "provider_unavailable", "message": str(error)}],
            )
        if provider_response.tool_calls:
            allowed_model_names = (
                None
                if allowed_tools is None
                else allowed_tools | {name.replace(".", "_") for name in allowed_tools}
            )
            denied = [
                call.name
                for call in provider_response.tool_calls
                if allowed_model_names is not None and call.name not in allowed_model_names
            ]
            if denied:
                return CoordinatedResponse(
                    text="VIC proposed work outside this task's proactive capability scope. Nothing ran.",
                    provider="policy",
                    errors=[
                        {"type": "capability_scope_denied", "tool": name}
                        for name in denied
                    ],
                )
            responses = [
                self._tool_response(self.tools.execute(call.name, call.arguments))
                for call in provider_response.tool_calls
            ]
            approvals = [approval for response in responses for approval in response.approvals]
            errors = [error for response in responses for error in response.errors]
            results = [result for response in responses for result in response.results]
            response_text = " ".join(response.text for response in responses)
            if results and not approvals and not errors:
                try:
                    synthesis = self.router.respond(
                        (
                            "Answer the user's original request using the verified tool results below. "
                            "Explain what the evidence means and directly address any requested recommendation. "
                            "Do not request another tool and do not merely repeat a generic status summary.\n\n"
                            f"Original request: {text}\n"
                            f"Verified tool results: {json.dumps(results, separators=(',', ':'))}"
                        ),
                        provider=provider_response.provider,
                        tools=[],
                        context=document_context,
                        conversation_id=conversation_id,
                    )
                    if synthesis.text.strip():
                        response_text = synthesis.text.strip()
                except ProviderUnavailable:
                    pass
            return CoordinatedResponse(
                text=response_text,
                provider=f"{provider_response.provider}+tools",
                tool_calls=[call for response in responses for call in response.tool_calls],
                approvals=approvals,
                results=results,
                errors=errors,
                evidence=responses[0].evidence if len(responses) == 1 else None,
                input_tokens=provider_response.input_tokens,
                output_tokens=provider_response.output_tokens,
                cost_usd=provider_response.cost_usd,
            )
        return CoordinatedResponse(
            text=provider_response.text,
            provider=provider_response.provider,
            approvals=[
                {
                    "request_id": approval.request_id,
                    "tool": approval.tool,
                    "arguments": approval.arguments,
                    "required": True,
                    "status": "pending",
                    "provider": provider_response.provider,
                    "provider_run_id": approval.provider_run_id,
                    "evidence": approval.evidence,
                }
                for approval in provider_response.approvals
            ],
            results=[{"events": provider_response.events}] if provider_response.events else [],
            input_tokens=provider_response.input_tokens,
            output_tokens=provider_response.output_tokens,
            cost_usd=provider_response.cost_usd,
        )

    def complete_provider_approval(
        self, provider: str, run_id: str, approve: bool
    ) -> CoordinatedResponse:
        try:
            response = self.router.complete_provider_approval(provider, run_id, approve)
        except ProviderUnavailable as error:
            return CoordinatedResponse(
                text=f"The provider approval could not be completed. {error}",
                provider="unavailable",
                errors=[{"type": "provider_approval_failed", "message": str(error)}],
            )
        return CoordinatedResponse(
            text=response.text,
            provider=response.provider,
            results=[{"events": response.events}] if response.events else [],
            input_tokens=response.input_tokens,
            output_tokens=response.output_tokens,
            cost_usd=response.cost_usd,
        )

    def complete_approved_tool(
        self, request_id: str, name: str, arguments: dict[str, object]
    ) -> CoordinatedResponse:
        return self._tool_response(
            self.tools.execute(name, arguments, approved=True, request_id=request_id)
        )

    def _tool_response(self, outcome: ToolResult) -> CoordinatedResponse:
        tool_call = {
            "request_id": outcome.request_id,
            "name": outcome.name,
            "arguments": outcome.arguments,
            "status": outcome.status,
        }
        result = outcome.as_dict()
        if outcome.status == "approval_required":
            return CoordinatedResponse(
                text=(
                    f"Running {outcome.name} requires your approval. "
                    "Nothing has been executed."
                ),
                provider="deterministic",
                tool_calls=[tool_call],
                approvals=[
                    {
                        "request_id": outcome.request_id,
                        "tool": outcome.name,
                        "arguments": outcome.arguments,
                        "required": True,
                        "status": "pending",
                    }
                ],
            )
        if outcome.status != "completed":
            return CoordinatedResponse(
                text=f"The {outcome.name} tool could not run: {outcome.error}",
                provider="deterministic",
                tool_calls=[tool_call],
                errors=[{"type": outcome.status, "message": outcome.error}],
            )

        evidence = outcome.result
        if outcome.name == "system.health" and evidence is not None:
            message = summarize_system_health(evidence)
        elif outcome.name == "disk.space" and evidence is not None:
            message = f"The project disk has {evidence['free_percent']} percent space free."
        elif outcome.name == "network.status" and evidence is not None:
            message = (
                f"The gateway host is {evidence['hostname']} and has "
                f"{len(evidence['addresses'])} local network addresses."
            )
        elif outcome.name == "service.status" and evidence is not None:
            state = "running" if evidence["active"] else "not running"
            message = f"The {evidence['service']} service is {state}."
        elif outcome.name == "project.tests" and evidence is not None:
            state = "passed" if evidence["passed"] else "failed"
            message = f"The VoiceOS gateway test suite {state}."
        elif outcome.name.startswith("task.") and evidence is not None:
            detail = evidence.get("detail") if isinstance(evidence, dict) else None
            progress = detail.get("progress") if isinstance(detail, dict) else None
            lane = progress.get("lane") if isinstance(progress, dict) else None
            message = (
                f"I updated the task and recorded the progress. Its current responsibility lane is "
                f"{str(lane).replace('_', ' ') if lane else 'shared'}."
            )
        else:
            message = f"The {outcome.name} tool completed."
        return CoordinatedResponse(
            text=message,
            provider="deterministic",
            tool_calls=[tool_call],
            results=[result],
            evidence=evidence,
        )

    @staticmethod
    def _deterministic_tool_request(text: str) -> tuple[str, dict[str, object]] | None:
        normalized = text.casefold()
        if "health" in normalized or "system status" in normalized:
            return "system.health", {}
        if "disk space" in normalized or "free space" in normalized:
            return "disk.space", {}
        if "network status" in normalized or "network information" in normalized:
            return "network.status", {}
        if "tailscale" in normalized and any(
            word in normalized for word in ("status", "running", "service")
        ):
            return "service.status", {"service": "tailscale"}
        if "run" in normalized and "test" in normalized:
            return "project.tests", {"suite": "gateway"}
        return None
