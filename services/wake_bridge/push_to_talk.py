"""Hyprland push-to-talk controller for the VIC desktop microphone."""

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import time
from pathlib import Path
from typing import Sequence

from .listener import (
    GatewayClient,
    SAMPLE_RATE,
    show_command_center,
    speak,
    transcribe_wav,
    usable_transcript,
)


def _paths(state_dir: Path) -> tuple[Path, Path]:
    state_dir.mkdir(parents=True, exist_ok=True)
    return state_dir / "push-to-talk.raw", state_dir / "push-to-talk.json"


def _read_pid(metadata_path: Path) -> int | None:
    try:
        value = json.loads(metadata_path.read_text(encoding="utf-8"))
        return int(value["pid"])
    except (FileNotFoundError, KeyError, TypeError, ValueError, json.JSONDecodeError):
        return None


def _stop_recorder(pid: int) -> None:
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    for _ in range(30):
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return
        time.sleep(0.05)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


def start(args: argparse.Namespace) -> None:
    audio_path, metadata_path = _paths(args.state_dir)
    existing = _read_pid(metadata_path)
    if existing is not None:
        try:
            os.kill(existing, 0)
            return
        except ProcessLookupError:
            metadata_path.unlink(missing_ok=True)
    audio_path.unlink(missing_ok=True)
    output = audio_path.open("wb")
    command = [
        "pw-record", "--raw", "--rate", str(SAMPLE_RATE),
        "--channels", "1", "--format", "s16", "-",
    ]
    if args.source:
        command[1:1] = ["--target", args.source]
    recorder = subprocess.Popen(
        command,
        stdout=output,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    output.close()
    metadata_path.write_text(
        json.dumps({"pid": recorder.pid, "started_at": time.time()}),
        encoding="utf-8",
    )
    show_command_center()
    GatewayClient(args.gateway_url, args.session_id).change_floor(
        "claim", "listening", partial_transcript="Push-to-talk listening"
    )


def stop(args: argparse.Namespace) -> None:
    audio_path, metadata_path = _paths(args.state_dir)
    pid = _read_pid(metadata_path)
    if pid is None:
        return
    _stop_recorder(pid)
    metadata_path.unlink(missing_ok=True)
    gateway = GatewayClient(args.gateway_url, args.session_id)
    try:
        audio = audio_path.read_bytes()
        if len(audio) < SAMPLE_RATE // 4:
            return
        transcript = transcribe_wav(audio, args.whisper_model)
        if not usable_transcript(transcript):
            return
        gateway.change_floor("update", "processing", partial_transcript=transcript)
        reply = gateway.submit_text(transcript)
        gateway.change_floor(
            "update", "speaking", partial_transcript=transcript, response_text=reply
        )
        speak(reply)
    finally:
        audio_path.unlink(missing_ok=True)
        gateway.change_floor("release", "idle")


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Hold-to-talk controller for VIC")
    parser.add_argument("action", choices=("start", "stop"))
    parser.add_argument("--gateway-url", default="http://127.0.0.1:8787")
    parser.add_argument("--session-id", default="wake:omarchy-desktop")
    parser.add_argument(
        "--state-dir",
        type=Path,
        default=Path.home() / ".local/state/voiceos",
    )
    parser.add_argument(
        "--whisper-model",
        type=Path,
        default=Path.home() / ".local/share/voiceos/models/ggml-base.en.bin",
    )
    parser.add_argument("--source", default=None)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> None:
    args = parse_args(argv)
    start(args) if args.action == "start" else stop(args)


if __name__ == "__main__":
    main()
