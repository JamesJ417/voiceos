"""Load the versioned Omarchy Voice master charter shared by every provider."""

from __future__ import annotations

import os
from functools import lru_cache
from pathlib import Path


@lru_cache(maxsize=1)
def master_system_prompt() -> str:
    configured = os.environ.get("VOICEOS_MASTER_PROMPT_PATH", "").strip()
    path = (
        Path(configured)
        if configured
        else Path(__file__).resolve().parents[2] / "contracts" / "master-system-prompt.md"
    )
    prompt = path.read_text(encoding="utf-8").strip()
    if not prompt:
        raise RuntimeError(f"Omarchy Voice master prompt is empty: {path}")
    return prompt
