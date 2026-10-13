#!/usr/bin/env bash
# Builds the official Kiro provider component to a wasm32-wasip2 component.
#
#   scripts/build-kiro-component.sh
#
# Output: components/provider-kiro/target/wasm32-wasip2/release/provider_kiro.wasm
#
# Requires the target: `rustup target add wasm32-wasip2`.
set -euo pipefail
# `scripts/prebuild-components.sh` (the nextest setup script) has already built every component.
if [ -n "${SOUTH_COMPONENTS_PREBUILT:-}" ]; then
  exit 0
fi
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/provider-kiro/Cargo.toml \
  --target wasm32-wasip2 \
  --release
