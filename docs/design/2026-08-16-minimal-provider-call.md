# Minimal Provider Call Library Vertical Slice

Status: approved for implementation after specification review

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
south-transport-reqwest ---> south-core ---> south-contracts
south-provider-conformance ----------------> south-contracts
south-testkit -------------> south-core
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
- `south-testkit` depends on conformance and core and owns a public runner that future hosts call
  against their assembled executor. Conformance never depends on testkit.

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
