"""Declarative route matching for the transitional Python-to-Rust proxy."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
import re


class ProxyTransport(StrEnum):
    JSON = "json"
    SSE = "sse"
    BINARY = "binary"


@dataclass(frozen=True)
class ProxyRoute:
    method: str
    template: str
    transport: ProxyTransport = ProxyTransport.JSON

    def matches(self, method: str, path: str) -> bool:
        if method != self.method:
            return False
        expression = re.sub(r"\{[a-zA-Z_][a-zA-Z0-9_]*\}", r"[^/]+", self.template)
        return re.fullmatch(expression, path) is not None


_GET = (
    "/v1/activity",
    "/v1/agents/runs",
    "/v1/agents/runs/{run_id}",
    "/v1/skills",
    "/v1/skills/usages",
    "/v1/skills/proposals",
    "/v1/tasks",
    "/v1/tasks/{task_id}",
    "/v1/memory/sleep/cycles/current",
    "/v1/memory/sleep/cycles/{cycle_id}",
    "/v1/memory/morning-report",
    "/v1/memory/search",
    "/v1/doctrine/status",
    "/v1/doctrine/sources",
    "/v1/doctrine/sources/records",
    "/v1/doctrine/candidates",
    "/v1/doctrine/candidates/{candidate_id}/provenance",
    "/v1/doctrine/active",
    "/v1/doctrine/lenses",
    "/v1/doctrine/contradictions",
    "/v1/attention",
    "/v1/calendar/events",
    "/v1/outreach",
    "/v1/outreach/policy",
    "/v1/automations",
    "/v1/updates",
    "/v1/files",
    "/v1/artifacts",
    "/v1/artifacts/{artifact_id}",
    "/v1/conversations/active",
    "/v1/conversations/active/messages",
    "/v1/conversations/active/floor",
    "/v1/ontology/catalog",
    "/v1/ontology/aliases",
)

_POST = (
    "/v1/conversations/active/floor",
    "/v1/agents/runs",
    "/v1/agents/runs/{run_id}/cancel",
    "/v1/artifacts/pdfs",
    "/v1/artifacts/{artifact_id}/revisions",
    "/v1/skills/usages/{usage_id}/feedback",
    "/v1/tasks",
    "/v1/tasks/{task_id}/status",
    "/v1/tasks/{task_id}/actions",
    "/v1/tasks/{task_id}/schedule",
    "/v1/memory/sleep/cycles",
    "/v1/memory/sleep/cycles/{cycle_id}/actions",
    "/v1/doctrine/sources/records",
    "/v1/doctrine/sources/records/{record_id}/process",
    "/v1/doctrine/sources/records/{record_id}/revoke",
    "/v1/doctrine/candidates/{candidate_id}/decision",
    "/v1/doctrine/candidates/{candidate_id}/status",
    "/v1/doctrine/evaluations",
    "/v1/attention",
    "/v1/attention/{attention_id}/actions",
    "/v1/calendar/events",
    "/v1/plans/daily/work",
    "/v1/outreach",
    "/v1/outreach/policy",
    "/v1/outreach/{outreach_id}/actions",
    "/v1/automations",
    "/v1/automations/{automation_id}/enabled",
    "/v1/updates/{update_id}/decision",
    "/v1/updates/{update_id}/actions",
    "/v1/ontology/interpret",
    "/v1/ontology/aliases",
    "/v1/ontology/interpretations/{interpretation_id}/correct",
)

PROXY_ROUTES = (
    ProxyRoute("GET", "/v1/agents/events", ProxyTransport.SSE),
    ProxyRoute("GET", "/v1/artifacts/events", ProxyTransport.SSE),
    ProxyRoute("GET", "/v1/conversations/active/events", ProxyTransport.SSE),
    ProxyRoute("GET", "/v1/artifacts/{artifact_id}/preview", ProxyTransport.BINARY),
    ProxyRoute("GET", "/v1/artifacts/{artifact_id}/download", ProxyTransport.BINARY),
    *(ProxyRoute("GET", route) for route in _GET),
    *(ProxyRoute("POST", route) for route in _POST),
    ProxyRoute("DELETE", "/v1/files/{document_id}"),
)


def match_proxy_route(method: str, path: str) -> ProxyRoute | None:
    return next((route for route in PROXY_ROUTES if route.matches(method, path)), None)
