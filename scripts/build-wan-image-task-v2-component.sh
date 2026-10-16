#!/usr/bin/env bash
# Build the local task-v2 candidate from the same source as its native suite.
# Output: components/task-wan-image-v2/target/wasm32-wasip2/release/task_wan_image_v2.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" task-wan-image-v2
