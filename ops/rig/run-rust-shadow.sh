#!/usr/bin/env bash
set -euo pipefail

export VOICEOS_CODEX_SOCKET=/run/voiceos-codex/codex.sock
export VOICEOS_OLLAMA_URL=http://127.0.0.1:11434
export VOICEOS_RUST_DATA_DIR=/home/llm/.local/share/voiceos-rust-shadow
export VOICEOS_LEGACY_AUDIT_PATH=/var/lib/voiceos/audit.sqlite3
export VOICEOS_GEMMA_MODEL=gemma4-fast:12b
export VOICEOS_GPT_OSS_MODEL=gpt-oss:20b
export VOICEOS_ONTOLOGY_MODEL_FALLBACK=1
export VOICEOS_REQUIRE_DEVICE_AUTH=0
export VOICEOS_ONTOLOGY_MODEL=gemma
export VOICEOS_CODEX_ENABLED=1
export VOICEOS_RUST_LISTEN=127.0.0.1:8790

exec /home/llm/voiceos-rust-shadow/target/release/voiceos-gateway
