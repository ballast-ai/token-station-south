# The Multipart Request Body: Opaque Bytes Under a Declared Media Type

Status: draft for ruling (D1–D6 in §6); targets 0.25.0

Date: 2026-09-09

Predecessors: `2026-08-16-minimal-provider-call.md` (the JSON POST shape everything else extends;
its non-goals list names multipart explicitly), `2026-08-27-task-adapter-vocabulary.md` (§5 sized
T1/T2/T3 and ordered them by blocked-model count), `2026-09-08-buffered-get-request.md` (T1, the
immediately preceding slice: its D1 established that a new body shape gets its own request type
rather than a field on the frozen one).

## 1. Problem

`JsonPostRequestV1` carries a mandatory `JsonBodyV1`, and `JsonBodyV1::parse` refuses anything
that is not one complete JSON value. Every request shape South offers therefore has a JSON body
by construction. The task-adapter record sized this gap as **T2** and ranked it second by
blocked-model count.

Per the host's 37-号 ledger, **34 of 143 non-text models are blocked by T2 alone**: 25 carrying
the `image-edit` label (whose `/v1/images/edits` path is a multipart passthrough) and 9 ASR
models (which have no JSON alternative at all — the audio *is* the request). Two more, the
`bailian` polling image models, were counted as `T1+T2`; T1 shipped in 0.24.0, so **landing T2
unblocks 36 models**, not 34. The 4 `reve` models counted as `T2+T3` stay blocked on T3
(buffered binary responses), which is a separate later slice.

What the host actually does on those paths — read from the two production call sites,
`proxy_audio_multipart` (`/v1/audio/transcriptions`, `/v1/audio/translations`) and `image_edits`
(`/v1/images/edits`) — is the load-bearing fact for this design:

1. It reads the **client's** raw multipart body as bytes, under its own 100 MiB media cap.
2. It parses the boundary out of the client's `content-type` and validates it (non-empty, RFC
   2046 §5.1.1 maximum of 70 characters).
3. It **splices bytes in place**: the `model` field's value is replaced with the canonical
   upstream name, located by a boundary-aware scan, deliberately so that a binary `image`/`mask`
   part is never touched. Some arms strip a field or rename one; the boundary never changes.
4. It forwards those bytes verbatim, with the client's `content-type` header — boundary and
   all — passed through unchanged.
5. The response is buffered JSON, which South already carries.

The host is not asking South to *build* a multipart body. It has one, correct to the byte, and it
needs South to *send* it.

## 2. Boundary claim

Unchanged, and this slice is where that matters most. South gains no multipart parser, no part
model, no encoder, and no notion of what a form field is. It gains one thing: the ability to
carry a bounded, opaque byte body under a media type it renders itself. Everything South refuses
to own about a JSON POST — routing, retry, admission, persistence, credential sources — it
equally refuses to own about a multipart POST.

The security question this raises honestly: an opaque body is a wider door than a validated JSON
one. Three things keep it bounded, and §3 makes each of them mechanical rather than advisory:
the byte cap, the media type being a **closed declaration** rather than a free-form header, and
a construction-time check that the body actually opens with the boundary it declared.

## 3. Design

### 3.1 Contract (`south-contracts`)

```rust
/// A bounded, opaque request body under a declared media type.
pub struct MultipartBodyV1 {
    bytes: Arc<[u8]>,
    boundary: MultipartBoundaryV1,
}

/// An RFC 2046 §5.1.1 boundary: 1–70 characters from the `bchars` set.
pub struct MultipartBoundaryV1 { /* … */ }

/// A bounded provider request for one multipart POST.
pub struct MultipartPostRequestV1 {
    relative_path: RelativePathV1,
    headers: SafeHeaders,
    body: MultipartBodyV1,
    auth: ProviderAuthV1,
    query: Option<QueryStringV1>,
    user_agent: Option<ControlledUserAgentV1>,
}
```

