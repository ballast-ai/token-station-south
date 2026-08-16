# Minimal Provider Call Library Vertical Slice

Status: implemented locally; remote CI pending; hosts not verified

Date: 2026-08-16

## Problem and decision

The repository bootstrap proves engineering boundaries but does not execute a provider request.
The next slice must prove a real host-neutral call path without pretending that streaming, provider
components, or host migration are complete.

The enterprise host is the migration-critical consumer and already pins reqwest 0.12 in its
asynchronous upstream stack. The first transport is therefore `south-transport-reqwest`, not ureq,
and it pins the same reqwest line. This aligns the library with the critical consumer but does not
claim that the enterprise host is integrated or verified. The slice is limited to
one buffered JSON POST with host-bound endpoint and Bearer authentication. It owns the first
versioned Rust HTTP, authentication, and error contracts. It does not define a stable WIT or JSON
wire format.

## Goals

1. Define bounded provider-facing request and response types in `south-contracts`.
2. Prove that a provider can select only a host-bound relative path and credential slot, never an
   arbitrary credential value or absolute destination.
3. Make endpoint validation happen before credential resolution and make transport construction
   impossible without that preparation step.
4. Execute one real asynchronous reqwest request with redirects, retries, compression, cookies,
   referer propagation, and implicit system proxies disabled.
5. Return bounded UTF-8 response bodies and minimal safe response metadata without reflecting
   credentials or arbitrary upstream headers to a provider.
6. Publish a reusable conformance fixture and runner for future host consumption.

## Non-goals

- Chat, embedding, image, audio, video, or other model-semantic Canonical IR.
- Streaming or SSE, multipart, binary responses, uploads, or downloads.
- OAuth exchange, arbitrary authentication headers, SigV4, self-signed JWT, or query credentials.
- Provider WIT, component ABI, Wasmtime runtime, or plugin loading.
- Retry, fallback, routing, billing, quota, audit, task persistence, or provider task polling.
- Full DNS rebinding or private-network SSRF prevention inside the library.
- The ureq transport or a blocking-to-async bridge.
- Database, cache, configuration, environment, keychain, Vault, or secret-store access.

## Ownership and dependency direction

```text
south-core ----------------> south-contracts
south-transport-reqwest ---> south-core
south-transport-reqwest ---> south-contracts
south-provider-conformance -> south-contracts
south-testkit -------------> south-core
south-testkit -------------> south-contracts
south-testkit -------------> south-provider-conformance
future hosts --------------> south-testkit
```

- `south-contracts` owns provider-visible values and hard limits. It contains no I/O or plaintext
  credential type.
- `south-core` owns endpoint/credential binding, credential resolution capabilities, cancellation,
  and the sealed prepared-request typestate.
- `south-transport-reqwest` owns reqwest configuration and error classification. No reqwest type
  crosses its public boundary.
- `south-provider-conformance` owns versioned fixtures.
- `south-testkit` depends on conformance, core, and contracts and owns a public runner that future
  hosts call against their assembled executor. Conformance never depends on testkit.

## Versioned contract

The slice introduces independent version constants:

```text
HTTP_CONTRACT_VERSION = 1
AUTH_CONTRACT_VERSION = 1
ERROR_CONTRACT_VERSION = 1
STREAM_CONTRACT_VERSION = null
```

Version one is a Rust API contract only. Types do not implement serde in this slice. WIT and wire
compatibility remain null in `compatibility.json`.

### Request

`JsonPostRequestV1` contains:

- a validated relative path with no scheme, authority, userinfo, fragment, query, dot segment,
  backslash, repeated slash, or percent-encoded slash/backslash;
- existing `SafeHeaders` ordinary headers;
- a validated, bounded JSON body;
- a Bearer `CredentialSlotV1`, which is an identifier and never a credential value.

The existing `SafeHeaders` gains only a read-only iterator for transport assembly; its storage,
mutation, and deserialization remain closed.

`CredentialSlotV1` is 1 to 64 ASCII bytes. Its first byte is `a-z`; remaining bytes are `a-z`,
`0-9`, `.`, `_`, or `-`. `JsonBodyV1` stores the exact UTF-8 text supplied after serde_json proves
that it contains one complete JSON value within the byte limit. It does not normalize key order or
numbers and does not implement serde in this slice. Its custom `Debug` prints only the byte count.

