#!/usr/bin/env bash
# Build the speech-groq component from the same source as its native suite.
# Output: components/speech-groq/target/wasm32-wasip2/release/speech_groq.wasm
set -euo pipefail
# `scripts/prebuild-components.sh` (the nextest setup script) has already built every component.
if [ -n "${SOUTH_COMPONENTS_PREBUILT:-}" ]; then
  exit 0
fi
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/speech-groq/Cargo.toml \
  --target wasm32-wasip2 \
  --release
