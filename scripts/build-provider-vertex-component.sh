#!/usr/bin/env bash
# Build the provider-vertex component from the same source as its native suite.
# Output: components/provider-vertex/target/wasm32-wasip2/release/provider_vertex.wasm
set -euo pipefail
exec bash "$(dirname "$0")/component-build.sh" provider-vertex
