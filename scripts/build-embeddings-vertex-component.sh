#!/usr/bin/env bash
# Build the embeddings-vertex component from the same source as its native suite.
# Output: components/embeddings-vertex/target/wasm32-wasip2/release/embeddings_vertex.wasm
set -euo pipefail
# `scripts/prebuild-components.sh` (the nextest setup script) has already built every component.
if [ -n "${SOUTH_COMPONENTS_PREBUILT:-}" ]; then
  exit 0
fi
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/embeddings-vertex/Cargo.toml \
  --target wasm32-wasip2 \
  --release
