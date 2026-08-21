#!/usr/bin/env bash
# Builds the official Anthropic Messages component to a wasm32-wasip2
# component.
#
#   scripts/build-anthropic-component.sh
#
# Output: components/provider-anthropic/target/wasm32-wasip2/release/
#         provider_anthropic.wasm
#
# Requires the target: `rustup target add wasm32-wasip2`.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/provider-anthropic/Cargo.toml \
  --target wasm32-wasip2 \
  --release
