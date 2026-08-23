"""Bounded local speech transcription for desktop microphone turns."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile


class TranscriptionUnavailable(RuntimeError):
    pass


WHISPER_THREADS = min(8, max(4, os.cpu_count() or 4))


def transcribe(audio: bytes, content_type: str) -> str:
    model = Path(os.environ.get("VOICEOS_WHISPER_MODEL", "")).expanduser()
    if not model.is_file():
        raise TranscriptionUnavailable("speech_transcription_not_configured")
    suffix = ".webm" if "webm" in content_type else ".ogg" if "ogg" in content_type else ".wav"
    with tempfile.TemporaryDirectory(prefix="voiceos-stt-") as directory:
        source = Path(directory) / f"source{suffix}"
        normalized = Path(directory) / "capture.wav"
        source.write_bytes(audio)
        converted = subprocess.run(
            [
                "/usr/bin/ffmpeg", "-nostdin", "-v", "error", "-y", "-i", str(source),
                "-ar", "16000", "-ac", "1", str(normalized),
            ],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        if converted.returncode != 0:
            raise TranscriptionUnavailable("audio_conversion_failed")
        recognized = subprocess.run(
            [
                "/usr/bin/whisper-cli", "--model", str(model), "--file", str(normalized),
                "--language", "en", "--no-timestamps", "--no-prints", "--threads", str(WHISPER_THREADS),
            ],
            capture_output=True,
            text=True,
            timeout=90,
            check=False,
        )
        if recognized.returncode != 0:
            raise TranscriptionUnavailable("speech_transcription_failed")
        transcript = " ".join(recognized.stdout.split()).strip()
        if not transcript:
            raise TranscriptionUnavailable("no_speech_detected")
        return transcript
