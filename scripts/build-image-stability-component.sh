#!/usr/bin/env bash
# Build the image-stability component from the same source as its native suite.
# Output: components/image-stability/target/wasm32-wasip2/release/image_stability.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" image-stability
