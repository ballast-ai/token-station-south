#!/usr/bin/env bash
# Build the image-minimax component from the same source as its native suite.
# Output: components/image-minimax/target/wasm32-wasip2/release/image_minimax.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" image-minimax