`JsonPostRequestV1`'s field set with `MultipartBodyV1` in place of `JsonBodyV1`, same builders,
same grammars for every other field. A separate type, not a body enum on the POST shape — see D1.

**The media type is South's to render, not the host's to state.** `content-type` is not in
`RESERVED_HEADERS`; the JSON arm sets it through the ordinary header channel today. That is safe
for JSON because the body is *validated* to be JSON, so header and body cannot disagree. For an
opaque body it is not safe: nothing would stop `multipart/form-data` bytes travelling under
`application/json`, or a boundary in the header that appears nowhere in the body. So
`MultipartPostRequestV1::new` **refuses a `content-type` in its `SafeHeaders`** (a check on this
type, not a change to the shared reserved list — the JSON arm is untouched), and the transport
emits exactly `multipart/form-data; boundary=<declared>`. One source, no disagreement possible.

`MultipartBodyV1::parse` validates three things and nothing else:

- **Length** against `MAX_MULTIPART_REQUEST_BODY_BYTES` (see D4).
- **The boundary grammar**, RFC 2046 §5.1.1 — the same rule the host already enforces, moved into
  the contract so a host that forgets it cannot send a request whose `content-type` is malformed.
- **That the body opens with `--<boundary>`** and carries the closing `--<boundary>--`. This is
  the one integrity check worth its cost: it catches exactly the failure the host's own comments
  worry about — a spliced body whose boundary no longer matches its header — and it is a byte
  comparison, not a parse. South still does not know what a part is.

`HTTP_CONTRACT_VERSION: 6 → 7`, additive: a version-six request is exactly a version-seven request
that is not a `MultipartPostRequestV1`. `JsonPostRequestV1` and `GetRequestV1` are untouched.

### 3.2 Seam (`south-core`)

The private `RequestParts` projection added by the GET slice is the extension point, and it was
built for this: its `body: Option<&JsonBodyV1>` becomes `Option<RequestBodyRefV1<'_>>`, a
two-variant borrow (`Json` / `Multipart`). The binding check, the auth-header assembly, the
finalizer view, and the biased cancellation race stay written once.

`PreparedHttpRequestV1::body()` returns that borrow; `content_type()` is new and returns
`Some(rendered)` **only for the multipart arm**, `None` for the other two. The JSON and GET arms
are deliberately left alone: the JSON arm's `content-type` travels through the ordinary header
channel today, and hosts legitimately send values South does not get to normalize (a `charset`
parameter, say). Rendering it for them would be a wire change dressed up as a refactor.
`FinalizeViewV1::body()` is already `&[u8]` and needs no change at all — a payload-hashing signer
sees the multipart bytes exactly as it sees JSON bytes.

One new entry point, buffered only (D5):

```rust
pub async fn execute_multipart_call_v1<R, T>(binding, request: &MultipartPostRequestV1, resolver, transport, deadline, cancellation) -> …;
```

The prelude gains `RawMultipartProviderCallV1` (the raw call with `body: &[u8]` plus `boundary:
&str`) with `parse_raw_multipart_call`, `raw_multipart_call_parses`, and
`execute_multipart_raw_call_v1`; `south-testkit` gains the owned builder.

### 3.3 Transport (`south-transport-reqwest`)

`attach_body` — also added by the GET slice — gains the multipart arm, sharing the body's
`Arc<[u8]>` through `Bytes::from_owner` exactly as `JsonBodyOwner` shares its `Arc<str>`, so a
100 MiB audio upload is not copied. `assemble_headers` emits `content-type` when — and only
when — the prepared request reports one, which is the multipart arm alone; the JSON arm's
headers are assembled byte for byte as they are today. `TRANSPORT_ADDED_HEADERS_V1` grows by
nothing: the name is added for a request that declares it, not by the transport on its own
behalf, which is the same footing as an auth header.

### 3.4 Conformance

