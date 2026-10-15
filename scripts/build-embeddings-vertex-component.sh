#!/usr/bin/env bash
# Build the embeddings-vertex component from the same source as its native suite.
# Output: components/embeddings-vertex/target/wasm32-wasip2/release/embeddings_vertex.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" embeddings-vertex
