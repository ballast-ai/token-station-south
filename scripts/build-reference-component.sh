#!/usr/bin/env bash
# Builds the official reference component to a wasm32-wasip2 component.
#
#   scripts/build-reference-component.sh
#
# Output: components/provider-openai-compatible/target/wasm32-wasip2/release/
#         provider_openai_compatible.wasm
#
# Requires the target: `rustup target add wasm32-wasip2`.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/provider-openai-compatible/Cargo.toml \
  --target wasm32-wasip2 \
  --release
