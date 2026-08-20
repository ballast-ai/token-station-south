# Host-Signed Auth: The Request Finalizer Seam

Status: designed; decision point 8 of the target-architecture plan and D1–D5 below ruled
2026-08-20. Ships after the host-prelude release (0.7.0) as its own slice (version assigned at
ship time — the 2026-08-21 ruling released 0.9.0 to the S2 conformance slice and retired
pre-allocation; renumbered
2026-08-20 — 0.6.0 was consumed by the controlled user-agent release, 0.8.0 is provider-api).

Date: 2026-08-20

Predecessors: `2026-08-16-minimal-provider-call.md` (assemble / transport split),
`2026-08-17-header-secret-auth.md` (the reserved-header tension and the frozen-enum discipline
this slice reuses), `2026-08-20-host-prelude.md` (D2: `RawAuthV1` and `ProviderAuthV1` become
`#[non_exhaustive]` in 0.7.0 so this slice is additive).

## 1. Problem

The auth contract knows two shapes, and both share one property: the credential is a value that
exists *before* the request does. The host resolves it, `assemble` binds it to one header, the
transport sends it. Signature-based authentication — AWS SigV4 for Bedrock and Kiro today — breaks
that property. The signature is computed **over** the final request: method, host, path, canonical
query, a chosen set of headers, and the SHA-256 of the exact body bytes. Nothing can be signed
until everything else is final, and nothing may change after it is signed.

Three consequences, each of which rules out the obvious fix:

1. **It cannot be front-loaded.** The target-architecture plan's original invariant ("dynamic auth
   stays host-side and runs first") is correct for minting and refresh, whose product is a Bearer
   token, and wrong for signing. A host that signs before South resolves the path against the
   binding, appends the sanctioned query, and hands the body to the transport is signing bytes
   South has not produced yet.
2. **It cannot be a plain header.** `authorization` and `host` sit on the reserved-header
   blacklist on purpose; `SafeHeaders` rejects them with `RESERVED_HEADER_FORBIDDEN`. The Bearer
   arm hard-codes the `Bearer ` prefix. There is no legal channel for `AWS4-HMAC-SHA256 …`.
3. **It cannot be just a third enum arm.** An `AuthV2::SigV4` variant that `assemble` handles
   like the other two would still sign at assemble time, before the transport fixes
   `content-length` and `host`, and would pull AWS canonicalisation into South.

So the shape is not an auth *value*; it is an auth *step* with a fixed position in the pipeline.

## 2. Boundary claim

Shared (this slice): a third declarative auth arm that names *which headers the host will emit*;
one host-injected seam positioned after `assemble` and before `transport.send`; the allow-list
diff South runs on what the seam returns; the transport's byte promise; a conformance suite with
a deterministic fake signer.

Host-owned (explicitly out of scope): every byte of SigV4 (canonical request, string-to-sign,
signing key derivation, STS session tokens), key storage and rotation, region / service
selection. South ships no AWS code and never sees a signing key.

The credential invariant gets *stronger*, not weaker: for this arm the plaintext never crosses
into South at all (see D2).

## 3. Contract design (`south-contracts`)

### 3.1 The arm

```rust
/// Frozen set of headers a host finalizer is permitted to emit. Fieldless, closed.
pub enum SignedHeaderV1 {
    Authorization,        // "authorization"
    XAmzDate,             // "x-amz-date"
    XAmzContentSha256,    // "x-amz-content-sha256"
    XAmzSecurityToken,    // "x-amz-security-token"
}

/// The ordered, duplicate-free set of headers a `HostSigned` declaration promises to emit.
pub struct SignedHeaderSetV1 { /* private; validated constructor */ }

#[non_exhaustive]            // from 0.7.0 (host-prelude D2)
pub enum ProviderAuthV1 {
    Bearer(BearerAuthV1),
    HeaderSecret { header: SecretHeaderV1, slot: BearerAuthV1 },
    /// The host signs the finalised request and emits exactly the declared headers.
    HostSigned { slot: BearerAuthV1, emits: SignedHeaderSetV1 },
}
```

Ruling embedded here (**D1**): `SignedHeaderV1` is a **frozen enum**, the same discipline as
`SecretHeaderV1`. A unit test asserts every variant's name is on `RESERVED_HEADERS`, so the plain
channel can never carry one. Adding a variant is a contract bump with a conformance case.

`slot` keeps the binding check identical to the other arms (`credential_slot()` still answers, the
`CredentialBindingMismatch` path is unchanged). What the slot *means* for this arm is "the signing
identity the host finalizer will use" — see D2 for why South never resolves it.

`emits` is the allow-list South enforces after finalisation. It is part of the request
declaration, so a provider adapter (native or component) states it, and the host cannot widen it
at finalise time.

### 3.2 The seam (`south-core`)

```rust
/// What the finalizer may see: the request exactly as the transport will send it.
pub struct FinalizeViewV1<'a> {
    pub method: &'a Method,
    pub url: &'a Url,                 // binding-resolved, sanctioned query already appended
    pub headers: &'a SafeHeaders,     // ordinary headers, validated, in declaration order
    pub body: &'a [u8],               // the exact JSON bytes the transport will write
    pub slot: &'a CredentialSlotV1,
    pub emits: &'a SignedHeaderSetV1,
}

/// Headers emitted by the finalizer; South diffs them against `emits` before sending.
pub struct FinalizedHeadersV1 { /* private; zeroizing values */ }

pub trait RequestFinalizerV1: Send + Sync {
    fn finalize<'a>(&'a self, view: FinalizeViewV1<'a>) -> FinalizeFutureV1<'a>;
}
```

Position in `execute_provider_call_v1` / `open_streaming_provider_call_v1`, for the `HostSigned`
arm only:

