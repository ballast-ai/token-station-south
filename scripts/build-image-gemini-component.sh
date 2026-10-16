#!/usr/bin/env bash
# Build the image-gemini component from the same source as its native suite.
# Output: components/image-gemini/target/wasm32-wasip2/release/image_gemini.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" image-gemini
