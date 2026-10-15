#!/usr/bin/env bash
# Build the speech-openai-compatible component from the same source as its native suite.
# Output: components/speech-openai-compatible/target/wasm32-wasip2/release/speech_openai_compatible.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" speech-openai-compatible
