# CI: documentation-only pull requests skip the Rust steps, and component tests run in their own job

Status: **proposed** 2026-10-29, for lv's review (§8). No workflow change yet.

Date: 2026-10-29

Baseline: south `origin/main` = `be2c878d` (release 0.81.0 plus docs). CI figures come from the `quality` job of the
push-to-main run for #309 on 2026-10-29 (a documentation-only commit, cache restored; `ubuntu-latest`, 4 vCPU). Commit
figures come from the 176 first-parent commits on `main` between 2026-10-01 and `be2c878d`. Figures marked
**estimate** are arithmetic on those numbers, not measured runs.

Updated 2026-11-04 on `origin/main` = `98be5d5a`. Since the baseline, `ci.yml` changed only in two action pins
(`taiki-e/install-action`, `Swatinem/rust-cache` 2.9.3), so the decisions below are unaffected. Of the 20 first-parent
commits since the baseline, 12 changed only `docs/` (most of them the new `docs/upstreams/` notes).

## 1. Problem

lv asked whether moving the forty-one component packages to a repository of their own would reduce South's CI and
build load, and the host's. §7 answers that question (no). This record proposes two changes inside this repository
that address the same load.

The `quality` job of the baseline run took about 10.4 minutes. Its Test step took 372 s:

| Part of the Test step | Time |
|---|---|
| Compiling the 248 test binaries | 90 s |
| The `prebuild-components` setup script (forty-one packages and six test guests, four at a time) | 131 s |
| Running the 2448 tests, after the setup script | 149 s |

Of the tests, 290 are in the ten binary groups that load a real wasm component (the setup script's filter in
`.config/nextest.toml`). They account for 308 s of the 593 s of summed test time, and the longest of them takes 55 s.
The other 2158 tests sum to 285 s, the longest 25 s. The remaining steps of the job took about 4.2 minutes, of which
the no-default-features tests of `south-component-conformance` are 128 s and Clippy 48 s.

Two observations follow.

1. **A documentation-only change pays for the whole Rust matrix.** The baseline commit changed only Markdown files and
   still compiled and ran everything. (One of its three files is a fixture README under `crates/`, so C1 below would
   still run it in full.) In October, 31 of the 176 commits on `main` changed only `docs/` or root-level Markdown
   files. No crate embeds or reads a file under `docs/` or a root-level Markdown file (checked with a search
   for `include_str!`, `include_bytes!` and file reads in `crates/`), so for those commits every Rust step is a re-run
   of the previous result.
2. **The component half of the Test step is serial with the rest of the job.** The prebuild and the component tests
   (about 131 s plus most of the 149 s test phase) run on the same 4 vCPU as Clippy, the other tests and the
   no-default-features tests, one after another.

## 2. What already exists

- `prebuild-components` is a nextest setup script. It runs only when at least one test matching its filter is selected
  (checked locally with nextest 0.9.140: `cargo nextest run -p south-task-core` runs no setup script). Without it, a
  component test builds the package it needs on demand through `scripts/component-build.sh`, so a component test that
  runs without the prebuild is slow, not wrong.
- Tests already run only for pull requests into `main`, pushes to `main` and the weekly schedule; a stacked pull request
  runs Clippy and the other non-test steps.
- The verification-gate record (2026-10-16, accepted) decided V2: a push to `main` skips `quality` when the tree equals
  one a pull request run verified, using a `verified-tree` artifact. At `be2c878d` V2 is not yet in `ci.yml`.
- Branch protection is not enabled on `main` (verification-gate record Q2), so no required status check names a job.
- The repository is public, so standard hosted runner minutes are not billed.

## 3. Decision

### C1. A documentation-only pull request skips the Rust steps

A new job `scope` runs first on every event and writes one output, `scope`, with value `full` or `docs`. It checks out
with `fetch-depth: 2` and runs `python3 scripts/ci_scope.py`, which decides:

- `full` for every event other than `pull_request`: a push to `main` and the weekly schedule always run in full.
- `full` for a pull request whose head branch starts with `release/`.
- `docs` when every path in `git diff --name-only HEAD^1 HEAD` (the pull request's merge commit against its base) is
  under `docs/` or is a Markdown file at the repository root.
- `full` in every other case, including an empty diff, a failing `git` command, or any output it cannot parse.

The rule only ever removes work whose answer is known; every failure to decide runs in full. Markdown files below the
root (crate, component and fixture READMEs) do not qualify, because a future `include_str!` of one would make it
compiled input.

When `scope` is `docs`, `quality` runs only the steps that read documentation or need no Rust build: checkout, the
dependency boundaries check, and the release tooling tests. The toolchain install, the cache restore, Format, Clippy,
the tests, the doctests, the no-default-features steps, the fuzz compile check, Documentation, MSRV, cargo deny,
cargo audit and cargo machete are skipped. `language` is unchanged and runs on every event.

cargo deny and cargo audit read only the lockfiles and the advisory database. A documentation-only change cannot touch
the lockfiles, and advisory drift is caught by the next push to `main` and by the weekly run.

When V2 is implemented, a run whose `scope` is `docs` must not upload `verified-tree`, so the push to `main` after a
documentation-only pull request still runs in full and keeps saving the cache.

### C2. Component tests run in their own job, in parallel with `quality`

The ten binary groups that load real components move to a new job, `components`. The filter stays in one place, the
setup script's `filter` in `.config/nextest.toml`; `scripts/ci_scope.py --component-filter` prints it and both jobs
read it from there.

| Job | Test command | Other steps |
|---|---|---|
| `quality` | `cargo nextest run --workspace --all-features -E "not (<filter>)"` | everything it runs today, except the two steps in the next row |
| `components` | `cargo nextest run -p south-component-conformance -p south-provider-runtime --all-features -E "<filter>"` | the declared-runtime step for `release/*` pull requests, moved from `quality` |

- `components` runs under the same conditions as the Test step today (not for a stacked pull request) and only when
  `scope` is `full`.
- The declared-runtime step moves with the prebuild, so its build scripts stay no-ops after the setup script, which is
  why the CI deduplication record (D4) put it after Test.
- `components` gets its own `Swatinem/rust-cache` entry: the root workspace and the runtime-test guest workspaces now
  listed under `quality`. `quality` keeps the root and `fuzz`. Each entry is about 1 GB (the current `quality` keys are
  0.92 to 1.15 GiB), within the repository's 10 GB cache limit. Both save only on `main`, as today.
- `quality` ends with a check that no `components/*/target/` directory exists. That directory is created only by
  `scripts/component-build.sh`, and is not in any cache entry since D6 of the deduplication record. If it exists, a test
  outside the filter built a component, and the step fails with the name of the package.
- When V2 is implemented, `verified-tree` is uploaded by a final job that needs both `quality` and `components`.

`cargo nextest run` still compiles every test binary in `quality` (the filter applies at run time), and `components`
compiles its two crates' test binaries again. That duplicate compile is the price of the parallelism.

## 4. What does not change

- What is checked: every test, lint and audit that runs today still runs on every push to `main`, every release pull
  request and the weekly schedule, and on every pull request into `main` that changes anything outside `docs/` and
  root-level Markdown.
- `release.yml`, the package build scripts, the shared component target directory, and how hosts obtain packages.
- The local verification commands: `cargo nextest run --workspace --all-features` remains the union of the two CI test
  commands (§8 CQ-3 decides how the README lists them).

## 5. Expected effect

All figures are **estimates** from the baseline run.

| Case | Today | With C1 and C2 |
|---|---|---|
| Documentation-only pull request into `main` (31 of 176 in October) | ~10.4 min | ~1 min (`scope` plus a `quality` without Rust steps) |
| Any other pull request into `main`, wall time | ~10.4 min | ~7 min: `quality` ~7.0 min (its Test step ~90 s compile plus ~75 s of tests), `components` ~4.5 to 6.5 min, plus ~15 s for `scope` |
| Any other pull request into `main`, runner time | ~10.4 min | ~13 min (+25%: a second job setup and the duplicate compile) |

The critical path after C2 is `quality`, and within it the no-default-features tests (128 s); §9 lists it as a
possible follow-up. The prebuild's 131 s on a documentation-only commit with the cache restored suggests component
builds are not served from the cache; why is not established here (§9).

## 6. Risks

- **A path rule that is wrong turns a real failure green.** C1 is the only rule that skips tests, and it is limited to
  paths nothing compiles or reads. Any change outside them runs in full, and the next push to `main` runs in full
  regardless.
- **A new component test outside the filter.** It would run in `quality` and build its package there: slower, not
  wrong. The `components/*/target/` check in C2 makes that visible instead of silently slowing `quality` down.
- **A second job doubles the setup work and the cache.** Runner minutes are free on a public repository, and the cache
  stays well under the limit. If the repository becomes private, C2 costs about 25% more billed minutes per pull
  request.
- **The two test commands drift from the README.** Covered by CQ-3 and the test in §10.

## 7. Alternatives considered

**A separate repository for the component packages.** Rejected; these are the facts against it.

- **The host does not build components.** token-station-server's CI (`ci.yml`) and image build (`deploy/Dockerfile`)
  fetch the published release archives through `scripts/fetch_south_components.sh` and verify them against
  `SHASUMS256.txt` and the release index. Moving the packages removes no host build step.
- **The component directories are thin.** The forty-one packages hold about 2,500 lines of Rust; their translation
  logic is `south-component-conformance` (about 81,000 lines), which is also gate ②'s native reference. Moving the
  packages would take only the prebuild out of this repository, while `sandbox_parity` would have to fetch packages
  from another repository or move away from the reference it compares against.
- **Component changes are never component-only.** Of the 81 October commits that touched `components/`, none changed
  only `components/`; every one also changed `south-component-conformance` or another crate. Each would become a pair
  of pull requests across two repositories, released in order, and the host would pin two release indexes instead of
  one.

**A path rule that also skips component tests for Rust changes outside the components' dependency closure** (for
example `south-core` or `south-transport-reqwest` only). It would have applied to 3 of the 176 October commits, and it
needs a dependency closure that stays correct as crates change. Not proposed; CQ-2 asks whether to add it later.

