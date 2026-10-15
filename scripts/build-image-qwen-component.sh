#!/usr/bin/env bash
# Build the image-qwen component from the same source as its native suite.
# Output: components/image-qwen/target/wasm32-wasip2/release/image_qwen.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" image-qwen
