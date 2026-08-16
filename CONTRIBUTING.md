# Contributing

## Language and design

Use English for code, comments, documentation, diagnostics, logs, tests, and commit messages. Any
change to public contracts, state, runtime behavior, transport behavior, or release behavior starts
with an English design record in `docs/design/` and ships with that record.

The applied Rust standards come from
[`GlimpseEngine/rust-coding-standards`](https://github.com/GlimpseEngine/rust-coding-standards)
at commit `1ba098e53a1971d2a3937b90ebb95a8e4928d750`. This repository pins the applicable rules below so
contributors do not need a sibling checkout.

## Required Rust baseline

- Rust edition 2024, MSRV 1.96, exact toolchain 1.96.0.
- `rustfmt` uses stable options only. The upstream standards' `imports_granularity` and
  `group_imports` recommendations are intentionally omitted because they still require nightly,
  while the same standards prohibit nightly production toolchains.
- Production and normal CI never require nightly. The scheduled `cargo-fuzz` tooling job is
  isolated on the exact `nightly-2026-08-15` toolchain because sanitizer instrumentation requires
  nightly compiler flags; this does not change the library's stable MSRV.
- Shared dependencies live in `[workspace.dependencies]`; wildcard versions are forbidden.
- Production code uses typed `thiserror` errors and contains no `unwrap` or `expect`.
- Error messages are English and never contain credentials, personal data, request bodies, or
  response bodies.
- `unsafe` is forbidden at workspace level. A future exception requires a dedicated crate, a
  documented safety contract, `// SAFETY:` comments, and explicit owner review.
- Core libraries do not read environment variables or initialize a global tracing subscriber.
- Async work accepts explicit cancellation and deadlines, does not hold locks across `.await`, does
  not use unbounded channels, and accounts for every spawned task.
- Features are additive and must pass both all-features and no-default-features builds.
- Pure library workspaces do not commit `Cargo.lock`. If this workspace gains a binary, this policy
  must be changed in the same pull request and the lockfile must be committed. The nested fuzz
  binary workspace therefore commits `fuzz/Cargo.lock` and receives separate supply-chain checks.

## Test-driven workflow

1. Add a public behavior test and run it to observe the expected failure.
2. Implement the minimum production code that makes the test pass.
3. Run formatting, Clippy, all feature configurations, doctests, documentation, dependency
   boundaries, license checks, security audit, and unused dependency checks.
4. Keep tests deterministic. Inject time and I/O; do not sleep or mutate process environment in
   tests. Untrusted parsers require property tests and a scheduled fuzz target.

Run the commands listed in the root README before opening a pull request.
