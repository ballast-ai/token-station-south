#!/usr/bin/env bash
# Runs one fuzz target for a fixed time and reports every finding (stream parser first-error
# record 2026-11-04, F4).
#
#   bash scripts/fuzz-target.sh <target> <seconds>
#
# libFuzzer stops at its first crash by default, so a weekly run reported one finding and could hide
# others. Fork mode with `-ignore_crashes=1` keeps fuzzing after a crash for the whole time and
# writes each finding to `fuzz/artifacts/<target>/`. The step fails when any finding was written,
# and prints each one as base64 so it can be reproduced from the log alone. A failure without a
# finding (a build error, a missing tool) keeps its own exit status.
#
# The seeds in `fuzz/seeds/<target>/`, when present, are copied into the corpus first. The
# nightly toolchain and the explicit target are those of the scheduled job: the cargo-fuzz binary
# install-action fetches is built for musl, and the address sanitizer cannot link against a
# statically linked libc, so the runner's own target is named.
set -euo pipefail

toolchain=nightly-2026-08-15
target_triple=x86_64-unknown-linux-gnu

target="${1:-}"
seconds="${2:-}"
case "${target}" in
  safe_headers | contract_parsers | local_write_parser) ;;
  *)
    echo "fuzz-target.sh: unknown fuzz target '${target}'" >&2
    exit 2
    ;;
esac
if ! [[ "${seconds}" =~ ^[0-9]+$ ]]; then
  echo "fuzz-target.sh: the time must be a whole number of seconds, not '${seconds}'" >&2
  exit 2
fi

corpus="fuzz/corpus/${target}"
artifacts="fuzz/artifacts/${target}"
mkdir -p "${corpus}" "${artifacts}"
if [ -d "fuzz/seeds/${target}" ]; then
  cp fuzz/seeds/"${target}"/* "${corpus}/"
fi

status=0
rustup run "${toolchain}" cargo fuzz run --target "${target_triple}" "${target}" -- \
  -fork=1 -ignore_crashes=1 -max_total_time="${seconds}" || status=$?

findings=()
while IFS= read -r -d '' finding; do
  findings+=("${finding}")
done < <(find "${artifacts}" -maxdepth 1 -type f \
  \( -name 'crash-*' -o -name 'leak-*' -o -name 'timeout-*' -o -name 'oom-*' \) -print0 | sort -z)

if [ "${#findings[@]}" -gt 0 ]; then
  echo "::error::${target}: ${#findings[@]} finding(s)"
  echo "${target}: ${#findings[@]} finding(s)"
  for finding in "${findings[@]}"; do
    echo "--- ${finding} ($(wc -c < "${finding}" | tr -d ' ') bytes), base64:"
    base64 < "${finding}" | tr -d '\n'
    echo
  done
  exit 1
fi
exit "${status}"
