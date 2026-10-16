#!/usr/bin/env bash
# Builds one official component package, in the target directory every package shares.
#
#   scripts/component-build.sh <package>        # e.g. image-qwen
#
# Output: components/<package>/target/wasm32-wasip2/release/<package with - as _>.wasm, the path
# the tests, `scripts/check-declared-runtime.sh` and the release workflow read.
#
# Every package is its own cargo workspace, so with its own target directory each one compiled the
# dependencies they all share (serde, wit-bindgen, the south-* crates) from scratch: on the full
# set that is about forty release builds of the same crates. All of them now build into one
# target directory, `target/components`, where a dependency compiles once per distinct version and
# feature set, and the finished wasm is copied to the package's own path. Measured cold on eight
# packages: 78-85 s and 1.8 GB apart, 22-23 s and 236 MB shared, with byte-identical wasm, so the
# release's digest-stability check sees the same bytes (record
# docs/design/2026-10-15-ci-build-deduplication.md).
#
# The per-package `scripts/build-*-component.sh` scripts call this; their names are what the
# shipped-packages test counts against the official component list and the release workflow, and
# this script's name deliberately does not match theirs.
set -euo pipefail
# `scripts/prebuild-components.sh` (the nextest setup script) has already built every component.
if [ -n "${SOUTH_COMPONENTS_PREBUILT:-}" ]; then
  exit 0
fi
cd "$(dirname "$0")/.."
package="${1:-}"
if [[ ! "${package}" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]] || [ ! -f "components/${package}/Cargo.toml" ]; then
  echo "component-build.sh: '${package}' is not a package under components/" >&2
  exit 2
fi
cargo build \
  --manifest-path "components/${package}/Cargo.toml" \
  --target wasm32-wasip2 \
  --release \
  --target-dir target/components
artifact="${package//-/_}.wasm"
mkdir -p "components/${package}/target/wasm32-wasip2/release"
cp "target/components/wasm32-wasip2/release/${artifact}" \
  "components/${package}/target/wasm32-wasip2/release/${artifact}"
