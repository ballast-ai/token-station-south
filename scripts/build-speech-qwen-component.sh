#!/usr/bin/env bash
# Build the speech-qwen component from the same source as its native suite.
# Output: components/speech-qwen/target/wasm32-wasip2/release/speech_qwen.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" speech-qwen