The endpoint is not provider-facing. A trusted host creates `ProviderBindingV1` from a validated
base endpoint and the one credential slot authorized for that endpoint. `south-core` joins the
relative path to that base only after proving the requested slot matches the host binding.

Version-one limits:

| Field | Limit |
| --- | ---: |
| Base endpoint | 8 KiB |
| Relative path | 2 KiB |
| Credential slot | 64 ASCII bytes |
| JSON request body | 32 MiB |
| Ordinary request headers | Existing policy: 64 entries and 64 KiB total |
| UTF-8 response body | 32 MiB |
| Response `content-type` | 256 bytes |
| Response `retry-after` | 256 bytes |

`ProviderEndpointV1` accepts only absolute `http` or `https` URLs with authority and no userinfo,
query, or fragment. Its path is normalized to a trailing-slash prefix. HTTP remains representable
because local providers and deterministic loopback tests need it; the trusted host decides which
endpoint is authorized. South does not claim that URL validation alone prevents DNS rebinding.
Production deployment still needs host policy and network-layer egress controls.

Relative paths are non-empty ASCII, never start with `/`, and contain no query or fragment. The
validator rejects empty or dot segments, repeated slashes, backslashes, control/space bytes,
scheme-like first segments, and case-insensitive percent encodings of `.`, `/`, `\\`, or `%`
(`%2e`, `%2f`, `%5c`, `%25`). The endpoint path receives the same segment checks. South joins by
appending the validated relative path to the normalized endpoint prefix, parses the result with the
`url` crate, then rechecks exact scheme, normalized host, effective port, and segment-aware base
path prefix before resolving a credential. No Bearer value exists during URL processing.

### Authentication

Provider-facing code sees only `CredentialSlotV1` and `BearerAuthV1`. A host supplies an async
`CredentialResolver: Send + Sync` capability. Its returned future is `Send`, borrows the resolver
and slot for the call, and produces `Result<SecretValue, CredentialResolutionErrorV1>`. Resolution
is a read-only operation and its future must be cancellation-safe when dropped. Resolution returns
`SecretValue`, which:

- is not `Clone`, `Display`, or serializable;
- has a redacted `Debug` implementation;
- zeroizes its owned bytes on drop;
- is exposed only to the prepared-request/transport boundary.

This supersedes the earlier cross-repository draft that passed a resolved `(header, value)` pair
into South. No plaintext header pair is a public provider-call contract.

The caller injects a `tokio::time::Instant` absolute deadline and a `CancellationToken`. The public
execution function races cancellation and `timeout_at(deadline, inner_execution)` around the whole
inner future, including credential resolution and transport, and performs this order:

1. validate the relative request against the host binding;
2. compare the requested credential slot with the bound slot;
3. check preflight cancellation and deadline;
4. resolve the credential within the same cancellation/deadline scope;
5. construct a private-field `PreparedHttpRequestV1`;
6. invoke the injected async transport.

There is no public constructor for `PreparedHttpRequestV1`. Transport implementations can read its
bounded fields but cannot receive an unprepared provider request.

### Response

`BufferedHttpResponseV1` exposes only:

- the upstream status code;
- a bounded UTF-8 body;
- bounded `content-type` and `retry-after` metadata.

It does not expose a generic response-header map. This prevents an upstream from reflecting
`authorization`, cookies, API keys, proxy credentials, or hop-by-hop headers back to a provider.
A future requirement for another response header must add a named, bounded field with a security
review.

HTTP 4xx and 5xx responses are successful transport outcomes so a provider adapter can interpret
the upstream body. HTTP 3xx is `REDIRECT_DENIED`; no second request is sent.

### Errors

Contract, preparation, and transport failures use exhaustive `thiserror` enums with stable
`SCREAMING_SNAKE_CASE` codes. Version one has this exact code set:

| Layer | Codes |
| --- | --- |
| Contract | `INVALID_ENDPOINT`, `INVALID_RELATIVE_PATH`, `INVALID_CREDENTIAL_SLOT`, `INVALID_JSON_BODY`, `REQUEST_BODY_TOO_LARGE` |
| Preparation | `URL_OUTSIDE_BINDING`, `CREDENTIAL_BINDING_MISMATCH`, `CREDENTIAL_RESOLUTION_FAILED`, `CANCELLED`, `DEADLINE_EXCEEDED` |
| Transport | `CLIENT_BUILD_FAILED`, `TRANSPORT_TIMEOUT`, `CONNECT_FAILED`, `REQUEST_FAILED`, `RESPONSE_READ_FAILED`, `RESPONSE_BODY_TOO_LARGE`, `RESPONSE_BODY_NOT_UTF8`, `RESPONSE_METADATA_INVALID`, `REDIRECT_DENIED` |

