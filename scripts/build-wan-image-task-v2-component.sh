#!/usr/bin/env bash
# Build the local task-v2 candidate from the same source as its native suite.
# Output: components/task-wan-image-v2/target/wasm32-wasip2/release/task_wan_image_v2.wasm
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/task-wan-image-v2/Cargo.toml \
  --target wasm32-wasip2 \
  --release
