#!/usr/bin/env bash
# Builds every official component once, before the test run.
#
#   scripts/prebuild-components.sh
#
# Each sandbox parity test needs its component's wasm. `cargo nextest` runs every test in its own
# process, so a per-process build ran the same release build once per test, and the tests of one
# binary raced for the same package's build lock. This script is the nextest setup script (see
# `.config/nextest.toml`): it builds each package a single time, then exports
# SOUTH_COMPONENTS_PREBUILT=1 to the tests through $NEXTEST_ENV, which makes the per-package
# `scripts/build-*-component.sh` return immediately. Outside nextest the variable is unset and
# those scripts build as before.
#
# The name deliberately does not match `build-*-component.sh`: the shipped-packages test counts
# those scripts against the official component list and the release workflow.
set -euo pipefail
cd "$(dirname "$0")/.."
# Bounded parallelism: every build already uses all cores, so more than a few at once only
# thrashes the disk. The variable is cleared for the children in case the caller exported it.
printf '%s\n' scripts/build-*-component.sh |
  xargs -n 1 -P "${SOUTH_PREBUILD_JOBS:-4}" env -u SOUTH_COMPONENTS_PREBUILT bash
if [ -n "${NEXTEST_ENV:-}" ]; then
  echo "SOUTH_COMPONENTS_PREBUILT=1" >> "${NEXTEST_ENV}"
fi
