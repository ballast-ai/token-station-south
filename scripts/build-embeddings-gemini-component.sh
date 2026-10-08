#!/usr/bin/env bash
# Build the embeddings-gemini component from the same source as its native suite.
# Output: components/embeddings-gemini/target/wasm32-wasip2/release/embeddings_gemini.wasm
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/embeddings-gemini/Cargo.toml \
  --target wasm32-wasip2 \
  --release
