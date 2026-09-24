#!/usr/bin/env bash
# Builds the official AWS Bedrock Converse component to a wasm32-wasip2
# component.
#
#   scripts/build-bedrock-converse-component.sh
#
# Output: components/provider-bedrock-converse/target/wasm32-wasip2/release/
#         provider_bedrock_converse.wasm
#
# Requires the target: `rustup target add wasm32-wasip2`.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/provider-bedrock-converse/Cargo.toml \
  --target wasm32-wasip2 \
  --release
