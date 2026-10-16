# Component packaging: what the plug-in boundary delivers today

Status: proposed. This record makes no change. It measures, states the questions for lv, and proposes the order of the
work, so that the changes it leads to each get a record of their own.

Date: 2026-10-16

Baseline: south `origin/main` = `56face6` (after v0.69.0). Host facts come from token-station-server `origin/dev-v2` =
`0f9eef6d`. Local timings were taken on an Apple-silicon laptop while other builds were running, so they show ratios,
not absolute times. CI timings come from the `quality` job of run `38841259115` (PR #244, cold cache: the job logged
`No cache found`).

## 1. Problem

The packaging promised that each provider is a plug-in. A host loads a directory of packages, each package is admitted
on its own (gate ①, range handshake, import scan, identity probe), and adding a family means dropping a package into
that directory without rebuilding the host (host plan 46: "a new dialect means only dropping a wasm package from South; the host is not rebuilt"). The loader
does all of that. Four facts keep the promise from being kept.

1. **The deployment ships no packages.** The host's `deploy/Dockerfile` copies the binary and CA certificates into a
   `scratch` image. No compose file, Helm chart or production config template mounts a components directory.
   `scripts/fetch_south_components.sh` is run by the local matrix and by acceptance runs. The host's own review records
   this as intended (the components directory is not in the tree, so by default the references run instead of the sandbox; "intended: fetching packages is a deployment step"). So unless an
   operator fetches packages outside the repository, the known dialects run on the native reference implementations and
   no other package is loaded. Which of the two happens in production is not recorded anywhere I could read (Q1).
2. **Every provider's logic is compiled into the host anyway.** The translation code of all forty packages lives in
   one crate, `south-component-conformance`: 36,017 lines, of which 18,971 are `reference_*.rs` modules, about 3,800
   are helpers shared by a family of vendors (`anthropic_dialect`, `gemini_text`, `asr_shared`, `image_shared`, ...),
   and the rest are the ABI shims, sandbox seams, suites and fixtures. Each package under `components/` is a 60-line
   shell (2,466 lines for all forty) that compiles this crate to wasm. The host links the same crate for its native
   fallback (`south_component.rs`, `south_task_executor.rs`). The fallback was added so that the host's test matrix
   needs no wasm toolchain; it also means a host binary carries every provider whether or not it loads a package.
3. **No package can change alone.** Every package links the shared crate, so a change to any one provider changes
   every `component.wasm`. Q47 (boundary record §13.12) gave the shared crates their own versions so that a release
   that leaves them unchanged re-identifies no package. Since Q47, 18 of the 20 releases (v0.51.0 to v0.69.0) changed
   `south-component-conformance`; only v0.51.1 and v0.53.0 did not. In the other 18, every package took a patch version
   with unchanged behaviour, a new digest, and a new gate ② report. Q47 works as specified; it does not help, because
   new providers are added to the crate it versions.
4. **The host pays for each release by hand.** The last four host repins of South (v0.65.0 to v0.68.0) changed 57 to
   72 files each (600 to 1,270 lines). Part of that is new host behaviour, but part is bookkeeping that the shared
   crate causes. `south_task_executor.rs` keeps hand-copied native manifests whose version line is edited every
   release, and its comment has become a release log. The operator manual's family pins (`P13`) must be re-entered
   after every release, because a pin that names an old digest refuses to start (R26). The host's South glue is
   13,033 lines, one adapter per world (text, task, embeddings, image, speech).

## 2. Measurements

### 2.1 Where CI time goes

The `quality` job of run `38841259115` (cold cache) spent its Test step (831 s) as follows:

| Phase | Time |
| --- | --- |
| Compile the workspace test binaries | 393 s |
| Nextest setup: `prebuild-components.sh` (40 packages and 6 test guests, four at a time) | 259 s |
| — the 40 packages (first build compiles the shared crates, about 33 s; then about 9 s each) | 118 s |
| — after the last package: the rest of the 6 test guests (each in its own target directory) | 140 s |
| Run the tests | 173 s |

A warm cache removes most of the first row (wasmtime and cranelift dominate a cold build: in the local cold build
below, `cranelift-codegen` alone took 101 s of the 197 s wall time), but not the second.

### 2.2 What one provider change rebuilds

Local measurement in a clean worktree, `cargo test --workspace --all-features --no-run --timings` and
`scripts/prebuild-components.sh`, after touching one file:

| Experiment | Test build | Package builds |
| --- | --- | --- |
| Cold | 197 s | 96 s |
| E0: nothing touched | — | 1 s |
| E1: touch `south-component-conformance/src/reference_xai_image.rs` (one provider) | 19 to 21 s | 36 s, all 40 packages |
| E2: touch `components/image-xai/src/lib.rs` (one package shell) | 0 s | 2 s, one package |
| E3: touch `south-provider-runtime/src/lib.rs` (the runtime) | 19 s | 1 s, none |

E1 shows the cost of item 3 inside the workspace. The library itself recompiled incrementally in 1.4 s, but all 117
integration-test binaries of `south-component-conformance` were relinked (117 units, 189 unit-seconds, 21 s wall), and
nextest then runs all of them. CI builds with `CARGO_INCREMENTAL=0`, so there the library rebuild is the full 18 s.

E2 is what a provider change would cost if each family's logic sat in its own crate: one package rebuilt in 2 s,
against 36 s and forty new digests for E1. E3 shows that the runtime is already separate from the packages: a runtime
change relinks the runtime's tests and rebuilds no package. The E1 and E2 package builds are local, warm and
incremental; in CI the shared crate's release build is not incremental, so the gap is wider there.

### 2.3 Two costs that do not need the split

- **117 integration-test binaries in one crate.** Each file under `south-component-conformance/tests/` is its own
  binary, linked separately against the whole crate and its dev-dependencies. Folding them into one test binary
  (`tests/main.rs` with one module per file) is a test-layout change with no runtime effect, and it removes most of the
  E1 relink cost whatever happens to the packaging.
- **Six test guests with six target directories.** The guests built by `prebuild-components.sh` do not share
  `target/components`, so each compiles wit-bindgen and serde again (they finish 140 s after the last package, of the 259 s above). This was left out of
  the CI deduplication record (#244, §5).

## 3. Proposed direction

The choice is between two ends, and it is lv's (Q2).

- **(A) Make the plug-ins real.** Production loads only packages; the native references become test-only. A stable,
  thin guest SDK (the ABI shims, `south-contracts`, the shared grammars) is the only crate every package links, and each
  vendor family's logic moves into a crate of its own that only its packages link. A change to one family then rebuilds,
  retests, re-identifies and republishes only that family's packages, and CI can select the affected packages from the
  cargo dependency graph. The host keeps one adapter per world and loses the hand-copied manifests.
- **(B) Accept a library.** If no deployment needs to change providers without rebuilding the host, the wasm path, the
  per-package admission and the range handshake cost more than they return for the text world, and can be simplified.

I recommend (A), in this order, each step gated on the work it would otherwise collide with:

1. **Measure and decide** (this record): fill in §2.2, answer Q1, rule Q2. No code changes.
2. **Ship packages with the deployment** (host). Put the packages in the image or a mounted volume as part of the
   host's cutover checklist, not as a separate deployment. Run on the wasm path in production for a period before the
   native references become test-only.
3. **Split the shared crate** (South). In a release cycle with no new world in progress. Start with the embeddings world
   (three packages) to prove the SDK boundary, then one world at a time. Families that share code today
   (`reference_anthropic` is used by the Bedrock Invoke, Vertex and Claude Code paths; `asr_shared` by four speech
   references) stay one crate each; the unit is the family, not the package.
4. **One adapter per world in the host** (host). After the host's native-arm deletion reaches zero, because that work
   edits the same files.

The two items in §2.3 are independent of the choice and can be done at any time between releases, each under a short
record of its own.

## 4. Questions for lv

- **Q1.** In production today, does the host load packages, or does it run the native references? The host's system
  page (`/admin/api/system`, `south_packages`) shows the source of each dialect.
- **Q2.** (A) or (B)? Recommended: (A).
- **Q3.** Under (A), is a family crate's version the identity that moves, with the SDK versioned on its own as Q47's
  three crates are now? Recommended: yes; Q47's rule then applies to the SDK alone.
- **Q4.** Should the §2.3 test-layout changes go ahead now, separately from Q2? Recommended: yes.

## 5. What does not change

Nothing changes with this record: no code, CI, contract, package, manifest or release procedure.

## 6. Out of scope

- The host's own CI and matrix, beyond what the host-side steps above would remove.
- wasmtime compilation cost at host start-up (host review #119). Under (A), shipping precompiled artifacts or a warm
  compilation cache becomes the host's concern; its record will decide it.
