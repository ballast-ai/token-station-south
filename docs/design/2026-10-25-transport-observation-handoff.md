# Transport observation branch handoff

The owner requested a new branch for another maintainer to take over. This is a handoff commit, not a release or a claim that the complete verification matrix is green.

## Contents

- Explicit buffered transport stages and opaque request IDs; no payload, credential or socket-address emission.
- Connection history bounded to 256 entries and one minute, with monotonic IDs; historical observation is not proof of physical reuse.
- A checksum-pinned optional reqwest 0.13.5/South local-write patch bundle, strict preparation tooling, standalone host type-identity check and usage instructions. Default South still uses registry reqwest.
- Partial/vectored/Pending/error/cancellation tests, source-identical parser properties and a scheduled fuzz target. Upstream application receipt and queue timing remain unknown.

## Verification completed

- Stock transport tests, optional patched transport tests, HTTP parser/I/O tests, preparation-tool tests and independent host constructor/type-identity executable.
- Workspace formatting, all-target/all-feature Clippy, doctests, no-default checks, no-default component-conformance tests, fuzz compile, rustdoc with warnings denied, MSRV, dependency boundaries, Python tooling tests and language self-test.
- Optional prepared workspace formatting and Clippy; every optional transport test binary ran with a fourteen-second hard runtime bound. HTTP filtered tests use the same bound. These are local short checks, not a long-running fuzz campaign.

## Verification still required

The complete all-feature nextest run is not green. One attempt selected Homebrew Rust while the WASI target was installed in rustup. A corrected-toolchain attempt ran 886/2422 tests (883 passed, 3 synthetic-speech fixture failures, 4 skipped), then stopped on missing guest artifact paths: a global CARGO_TARGET_DIR redirected guest output away from paths the tests read. A rerun with the cache override removed was in progress and was stopped when the owner requested handoff. No product failure was hidden or counted as passed.

Resume with the pinned toolchain and repository target layout:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTUP_TOOLCHAIN=1.96.0
rustup target add --toolchain 1.96.0 wasm32-wasip2
unset CARGO_TARGET_DIR
PROPTEST_CASES=32 cargo nextest run --workspace --all-features --no-fail-fast
```

Then finish all remaining README verification, especially root/fuzz cargo deny and cargo audit, and the commit-range language gate. Do not infer those passed from the successful transport checks. The scheduled fuzz campaign was added but not executed locally.

No model API calls, credential files, football images or host gateway changes are part of this task. The production gateway was not deployed or switched. Package versions and release tags were not advanced. See the [bundle instructions](../../patches/reqwest-0.13.5-local-write/README.md) and [design](2026-10-25-transport-observation.md).
