#!/usr/bin/env bash
# Builds the official OpenAI Responses component to a wasm32-wasip2 component.
#
#   scripts/build-openai-responses-component.sh
#
# Output: components/provider-openai-responses/target/wasm32-wasip2/release/
#         provider_openai_responses.wasm
#
# Requires the target: `rustup target add wasm32-wasip2`.
set -euo pipefail
# `scripts/prebuild-components.sh` (the nextest setup script) has already built every component.
if [ -n "${SOUTH_COMPONENTS_PREBUILT:-}" ]; then
  exit 0
fi
cd "$(dirname "$0")/.."
cargo build \
  --manifest-path components/provider-openai-responses/Cargo.toml \
  --target wasm32-wasip2 \
  --release
