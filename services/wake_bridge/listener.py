"""Always-on local wake-word bridge for Omarchy Voice."""

from __future__ import annotations

import argparse
from collections import deque
import hashlib
import json
import logging
import signal
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request
import uuid
import wave
import threading
from pathlib import Path
from typing import Sequence
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

import numpy as np

SAMPLE_RATE = 16_000
FRAME_SAMPLES = 1_280
MODEL_URL = "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2"
MODEL_DIRNAME = "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01"
MODEL_SHA256 = "f170013b4716e41b62b9bfd809687c207cef798ef9bc6534d524e17af9b6561a"
LOG = logging.getLogger(__name__)
QUIET_FRAMES_TO_END = 8  # about 0.64 seconds
IGNORED_TRANSCRIPTS = {
    "[blank_audio]",
    "[blank audio]",
    "(dramatic music)",
    "(music)",
    "[music]",
}
CACHED_SPEECH = {
    "ack": "Yes, I'm here.",
    "progress": "One moment. I'm working on that.",
    "goodbye": "Okay. Say Hey VIC when you need me.",
    "error": "I hit a delay. Please try that again.",
}


class GatewayClient:
    def __init__(self, base_url: str, session_id: str | None = None) -> None:
        self.base_url = base_url.rstrip("/")
        self.session_id = session_id or f"wake:{uuid.uuid4()}"

    def submit_text(self, text: str) -> str:
        payload = json.dumps({"session_id": self.session_id, "text": text}).encode("utf-8")
        request = Request(
            f"{self.base_url}/v1/turns/text",
            data=payload,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urlopen(request, timeout=180) as response:
                result = json.loads(response.read(128 * 1024))
        except (HTTPError, URLError, TimeoutError, json.JSONDecodeError) as error:
            raise RuntimeError(f"gateway_turn_failed: {error}") from error
        if not isinstance(result, dict) or not isinstance(result.get("response_text"), str):
            raise RuntimeError("gateway_turn_invalid_response")
        returned_session = result.get("session_id")
        if isinstance(returned_session, str) and returned_session.strip():
            self.session_id = returned_session.strip()
        return result["response_text"].strip()


def ensure_model(root: Path) -> Path:
    target = root / MODEL_DIRNAME
    if (target / "tokens.txt").is_file():
        return target
    root.mkdir(parents=True, exist_ok=True)
    archive = root / f"{MODEL_DIRNAME}.tar.bz2"
    LOG.info("Downloading the Sherpa wake-word model once (~13 MB)")
    urllib.request.urlretrieve(MODEL_URL, archive)  # noqa: S310 - fixed project release URL
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if digest != MODEL_SHA256:
        archive.unlink(missing_ok=True)
        raise RuntimeError("wake_model_checksum_mismatch")
    with tarfile.open(archive, "r:bz2") as bundle:
        bundle.extractall(root, filter="data")
    archive.unlink(missing_ok=True)
    if not (target / "tokens.txt").is_file():
        raise RuntimeError(f"invalid_sherpa_model: {target}")
    return target


class SherpaWakeWord:
    def __init__(self, phrase: str, model_dir: Path, sensitivity: float) -> None:
        import sherpa_onnx
        from sherpa_onnx import text2token

        phrase = phrase.strip()
        if not phrase:
            raise ValueError("wake phrase is required")
        token_file = model_dir / "tokens.txt"
        tokens = text2token([phrase.upper()], tokens=str(token_file), tokens_type="bpe", bpe_model=str(model_dir / "bpe.model"))
        keywords = tempfile.NamedTemporaryFile(mode="w", suffix=".txt", prefix="voiceos-kws-", delete=False, encoding="utf-8")
        self._keywords_path = Path(keywords.name)
        self._display = phrase.upper().replace(" ", "_")
        keywords.write(" ".join(tokens[0]) + f" @{self._display}\n")
        keywords.close()

        def model(pattern: str) -> str:
            found = sorted(model_dir.glob(pattern))
            if not found:
                raise RuntimeError(f"missing_sherpa_model_file: {pattern}")
            return str(found[0])

        threshold = 0.05 + 0.4 * min(1.0, max(0.0, sensitivity))
        self._spotter = sherpa_onnx.KeywordSpotter(
            tokens=str(token_file), encoder=model("encoder-*[!8].onnx"), decoder=model("decoder-*[!8].onnx"),
            joiner=model("joiner-*[!8].onnx"), keywords_file=str(self._keywords_path),
            keywords_threshold=threshold, num_threads=1,
        )
        self._stream = self._spotter.create_stream()

    def process(self, frame: bytes) -> bool:
        samples = np.frombuffer(frame, dtype=np.int16).astype(np.float32) / 32768.0
        self._stream.accept_waveform(SAMPLE_RATE, samples)
        while self._spotter.is_ready(self._stream):
            self._spotter.decode_stream(self._stream)
            if self._spotter.get_result(self._stream):
                self._spotter.reset_stream(self._stream)
                return True
        return False

    def reset(self) -> None:
        self._stream = self._spotter.create_stream()

    def close(self) -> None:
        self._keywords_path.unlink(missing_ok=True)


def capture_process(source: str | None) -> subprocess.Popen[bytes]:
    command = ["pw-record", "--raw", "--rate", str(SAMPLE_RATE), "--channels", "1", "--format", "s16", "-"]
    if source:
        command[1:1] = ["--target", source]
    return subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)


