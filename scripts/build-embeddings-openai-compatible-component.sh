#!/usr/bin/env bash
# Build the embeddings-openai-compatible component from the same source as its native suite.
# Output: components/embeddings-openai-compatible/target/wasm32-wasip2/release/embeddings_openai_compatible.wasm
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/embeddings-openai-compatible/Cargo.toml \
  --target wasm32-wasip2 \
  --release
