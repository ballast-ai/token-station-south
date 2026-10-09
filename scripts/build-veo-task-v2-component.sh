#!/usr/bin/env bash
# Build the local task-v2 candidate from the same source as its native suite.
# Output: components/task-veo-v2/target/wasm32-wasip2/release/task_veo_v2.wasm
set -euo pipefail
# `scripts/prebuild-components.sh` (the nextest setup script) has already built every component.
if [ -n "${SOUTH_COMPONENTS_PREBUILT:-}" ]; then
  exit 0
fi
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/task-veo-v2/Cargo.toml \
  --target wasm32-wasip2 \
  --release
