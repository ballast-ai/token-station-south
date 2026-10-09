#!/usr/bin/env bash
# Builds every official component, and the guests the runtime tests load, once before the test run.
#
#   scripts/prebuild-components.sh
#
# Each sandbox parity and runtime test needs its component's or guest's wasm. `cargo nextest` runs every test in its own
# process, so a per-process build ran the same release build once per test, and the tests of one
# binary raced for the same package's build lock. This script is the nextest setup script (see
# `.config/nextest.toml`): it builds each package a single time, then exports
# SOUTH_COMPONENTS_PREBUILT=1 to the tests through $NEXTEST_ENV, which makes the per-package
# `scripts/build-*-component.sh` and the tests that build a guest themselves return immediately.
# Outside nextest the variable is unset and they build as before.
#
# The name deliberately does not match `build-*-component.sh`: the shipped-packages test counts
# those scripts against the official component list and the release workflow.
set -euo pipefail
cd "$(dirname "$0")/.."
guests=crates/south-provider-runtime/tests/guests
jobs=()
for script in scripts/build-*-component.sh; do
  jobs+=("bash ${script}")
done
# The guests the runtime tests load, built with the exact commands those tests used to run.
# `test-provider` is built in both profiles by `package_set_v1`, in one job so the two builds
# do not queue on one another's target directory lock.
jobs+=("cd ${guests}/test-provider && cargo build --target wasm32-wasip2 && cargo build --release --target wasm32-wasip2")
jobs+=("cd ${guests}/test-embeddings && cargo build --target wasm32-wasip2")
jobs+=("cd ${guests}/t21-unseen-eventstream && cargo build --target wasm32-wasip2")
# Bounded parallelism: every build already uses all cores, so more than a few at once only
# thrashes the disk. The variable is cleared for the children in case the caller exported it.
printf '%s\n' "${jobs[@]}" |
  xargs -I{} -P "${SOUTH_PREBUILD_JOBS:-4}" env -u SOUTH_COMPONENTS_PREBUILT bash -c {}
if [ -n "${NEXTEST_ENV:-}" ]; then
  echo "SOUTH_COMPONENTS_PREBUILT=1" >> "${NEXTEST_ENV}"
fi
