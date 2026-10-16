#!/usr/bin/env bash
# Build the speech-minimax component from the same source as its native suite.
# Output: components/speech-minimax/target/wasm32-wasip2/release/speech_minimax.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" speech-minimax
