#!/usr/bin/env bash
# Build the speech-xai component from the same source as its native suite.
# Output: components/speech-xai/target/wasm32-wasip2/release/speech_xai.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" speech-xai
