from __future__ import annotations

import unittest
from unittest.mock import patch

from services.gateway.connector_signals import calendar_signals, email_signals


class ConnectorSignalTests(unittest.TestCase):
    def test_missing_credentials_are_healthy_and_empty(self) -> None:
        with patch("services.gateway.connector_signals._secret", return_value=None):
            self.assertEqual("credentials_required", email_signals()["status"])
            self.assertEqual([], calendar_signals()["signals"])

    def test_email_metadata_is_normalized_without_body_content(self) -> None:
        def google(path: str, _environment: str):
            if "messages?" in path:
                return {"messages": [{"id": "mail-1"}]}
            return {
                "snippet": "Please review",
                "labelIds": ["UNREAD", "IMPORTANT"],
                "payload": {"headers": [
                    {"name": "Subject", "value": "Quarterly review"},
                    {"name": "From", "value": "person@example.test"},
                ]},
            }
        with patch("services.gateway.connector_signals._google", side_effect=google):
            signal = email_signals()["signals"][0]
        self.assertEqual("Quarterly review", signal["title"])
        self.assertTrue(signal["needs_you"])
        self.assertTrue(signal["requires_interpretation"])


if __name__ == "__main__":
    unittest.main()
