#!/usr/bin/env bash
# Builds the official Kling task component to a wasm32-wasip2 component.
#
#   scripts/build-kling-task-component.sh
#
# Output: components/task-kling/target/wasm32-wasip2/release/task_kling.wasm
#
# Requires the target: `rustup target add wasm32-wasip2`.
#
# Note: this builds the guest. Nothing yet *instantiates* it — the runtime is
# bound to `provider-adapter-v2` alone, so a second world needs its own slice
# (and its own design record) before gate ② can judge a task component inside
# the sandbox.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/task-kling/Cargo.toml \
  --target wasm32-wasip2 \
  --release
