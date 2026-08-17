# Provider Quota Response Metadata

Status: implemented; fixed-candidate host CI green; final review and release pending

Date: 2026-08-17

## Problem

South provider-call v1 exposes only `content-type` and `retry-after`. The Token Station community
host also reads nine provider response headers to update its host-owned quota ledger:

- `x-ratelimit-limit-tokens`
- `x-ratelimit-remaining-tokens`
- `x-ratelimit-reset-tokens`
- `anthropic-ratelimit-tokens-limit`
- `anthropic-ratelimit-tokens-remaining`
- `anthropic-ratelimit-tokens-reset`
- `anthropic-ratelimit-unified-limit`
- `anthropic-ratelimit-unified-remaining`
- `anthropic-ratelimit-unified-reset`

The explicit community diagnostic can use South today, but production traffic cannot move without
losing those inputs. Returning an arbitrary response-header map would recreate an unbounded and
unreviewed trust boundary, so the missing data must be represented by a closed contract.

This is a cross-repository release slice. South must first publish a fixed candidate. The community
host must then prove that the candidate preserves its existing quota-window output before South can
claim host compatibility and publish the final immutable release.

## Goals

- Add one versioned, closed, named, and bounded contract for exactly the nine approved fields.
- Preserve the existing provider-call and provider-stream APIs for consumers that do not use quota
  metadata.
- Capture valid approved fields in both buffered and headers-ready streaming responses.
- Keep malformed optional quota metadata from turning an otherwise valid provider response into an
  outage.
- Add public conformance evidence for an assembled host adapter carrying all nine fields without
  synthesizing absent fields.
- Prove the community adapter produces the same `WindowSnapshot` values as its legacy response path
  for the same deterministic loopback response.
- Publish the contract and manifest-schema addition as South `0.2.0` only after fixed-candidate
  host CI succeeds.

## Non-goals

- No arbitrary response-header map, string-key lookup, wildcard prefix capture, or provider-defined
  extension field.
- No parsing of numeric limits, remaining values, durations, or timestamps inside South.
- No quota ledger, routing, admission, retry, fallback, billing, receipt, database, cache, or
  persistence ownership in South.
- No community production traffic switch, canary configuration, UI, config schema, or data schema
  change in this slice.
- No new authentication mode, proxy support, request method, provider protocol, or body format.
- No live provider-account request as a reproducible compatibility gate.

## Ownership and data flow

```text
provider response headers
        |
        v
south-transport-reqwest
  - inspect exactly nine names
  - drop malformed optional values
  - never retain unknown headers
        |
        v
ProviderQuotaMetadataV1
  - closed field enum
  - nine read-only named accessors
  - per-value and total byte bounds
        |
        +-------------------------------+
        |                               |
        v                               v
BufferedHttpResponseV1          StreamingResponseHeadV1
        |
        v
community diagnostic adapter
  - restore the same nine lowercase keys
        |
        v
existing host parse_quota_windows / quota ledger
```

South transports bytes and validates the boundary. The host remains the only owner of provider
semantics and quota state. No South crate reads a database, keychain, environment variable, config
file, clock, or quota store as part of this change.

## Public contract

### Version and bounds

`south-contracts` adds:

```rust
pub const PROVIDER_QUOTA_METADATA_CONTRACT_VERSION: u16 = 1;
pub const PROVIDER_QUOTA_METADATA_FIELD_COUNT: usize = 9;
pub const MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES: usize = 256;
pub const MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES: usize =
    PROVIDER_QUOTA_METADATA_FIELD_COUNT * MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES;
```

The value bound matches the existing reviewed `content-type` and `retry-after` metadata bound. A
fixed field count makes the total retained text at most 2,304 bytes, excluding fixed object
overhead. Checked addition still enforces the total bound so the invariant does not depend on the
constant multiplication alone.

The HTTP contract remains version one. This is an additive, independently versioned optional
sub-contract, not a reinterpretation of existing fields.

### Closed field vocabulary

`ProviderQuotaMetadataFieldV1` is a fieldless enum with exactly these variants:

```text
XRateLimitLimitTokens
XRateLimitRemainingTokens
XRateLimitResetTokens
AnthropicRateLimitTokensLimit
AnthropicRateLimitTokensRemaining
AnthropicRateLimitTokensReset
AnthropicRateLimitUnifiedLimit
AnthropicRateLimitUnifiedRemaining
AnthropicRateLimitUnifiedReset
```

The enum may expose the corresponding canonical lowercase header name because every returned name
is a fixed, reviewed literal. It has no parser from an arbitrary string and no `Unknown` variant.

