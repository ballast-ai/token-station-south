#!/usr/bin/env bash
# Builds the official Bedrock InvokeModel Anthropic component to a wasm32-wasip2
# component.
#
#   scripts/build-anthropic-bedrock-invoke-component.sh
#
# Output: components/provider-anthropic-bedrock-invoke/target/wasm32-wasip2/release/
#         provider_anthropic_bedrock_invoke.wasm
#
# Requires the target: `rustup target add wasm32-wasip2`.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/provider-anthropic-bedrock-invoke/Cargo.toml \
  --target wasm32-wasip2 \
  --release
