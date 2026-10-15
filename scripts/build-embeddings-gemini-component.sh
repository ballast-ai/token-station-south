#!/usr/bin/env bash
# Build the embeddings-gemini component from the same source as its native suite.
# Output: components/embeddings-gemini/target/wasm32-wasip2/release/embeddings_gemini.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" embeddings-gemini
