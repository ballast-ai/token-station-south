# Documentation Simplification Design

Date: 2026-10-21

Status: approved

## Purpose

Make the repository entry documents describe the current system clearly and briefly. `README.md`
must serve a host integrator who is new to South. `ARCHITECTURE.md` must describe current
architecture and invariants, not act as a release ledger. Historical and package-level detail must
remain discoverable in focused reference documents.

All changed prose must be English.

## Problems to correct

The current documentation has four concrete defects:

1. `README.md` is about 10,000 words and combines onboarding, architecture, a release ledger, and
   the full package catalog.
2. `ARCHITECTURE.md` contains 55 lines of CJK prose despite the repository's English-only rule.
3. `ARCHITECTURE.md` describes workspace release 0.74.0 and guest-linked crate versions 0.56.0,
   0.62.0, and 0.73.0, while the current versions are 0.76.0, 0.56.0, 0.63.0, and 0.75.0.
   It also omits the current 0.75.0 and 0.76.0 capabilities.
4. Hundreds of lines of release notes are nested under an unrelated shared-task-core heading and
   duplicate the release table in `README.md`. The dependency graph also does not say explicitly
   that it omits development-only edges.

## Document responsibilities

### `README.md`

Target length: 100–150 lines.

The README is the landing page. It contains:

- a short description and maturity warning;
- the dependency-inversion relationship between South and its two hosts;
- concise lists of what South owns and what remains host-owned;
- a grouped crate summary, with one sentence per group rather than one long paragraph per crate;
- a summary of the five component worlds and links to the package catalog;
- links to architecture, compatibility, security, contributing, releases, and components;
- the complete authoritative local verification commands.

It does not contain a release-by-release table or per-package protocol details.

### `ARCHITECTURE.md`

Target length: 200–300 lines.

The architecture document describes only the current 0.76.0 state:

- dependency inversion and repository boundary;
- crate ownership and the production/optional dependency graph;
- the provider, task, embeddings, image, and speech component worlds;
- package admission and the four conformance/runtime gates;
- host-owned policy, secret, persistence, network, and settlement responsibilities;
- compatibility range semantics and the current versions of workspace and guest-linked crates;
- current capabilities introduced by 0.75.0 and 0.76.0 where they affect the present model:
  Kiro image input, Bailian reasoning caps, Qwen ASR, and Wan media tasks;
- links to the focused references for historical and package detail.

The dependency graph is labelled as production plus optional-feature edges. Development-only
dependencies are deliberately excluded and that exclusion is stated next to the graph.

The document contains no chronological release notes and no CJK prose.

### `docs/releases.md`

This is the single human-readable release index. It preserves the existing release table's useful
information in a compact form and links to the existing release design records. It does not copy
the long narrative release sections from the old architecture document when an existing design
record already carries that evidence.

The latest entries include 0.75.0 and 0.76.0. Release status language must match the corresponding
release records and must not imply host adoption; host adoption remains authoritative in
`compatibility.json`.

### `docs/components.md`

This is the package catalog. It contains:

- the package name, world, families, build command, and a concise current-purpose summary;
- package admission and artifact identity notes that currently follow the README table;
- links to package manifests, fixture-pack READMEs, and the relevant design records where useful.

It avoids embedding a second release history. Machine-readable identities and adoption remain
authoritative in manifests, release indexes, and `compatibility.json`.

## Migration rules

- Preserve factual content that has current operational value.
- Move historical detail only when it is not already present in a release or design record.
- Replace duplicated historical prose with links to the authoritative record.
- Do not change public contracts, runtime behavior, manifests, compatibility status, package
  identity, or verification commands.
- Do not add generated documentation tooling in this change.
- Keep every existing valid document target currently linked by `README.md` or `ARCHITECTURE.md`
  directly linked from at least one of the four resulting documents.

## Verification

The documentation change is complete only when:

1. `README.md` and `ARCHITECTURE.md` meet their target responsibilities and contain no CJK prose.
2. All relative Markdown links in the four documents resolve to tracked files or valid anchors.
3. `scripts/check-language.sh --regen` lowers the language baseline, followed by a passing
   `scripts/check-language.sh`.
4. The current versions and compatibility facts agree with `Cargo.toml`, crate manifests, and
   `compatibility.json`.
5. Every local verification command listed in the rewritten README passes before completion is
   claimed.

## Non-goals

- No source, manifest, component, fixture, compatibility, or runtime changes.
- No rewrite of existing design records.
- No change to release or package versioning policy.
- No attempt to turn the README into full host-integration documentation.
