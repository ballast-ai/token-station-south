#!/usr/bin/env bash
# Build the image-ideogram component from the same source as its native suite.
# Output: components/image-ideogram/target/wasm32-wasip2/release/image_ideogram.wasm
set -euo pipefail
# `scripts/prebuild-components.sh` (the nextest setup script) has already built every component.
if [ -n "${SOUTH_COMPONENTS_PREBUILT:-}" ]; then
  exit 0
fi
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/image-ideogram/Cargo.toml \
  --target wasm32-wasip2 \
  --release
