import unittest

from services.gateway.proxy_routes import ProxyTransport, match_proxy_route


class ProxyRouteTests(unittest.TestCase):
    def test_matches_exact_and_parameterized_routes(self) -> None:
        self.assertIsNotNone(match_proxy_route("GET", "/v1/tasks"))
        self.assertIsNotNone(match_proxy_route("GET", "/v1/tasks/task-123"))
        self.assertIsNotNone(
            match_proxy_route("POST", "/v1/doctrine/candidates/candidate-1/decision")
        )

    def test_rejects_wrong_methods_extra_segments_and_native_routes(self) -> None:
        self.assertIsNone(match_proxy_route("POST", "/v1/tasks/task-123"))
        self.assertIsNone(match_proxy_route("GET", "/v1/tasks/task-123/status"))
        self.assertIsNone(match_proxy_route("GET", "/v1/health"))
        self.assertIsNone(match_proxy_route("POST", "/v1/turns/text"))

    def test_selects_stream_and_binary_transports(self) -> None:
        self.assertEqual(
            match_proxy_route("GET", "/v1/artifacts/events").transport,
            ProxyTransport.SSE,
        )
        self.assertEqual(
            match_proxy_route("GET", "/v1/artifacts/a1/preview").transport,
            ProxyTransport.BINARY,
        )


if __name__ == "__main__":
    unittest.main()
