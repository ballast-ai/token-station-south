#!/usr/bin/env bash
# Build the image-vertex component from the same source as its native suite.
# Output: components/image-vertex/target/wasm32-wasip2/release/image_vertex.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" image-vertex