```
resolve path + query → binding check → cancel/deadline pre-check
  → assemble (no auth header for this arm)
  → finalizer.finalize(view)            ← inside the biased-select race, once
  → allow-list diff                      ← south, synchronous
  → transport.execute / open
```

The finalizer runs **inside** the existing `tokio::select! { biased; … }` scope, so cancellation
and the absolute deadline pre-empt it exactly as they pre-empt credential resolution today. It is
called at most once per call; a retry is a new call with a new `x-amz-date`.

The allow-list diff rejects, in order: an emitted name not in `emits`; a declared name not emitted
(a signer that silently dropped `x-amz-security-token` is a broken signer, not a valid request);
an empty value; a duplicate. Every rejection is `PreparationErrorV1::RequestFinalizationRejected`.
A finalizer error is `PreparationErrorV1::RequestFinalizationFailed`. Both are preparation
errors: nothing has reached the network.

`PreparedHttpRequestV1` grows from "one auth header" to "zero or one auth header plus zero or
more finalised headers"; `auth_header()` becomes `auth_headers()` returning an iterator over
`(&'static str, &[u8])`. For the two existing arms it yields exactly one element, as today.

### 3.3 The transport's byte promise

A signature is only as good as the transport's restraint. The promise, stated as a contract and
checked by conformance (D5):

- The body on the wire is byte-identical to `FinalizeViewV1::body`. `ReqwestTransportV1` already
  sends `body().shared_owner()` unchanged; this makes it an obligation.
- `host` is derived from `FinalizeViewV1::url` and nothing else; `content-length` from the body
  length. These are the only headers the transport may add. (`user-agent` is already a
  `SafeHeaders` host obligation and therefore in the view.)
- No header in the view or in the finalised set is renamed, re-cased, re-ordered relative to
  itself, or re-encoded.
- No redirect, no proxy, no transfer-encoding change, no compression negotiation. All four are
  already hardening rules; here they become signature-preserving rules, which is why the
  hardening was cheap to extend.

A finalizer therefore *can* include `host` in `SignedHeaders`, because the transport's `host` is a
pure function of a URL the finalizer saw.

## 4. Conformance: `south.host-signed.v1` (D3)

A new frozen suite, separate from the three existing ones, with a **deterministic fake finalizer**
in `south-testkit`: HMAC over a canonical rendering of the view with a fixed test key, emitting
the full four-header set. Cases:

| Case | Asserts |
|---|---|
| `emits_exactly_declared` | on-wire auth headers are byte-equal to the fixture's expected values |
| `view_is_final` | the view's URL carries the sanctioned query and the binding-resolved path; body bytes equal the fixture body |
| `called_once` | counting finalizer observes exactly one call per entry-point invocation |
| `after_assemble_before_send` | a finalizer that records the transport's call count sees zero |
| `undeclared_header_rejected` | finalizer emits an extra name → `REQUEST_FINALIZATION_REJECTED`, transport never called |
| `missing_declared_header_rejected` | finalizer omits a declared name → same |
| `finalizer_failure_is_preparation` | erroring finalizer → `REQUEST_FINALIZATION_FAILED`, transport never called |
| `cancelled_during_finalize` | cancellation observed inside the finalizer future → `CANCELLED` |
| `deadline_during_finalize` | → `DEADLINE_EXCEEDED` |
| `transport_adds_only_host_and_length` | recording transport: wire headers = view ∪ emitted ∪ {host, content-length} |
| `streaming_parity` | every case above under `open_streaming_provider_call_v1` |
| `plain_channel_still_rejects` | `SafeHeaders` with any `SignedHeaderV1` name → `RESERVED_HEADER_FORBIDDEN` |

The host's `verified` judgement for this arm is this suite plus the unchanged three.

## 5. Scope

- The eventstream *response* framing used by Bedrock streaming is orthogonal: it is bounded bytes
  through `StreamChunkV1` and decodes in the provider component. It neither waits for nor depends
  on this slice.
- Request-body streaming with chunked signatures (S3-style `aws-chunked`) is out of scope; the
  view exposes a complete body and the arm assumes one.
- Kiro is covered to the extent it is SigV4 over JSON POST; anything beyond that is a new row in
  the capability matrix, not a change here.
- The server's existing SigV4 code moves behind a `RequestFinalizerV1` impl in the server; the
  community host has no `HostSigned` providers and needs no impl.

## 6. Versioning

Additive on top of 0.7.0's `#[non_exhaustive]` enums; ships as its own minor, numbered at ship
time (pre-allocation retired 2026-08-21; the previously reserved 0.9.0 went to the S2
conformance slice). New `PreparationErrorV1`
variants are additive under the same 0.7.0 ruling (that enum gains `#[non_exhaustive]` in 0.7.0
too — see D4).

## 7. Decisions — ruled 2026-08-20

- **D1 — `SignedHeaderV1` is a frozen enum**, four variants; adding one is a contract bump with a
  conformance case.
- **D2 — `HostSigned` bypasses `CredentialResolver`.** South never resolves the slot for this arm;
  the finalizer owns the signing material and South sees only emitted header values. The slot
  still participates in the binding check.
- **D3 — separate `south.host-signed.v1` suite**, the three existing suites stay frozen.
- **D4 — `PreparationErrorV1` gains `#[non_exhaustive]` in 0.7.0** alongside `ProviderAuthV1` and
  `RawAuthV1`, so `RequestFinalizationFailed` / `RequestFinalizationRejected` land additively.
- **D5 — the transport byte promise is enforced by conformance** (`transport_adds_only_host_and_length`
  and friends); a type-level `FinalizedRequestV1` is deferred until a second transport
  implementation exists.
