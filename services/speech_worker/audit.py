from __future__ import annotations

import json
import threading
import time
from pathlib import Path


class LifecycleAudit:
    def __init__(self, path: str) -> None:
        self.path = Path(path)
        self._lock = threading.Lock()

    def record(self, event: str, *, session_id: str, detail: dict[str, object] | None = None) -> None:
        entry = {
            "timestamp_unix": int(time.time()),
            "event": event,
            "session_id": session_id,
            "detail": detail or {},
        }
        encoded = json.dumps(entry, separators=(",", ":"), sort_keys=True) + "\n"
        with self._lock:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            with self.path.open("a", encoding="utf-8") as stream:
                stream.write(encoded)
