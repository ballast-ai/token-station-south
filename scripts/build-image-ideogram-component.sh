#!/usr/bin/env bash
# Build the image-ideogram component from the same source as its native suite.
# Output: components/image-ideogram/target/wasm32-wasip2/release/image_ideogram.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" image-ideogram