def capture_utterance(stream, initial: bytes, maximum_seconds: float = 12.0) -> bytes:
    frames = [initial]
    started = time.monotonic()
    speech_started = False
    quiet_frames = 0
    while time.monotonic() - started < maximum_seconds:
        frame = stream.read(FRAME_SAMPLES * 2)
        if len(frame) != FRAME_SAMPLES * 2:
            break
        frames.append(frame)
        rms = float(np.sqrt(np.mean(np.frombuffer(frame, dtype=np.int16).astype(np.float32) ** 2)))
        if rms > 420:
            speech_started = True
            quiet_frames = 0
        elif speech_started:
            quiet_frames += 1
            if quiet_frames >= QUIET_FRAMES_TO_END:
                break
    return b"".join(frames)


def capture_followup(
    stream, wait_seconds: float, maximum_seconds: float = 12.0, stop_requested=lambda: False
) -> bytes | None:
    """Wait for follow-up speech, retaining a short pre-roll and stopping on silence."""
    deadline = time.monotonic() + wait_seconds
    pre_roll: deque[bytes] = deque(maxlen=4)
    frames: list[bytes] = []
    speech_started = False
    quiet_frames = 0
    speech_started_at = 0.0
    while not speech_started or time.monotonic() - speech_started_at < maximum_seconds:
        if stop_requested():
            return None
        if not speech_started and time.monotonic() >= deadline:
            return None
        frame = stream.read(FRAME_SAMPLES * 2)
        if len(frame) != FRAME_SAMPLES * 2:
            raise RuntimeError("microphone_stream_ended")
        rms = float(np.sqrt(np.mean(np.frombuffer(frame, dtype=np.int16).astype(np.float32) ** 2)))
        if not speech_started:
            pre_roll.append(frame)
            if rms > 420:
                speech_started = True
                speech_started_at = time.monotonic()
                frames.extend(pre_roll)
            continue
        frames.append(frame)
        if rms > 420:
            quiet_frames = 0
        else:
            quiet_frames += 1
            if quiet_frames >= QUIET_FRAMES_TO_END:
                break
    return b"".join(frames)


def transcribe_wav(audio: bytes, model: Path) -> str:
    with tempfile.TemporaryDirectory(prefix="voiceos-wake-") as temporary:
        wav_path = Path(temporary) / "turn.wav"
        with wave.open(str(wav_path), "wb") as output:
            output.setnchannels(1)
            output.setsampwidth(2)
            output.setframerate(SAMPLE_RATE)
            output.writeframes(audio)
        result = subprocess.run(
            ["whisper-cli", "--model", str(model), "--file", str(wav_path), "--language", "en", "--no-timestamps", "--no-prints", "--threads", "4"],
            capture_output=True, text=True, timeout=120, check=False,
        )
    if result.returncode:
        raise RuntimeError("local_transcription_failed")
    return " ".join(result.stdout.split()).strip()


def command_after_wake_phrase(transcript: str, phrase: str) -> str:
    """Remove a transcribed wake phrase while leaving ordinary commands intact."""
    words = transcript.strip().split()
    phrase_words = phrase.strip().split()
    if len(words) >= len(phrase_words):
        heard = [word.strip(".,!?;:").casefold() for word in words[: len(phrase_words)]]
        expected = [word.casefold() for word in phrase_words]
        if heard == expected:
            words = words[len(phrase_words) :]
    return " ".join(words).strip()


def ends_conversation(transcript: str) -> bool:
    normalized = " ".join(
        word.strip(".,!?;:").casefold() for word in transcript.strip().split()
    )
    return normalized in {
        "goodbye",
        "bye vic",
        "goodbye vic",
        "stop listening",
        "stop listening vic",
        "that's all",
        "that is all",
    }


def usable_transcript(transcript: str) -> bool:
    normalized = " ".join(transcript.strip().casefold().split())
    return bool(normalized) and normalized not in IGNORED_TRANSCRIPTS


def stop_recorder(recorder: subprocess.Popen[bytes]) -> None:
    recorder.terminate()
    try:
        recorder.wait(timeout=3)
    except subprocess.TimeoutExpired:
        recorder.kill()


def generate_speech(text: str, audio_path: Path) -> None:
    audio_path.parent.mkdir(parents=True, exist_ok=True)
    generated = subprocess.run(
        [sys.executable, "-m", "edge_tts", "--voice", "en-US-AvaMultilingualNeural",
         "--text", text[:4_000], "--write-media", str(audio_path)],
        capture_output=True, text=True, timeout=120, check=False,
    )
    if generated.returncode or not audio_path.is_file():
        raise RuntimeError("tts_generation_failed")


