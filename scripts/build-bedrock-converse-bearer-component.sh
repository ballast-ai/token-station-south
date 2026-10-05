#!/usr/bin/env bash
# Builds the official AWS Bedrock Converse component for Bedrock API keys (the
# Bearer sibling of provider-bedrock-converse) to a wasm32-wasip2 component.
#
#   scripts/build-bedrock-converse-bearer-component.sh
#
# Output: components/provider-bedrock-converse-bearer/target/wasm32-wasip2/release/
#         provider_bedrock_converse_bearer.wasm
#
# Requires the target: `rustup target add wasm32-wasip2`.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/provider-bedrock-converse-bearer/Cargo.toml \
  --target wasm32-wasip2 \
  --release