`ProviderQuotaMetadataV1` has private storage and is `Default`-empty. Its public checked constructor
accepts an iterator of `(ProviderQuotaMetadataFieldV1, String)` pairs. It rejects:

- a duplicate enum field;
- a value longer than 256 bytes;
- a value that is not a valid HTTP header value;
- checked total-byte overflow or a total above 2,304 bytes.

The existing `TransportErrorV1::ResponseMetadataInvalid` remains the exact construction error. No
new stable failure code is needed because the public contract already owns that classification.

The type exposes a closed `value(field)` accessor plus nine explicit named accessors mirroring the
canonical header spellings. It exposes no map, mutable reference, unchecked constructor,
deserialization implementation, or arbitrary string-key lookup.

`Debug` reports only contract version, present-field count, and fixed presence booleans. It never
prints metadata values. The type has no `Display` or serialization implementation.

### Buffered and streaming response integration

`BufferedHttpResponseV1` and `StreamingResponseHeadV1` each gain a private
`ProviderQuotaMetadataV1` member and a read-only `provider_quota_metadata()` accessor.

The existing constructors remain source-compatible and create empty quota metadata:

```rust
BufferedHttpResponseV1::try_from_parts(...)
StreamingResponseHeadV1::try_from_parts(...)
```

New explicit constructors accept the bounded metadata object:

```rust
BufferedHttpResponseV1::try_from_parts_with_provider_quota_metadata(...)
StreamingResponseHeadV1::try_from_parts_with_provider_quota_metadata(...)
```

The new methods do not accept nine loose strings or a header map. Redirect, body, UTF-8,
`content-type`, and `retry-after` behavior remains unchanged. Buffered responses remain non-Clone.
Empty quota metadata performs no heap allocation. Present values live behind one immutable shared
allocation, so cloning `StreamingResponseHeadV1` copies only a pointer rather than nine `String`
slots or their values. A size/owner-sharing test prevents the metadata addition from inflating the
public response and error enums.

## Reqwest transport policy

The reqwest transport owns a private fixed table from the nine `http::HeaderName` constants to the
closed contract enum. It inspects the table at headers-ready, before buffering a response body or
opening a byte stream.

Each approved field is independent:

| Wire state | Contract result | Call result |
| --- | --- | --- |
| header absent | field absent | unchanged |
| exactly one UTF-8 value at or below 256 bytes | exact value present | unchanged |
| duplicate values, including identical duplicates | field absent | response continues |
| value above 256 bytes | field absent | response continues |
| non-UTF-8 value | field absent | response continues |

Malformed quota metadata is deliberately fail-soft because it is optional routing evidence. The
legacy community path already ignores unusable quota input, and an untrusted provider must not be
able to turn a valid model response into an outage merely by attaching malformed advisory
metadata. Dropping only the malformed field also lets another complete quota family remain useful.

This policy differs intentionally from `content-type` and `retry-after`, whose existing duplicate
or invalid values remain `RESPONSE_METADATA_INVALID`. Those fields affect response interpretation
and retry policy and are outside this change.

Unknown response headers are never copied. HTTP header matching remains ASCII case-insensitive;
mixed-case spellings map to the same one closed field. Known values are copied once into the bounded
contract; there is no second arbitrary header collection. Buffered and streaming transports use the
same private extraction function so their header semantics cannot drift.

## Conformance extension

The existing `south.provider-call.v1` and `south.provider-stream.v1` suites stay at version one.
Changing either canonical table would invalidate already recorded base-call compatibility for hosts
that have not adopted quota metadata. Instead, South adds a separate extension suite:

```text
suite id: south.provider-quota-metadata.v1
suite version: 1
cases: ALL_FIELDS, NO_FIELDS
```

The `ALL_FIELDS` fixture carries nine distinct, parseable, synthetic static values. `NO_FIELDS`
proves an adapter does not synthesize metadata. Both cases use one valid buffered provider call and
expect one resolver call and one transport call. They do not repeat cancellation, deadline,
redirect, or body-limit cases already frozen by the base provider-call suite.

`south-provider-conformance` owns immutable raw fixtures with private fields and read-only
accessors. `south-testkit` owns an object-safe boxed-`Send` assembled-executor trait, a reference
executor using real `south-core`, and a sequential non-fail-fast runner. Observations are either a
bounded `ProviderQuotaMetadataV1` or a closed `ProviderCallFailureCodeV1`, plus saturated resolver
and transport counts.

The exact mismatch categories are:

