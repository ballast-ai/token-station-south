# CI and local verification: build each thing once

Status: proposed.

Date: 2026-10-15

Baseline: south `origin/main` = `e1244c0`. CI timings come from the `quality` job of run `38676424278` (a push to main
on 2026-10-15). Local measurements were taken on an Apple-silicon laptop with other builds running (load average 8 to
37), so they show ratios, not absolute times.

## 1. Problem

The `quality` job and the README's local verification list are the same commands, and four of them compile or run
work that another step has already done.

1. **The component packages share no build.** Every package under `components/` is its own cargo workspace with its own
   `target/`. The nextest setup script `scripts/prebuild-components.sh` already builds each package once per test run
   (before it, each test process built its own), but forty release builds each still compile the crates they all link
   (serde, wit-bindgen, `south-contracts`, `south-provider-api`, `south-component-conformance`) from scratch. In the CI
   run the Test step took 711 s: 279 s to compile the test binaries and 430 s of `nextest`, of which the component
   builds (32 to 40 s each, four at a time) are the larger part. Locally the duplicate targets are also what fills a
   disk: a south worktree reached 20 to 25 GB of `target/`.
2. **The MSRV step repeats the Clippy step.** `rust-toolchain.toml` pins `1.96.0` and the workspace `rust-version` is
   `1.96`, so `rustup run 1.96.0 cargo check --workspace --all-targets` checks with the compiler Clippy has just run.
   The pinned standards require the MSRV to be tested in CI so it cannot drift; while the two are the same release,
   Clippy already is that test.
3. **The no-default-features step reruns every test.** The only feature in the workspace is `sandbox` on
   `south-component-conformance`, and it is not a default feature. For the other eleven crates
   `cargo test --workspace --no-default-features` is the all-features test run again. In CI the step took 91 s: 31 s
   to compile the test binaries and 57 s running 173 of them, of which `south-component-conformance`'s own took 37 s
   and the other crates' 20 s. CONTRIBUTING.md requires every crate to *build* without default features; only one
   crate's tests change. The saving is therefore modest: the other crates' 20 s of tests, and a check build instead of
   a test build for them.
4. **A release pull request builds every component twice.** The `declared-runtime` job, which runs only on `release/*`
   pull requests, checks out and builds every component on its own runner, while the `quality` job of the same pull
   request has just built them all for its tests.

The local list also lacks one check CI runs: `scripts/check-language.sh --commits` on the pull request's commits, so
a Chinese commit message passes locally and fails in CI.

## 2. Decision

| # | Change |
|---|---|
| D1 | New `scripts/component-build.sh <package>`: builds the package with `--target-dir target/components` and copies the wasm to `components/<package>/target/wasm32-wasip2/release/`, the path every reader already uses. Each `scripts/build-*-component.sh` becomes a call to it. |
| D2 | New `scripts/check_msrv.py`: exits 0 with a message while the pinned toolchain is a release of the MSRV; otherwise runs `rustup run <msrv> cargo check --workspace --all-targets`. CI's MSRV step and the README run it. |
| D3 | The no-default-features step becomes `cargo check --workspace --all-targets --no-default-features` plus `cargo test -p south-component-conformance --no-default-features`. |
| D4 | The `declared-runtime` job becomes a step of `quality` after Test, under the same `release/*` condition. The build scripts it runs first are fresh no-ops there. |
| D5 | The README's local list gains `scripts/check-language.sh --commits origin/main..HEAD`, and follows D2 and D3. |
| D6 | CI's `rust-cache` list drops the forty `components/*` entries: their builds now live in `target/components`, inside the root entry. |

### 2.1 Why one shared target directory is safe

- **Bytes.** The release's digest-stability check refuses a package whose `component.wasm` changed under an unchanged
  version, so the build location must not enter the wasm. Eight packages (`image-qwen`, `provider-anthropic`,
  `task-kling-v2`, `speech-groq`, `embeddings-gemini`, `provider-openai-compatible`, `task-veo-v2`, `image-xai`) built
  cold in separate directories and in one shared directory produced SHA-256-identical wasm.
- **Readers.** The tests, `scripts/check-declared-runtime.sh` and the release workflow's packaging step read
  `components/<package>/target/wasm32-wasip2/release/<package with - as _>.wasm`. The copy keeps that path, and every
  official package's library name follows that rule (checked against each `Cargo.toml`).
- **Concurrency.** The prebuild runs four builds at once. On one target directory cargo serialises them on its build
  lock, and the measurement below includes that waiting.
- **Lockfiles.** Each package keeps its own `Cargo.lock`. Two packages that lock different versions of a crate get two
  artifacts side by side; they share only what is identical.

### 2.2 Measurement

Eight packages, four at a time, each mode cold, run twice in alternating order:

| Mode | Wall time | Disk |
|---|---|---|
| A target directory per package (today) | 85 s, 78 s | 1.8 GB |
| One shared target directory (D1) | 22 s, 23 s | 236 MB |

## 3. What does not change

- The tests, their `SOUTH_COMPONENTS_PREBUILT` handling and the runtime-test guests (still built in their own
  directories; six debug builds, out of scope here).
- `release.yml`: it calls the same build scripts and reads the same paths.
- What CI checks: every check that ran before still runs, on the same toolchain; D2 and D3 drop only repetitions.

## 4. Tests

- `scripts/test_component_build.py`: with a stub `cargo`, `component-build.sh` passes `--target-dir
  target/components`, copies the wasm to the package path, does nothing under `SOUTH_COMPONENTS_PREBUILT`, and refuses
  an empty, unknown or path-like package name; every `scripts/build-*-component.sh` is exactly one call to it for an
  existing package.
- `scripts/test_check_msrv.py`: the toolchain decision for pinned, newer and floating channels and malformed MSRVs, and
  that this repository's pin needs no second check.

Both failed before the scripts existed and run in CI's Release tooling step.

## 5. Out of scope

- The south tests leave their temporary directories behind (about 2,800 directories, 10 GB, in the system temporary
  directory after a few days of runs). That is a test change and gets its own record.
- The push to main after a squash merge reruns the pull request's `quality` job. It also writes the cache every pull
  request restores, so it stays.
