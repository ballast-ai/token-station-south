#!/usr/bin/env bash
# Build the embeddings-openai-compatible component from the same source as its native suite.
# Output: components/embeddings-openai-compatible/target/wasm32-wasip2/release/embeddings_openai_compatible.wasm
set -euo pipefail
# `scripts/prebuild-components.sh` (the nextest setup script) has already built every component.
if [ -n "${SOUTH_COMPONENTS_PREBUILT:-}" ]; then
  exit 0
fi
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/embeddings-openai-compatible/Cargo.toml \
  --target wasm32-wasip2 \
  --release
