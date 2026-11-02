# The streamed artifact GET: credential-gated task artifacts through South

Status: **decided, amended, awaiting re-review** 2026-11-02. lv decided AQ-1 to AQ-5 (§7). The review of pull request
#320 required changes, applied here; the transport seam in §2.2 is new and has not been reviewed. Requested by lv for
the server's south interface review, finding M2. No code yet.

Date: 2026-10-31; amended 2026-11-02 after review.

Baseline: south `origin/main` = `92124cfc`; the review read `3bbcb8ae`. Host references carry a `server:` prefix and
refer to token-station-server `dev-v2` = `23150901e`; the review read `0fb9589a5`. Statements marked **unverified**
come from reading code, not from running anything or capturing traffic.

## 1. Problem

Some task families deliver an artifact URL that downloads only with the credential the task was submitted with.
`task-veo-v2` marks every artifact `fetch_with_credential` (contract 6): the Gemini API line answers
`{base}/v1beta/files/{id}:download?alt=media` under `x-goog-api-key`, the Vertex line
`{base}/v1/projects/{p}/locations/{l}/files/{id}:download?alt=media` under the minted bearer (fixtures in
`fixtures-veo-task-v2`).

The server fetches those with its own HTTP client (`server:gateway/src/modules/inference/engine/endpoint_fetch.rs`,
`fetch_with_endpoint_credential`): the first hop must sit under the bound endpoint (`decompose_upstream_url` in
`fetch_bound_media`), the keyed hop does not follow redirects, a redirect target with the bound endpoint's **origin**
keeps the credential, and a redirect that leaves the origin is followed by the host's egress guard without it. Every
other credentialed upstream call goes through South's transport, which owns the request shape,
the auth arms and the no-redirect rule; this one is the exception, and the server's interface review (M2) traced it
to a missing shape: **South has no GET with a binary response**.

The buffered-binary record ruled that out on 2026-09-10 (`2026-09-09-buffered-binary-response.md` D6): a binary GET
had no consumer, because the two binary GETs of that time fetched absolute, non-provider URLs that a `GetRequestV1`
(a relative path under the bound base) cannot express. A credential-gated artifact is different: the host refuses
its first hop unless it sits under the bound endpoint, so that hop **is** a relative path under the bound base, and
it has a consumer.

Two facts decide the shape:

- **Size.** The host stores artifacts up to `max_artifact_bytes`, 512 MiB by default
  (`server:gateway/src/infra/config/sections.rs`); a generated video is routinely above South's buffered cap of
  32 MiB (`MAX_RESPONSE_BODY_BYTES`). The body must stream.
- **Redirects.** A download endpoint may answer with a redirect to a presigned URL elsewhere (**unverified** for the
  Google file download; the host handles the case today). South's streaming head refuses redirect statuses
  (`StreamingResponseHeadV1::try_from_parts`), so today a redirect would fail the call instead of reaching the host.

## 2. Proposal

### 2.1 The entry

A streamed GET twin of the buffered GET (`RawGetProviderCallV1`, `south-core/src/raw.rs`):

```rust
pub enum ArtifactGetOutcomeV1 {
    /// A 2xx: the head (status, bounded `content-type`) and the body as a byte stream, pulled as the
    /// streaming POST is (`StreamingCallV1`), plus the declared length when the upstream sent one.
    Body { call: StreamingCallV1, content_length: Option<u64> },
    /// A 301, 302, 303, 307 or 308 with a `Location`. Never followed.
    Redirect(ArtifactRedirectV1),
}

/// `Debug` omits the query: a presigned target carries its signature there.
pub struct ArtifactRedirectV1 {
    status: StatusCode,
    /// The target resolved against the request URL, within the rules below.
    location: Url,
}

pub async fn open_artifact_get_raw_call_v1<R, T>(
    raw: &RawGetProviderCallV1<'_>, resolver: &R, transport: &T,
    deadline: Option<Instant>, cancellation: &CancellationToken,
) -> Result<ArtifactGetOutcomeV1, RawProviderCallErrorV1>;
```

