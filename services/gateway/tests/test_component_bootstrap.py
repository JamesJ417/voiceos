from __future__ import annotations

import json
import tempfile
import threading
import unittest
from http.client import HTTPConnection
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from services.gateway.audit import AuditStore
from services.gateway.server import create_server


REPOSITORY = Path(__file__).resolve().parents[3]


class ComponentRegistryHandler(BaseHTTPRequestHandler):
    registry = json.loads(
        (REPOSITORY / "contracts" / "component-registry.json").read_text(
            encoding="utf-8"
        )
    )

    def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
        if self.path != "/v1/client/bootstrap":
            self.send_error(404)
            return
        payload = json.dumps(
            {
                "contract_version": 1,
                "device_id": "development-device",
                "authentication": {"scheme": "bearer"},
                "component_registry": self.registry,
                "endpoints": {
                    "bootstrap": "/v1/client/bootstrap",
                    "conversation": "/v1/conversations/active",
                    "conversation_events": "/v1/conversations/active/events",
                    "turn": "/v1/turns/text",
                },
                "transport": {
                    "private_network_required": True,
                    "tls_required": True,
                },
            },
            separators=(",", ":"),
        ).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_: object) -> None:
        return


class ComponentBootstrapIntegrationTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.temporary_directory = tempfile.TemporaryDirectory()
        cls.rust = ThreadingHTTPServer(("127.0.0.1", 0), ComponentRegistryHandler)
        cls.rust_thread = threading.Thread(target=cls.rust.serve_forever, daemon=True)
        cls.rust_thread.start()
        cls.audit = AuditStore(
            Path(cls.temporary_directory.name) / "component-bootstrap.sqlite3"
        )
        cls.gateway = create_server(
            "127.0.0.1",
            0,
            audit_store=cls.audit,
            memory_url=f"http://127.0.0.1:{cls.rust.server_port}",
            # This fixture intentionally exercises the local loopback development path.
            require_device_auth=False,
        )
        cls.gateway_thread = threading.Thread(
            target=cls.gateway.serve_forever, daemon=True
        )
        cls.gateway_thread.start()

    @classmethod
    def tearDownClass(cls) -> None:
        cls.gateway.shutdown()
        cls.gateway.server_close()
        cls.gateway_thread.join(timeout=2)
        cls.rust.shutdown()
        cls.rust.server_close()
        cls.rust_thread.join(timeout=2)
        cls.audit.close()
        cls.temporary_directory.cleanup()

    def test_touch_discovers_voiceos_vic_and_console_through_python_ingress(self) -> None:
        connection = HTTPConnection(
            "127.0.0.1", self.gateway.server_port, timeout=2
        )
        connection.request("GET", "/v1/client/bootstrap")
        response = connection.getresponse()
        payload = json.loads(response.read())
        connection.close()
        self.assertEqual(200, response.status)
        self.assertEqual(
            {
                "backend_control_plane": "voiceos",
                "voice_interface_controller": "vic",
                "touchscreen_system_interface": "touch",
            },
            payload["component_registry"]["roles"],
        )
        components = {
            component["id"]: component
            for component in payload["component_registry"]["components"]
        }
        self.assertEqual("production", components["vic-console"]["lifecycle"])
        self.assertEqual(
            "owner_only_unix_socket_v1",
            components["vic-console"]["integration"]["transport"],
        )
        self.assertEqual(
            ["show_weather", "refresh_dashboard"],
            components["vic-console"]["integration"]["commands"],
        )


if __name__ == "__main__":
    unittest.main()