Errors and `Debug` output never contain an endpoint, relative path, query, header name/value,
credential slot/value, request body, response body, or raw reqwest error. South reports upstream
status as upstream fact only; each host owns its client-facing status mapping.

Body-bearing request and response types use custom redacted `Debug` implementations that expose
only contract versions and byte counts. Reqwest's source chain is classified into the stable
categories it exposes reliably: timeout, connect, request write, and response read. Version one
does not pretend it can reliably distinguish DNS from TLS failures through reqwest's stable API.
Any reqwest total, connect, or per-read timeout that fires before the caller's absolute deadline is
`TRANSPORT_TIMEOUT`; only the outer injected absolute deadline is `DEADLINE_EXCEEDED`.

## Async transport behavior

`AsyncHttpTransport` is an injected port defined by `south-core`. Its future is cancellation-safe:
`south-core` selects between the injected cancellation token, absolute deadline, resolver future,
and transport future without spawning or detaching work. Cancellation or deadline while a resolver
is pending returns the stable code and never invokes transport. Dropping the reqwest future cancels
the in-flight request; no `spawn_blocking` bridge exists. Tokio's paused clock is used in deadline
tests, so no wall-clock sleep is required.

`south-transport-reqwest` builds a dedicated client with:

- reqwest `0.12.28`, matching the enterprise host's direct dependency line, with default features
  disabled and only `rustls-tls` and `stream` enabled;
- `no_proxy()` and no `system-proxy` feature;
- `redirect(Policy::none())`;
- `retry(reqwest::retry::never())`;
- automatic gzip, Brotli, deflate, and zstd decoding disabled;
- cookies disabled and automatic referer disabled;
- explicit total, connect, and per-read timeouts;
- per-request timeout bounded by the caller's explicit execution timeout.

Response bodies are read incrementally to `MAX_RESPONSE_BODY_BYTES + 1`; exceeding the limit is an
error rather than silent truncation. The transport never calls `error_for_status`, so 4xx and 5xx
bodies remain available.

## Conformance and testkit API version one

This is a test API for checking an assembled provider-call path. It is not a production host
adapter contract, does not expose reqwest, and does not make a host `verified`. A host becomes
verified only in its separate adoption slice after its real adapter runs this suite and its wiring
is reviewed.

`south-provider-conformance` depends only on `south-contracts`. It exports the immutable fixture
table and these stable identifiers:

```text
PROVIDER_CALL_CONFORMANCE_SUITE_VERSION: u32 = 1
PROVIDER_CALL_CONFORMANCE_SUITE_ID: &str = "south.provider-call.v1"
PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1: Duration = Duration::from_secs(1)
```

The table contains exactly these `ProviderCallCaseIdV1` values:

```text
SUCCESS
INVALID_RELATIVE_PATH
CREDENTIAL_SLOT_MISMATCH
REDIRECT_DENIED
RESPONSE_BODY_TOO_LARGE
CANCELLED
DEADLINE_EXCEEDED
```

`provider_call_fixtures_v1()` returns `&'static [ProviderCallFixtureV1]`. Fixtures and their nested
values have private fields and read-only accessors; callers cannot mutate or extend the canonical
table. Each fixture contains:

- `case_id: ProviderCallCaseIdV1`;
- `input: ProviderCallInputV1`, with raw `&'static str` endpoint, bound credential slot, requested
  credential slot, relative path, JSON body, and a borrowed static slice of ordinary header-name
  and header-value pairs;
- `control: ProviderCallControlV1`, one of `Complete`, `CancelWhileResolverPending`, or
  `ExpireWhileTransportPending`;
- `upstream: ProviderCallUpstreamV1`, one of `Response(ProviderCallRawResponseV1)`,
  `TransportFailure(TransportErrorV1)`, `Pending`, or `NotReached`;
- `expected: ProviderCallExpectedV1`, containing an expected outcome and expected evidence.

The canonical table freezes the only legal control/upstream combinations:

