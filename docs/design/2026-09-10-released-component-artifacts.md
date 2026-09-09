# Released Component Artifacts: Publishing What the Repository Already Builds

Status: proposed

Date: 2026-09-10

Predecessors: `2026-08-21-provider-runtime.md` (the sandbox that loads a component from
`manifest.json` + `component.wasm`), `2026-08-21-component-conformance.md` (gates ① and ②, which
decide whether a package is admissible at all), `2026-08-22-anthropic-provider-component.md` and
`2026-08-22-gemini-provider-component.md` (the two dialects that followed the reference shell).

## 1. Problem

This repository builds three provider components and ships none of them.

`components/provider-{openai-compatible,anthropic,gemini}` each hold a `manifest.json` and a
wit-bindgen shell that compiles to `wasm32-wasip2`. CI builds all three on every pull request — but
only as a side effect of the sandbox parity tests, which invoke `scripts/build-*-component.sh` as a
child cargo invocation, assert the sandboxed output equals the native reference, and then discard
the `.wasm`. No tag carries an artifact; the repository has published no GitHub Release at all.

The runtime, meanwhile, loads a component from exactly one shape: a directory holding
`manifest.json` next to `component.wasm`. So an adopting host that wants the sandbox has to produce
that directory itself, and the three crates here cannot help it — they depend on
`south-component-conformance` by `path`, which resolves only inside this workspace.

The consequence is visible in `token-station-server`, the one host that has adopted the v2
components. It carries `components/provider-{openai-compatible,anthropic,gemini}-v2`: three crates
whose `manifest.json` files are **byte-identical** to the ones here, and whose `src/lib.rs` differs
from ours only in doc-comment wording, a `#[cfg(target_arch = "wasm32")] mod shell` wrapper, a
vendored copy of the WIT world, and one test asserting that vendored copy still matches
`south_provider_api::ADAPTER_WIT`. Not one line of translation logic is duplicated — the shells
forward to the same `reference_*` implementations this crate exports. What the host duplicated is
not our code. It is our **build**.

That duplicate costs the host three assertions whose only job is to notice the copy drifting, and
both were added *after* a drift got through:

- crate version, `manifest.json` version, and the component's own `metadata()` disagreeing — which
  does not fail compilation and surfaces only at load time (host caught this on 2026-08-21);
- the vendored crate pinning a different South tag than the host, so the component's bytes come
  from one release while its declaration comes from another — which gate ① cannot see, because it
  compares the manifest against host expectations and both sides stay self-consistent (host caught
  this on 2026-08-22).

Both failures are silent by construction: they do not break a build, and they do not fail a
pre-commit gate. They fail at load. A host should not have to invent guards against a copy it only
made because we withheld a binary.

## 2. Boundary claim

Publishing a build artifact adds no capability to any crate. South still reads no databases, no
environment, no config files, no secret stores. Components still have no network, no filesystem,
and no secret access. The WIT world is untouched, `compatibility.json` gains no contract number,
and no crate's public API changes.

What changes is one repository behavior: a tag now produces downloadable, checksummed copies of the
three packages CI already builds and already proves equivalent to the native reference. This record
exists because `CONTRIBUTING.md` requires one before a change to release behavior, not because a
contract moved.

The artifact is deliberately **not** signed in this slice. Signing is component *operations* — key
custody, rotation, revocation, and a host-side trust anchor — and it belongs with the third-party
component story, not with publishing our own first-party build. §6 records that as the explicit
next slice rather than a silent gap.

## 3. Design

### 3.1 What a release carries

For each tag `vX.Y.Z`, three archives and one checksum file:

```
provider-openai-compatible-vX.Y.Z.tar.gz
provider-anthropic-vX.Y.Z.tar.gz
provider-gemini-vX.Y.Z.tar.gz
SHASUMS256.txt
```

Each archive holds exactly the two files the runtime loads, at the archive root:

```
manifest.json
component.wasm
```

That is the runtime's directory shape, so a host consumes a release by extracting an archive and
pointing its component directory at the result. No renaming step, no layout translation, and
nothing for a host to get wrong between download and load.

`SHASUMS256.txt` is `sha256sum` output covering the three archives, so a host verifies with
`sha256sum --check` and no bespoke tooling.

### 3.2 What builds it

A tag-triggered workflow, `.github/workflows/release.yml`, which:

1. checks out the tag;
2. installs the pinned 1.96.0 toolchain with the `wasm32-wasip2` target;
3. **refuses to proceed unless the tag matches the workspace version** (§3.3);
4. runs each `scripts/build-*-component.sh`, unchanged — the same scripts the parity tests already
   drive, so a release builds the artifact CI proved, not a second recipe that could diverge;
5. packages each `manifest.json` with its `.wasm` renamed to `component.wasm`;
6. writes `SHASUMS256.txt`;
7. creates the GitHub Release with the four files attached.

The workflow needs `contents: write`; the repository's default `permissions: contents: read` stays
as it is, and the elevated grant lives on this one job.

