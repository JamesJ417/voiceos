"""Load the configured VoiceOS model into Ollama during host startup."""

from __future__ import annotations

import json
import os
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


def main() -> None:
    base_url = os.environ.get("VOICEOS_OLLAMA_URL", "http://127.0.0.1:11434").rstrip("/")
    model = os.environ.get("VOICEOS_OLLAMA_MODEL", "").strip()
    if not model:
        raise SystemExit("VOICEOS_OLLAMA_MODEL is not configured")

    payload = json.dumps(
        {
            "model": model,
            "messages": [{"role": "user", "content": "Reply with READY."}],
            "stream": False,
            "think": False,
            "keep_alive": -1,
        }
    ).encode("utf-8")
    deadline = time.monotonic() + 150
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        request = Request(
            f"{base_url}/api/chat",
            data=payload,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urlopen(request, timeout=120) as response:
                result = json.loads(response.read())
            if not isinstance(result.get("message"), dict):
                raise RuntimeError("Ollama returned no assistant message")
            print(f"VoiceOS model ready: {model}")
            return
        except (HTTPError, URLError, TimeoutError, OSError, json.JSONDecodeError, RuntimeError) as error:
            last_error = error
            time.sleep(2)
    raise SystemExit(f"Could not warm VoiceOS model {model}: {last_error}")


if __name__ == "__main__":
    main()
