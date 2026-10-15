#!/usr/bin/env bash
# Build the speech-groq component from the same source as its native suite.
# Output: components/speech-groq/target/wasm32-wasip2/release/speech_groq.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" speech-groq