Artifacts are built by CI from the tag and never uploaded by hand. A hand-uploaded artifact would
reintroduce the exact question this record removes — whether the bytes came from the tag they claim
— and would put it back where nobody can check it.

### 3.3 The tag-matches-version gate

`compatibility.json`'s `release.version`, the workspace `version`, and each component manifest's
`compatibility.south_runtime` already agree today, and CI keeps the last of those honest:
`sandbox_parity_v1.rs` builds its `HostExpectationsV1` with `south_runtime: env!("CARGO_PKG_VERSION")`
and runs it through gate ①'s `compatibility_matches`, so a stale `south_runtime` in a shipped
manifest fails the test.

That guard has one blind spot, and releasing is precisely where it opens: it proves the manifest
matches *the version being built*, never that the version being built matches *the tag being
released*. Tagging `v0.27.0` on a tree whose workspace version still reads `0.26.0` would publish
three manifests declaring `south_runtime: "0.26.0"` under a `v0.27.0` release name, and every
existing test would pass, because each one is individually telling the truth.

So the workflow reads the workspace version and fails when it differs from the tag. This is the one
new check the slice introduces, and it exists because publishing is the first operation that makes
the tag and the version two separate facts.

### 3.4 What does not change

The three build scripts keep their current form. They are already the parity tests' entry point;
giving the release its own build recipe would mean the released bytes and the proven bytes come
from two different commands.

## 4. Consumer

`token-station-server` is the immediate consumer and the reason this slice is sized the way it is.
Its retirement path, once a release carries artifacts: delete the three vendored crates, replace the
three near-identical 44-line build scripts with one download-and-verify script, and delete the two
drift assertions named in §1 — a copy that no longer exists needs no guard.

One knock-on effect is worth naming, because it is the largest practical gain and it is not obvious
from the diff: that host's local test matrix currently needs a `wasm32-wasip2` toolchain purely to
build its vendored copy, and its build script carries a sysroot probe guarding against a `rustc` on
`PATH` that differs from the one `rustup` reports. Consuming a published artifact removes the local
wasm build, and the probe with it.

Nothing obliges a host to consume the artifact. A host that prefers to build from source keeps
doing exactly what it does now; this adds a supported path, it does not close one. Hosts with no
sandbox at all — the native reference implementation is the same source, as gate ② proves — are
unaffected either way.

## 5. Evidence

The artifact shape was verified against the real consumer before this record was written, by
running the workflow's own build and packaging steps at `v0.25.0` — the tag `token-station-server`
currently pins — and pointing that host's sandbox suite at the extracted archive:

```
GATEWAY_SOUTH_COMPONENT_DIR=<extracted archive> \
  cargo nextest run -p cloud_ai_gateway --test south_component_sandbox --run-ignored all
8 tests run: 8 passed, 0 skipped
```

The suite includes gate ①'s tuple handshake, the host translator loading the component, and
`sandboxed_component_matches_the_native_reference_byte_for_byte`. The host needed no change: it
consumed a packaged release exactly where it consumes its own vendored build today.

The negative case was run first and is the more informative half. The same suite, pointed at an
archive built from `main` (`0.26.0`) while the host pins `0.25.0`, failed all five loading tests
with:

```
component is not compatible with this host:
component was verified with south runtime `0.26.0`; this host runs `0.25.0`
```

That is gate ① doing its job on a real package: the archive parsed, the manifest was read, and the
refusal came from the version tuple rather than from anything about the archive's shape. A
mismatched release is refused at load, loudly, which is the property the tuple exists to provide.

Both runs used artifacts produced by the steps in §3.2 verbatim, including `sha256sum --check`
against the generated `SHASUMS256.txt`.

## 6. Versioning

No contract number moves. `compatibility.json` gains no field: it describes what the library
guarantees, and an archive on a release page is distribution, not guarantee. The first tag cut after
this merges is the first to carry artifacts; earlier tags stay bare, and nothing backfills them.

Deliberately left for later, each its own slice:

- **Artifact signing** (sigstore or minisign) with a host-side trust anchor. This is the gating
  requirement for loading a *third-party* component, where the question is no longer "did these
  bytes come from this tag" but "who authored them". A checksum answers the first question only.
- **A stable per-component version stream.** Component versions (`2.1.0`, `1.0.1`, `1.1.0`) move
  independently of the South version, and this slice names archives by the South tag alone. If a
  component ever needs to ship between South tags, the naming has to carry both.

## 7. Decisions to rule

D1. Archive layout: `manifest.json` + `component.wasm` at the archive root, matching the runtime's
directory shape exactly, rather than nesting under a per-component directory.

D2. Checksums as a single `SHASUMS256.txt` over the three archives, rather than one `.sha256` file
per archive.

D3. The release workflow fails when the tag does not match the workspace version (§3.3).

D4. Signing is out of scope for this slice, and named in §6 as the next one rather than left
unstated.

D5. The release reuses `scripts/build-*-component.sh` rather than defining its own build recipe.