| Case ID | Control | Upstream |
| --- | --- | --- |
| `SUCCESS` | `Complete` | `Response` |
| `INVALID_RELATIVE_PATH` | `Complete` | `NotReached` |
| `CREDENTIAL_SLOT_MISMATCH` | `Complete` | `NotReached` |
| `REDIRECT_DENIED` | `Complete` | `TransportFailure(REDIRECT_DENIED)` |
| `RESPONSE_BODY_TOO_LARGE` | `Complete` | `TransportFailure(RESPONSE_BODY_TOO_LARGE)` |
| `CANCELLED` | `CancelWhileResolverPending` | `NotReached` |
| `DEADLINE_EXCEEDED` | `ExpireWhileTransportPending` | `Pending` |

`Response` and `TransportFailure` are legal only with `Complete`; `Pending` is legal only for the
deadline case. Fixtures have no public constructor, and table tests assert that no other pairing is
published. `ExpireWhileTransportPending` means the executor uses an absolute deadline exactly
`PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1` after that case starts.

`ProviderCallRawResponseV1` holds a `u16` status plus borrowed static body, `content-type`, and
`retry-after` values, so raw upstream responses own no allocation. Metadata fields are
`Option<&'static str>`. The success body, present metadata, and every raw input remain within the
version-one contract limits. The oversized-response case is represented by
`TransportErrorV1::ResponseBodyTooLarge`; the fixture does not embed a 32 MiB allocation. The only
Bearer value in the package is the documented literal `FAKE_BEARER_SECRET_V1`; it is synthetic test
data, not configurable host data. A fixture never contains a resolved `SecretValue`, and neither
the runner nor its report retains the fake value. Table tests parse and bound every canonical raw
field through its production contract, except that the one deliberately invalid relative path must
fail with `INVALID_RELATIVE_PATH` while remaining within the raw byte limit.

`ProviderCallExpectedOutcomeV1` is either
`Response { status, body, content_type, retry_after }` or `Failure { code }`. Body and present
metadata values are borrowed static strings. The runner compares status, body bytes,
`content-type`, and `retry-after` exactly, including `None` versus `Some`.

Failure codes are not arbitrary strings. `ProviderCallFailureCodeV1` is a closed, fieldless enum
with exactly one variant for every contract, preparation, and transport code frozen in the Errors
table above. Its `as_str()` returns that stable `&'static str`; there is no `Unknown` or constructor
from an unchecked string. Expected and observed failures both use this enum.

Expected and observed call counts use the same closed `ProviderCallCountV1` enum: `Zero`, `One`, or
`MoreThanOne`. `ProviderCallCountV1::from_usize` maps zero and one exactly and saturates every value
of two or greater to `MoreThanOne`; it never narrows to an integer field. Boundary tests prove 256
and 257 both remain `MoreThanOne` and therefore cannot wrap into an expected zero or one. Expected
evidence also contains `resolver_future_dropped_while_pending` and
`transport_future_dropped_while_pending` booleans. `ProviderCallExpectedV1` stores that data in a
private-field `ProviderCallExpectedEvidenceV1`; its accessors return only the shared count enum and
the two booleans.

`south-testkit` depends on `south-core`, `south-provider-conformance`, and `south-contracts`. It owns
the object-safe assembled-executor boundary:

```rust
pub type AssembledExecutionFutureV1<'a> = Pin<Box<
    dyn Future<Output = ProviderCallObservationV1> + Send + 'a,
>>;

pub trait AssembledProviderCallExecutorV1: Send + Sync {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderCallFixtureV1,
    ) -> AssembledExecutionFutureV1<'a>;
}
```

The lifetime binds only the executor and fixture borrows to the boxed `Send` future. The trait has
no associated types, generic return type, reqwest type, or raw-transport parameter and can be used
as `dyn AssembledProviderCallExecutorV1`. A future host implements this test trait around its fully
assembled call path; the runner never accepts `AsyncHttpTransport` directly.

External implementations construct observations only through these public functions:

```rust
ProviderCallEvidenceV1::new(
    resolver_calls: usize,
    transport_calls: usize,
    resolver_future_dropped_while_pending: bool,
    transport_future_dropped_while_pending: bool,
) -> ProviderCallEvidenceV1

ProviderCallObservationV1::response(
    response: BufferedHttpResponseV1,
    evidence: ProviderCallEvidenceV1,
) -> ProviderCallObservationV1

ProviderCallObservationV1::failure(
    code: ProviderCallFailureCodeV1,
    evidence: ProviderCallEvidenceV1,
) -> ProviderCallObservationV1
```

