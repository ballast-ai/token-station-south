#!/usr/bin/env bash
# Build the speech-azure component from the same source as its native suite.
# Output: components/speech-azure/target/wasm32-wasip2/release/speech_azure.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" speech-azure