```text
OUTCOME_KIND
X_RATELIMIT_LIMIT_TOKENS
X_RATELIMIT_REMAINING_TOKENS
X_RATELIMIT_RESET_TOKENS
ANTHROPIC_RATELIMIT_TOKENS_LIMIT
ANTHROPIC_RATELIMIT_TOKENS_REMAINING
ANTHROPIC_RATELIMIT_TOKENS_RESET
ANTHROPIC_RATELIMIT_UNIFIED_LIMIT
ANTHROPIC_RATELIMIT_UNIFIED_REMAINING
ANTHROPIC_RATELIMIT_UNIFIED_RESET
RESOLVER_CALL_COUNT
TRANSPORT_CALL_COUNT
```

Two cases multiplied by twelve categories bound a complete failure report at 24 entries. Debug
output for fixtures, expected outcomes, observations, mismatches, reports, and failures contains
only fixed enum names, counts, and presence/byte-count summaries, never raw metadata.

A passing runner is necessary but not sufficient for host verification. Review must still confirm
that the host executor invokes its real adapter and reports counts at the real resolver and
transport boundaries.

## Community host integration

The Token Station community repository consumes a fixed South `0.2.0` candidate in a pull request
targeting `develop-v2`.

Its existing non-test diagnostic adapter function `execute_prepared_provider_call_v1` copies the
nine present values from `provider_quota_metadata()` into `HttpResponseParts.headers` using the
same canonical lowercase names expected by `parse_quota_windows`. It continues to copy
`content-type` and `retry-after` exactly as before and does not expose any other response header.

Host tests must prove:

1. the public `south.provider-quota-metadata.v1` suite passes through the real adapter wrapper;
2. two independent executions of the same immutable loopback response fixture, containing all
   three complete quota families, produce the exact same ordered `Vec<WindowSnapshot>` through
   legacy and South paths at one injected `now_ms`; this is test-only comparison, never production
   double-send or replay;
3. absent, partial, duplicate, oversized, and non-UTF-8 approved headers do not synthesize a quota
   window or fail an otherwise valid response;
4. unknown response headers never appear in the South-adapted `HttpResponseParts`;
5. response, metadata, endpoint, path, slot, request, and fake-secret sentinels remain absent from
   `Debug`, `Display`, and stable errors;
6. the South attempt count remains exactly one and no legacy replay is introduced.

This slice still does not call `note_authoritative` from a production South path and does not alter
the quota database. It only removes the metadata-loss blocker. Production canary wiring remains a
separate host pull request.

Because the host adapter under `apps/cli` changes executable code, the host pull request must run
the repository's complete Rust/Desktop gates and `scripts/install-local-desktop.sh`, then verify
the installed bundle identifier, signature, and launched process.

## Compatibility manifest and release

The compatibility manifest schema moves to version two because strict readers must be able to
distinguish the new required contract, limits, suite, and host capability fields. It adds:

```text
contracts.provider_quota_metadata = 1
contracts.provider_quota_metadata_limits.field_count = 9
contracts.provider_quota_metadata_limits.value_bytes = 256
contracts.provider_quota_metadata_limits.total_bytes = 2304
conformance.provider_quota_metadata_suite_id = south.provider-quota-metadata.v1
conformance.provider_quota_metadata_suite = 1
host_capabilities.<host>.provider_quota_metadata = verified | not_verified
```

The existing legacy host summary continues to mirror only `provider_call`; the new capability does
not downgrade previously verified base-call integration. `token-station-server` starts
`provider_quota_metadata=not_verified`. Token Station becomes `verified` only after its fixed
candidate commit and public CI evidence are recorded.

South moves every workspace package and exact internal dependency from `0.1.1` to `0.2.0`, updates
the nested fuzz workspace, and publishes immutable tag `v0.2.0`. The Rust API is additive, old
constructors keep their behavior, base conformance suites do not change, and malformed optional
quota metadata cannot create a new call failure. Nevertheless, a pre-1.0 minor release is required:
`compatibility.json` is a binding public artifact and its schema changes from one to two. Calling
that schema change a `0.1.2` patch would understate compatibility risk for strict manifest readers.

## Test-driven implementation order

1. Add the English design record only.
2. Add public contract tests for exact vocabulary, bounds, duplicates, accessors, old-constructor
   compatibility, and redacted diagnostics; observe unresolved-import or missing-method RED.
3. Implement the minimal contract and observe contract/property tests GREEN.
4. Extend the parser fuzz target and add fixed-seed property invariants.
5. Add buffered and streaming reqwest loopback tests for all fields, absence, duplicate,
   over-limit, non-UTF-8, unknown-header exclusion, and shared semantics; observe RED before
   transport implementation.
6. Implement one private extraction path and observe transport tests GREEN.
7. Add conformance fixture/runner/reference tests, including one-mismatch isolation and report
   bounds; observe RED before implementation.
