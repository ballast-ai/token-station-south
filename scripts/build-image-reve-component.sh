#!/usr/bin/env bash
# Build the image-reve component from the same source as its native suite.
# Output: components/image-reve/target/wasm32-wasip2/release/image_reve.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" image-reve
