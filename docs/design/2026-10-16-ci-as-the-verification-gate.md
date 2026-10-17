# Verification: CI is the gate, local runs the fast checks, and a verified tree is not verified again

Status: accepted (lv, 2026-10-17: Q1–Q3 as recommended, and the release owner's local replay is dropped too, V3;
confirmed in this repository's session). lv chose the direction on 2026-10-16: South's full verification runs in CI,
and locally only the fast checks; the host keeps its local matrix as its gate and records its verdict (a host record
of its own).

Date: 2026-10-16

Baseline: south `origin/main` after v0.70.0. CI figures come from the GitHub API for workflow `ci.yml` between
2026-10-09 and 2026-10-16. Local figures come from the README run for #255 on 2026-10-16 (warm cache, Apple-silicon
laptop shared with other sessions).

## 1. Problem

One South change is verified up to three times on identical input, and for a release twice more.

1. **Locally**, the README's 21 commands. Since the CI deduplication record (#244, D5) they are the commands of CI's
   `quality` job, one for one. The run for #255 took about 8 minutes warm (nextest 197 s, the conformance crate without
   default features 121 s, language 60 s, clippy 48 s).
2. **On the pull request**, the same commands in `quality` (12 to 20 minutes).
3. **On the push to main**, `quality` again. Since 2026-10-09 there were 96 such runs, 19.6 minutes on average, about
   31 runner hours in a week. When the pull request was up to date with main, the merged tree is the tree step 2 tested,
   so the run learns nothing new. (Branch protection is not enabled, so a pull request may also merge behind main.)
4. **On a release pull request**, `quality` with the declared-runtime step, and **on the tag**, `release.yml`, which
   builds the forty packages one after another and runs the gate ② tests again.

What is scarce is not CI time but the laptop. Every session on it shares one build slot (a timed build and a host
matrix collided at load 38 on 2026-10-16 and the host matrix's 14-second deadlines produced false reds), and its disk
filled during verification more than once. The local full run is the step that costs that resource, and step 2 repeats
it on a runner that costs nothing local.

The push-to-main run has a second job: `Swatinem/rust-cache` saves only on `refs/heads/main`, so it is the run that
keeps pull requests' dependency cache warm. In the last four push runs the cache was restored and reported
`Cache up-to-date`: nothing was saved, because the key (lockfile, toolchain, workflow) had not changed.

## 2. Decision

### V1. Two local tiers; CI is the gate

The README's "Local verification" becomes two lists.

- **Before every push (fast)**: `cargo fmt --all -- --check`, Clippy as CI runs it, `scripts/check-language.sh`
  (with `--commits origin/main..HEAD`), `scripts/check-boundaries.sh`, and the scripts' unit tests. About 2 minutes
  warm in the #255 run (Clippy 48 s and the language scan 60 s are most of it). The tests of the crates the change
  touches (`cargo nextest run -p <crate>`) are recommended, not required.
- **Full (what CI runs)**: the present 21 commands, kept so that a CI failure can be reproduced locally. They are run
  on demand, not before every pull request.

Work is complete when the fast list passes locally and `quality` and `language` are green on the pull request's head.
This changes a repository rule: AGENTS.md ("Run every local verification command in README.md before declaring work
complete") and CONTRIBUTING.md ("Run the commands listed in the root README before opening a pull request") are
reworded to say this (Q1).

### V2. A push to main does not re-verify a verified tree

The push-to-main `quality` run first decides whether the tree has already passed:

1. Compute the tree of `HEAD` (`git rev-parse HEAD^{tree}`).
2. Find the pull request that produced the commit (`GET /repos/{repo}/commits/{sha}/pulls`), and that pull request's
   latest `ci.yml` run whose `quality` job succeeded.
3. Read the tree that run tested. The `quality` job on a pull request records it: it uploads a one-line artifact
   `verified-tree` holding `git rev-parse HEAD^{tree}` of the merge commit it checked out, after every other step has
   passed.
4. If the trees are equal **and** `rust-cache` restored an exact key match (`cache-hit` is `true`), the run skips every
   step after the cache restore and reports success, naming the run it relied on in the job summary. Otherwise it runs
   in full, as today.

Every failure to decide (no pull request, no successful run, no artifact, an API error) runs in full. The rule only
ever removes a run whose answer is already known; it never removes the run that saves a new cache key.

`language`, `crate-versions` and the weekly `fuzz` and full scheduled run are unchanged. The weekly scheduled `quality`
run is never skipped, so a stale advisory database, or a regression that only a fresh runner shows, still surfaces
within a week.

The decision is a small Python script, `scripts/verified_tree.py`, so it has unit tests like the other release tooling
(`scripts/test_verified_tree.py`): equal trees and a cache hit skip; unequal trees run; a cache miss runs; no pull
request, no successful run, a missing artifact and an API error each run.

### V3. A release is not replayed locally

CONTRIBUTING.md asks the release owner to run `scripts/check-declared-runtime.sh --build` and
`python3 scripts/check_crate_versions.py --unreleased` locally before a release. CI already runs both on every
`release/*` pull request, on the same tree, so the local replay repeats a check whose answer CI gives anyway, on the
shared laptop. It is dropped: the release pull request's CI is the gate. The two commands stay documented so that a CI
failure can be reproduced locally, and the release owner may run them on demand.

## 3. What does not change

- What CI checks, and on which events `quality` starts.
- The release path in CI: a release pull request and `release.yml` run in full (Q3). Only the local replay goes (V3).
- The host: its matrix stays its gate (the host's own record).

## 4. Expected effect

- Local: about 2 minutes of the shared laptop per change instead of about 8 warm (more when cold), and no full test
  build in each South worktree, which is what fills the disk.
- CI: most of the 96 push-to-main runs a week skip, about 31 runner hours. A pull request merged behind main still
  runs in full. These are estimates from the figures above; the record that implements V2 will report the measured
  skip rate after a week.
- The cost: a test failure that the local full run would have caught is found by CI 12 to 20 minutes later.

## 5. Questions for lv, and the rulings

Ruled by lv on 2026-10-17: every question as recommended. Q1: AGENTS.md and CONTRIBUTING.md are reworded as in V1 (and,
for V3, CONTRIBUTING.md's two "run it locally before a release" sentences). Q2: no branch protection now; V2's skip
rate is measured first. Q3: reusing the release pull request's artifacts is a separate record, later. lv added V3.

- **Q1.** Reword AGENTS.md and CONTRIBUTING.md as in V1? Recommended: yes; without it every session keeps running the
  full list because the rule says so.
- **Q2.** Enable branch protection's "require branches to be up to date before merging" on `main`? It makes V2's trees
  equal on almost every merge, at the cost of an extra CI run whenever main moved under a pull request. Recommended:
  not now; measure V2's skip rate first.
- **Q3.** Should `release.yml` reuse the release pull request's packages and gate ② reports, when the tag's tree equals
  the tree they were built from, instead of building again? It would save 8 to 20 minutes per release, but it moves
  where the published artifacts are built, which is a provenance question. Recommended: a separate record, later.

## 6. Tests

- `scripts/test_verified_tree.py`, written first and failing, as listed in V2.
- A new `scripts/test_readme_lists.py` checks that the fast list is a subset of the full one, and that every command
  of the full list appears among `ci.yml`'s `run:` steps. Nothing enforces #244's D5 (the README list and `quality`
  are the same commands) today; the full list would otherwise drift from CI unnoticed once it is no longer run before
  every pull request.
