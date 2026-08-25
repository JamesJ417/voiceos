"""Load the versioned VoiceOS master charter shared by every provider."""

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
        raise RuntimeError(f"VoiceOS master prompt is empty: {path}")

    deployment_context_path = os.environ.get(
        "VOICEOS_DEPLOYMENT_CONTEXT_PATH", ""
    ).strip()
    if not deployment_context_path:
        return prompt

    context_path = Path(deployment_context_path)
    deployment_context = context_path.read_text(encoding="utf-8").strip()
    if not deployment_context:
        raise RuntimeError(f"VoiceOS deployment context is empty: {context_path}")
    return f"{prompt}\n\n## Active deployment context\n\n{deployment_context}"
