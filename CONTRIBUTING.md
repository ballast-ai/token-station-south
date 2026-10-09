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

## Usage judges for released provider packages

Gate ② proves a component matches its own fixtures; it cannot prove the fixtures read a provider's
usage correctly, because a third-party author cannot vouch for a provider's documentation on South's
behalf. So every provider package South itself publishes also has a usage judge
(`crates/south-component-conformance/tests/usage_ir_contract_v1.rs`) whose expectations are **not
derived from the reference implementation**: they come from the provider's documentation where it
exists, otherwise from captured upstream traffic archived with the fixtures, and for a package
declaring `usage_evidence: absent` from the property that it never reports usage. A new provider
package, or a change to how one reads usage, ships with its judge cases, and each new case is shown
to fail on the code it guards before the change lands (B1,
`docs/design/2026-09-30-host-zero-vendor-boundary.md` §6.2 item 5).


## A package declares the oldest runtime it needs

A package's `compatibility.south_runtime` is the oldest south runtime that admits and correctly runs it, not the
release that carries it (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §13.6, host feedback SF10). A host
links one runtime and refuses a package that declares a newer one, so a package stamped with every release would be
refused by every host that has not yet re-pinned, even when nothing in it needs the new runtime.

- **When it moves.** Raise a package's `south_runtime` only when the package itself starts relying on something a
  release introduced: a manifest field or value an older runtime's gate ① refuses or reads differently, or a
  runtime-side meaning its descriptors depend on. Raise it to that release, and bump the package's version with it.
- **When it stays.** A release does not re-stamp packages. A package whose `component.wasm` and `manifest.json` are
  unchanged keeps its version, its digests and its `south_runtime`; the release's digest-stability check fails a
  package that changed either without a version bump.
- **How it is checked.** `scripts/check-declared-runtime.sh` loads each package under exactly the runtime it
  declares, in a checkout of that runtime's release tag: gate ①, the range handshake, the import scan and the
  identity probe. Release CI runs it on the published archives, and CI runs it on `release/*` pull requests. Run it
  locally before a release with `scripts/check-declared-runtime.sh --build` after the component build scripts.

## The crates every component links carry their own versions

Every component links `south-contracts`, `south-provider-api` and `south-component-conformance`, and a crate's
version enters the bytes of every component that links it. These three crates therefore declare a `version` of their
own instead of `version.workspace = true` (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §13.12, §16 Q47), so
a release that leaves them alone, such as one that only ships catalog data, keeps every package's `component.wasm`.

- **A release bump** moves `[workspace.package] version`, `compatibility.json`'s `release.version`, and the version
  requirements on the workspace-versioned crates (`south-core`, `south-testkit`, `south-provider-conformance`,
  `south-north-codec`, `south-provider-runtime`). It does not touch these three crates' versions or the requirements
  on them, and no component lockfile changes.
- **A change to one of the three crates** bumps that crate's version in the same change (patch for a compatible
  change, minor otherwise), updates the requirements on it and the component lockfiles, and, because it changes every
  `component.wasm`, bumps every package's version; the release's digest-stability check fails a package that kept its
  version.
- **How the crate bump is checked.** `scripts/check_crate_versions.py` compares the three crate directories with the
  previous release tag and fails when any file in one of them changed (tests and fixtures included) while its version
  did not, naming the crate and the files. Release CI runs it on the tag and CI runs it on `release/*` pull requests;
  run `python3 scripts/check_crate_versions.py --unreleased` locally after the release bump.
- **A component links no other workspace crate.** `shipped_packages_v1` fails when a component lockfile names a
  workspace crate outside these three, or records a version the crate does not declare.
- **The runtime release is the workspace version.** Tests read it from the workspace `Cargo.toml`, never from
  `CARGO_PKG_VERSION` in one of these crates, and the release index refuses a gate ② report naming another release.
