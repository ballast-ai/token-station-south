#!/usr/bin/env bash
# Build the image-openai-compatible component from the same source as its native suite.
# Output: components/image-openai-compatible/target/wasm32-wasip2/release/image_openai_compatible.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" image-openai-compatible
