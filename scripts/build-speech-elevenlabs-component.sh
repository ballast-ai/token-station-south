#!/usr/bin/env bash
# Build the speech-elevenlabs component from the same source as its native suite.
# Output: components/speech-elevenlabs/target/wasm32-wasip2/release/speech_elevenlabs.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" speech-elevenlabs
