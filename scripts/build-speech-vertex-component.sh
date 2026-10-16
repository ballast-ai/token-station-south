#!/usr/bin/env bash
# Build the speech-vertex component from the same source as its native suite.
# Output: components/speech-vertex/target/wasm32-wasip2/release/speech_vertex.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" speech-vertex