8. Implement the extension suite and update compatibility tests, manifest schema, README,
   architecture, versions, and fuzz lockfile.
9. Run every South verification command in the root README and obtain independent review.
10. Push the fixed South candidate without tagging it.
11. In Token Station, change dependency policy first and observe RED on `0.1.1`; then pin all five
    crates and both lockfiles to exact `=0.2.0` plus the candidate commit.
12. Add host conformance and legacy-equivalence tests before the adapter projection change, observe
    RED, then implement the nine-field projection and observe GREEN.
13. Run Token Station root/Desktop/supply-chain gates, install the local App, push a public PR to
    `develop-v2`, and require its Linux CI to pass. macOS/Windows Platform Gates are not required
    for this non-`main` pull request under the host's current policy.
14. Record the immutable candidate, host validation commit, host pull request, and successful CI in
    this design, then change only `compatibility.json` from
    `token-station/provider_quota_metadata=not_verified` to `verified`. The compatibility test must
    validate allowed status vocabulary and summary relationships generically rather than require a
    second Rust-source edit for that evidence transition.
15. From the host-tested candidate to the final South merge, permit changes only to this evidence
    design and `compatibility.json`. Rust library sources, Cargo manifests, lockfiles, conformance
    fixtures, runtime tests, and compatibility schema/limits/suite identifiers must be byte-for-byte
    unchanged. Merge South to `main`, require post-merge South CI success, create immutable `v0.2.0`,
    update the host dependency from candidate to final merge SHA, rerun host CI, and merge the host
    PR to `develop-v2`.

## Acceptance

South is ready for its candidate validation when:

- all nine fields are closed, named, bounded, independently optional, and value-redacted;
- old response constructors and both base conformance suites remain source- and behavior-compatible;
- buffered and streaming transports share exact extraction semantics;
- malformed optional quota metadata is omitted without failing the provider response;
- unknown response headers are never retained;
- the extension conformance runner proves all-fields and no-fields behavior through an assembled
  executor;
- compatibility schema/version/crate versions/fuzz lock agree on `0.2.0` and the new contract;
- every South README gate passes with no open P0 or P1 finding.

The release is complete only when:

- the fixed candidate passes the community host's public conformance, parity, Linux CI, and local
  App verification;
- the evidence and exact candidate/final invariant are recorded;
- the South pull request merges, post-merge CI succeeds, and immutable `v0.2.0` is published;
- the community host pins the final merge SHA, reruns its gates, and merges to `develop-v2`.

Even then, community production traffic remains disabled. The next slice is an explicit
non-streaming canary that preserves routing, fallback, health, quota, receipt, cancellation, and
user-visible error semantics under host ownership.

## Fixed-candidate host evidence

The immutable South `0.2.0` candidate and its community-host validation are:

- South candidate commit: `b1f7aa21362a5332fd6f2c5b9de8f6407ff39e4e`;
- Token Station validation commit: `923cb386939ce0ff2f67b86894918fabe5f021f6`;
- Token Station pull request: [GlimpseEngine/token-station#93](https://github.com/GlimpseEngine/token-station/pull/93),
  targeting `develop-v2` and kept draft until the final South tag exists;
- Token Station CI run: [32021500390](https://github.com/GlimpseEngine/token-station/actions/runs/32021500390),
  successful for root Rust, Desktop Rust, Desktop coverage, frontend, Rust 1.96, supply-chain, and
  release gates; root coverage was skipped by the host workflow's existing pull-request policy;
- South candidate CI run: [32016637890](https://github.com/ballast-ai/token-station-south/actions/runs/32016637890),
  successful for the required quality job.

The host pinned all five South packages to exact `=0.2.0` and the candidate revision. Its real
community adapter passed the seven-case base provider-call suite and the two-case quota-metadata
suite. Independent loopback executions produced the same three ordered host `WindowSnapshot`
values through the legacy and South paths with exactly one request per execution. Additional real
reqwest loopbacks proved that absent, duplicate, oversized, and non-UTF-8 approved fields remain
fail-soft, valid siblings survive, partial families do not create windows, and unknown headers are
not projected. The host's full root and Desktop gates, supply-chain checks, frontend tests and
build, local Desktop App build, artifact audit, signature check, installation, and launch passed.
Independent host review reported zero P0, P1, or P2 findings.

This evidence changes only this design record and the one
`host_capabilities.token-station.provider_quota_metadata` value in `compatibility.json`. From the
host-tested candidate to the final South merge, every Rust source, Cargo manifest, lockfile,
fixture, test, contract version, limit, suite identifier, and package artifact must remain
byte-for-byte unchanged. `token-station-server/provider_quota_metadata` remains `not_verified`, and
community production traffic remains disabled.