- The request is the buffered GET's: no body, a relative path under the bound endpoint, the same slot and auth arms
  (header auth and bearer), the same controlled query. A non-2xx outside the 3xx range is `Rejected`, with a bounded
  error body, as on the streaming POST.
- The other 3xx statuses (300, 304, 305, 306) stay `REDIRECT_DENIED`, as `StreamingResponseHeadV1::try_from_parts` and
  the buffered heads already answer them.
- South **never follows** the redirect (AQ-2). The hop budget stays the host's, and the decision is the host's:
  - A target keeps the credential only if it is under the bound endpoint **and** its path and query parse through
    `RawGetProviderCallV1`. It is then another `open_artifact_get_raw_call_v1`. A query outside the sanctioned set
    counts as outside.
  - Any other target is fetched by the host's egress guard without the credential.

  This tightens the host: it keeps the credential today for any same-origin target. It matters for a bound endpoint
  with a path prefix, which is how Vertex rows are written: a same-origin redirect outside that prefix loses the
  credential and, if that upstream needs it, the download fails. The host registers this as an intentional difference.
- The target rules (AQ-4): the `Location` is resolved against the request URL; the result must use `https` or the
  scheme of the request that was redirected, carry no userinfo and no fragment, and fit in 8 KiB. South need not
  decide cross-origin policy: the host's egress guard already refuses a non-`https` target there. A redirect with no
  `Location`, or with a target outside these rules, is `REDIRECT_DENIED`, not an outcome; no new error code is needed.
- `content_length` is the parsed `content-length` header, `None` when absent or malformed. The host uses it to refuse
  an oversized artifact before writing anything (`server:…/tasks/artifact_spool.rs`, `spool_response`). It lives on the
  outcome, not on `StreamingResponseHeadV1`, so the POST head stays as it is.
- The total size is the host's to enforce while it pulls the stream (it already caps at `max_artifact_bytes`); South
  keeps its per-chunk and idle-timeout behaviour of the streaming POST.

### 2.2 The transport

No redirect can reach the entry through today's streaming seam. `AsyncStreamingTransport::open` is specified as
"exactly one prepared streaming POST" with "redirects are refused with `REDIRECT_DENIED`",
`ReqwestStreamingTransportV1::open_one` returns that error and drops `Location`, and `StreamOpenErrorV1` has no
redirect arm. That guarantee stays. This exchange gets its own trait, on the reasoning that gave
`AsyncBinaryHttpTransport` its own: the response shape is fixed in the existing trait's signature, a required method
would break every host implementation, and a defaulted one would ship a capability that silently is not there.

```rust
pub enum OpenedArtifactGetV1 {
    /// A 2xx, as `AsyncStreamingTransport::open` would have resolved it.
    Body { stream: OpenedByteStreamV1, content_length: Option<u64> },
    /// A redirect status from §2.1's list with the raw, unresolved `Location`, capped at the 8 KiB bound.
    Redirect { status: StatusCode, location: String },
}

pub type ArtifactGetOpenFutureV1<'a> =
    Pin<Box<dyn Future<Output = Result<OpenedArtifactGetV1, StreamOpenErrorV1>> + Send + 'a>>;

pub trait AsyncArtifactGetTransport: Send + Sync {
    /// Opens exactly one prepared body-less GET up to headers-ready. Never follows a redirect.
    fn open_artifact_get<'a>(
        &'a self,
        request: &'a PreparedHttpRequestV1<'_>,
    ) -> ArtifactGetOpenFutureV1<'a>;
}
```

- `ReqwestStreamingTransportV1` implements it over its existing client, so the hardening is unchanged (no proxy, no
  redirects, no retries, no referer, no decoding) and `TransportPairV1` does not grow. That transport has no total
  bound, only the mandatory idle guard, which suits a long download.
- `south-core` resolves and validates the target, so a transport cannot widen the rules.
- `AsyncStreamingTransport` is unchanged, and so are the server's test doubles that implement it.

### 2.3 `alt=media`

The controlled query's `alt` admits `sse` and `json` (`QueryParameterV1::Alt`). Both download URLs above carry
`alt=media`. The grammar widens to `sse`, `json` and `media` (AQ-3), with an HTTP contract version bump.