`ProviderCallEvidenceV1::new` converts both `usize` values with the saturating
`ProviderCallCountV1::from_usize`. The response constructor accepts only the already bounded
contract response; the failure constructor accepts only the closed known-code enum. Observation
and evidence fields stay private and have read-only accessors. The runner compares the response
fields or failure code exactly, then compares both count categories and both pending-drop booleans.

No data-bearing public conformance or testkit type derives `Debug` or implements `Display`.
`ProviderCallInputV1` prints only byte lengths and header count. `ProviderCallRawResponseV1` and
response-shaped expected or observed outcomes print status, body byte count, metadata presence,
and metadata byte counts. `ProviderCallFixtureV1` prints case ID and control plus those redacted
nested summaries. `ProviderCallUpstreamV1` prints only its fixed variant and, for a transport
failure, the closed error code. Expected evidence, observed evidence, reports, and failures print
only count categories and booleans. Closed case, control, failure-code, count, and mismatch enums
use custom `Debug` that prints only their fixed variant names. No `Debug` output prints bodies,
metadata text, headers, endpoints, paths, credential slots, or either Bearer value.

`pub async fn run_provider_call_conformance_v1(&dyn AssembledProviderCallExecutorV1)` executes all
seven cases sequentially in canonical table order and returns
`Result<ProviderCallConformanceReportV1, ProviderCallConformanceFailureV1>`. It does not fail fast.
`Ok` means every case matched and reports the suite ID, version, and seven passed cases. `Err` owns
the complete bounded list of case ID and `ProviderCallMismatchCategoryV1` pairs, without copying
expected or actual sensitive values. It emits each category at most once per case, so the failure
contains at most 70 pairs. The exact mismatch categories are:

```text
OUTCOME_KIND
ERROR_CODE
STATUS
BODY
CONTENT_TYPE
RETRY_AFTER
RESOLVER_CALL_COUNT
TRANSPORT_CALL_COUNT
RESOLVER_PENDING_DROP
TRANSPORT_PENDING_DROP
```

The testkit includes a reference assembled executor that uses real `south-core` orchestration with
fake resolver and transport capabilities. It parses raw fixture input before calling core, so the
invalid-path case returns `INVALID_RELATIVE_PATH` before core, resolution, or transport. The valid
wrong-slot case reaches core and proves both call counts remain zero. Success uses an immediate fake
resolver and recording transport. Redirect and oversize cases are returned by the fake transport
as their frozen transport errors. Cancellation holds the resolver future pending, synchronizes its
start, cancels the token, and records its drop. Deadline uses an immediate resolver and a `Pending`
fake transport.

Reference deadline tests use
`#[tokio::test(flavor = "current_thread", start_paused = true)]`. The executor neither pauses nor
advances Tokio time. The test waits on a channel or barrier proving the deadline-case transport has
started, advances by `PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1`, and then observes transport
future drop. This remains compatible with Tokio's current-thread runtime and uses no wall-clock
sleep.

Call counts and drop flags are adapter-reported evidence. An opaque executor cannot be independently
instrumented by the runner, so a passing report alone is insufficient for host verification. The
separate host-adoption review must inspect that these values are wired to the real adapter's
resolver and transport boundaries and drop guards before changing that host to `verified`.

An incorrect executor can return a permanently pending future. The runner deliberately adds no
clock abstraction or internal watchdog; every caller must wrap the entire run and its deadline
driver in an outer Tokio timeout. The reference invocation is:

```rust
#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn assembled_provider_call_conforms() {
    let executor = reference_executor();
    let deadline_driver = async {
        executor.deadline_transport_started().await;
        tokio::time::advance(PROVIDER_CALL_CONFORMANCE_DEADLINE_OFFSET_V1).await;
    };

    let structured_run = async {
        let (result, ()) = tokio::join!(
            run_provider_call_conformance_v1(&executor),
            deadline_driver,
        );
        result
    };
    let result = tokio::time::timeout(Duration::from_secs(5), structured_run)
        .await
        .expect("conformance watchdog expired");
    assert!(result.is_ok());
}
```

