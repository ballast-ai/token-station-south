#!/usr/bin/env bash
# Build the local task-v2 candidate from the same source as its native suite.
# Output: components/task-gmi-image-v2/target/wasm32-wasip2/release/task_gmi_image_v2.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" task-gmi-image-v2