**A third-party path filter action.** Every action in this repository is pinned by commit, and the rule is a few lines
of standard-library Python with unit tests, like the other CI scripts. Not proposed.

## 8. Questions for lv

- **CQ-1.** Adopt C1: a pull request that changes only `docs/` and root-level Markdown skips the Rust steps of
  `quality`? Recommended: **yes**.
- **CQ-2.** Also skip the component job when a change is outside the components' dependency closure (§7)? Recommended:
  **not now**. It would have applied to 3 of 176 commits.
- **CQ-3.** The README's full verification list: list the two CI test commands of C2 exactly as CI runs them, or keep
  the single union command with a note? Recommended: **list the two commands**. The deduplication record (D5) keeps the
  README list and the CI commands one for one.
- **CQ-4.** Adopt C2: component tests in their own job, at about 25% more runner time for about 3.4 minutes less wall
  time per pull request into `main`? Recommended: **yes**.

## 9. Out of scope

- Why the component prebuild is not served from the restored cache (131 s on a documentation-only commit).
- Moving the no-default-features tests of `south-component-conformance` (128 s) off the critical path.
- Implementing V2 of the verification-gate record; this record only states how it interacts with C1 and C2.

## 10. Tests

Written first and failing, in `scripts/test_ci_scope.py`, run by the Release tooling step:

- Scope: a push and a scheduled event are `full`; a `release/*` head is `full`; only `docs/` paths are `docs`; only a
  root-level Markdown file is `docs`; `docs/` plus any other path is `full`; a crate README alone is `full`; an empty
  diff is `full`; a failing `git` command is `full`.
- Filter: `--component-filter` prints exactly the setup script's `filter` from `.config/nextest.toml`.
- Coverage: every test file under `crates/*/tests/` that names `wasm32-wasip2`, `SOUTH_COMPONENTS_PREBUILT` or a
  component build script, directly or through a `support` module it declares, belongs to a binary the filter matches.
- README: if CQ-3 is ruled as recommended, both CI test commands appear verbatim in the README's full list.
