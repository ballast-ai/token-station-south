#!/usr/bin/env bash
# Build the local task-v2 candidate from the same source as its native suite.
# Output: components/task-byteplus-v2/target/wasm32-wasip2/release/task_byteplus_v2.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" task-byteplus-v2
