#!/usr/bin/env bash
# Build the local task-v2 candidate from the same source as its native suite.
# Output: components/task-bailian-v2/target/wasm32-wasip2/release/task_bailian_v2.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" task-bailian-v2