The watchdog covers failure to reach the transport-start signal as well as failure to finish after
the deadline advance. `tokio::join!` keeps the runner and deadline driver in one structured future;
there is no spawned task or `JoinHandle`. If the watchdog expires, dropping `structured_run` drops
the entire future tree, including the assembled executor call, without detached work. Reference
tests otherwise use channels or barriers, never sleeps, public DNS, environment mutation, or a real
credential.

Neither package adds serde, a wire format, WIT, a provider runtime, or a generic fixture framework.
The dependency direction remains one-way: conformance to contracts, testkit to conformance and
core, and future host test code to testkit.

## Public behavior tests

Every behavior is introduced test-first and observed failing before production implementation.
Tests use no public network, DNS, process environment mutation, shared state, or sleeps.

1. Contract tests reject invalid endpoints, paths, credential slots, invalid JSON, and body limits.
2. Core tests prove endpoint/slot validation precedes credential resolution.
3. Core tests prove an already-cancelled call invokes neither resolver nor transport.
4. Core tests prove cancellation and deadline while the resolver is pending drop that future and
   never invoke transport.
5. Core tests prove a recording transport receives exactly one prepared POST and returns its result.
6. A loopback server on `127.0.0.1:0` verifies method, path, ordinary headers, Bearer injection, and
   body for a real reqwest call.
7. Loopback tests prove 201, 429, and 500 responses preserve status/body and the two allowed metadata
   fields.
8. A redirect fixture proves no second connection occurs and returns `REDIRECT_DENIED`.
9. A chunked or streamed body larger than the limit returns `RESPONSE_BODY_TOO_LARGE`.
10. Cancellation is synchronized with channels or barriers and never with sleeps.
11. A reqwest timeout that fires before the injected absolute deadline returns
    `TRANSPORT_TIMEOUT`, while the outer deadline returns `DEADLINE_EXCEEDED`.
12. URL tests cover a leading slash, mixed-case encoded dot segments, encoded separators, double
    encoding, a scheme-like first segment, and the post-join origin/base-prefix assertion.
13. Redaction tests place unique sentinels in endpoint, path, slot, ordinary header, secret, request
    body, response body, metadata, and a synthetic transport source; none appear in `Display` or
    `Debug` outside the explicitly readable non-secret value accessors.
14. The public conformance runner accepts an assembled provider-call executor, not a raw transport.
    Its suite covers success, invalid path/slot before resolution, redirect denial, oversized
    response, cancellation, and deadline.

## Implementation tasks

1. Add the contract types and their failing public behavior tests in `south-contracts`.
2. Add preparation, secret handling, async transport port, cancellation, and recording-transport
   tests in `south-core`.
3. Add the hardened reqwest implementation and deterministic loopback integration tests in
   `south-transport-reqwest`.
4. Add the versioned fixture and reusable runner in `south-provider-conformance` and
   `south-testkit`; update compatibility metadata and repository documentation.
5. Run all repository gates, independent specification review, independent code-quality review,
   and remote CI before changing either host compatibility status.

## Implementation and verification result

As of 2026-08-17, the contracts, core orchestration, hardened reqwest transport, immutable
conformance fixtures, assembled-executor runner, parser fuzz target, and dependency-boundary
self-tests described by this design are implemented on the feature branch.

Local verification passed with 99 workspace tests. Formatting, clippy with warnings denied,
all-feature and no-default-feature tests, rustdoc warnings, the Rust 1.96.0 MSRV check, locked fuzz
target compilation, boundary live checks and self-tests, dependency policy checks, vulnerability
audits, and unused-dependency checks also passed. Independent review found no open P0 or P1
findings. Remote CI has not yet run for this feature branch, and both hosts remain `not_verified`.

This result proves the library slice only. It does not prove enterprise migration or production
host adoption. No desktop App was installed because this repository does not change Token Station
desktop behavior.

## Acceptance and release status

The library slice is complete only when all local gates and remote CI pass and the final reviews have no
open critical or important findings. `compatibility.json` may report HTTP/Auth/Error contract
version one and the reqwest transport as implemented. Streaming, WIT, provider runtime, ureq, and
both real hosts remain null, placeholder, or `not_verified` until their own public acceptance gates
pass.

The next separate host-adoption slice must select one real enterprise Bearer JSON POST call site,
compile its adapter against South, and run this assembled-executor conformance suite. Until that
happens, this work is described as reqwest-aligned library infrastructure, not enterprise migration
readiness.

This slice changes no Token Station desktop behavior. It does not require a local desktop App
reinstall.
