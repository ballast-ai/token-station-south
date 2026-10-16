#!/usr/bin/env bash
# Builds the official Gemini component to a wasm32-wasip2
# component.
#
#   scripts/build-anthropic-component.sh
#
# Output: components/provider-gemini/target/wasm32-wasip2/release/
#         provider_gemini.wasm
#
# Requires the target: `rustup target add wasm32-wasip2`.
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" provider-gemini
