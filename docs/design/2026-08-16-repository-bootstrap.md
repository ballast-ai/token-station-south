# Repository Bootstrap

Status: implemented bootstrap; not production-ready.

Date: 2026-08-16

## Problem and decision

The enterprise host cannot migrate to a shared provider plugin execution path until a stable,
independently consumable southbound foundation exists. Waiting for the enterprise host to become a
consumer before creating that foundation is circular.

Token Station South is therefore a public Apache-2.0 repository with dependency inversion. It owns
provider-facing contracts, provider APIs, provider runtime, conformance, transports, and migration
test tooling. `token-station` and `token-station-server` consume South; South never depends on either
host.

All code, documentation, diagnostics, tests, and commits in this repository are English. Chinese
cross-repository decisions remain in the `token-station` repository.

## Bootstrap goals

1. Establish an independently buildable Rust workspace and the approved crate ownership map.
2. Encode the Glimpse Rust standards as executable toolchain, lint, test, documentation, license,
   advisory, fuzz, and dependency-boundary gates.
3. Provide one test-driven security contract: ordinary provider headers cannot set the versioned
   host-reserved authentication headers.
4. Publish machine-readable compatibility status without claiming a stable provider ABI or
   production host compatibility.

## Non-goals

This bootstrap does not implement provider calls, provider tasks, WIT worlds, Wasmtime execution,
real ureq or reqwest traffic, retry, fallback, routing, billing, quota ledgers, audit persistence,
task persistence, or host migration. Placeholder crates are unpublished ownership markers, not
promised public APIs.

There is no UI or desktop application change, so responsive, keyboard, and accessibility behavior
is not applicable.

## Security and data boundaries

- South does not read or write a database and has no database, cache, or migration dependency.
- Contracts and core code do not read network, filesystem, environment, clock, randomness, or
  secret-store state through hidden I/O.
- Hosts resolve secret references. Resolved plaintext credentials never enter provider components
  or serializable provider-facing contracts.
- Provider components have no network, filesystem, environment, or secret capabilities by default.
- `SafeHeaders` has private storage and no unchecked collection or deserialization implementation.
- The reserved-header set is an explicitly versioned policy. It is not a claim that a blacklist can
  recognize every authentication scheme. Future authentication uses separate declaration,
  resolution, and transport-injection contracts.
- Host-owned authentication, framing, and hop-by-hop headers are reserved. Ordinary provider
  headers are limited to 64 entries, 256 name bytes, 16 KiB per value, and 64 KiB total.
- Header errors and `Debug` output expose neither untrusted names nor values.
- Production code forbids `unsafe`, `unwrap`, and `expect` through compiler and Clippy gates.

## First public behavior

The initial `south-contracts` tests were written and observed failing before implementation. They
verify:

- valid header names are normalized and looked up case-insensitively;
- every version-one reserved authentication header is rejected;
- reserved-header matching is ASCII case-insensitive;
- duplicate normalized names are rejected;
- invalid values and duplicate errors do not expose values;
- the compatibility manifest matches the compiled reserved-header policy version.

Property tests vary the casing of `authorization`. A scheduled libFuzzer target exercises
arbitrary UTF-8 partitions for names and values. The first local fuzz verification completed more
than seven million executions without a crash.

## Engineering baseline

- Edition 2024, MSRV 1.96, exact stable toolchain 1.96.0.
- Stable rustfmt configuration and workspace Clippy lints with warnings denied in CI.
- `cargo nextest`, doctests, all-features, no-default-features, rustdoc warnings, MSRV check,
  `cargo deny`, `cargo audit`, and `cargo machete` are hard CI gates.
- Scheduled fuzzing uses the isolated exact `nightly-2026-08-15` toolchain because sanitizer
  instrumentation needs nightly flags. Production and normal CI remain stable-only.
- The pure-library workspace ignores `Cargo.lock`. Adding a binary requires changing this policy
  and committing the lockfile in the same change. The independent fuzz binary workspace commits
  `fuzz/Cargo.lock` and is checked separately by audit, deny, machete, boundaries, and PR compile.
- The dependency boundary gate parses the complete root and fuzz Cargo graphs, checks package
  names, dependency names, renames, sources, and paths, rejects migration directories, and tests
  itself against allowed, separator-variant, direct, and transitive forbidden fixtures.

The upstream Rust standards recommend two rustfmt import-grouping options that remain unstable on
Rust 1.96. Those options are intentionally omitted because requiring nightly would contradict the
same standards' stable production policy.

## Acceptance and release status

Bootstrap acceptance requires all local gates to pass, the public repository to use `main`, GitHub
CI to pass, and branch protection to require the quality job. `compatibility.json` must continue to
report both hosts as `not_verified` until each real host compiles against South and runs the public
contract suite.

The next slice must design versioned Canonical IR, HTTP, authentication, streaming, and error
contracts before implementing transport or runtime behavior. No placeholder crate may acquire a
speculative API before that design and its failing tests exist.
