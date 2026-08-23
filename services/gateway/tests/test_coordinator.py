from __future__ import annotations

import unittest

from services.gateway.coordinator import TurnCoordinator
from services.gateway.providers import ProviderResponse, ProviderToolCall
from services.gateway.tools import ToolBroker


class ToolThenAnswerRouter:
    def __init__(self) -> None:
        self.calls = 0

    def respond(self, text, provider=None, tools=None, context=None, conversation_id=None):
        del text, provider, context, conversation_id
        self.calls += 1
        if tools:
            return ProviderResponse(
                text="I will inspect the evidence.",
                provider="codex-sol",
                tool_calls=[ProviderToolCall("disk_space", {})],
            )
        return ProviderResponse(
            text="You have ample free disk space, so deleting files is unnecessary.",
            provider="codex-sol",
        )


class CoordinatorToolSynthesisTest(unittest.TestCase):
    def test_completed_read_only_tool_is_returned_to_provider_for_final_answer(self) -> None:
        router = ToolThenAnswerRouter()
        coordinator = TurnCoordinator(router=router, tool_broker=ToolBroker())
        response = coordinator.respond("Do I need to delete anything?")
        self.assertEqual(2, router.calls)
        self.assertIn("deleting files is unnecessary", response.text)
        self.assertEqual("codex-sol+tools", response.provider)
        self.assertTrue(response.results)


if __name__ == "__main__":
    unittest.main()
