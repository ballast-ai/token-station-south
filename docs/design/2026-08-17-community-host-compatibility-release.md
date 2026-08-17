# Community Host Compatibility Release

Status: implemented; South gates and fixed-candidate host CI green; final review and release pending

Date: 2026-08-17

## Problem

The Token Station community host now compiles a real host adapter against South, runs the public
`south.provider-call.v1` suite through that adapter, and exercises the real South reqwest transport
against deterministic loopback servers. The host change merged, but `compatibility.json` still
reports `token-station/provider_call` as `not_verified`, and the root README still says neither host
has adopted South.

The release record must be corrected without overstating migration scope. The community host has
adopted South only for an explicit diagnostic, buffered Bearer JSON POST path. Its production data
plane, streaming path, proxy paths, quota metadata, routing, retries, fallback, and persistence have
not moved to South.

## Compatibility meaning

South's binding verification rule is reproducible host integration evidence:

1. the real host compiles against South;
2. the real host adapter runs the public assembled-executor conformance suite;
3. review confirms that reported resolver, transport, call-count, deadline, cancellation, and
   pending-drop evidence is measured at the real adapter boundaries.

A live provider-account probe is a separate host operational check. It tests credentials, external
network reachability, account state, and provider availability; it is not deterministic public
library evidence and may incur cost. Token Station's adoption plan may continue to require one
explicitly authorized live probe before declaring the diagnostic rollout operationally complete,
but that probe is not required for South's `provider_call=verified` compatibility annotation.

This distinction prevents a public compatibility manifest from depending on private credentials or
an external provider's transient availability.

## Evidence

The community provider-call verification is based on these immutable or reviewable artifacts:

- host repository: `GlimpseEngine/token-station`;
- adoption pull request: `#91`, merged on 2026-08-17;
- adoption head: `63a3ceb05cf57acde8542b80302741c35056ed2c`;
- merge commit: `53c2f9fb851b6b41a5e3a26cb602c52e036c29d7`;
- final pull-request CI run: `31996921604`, successful for Rust 1.96, root Rust, Desktop Rust,
  Desktop coverage, frontend, supply-chain, and release gates;
- the host's non-test diagnostic-path adapter functions, wrapped by its
  `CommunityConformanceExecutorV1`, passed all seven `south.provider-call.v1` cases;
- deterministic loopback tests used the official OpenAI-compatible provider component and the real
  South core and reqwest transport for success, provider error, redirect denial, response-size,
  UTF-8, timeout, socket-close, and no-replay behavior;
- assembled conformance tests separately proved cancellation and deadline behavior at the adapter
  resolver and transport boundaries;
- independent wiring review closed all P0, P1, and P2 findings before merge.

The CI run's pull-request-only root coverage job was skipped by the host workflow's existing event
policy; Desktop coverage and all other required pull-request jobs succeeded. This release does not
depend on the unrelated Windows/macOS post-merge workflow.

The `0.1.1` release candidate was then verified independently of the earlier `0.0.1` adoption:

- South candidate commit: `ef1ab0d2c5ec108835d083c5f6f5b7da510520e4`;
- host validation commit: `323b85f795cdf03ea40aef4d3752395b3620af22`;
- host validation pull request: `GlimpseEngine/token-station#92`, targeting `develop-v2` and kept
  draft until the final South tag exists;
- host validation CI run: `32004432516`, successful for supply-chain, release gates, frontend,
  Rust 1.96, Desktop Rust, Desktop coverage, and root Rust;
- the host pinned all five South crates to exact version `=0.1.1` and the candidate commit, ran the
  seven-case public conformance suite through the real diagnostic adapter, and passed the real
  reqwest loopback tests;
- the host's pull-request-only root coverage job was skipped by existing workflow policy; no
  Windows or macOS Platform workflow was required for this non-main pull request.

After this evidence commit, the South candidate-to-final diff may contain only this design record.
Rust sources, package manifests, compatibility data, tests, and lockfiles must remain byte-for-byte
unchanged from the verified candidate. The final `v0.1.1` tag is valid only if that invariant holds.

## Goals

- Mark `token-station/provider_call` as `verified`.
- Keep `token-station/provider_stream` as `not_verified`.
- Keep the legacy top-level `hosts.token-station` summary equal to provider-call status.
- Record the evidence and the narrow diagnostic adoption scope in repository documentation.
- Publish the documentation and compatibility correction as patch release `0.1.1` after merge and
  green South CI.
- Give the community host an immutable tag to consume instead of its bootstrap revision.

## Non-goals

- Do not claim that community production traffic uses South.
- Do not claim streaming, proxy, GET, Header auth, OAuth, quota metadata, retry, fallback, routing,
  provider WIT, or provider runtime compatibility.
- Do not execute or record a live provider credential in this repository.
- Do not change any South Rust public API, contract version, conformance fixture, transport policy,
  error code, or runtime behavior.
- Do not change `token-station-server` capability status.
- Do not tag an unmerged commit or move an existing tag.

## Manifest and version changes

`compatibility.json` will keep schema version one and `library_slice` stability. Only these values
change:

