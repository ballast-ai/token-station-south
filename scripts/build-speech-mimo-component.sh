#!/usr/bin/env bash
# Build the speech-mimo component from the same source as its native suite.
# Output: components/speech-mimo/target/wasm32-wasip2/release/speech_mimo.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" speech-mimo
