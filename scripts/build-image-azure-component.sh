#!/usr/bin/env bash
# Build the image-azure component from the same source as its native suite.
# Output: components/image-azure/target/wasm32-wasip2/release/image_azure.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" image-azure