### 2.4 What does not change

- Which artifacts are credential-gated stays the component's declaration (`fetch_with_credential`, contract 6).
- Uncredentialed second hops (image and speech artifacts, ungated task artifacts) keep going through the host's
  `safe_fetch`; they are absolute URLs elsewhere and not provider calls (AQ-5).
- No world changes, and no component changes behaviour.

### 2.5 Package identities

The entry, the outcome and the new trait live in host-only crates, but §2.3 changes `QueryParameterV1` in
`south-contracts` and bumps the HTTP contract version. `south-contracts` is one of the three crates every component
links (boundary record §13.12, through `south-component-conformance`), so its version bump changes every
`component.wasm` and every package takes a Q47 patch: 41 packages at the 0.82.0 count. Identities move, and the host
re-pins them. No package is expected to raise `south_runtime`, because no manifest field or value changes.

For that reason this ships in the next regular South release, not a dedicated one (AQ-3).

## 3. Conformance

A host-facing suite `south.artifact-get.v1`, run by the host against its transport adapter as the provider-get and
provider-binary suites are:

- a body-less GET under the bound endpoint, with each auth arm and with `alt=media`;
- a 2xx body streamed in several chunks equals the upstream bytes, with the bounded `content-type` and the declared
  `content_length`, and `None` when the header is absent or malformed;
- each redirect status yields `Redirect` with the resolved target, relative and absolute `Location` alike, and the
  transport sends one request only;
- 300, 304, 305 and 306 are `REDIRECT_DENIED`;
- a redirect without `Location`, to a scheme outside the rule of §2.1, with userinfo or a fragment, or over the bound
  is `REDIRECT_DENIED`;
- a redirect's `Debug` output carries no query;
- a 4xx or 5xx is `Rejected` with a truncated body;
- the credential header appears on the bound request and on no other.

`compatibility.json` gains `artifact_get` under `host_capabilities`, `not_verified` until the host runs the suite.

## 4. Host adoption (for reference)

The server replaces the keyed hop of `fetch_with_endpoint_credential` with `open_artifact_get_raw_call_v1`, keeps its
egress-guarded path for cross-origin targets, deletes its own credentialed client, and registers the suite result.

The artifact port (`ArtifactFact`) and `spool_response` are tied to `reqwest::Response`, and the egress-guarded path
keeps returning one, so the server also needs a byte-source abstraction over both. That work is the host's.

## 5. What stays unverified

- Whether the Google file download redirects at all (§1).
- The 8 KiB bound. Presigned URLs carrying a session token can run past 2 KiB; that is experience, not a measurement.
  Check it against a real sample if one is available.

## 6. Considered: keeping the host's hop

The host's keyed hop already has the hardening of South's transport (no proxy, no redirects, no decoding), so this
change does not fix a security defect. What it buys is the boundary: the last credentialed upstream call moves behind
South's transport. It costs a contract bump and a package round. lv chose to do it, in a regular release.

## 7. Decisions

lv, 2026-11-02, as recommended in review.

| # | Question | Decision |
|---|---|---|
| AQ-1 | Streamed GET, or a buffered binary GET like the binary POST | Streamed: artifacts are up to 512 MiB on the host and videos exceed the 32 MiB buffered cap |
| AQ-2 | Surface a redirect to the host, or let South follow redirects that stay under the bound endpoint | Surface every redirect: South's transport never follows one, and the host already owns the cross-origin decision and the egress guard. The host's credential rule tightens as in §2.1 |
| AQ-3 | Widen `alt` to admit `media`, or add a separate sanctioned parameter | Widen `alt`: it is the same parameter name with one more value. Ship it in the next regular release (§2.5) |
| AQ-4 | Bound on the redirect target | `https` or the request's scheme, no userinfo, no fragment, 8 KiB. Changed in review from 2048 bytes and `https` only: that rule fails a same-origin redirect on an `http` bound endpoint, which works today |
| AQ-5 | Scope | Credential-gated task artifacts only; uncredentialed second hops stay on the host's `safe_fetch` |