- `release.version`: `0.1.0` to `0.1.1`;
- `hosts.token-station`: `not_verified` to `verified`;
- `host_capabilities.token-station.provider_call`: `not_verified` to `verified`.

`host_capabilities.token-station.provider_stream` remains `not_verified`. The manifest test will
continue to enforce that each legacy host summary exactly mirrors provider-call status.

The workspace package version and exact internal path-dependency versions move from `0.1.0` to
`0.1.1` because all published crates share one workspace release version. The nested fuzz
workspace's dependency declarations and lockfile must resolve the same package version. The root
library workspace must continue not to commit `Cargo.lock`.

Patch version `0.1.1` is appropriate because this release changes compatibility and documentation
evidence without changing public Rust API or contract behavior.

## Documentation changes

- README must describe the implemented buffered and streaming contract slices accurately.
- README must state that both real hosts have verified provider-call adapters while neither host is
  verified for streaming.
- The minimal provider-call design must record the community evidence and release status.
- The streaming design must keep community streaming adoption explicitly `not_verified`.
- Architecture, contributing, security, and repository instructions remain unchanged unless a
  factual audit finds a direct contradiction.

## Security and data redlines

- Never store a provider credential, endpoint, request body, response body, or private config value
  in this repository, its tests, documentation, errors, or CI logs.
- Verification evidence must remain reproducible without a live provider account.
- South must not gain access to host configuration, databases, secret stores, environment
  variables, keychains, or provider-component secrets.
- The compatibility update must not weaken redirect, proxy, authentication, response-bound,
  cancellation, deadline, or no-replay guarantees.
- The release tag must be created only from the merged commit after South CI succeeds and must never
  be moved.

## Test-driven implementation

1. Change the public compatibility-manifest test to require community provider-call verification.
2. Run that test and observe failure because the manifest still says `not_verified`.
3. Apply the minimal manifest status change and observe the focused test pass.
4. Bump workspace and internal dependency versions; use the existing release-version consistency
   assertion to observe the manifest mismatch before updating `release.version`.
5. Regenerate only the nested fuzz lockfile metadata required by the version bump.
6. Run every verification command in the root README.

## Release sequence

1. Commit and push a fixed South `0.1.1` candidate containing the design, failing-test evidence,
   compatibility change, version bump, and documentation.
2. In a separate Token Station pull request to `develop-v2`, pin exact version `=0.1.1` and the
   candidate commit, regenerate lockfiles, update the dependency policy, and require the host's
   Linux CI and South conformance gates to pass.
3. Record the immutable candidate, host commit, host pull request, and successful CI run here. From
   this point until the release tag, permit only this South design record to differ from the
   verified candidate.
4. Open the South pull request to `main` and require South CI and review to succeed before merge.
5. Merge the South pull request.
6. Create immutable tag `v0.1.1` on the merged commit and push the tag.
7. Update the existing Token Station validation pull request from the candidate commit to the final
   tag commit, prove the five crate contents are unchanged, and rerun the same host gates before
   merging it to `develop-v2`.
8. Keep the live provider-account probe as an explicit, separately authorized community
   operational acceptance item.

## Acceptance

This South release is ready to merge when:

- the manifest reports community provider-call `verified` and provider-stream `not_verified`;
- legacy host summaries and per-capability annotations remain consistent;
- workspace, internal dependency, fuzz dependency, and compatibility versions all equal `0.1.1`;
- README and design records describe both host adoptions without claiming production or streaming
  migration;
- the exact `0.1.1` candidate passes the real Token Station host's required Linux CI and public
  conformance gates, with immutable candidate, host-commit, pull-request, and run identifiers
  recorded above;
- the candidate-to-final diff outside this design record is empty;
- every README verification command passes;
- review finds no open P0 or P1 issue.

The release is complete only after the pull request merges, South CI is green on the merged commit,
and immutable tag `v0.1.1` is published. Community dependency migration and the optional live
provider probe remain separate follow-up work.

## Local implementation result

The compatibility test was changed first and failed because the manifest still reported the
community host as `not_verified`. After the two scoped status fields changed, that test passed. The
workspace version was then changed to `0.1.1`; the same test failed because the manifest still
reported `0.1.0`, and passed after all workspace, internal path-dependency, fuzz dependency, and
manifest versions were aligned.

All 158 workspace tests passed. Formatting, Clippy with warnings denied, doctests, all-feature and
no-default-feature tests, rustdoc warnings, Rust 1.96 MSRV, locked fuzz compilation, boundary
self-tests and live checks, license/source policy, security audits, and unused-dependency checks
passed. `cargo-deny` emitted only the repository's existing unmatched-license and duplicate-version
warnings. No Token Station desktop App was installed as part of the South worktree validation
because this repository change affects only South release metadata, tests, and documentation.

The separate Token Station candidate-validation branch pinned all five crates to
`ef1ab0d2c5ec108835d083c5f6f5b7da510520e4`. Its focused conformance and real-transport tests,
root and Desktop suites, coverage gates, supply-chain checks, Rust 1.96 check, and remote Linux CI
passed. The host validation also completed the required local Desktop App rebuild, audit,
installation, signature check, and launch. Independent review found no open P0, P1, or P2 issue.