def play_audio(audio_path: Path) -> None:
    played = subprocess.run(["pw-play", str(audio_path)], timeout=180, check=False)
    if played.returncode:
        raise RuntimeError("tts_playback_failed")


def ensure_speech_cache(cache_dir: Path) -> None:
    for name, text in CACHED_SPEECH.items():
        path = cache_dir / f"{name}.mp3"
        if not path.is_file():
            generate_speech(text, path)


def speak_cached(cache_dir: Path, name: str) -> None:
    play_audio(cache_dir / f"{name}.mp3")


def speak(text: str) -> None:
    if not text:
        return
    with tempfile.TemporaryDirectory(prefix="voiceos-tts-") as temporary:
        audio_path = Path(temporary) / "reply.mp3"
        generate_speech(text, audio_path)
        play_audio(audio_path)


def submit_with_progress(gateway: GatewayClient, transcript: str, cache_dir: Path) -> str:
    finished = threading.Event()

    def announce_delay() -> None:
        if not finished.wait(3.0):
            speak_cached(cache_dir, "progress")

    announcer = threading.Thread(target=announce_delay, daemon=True)
    announcer.start()
    started = time.monotonic()
    try:
        return gateway.submit_text(transcript)
    finally:
        finished.set()
        announcer.join(timeout=10)
        LOG.info("VIC response completed in %.2f seconds", time.monotonic() - started)


def run(args: argparse.Namespace) -> None:
    wake = SherpaWakeWord(args.phrase, ensure_model(args.model_root), args.sensitivity)
    gateway = GatewayClient(args.gateway_url, args.session_id)
    ensure_speech_cache(args.speech_cache)
    stopping = False

    def stop(*_: object) -> None:
        nonlocal stopping
        stopping = True

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    LOG.info("Listening locally for %r on %s", args.phrase, args.source or "the default microphone")
    try:
        while not stopping:
            transcript = ""
            recorder = capture_process(args.source)
            assert recorder.stdout is not None
            try:
                while not stopping:
                    frame = recorder.stdout.read(FRAME_SAMPLES * 2)
                    if len(frame) != FRAME_SAMPLES * 2:
                        if stopping:
                            break
                        raise RuntimeError("microphone_stream_ended")
                    if wake.process(frame):
                        LOG.info("Wake word detected")
                        utterance = capture_utterance(recorder.stdout, frame)
                        transcript = command_after_wake_phrase(
                            transcribe_wav(utterance, args.whisper_model), args.phrase
                        )
                        if not usable_transcript(transcript):
                            transcript = ""
                        wake.reset()
                        break
            finally:
                stop_recorder(recorder)

            if stopping:
                break
            if not transcript:
                speak_cached(args.speech_cache, "ack")
            while not stopping:
                if transcript:
                    if ends_conversation(transcript):
                        speak_cached(args.speech_cache, "goodbye")
                        LOG.info("Conversation ended by voice command")
                        break
                    LOG.info("Submitting conversation turn")
                    try:
                        speak(submit_with_progress(gateway, transcript, args.speech_cache))
                    except RuntimeError as error:
                        LOG.error("Voice turn failed: %s", error)
                        speak_cached(args.speech_cache, "error")
                        break

                recorder = capture_process(args.source)
                assert recorder.stdout is not None
                try:
                    audio = capture_followup(
                        recorder.stdout, args.conversation_timeout, stop_requested=lambda: stopping
                    )
                finally:
                    stop_recorder(recorder)
                if audio is None:
                    LOG.info("Conversation timed out; returning to wake-word mode")
                    break
                transcript = transcribe_wav(audio, args.whisper_model)
                if not usable_transcript(transcript):
                    LOG.info("Ignored empty or background-audio transcript: %r", transcript)
                    break
    finally:
        wake.close()


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="On-device Hey Vic wake-word listener")
    parser.add_argument("--phrase", default="hey vic")
    parser.add_argument("--gateway-url", default="http://127.0.0.1:8787")
    parser.add_argument("--session-id", default="wake:omarchy-desktop")
    parser.add_argument("--whisper-model", type=Path, default=Path.home() / ".local/share/voiceos/models/ggml-base.en.bin")
    parser.add_argument("--model-root", type=Path, default=Path.home() / ".local/share/voiceos/wakewords")
    parser.add_argument("--speech-cache", type=Path, default=Path.home() / ".local/share/voiceos/audio")
    parser.add_argument("--source", default=None, help="PipeWire source node name or serial; defaults to the current source")
    parser.add_argument("--sensitivity", type=float, default=0.5)
    parser.add_argument("--conversation-timeout", type=float, default=20.0,
                        help="Seconds to wait for each follow-up before requiring Hey VIC again")
    return parser.parse_args(argv)


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    run(parse_args())