A dedicated `south.provider-multipart.v1` suite (D6), five frozen cases: a buffered multipart
success under the Bearer arm; the same under a sanctioned header; a slot mismatch refused before
resolver and transport; a body whose bytes do not open with the declared boundary, refused at
construction with zero calls; and a request smuggling a `content-type` through the ordinary
header channel, likewise refused. Evidence: resolver and transport call counts, plus
`wire_content_type_exact` (the rendered value reached the boundary byte for byte) and
`wire_body_bytes_exact` (the transport was handed the declared bytes unmodified — the presence
polarity, so a probe that never reads the prepared body answers `false`).

## 4. Consumer

The server host's two multipart call sites. `proxy_audio_multipart` and `image_edits` each
already produce `(bytes, content-type)` immediately before their upstream send; the adoption is
to route that pair through `execute_multipart_raw_call_v1` behind a `SouthSurface::Multipart`
switch, default off until parity, exactly as the GET slice did.

Scope honesty: T2 unblocks the **contract** for 36 models, but the host's own auth scope decides
which route on day one. The OpenAI-compatible ASR arm and the OpenAI/Azure image-edit paths are
in scope today (Bearer and `api-key`). `ElevenLabs` and Azure Speech are *expressible* — their
`xi-api-key` and `ocp-apim-subscription-key` are already sanctioned `SecretHeaderV1` variants —
but that host's `south_auth_for` returns `None` for both, on a media-surface judgement that
predates this slice and is not ours to overturn.

## 5. Versioning

Additive; ships as **0.25.0** with `compatibility.json` `http: 7` and the new suite. Both hosts
are annotated `provider_multipart: not_verified` until each runs the suite through its own
adapter.

Fuzz obligations grow by one target: `MultipartBodyV1::parse` takes attacker-shaped bytes and a
boundary, which is exactly the shape the existing `contract_parsers` target covers for the other
grammars.

## 6. Decisions for lv

- **D1 — a separate `MultipartPostRequestV1` versus a body enum on `JsonPostRequestV1`.**
  Recommend separate, on the precedent that settled the GET slice's D1: the POST type's name, its
  mandatory JSON body, and every invariant the streaming path relies on stay exactly as frozen. A
  body enum would make "a JSON POST whose body is multipart" constructible and force the
  streaming entry points, which have no multipart consumer, to reject it at runtime.
- **D2 — an opaque byte body versus a modeled part list.** Recommend opaque. The host has already
  encoded a correct multipart body and spliced it byte-precisely; a part model would force it to
  *decode* that body so South could *re-encode* it, which changes the boundary, discards the
  host's careful "never touch a binary part" work, and breaks the byte-identity-with-legacy
  discipline every adoption so far has rested on. South also has no business learning what a form
  field is.
- **D3 — South renders `content-type` from the declaration, and the type refuses a
  `content-type` in its ordinary headers.** Recommend as designed. The alternative — let the host
  pass it as a header, as the JSON arm does — is safe only because a JSON body is validated; with
  opaque bytes it permits a header and body that disagree, which is the one thing an opaque body
  makes possible and a contract should not.
- **D4 — the byte cap.** Recommend a new `MAX_MULTIPART_REQUEST_BODY_BYTES = 100 MiB`, matching
  the host's existing media-inference limit, rather than reusing the 32 MiB JSON cap. Reusing 32
  MiB would silently fall back to legacy for audio between 32 and 100 MiB — a coverage hole in
  exactly the models this slice exists to unblock. The transport shares one allocation, so the
  cost is one buffer, not a copy per hop.
- **D5 — buffered only; no streaming multipart, no host-signed twin.** Recommend as designed. The
  image-edit handler rejects `stream=true` outright, ASR is buffered, and no host signs a
  multipart request. The task-adapter record's rule against reserving unconsumed shapes applies.
- **D6 — a dedicated `south.provider-multipart.v1` suite.** Recommend dedicated, per the
  header-auth, controlled-query, and provider-get precedents: the frozen provider-call table is
  burned into two hosts' evidence, and a multipart POST is a different call shape.
