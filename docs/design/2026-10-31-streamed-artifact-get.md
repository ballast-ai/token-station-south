# The streamed artifact GET: credential-gated task artifacts through South

Status: **proposed** 2026-10-31, awaiting review (AQ-1 to AQ-5 below). Requested by lv for the server's south
interface review, finding M2.

Date: 2026-10-31

Baseline: south `origin/main` = `92124cfc`. Host references carry a `server:` prefix and refer to
token-station-server `dev-v2` = `23150901e`. Statements marked **unverified** come from reading code, not from running
anything or capturing traffic.

## 1. Problem

Some task families deliver an artifact URL that downloads only with the credential the task was submitted with.
`task-veo-v2` marks every artifact `fetch_with_credential` (contract 6): the Gemini API line answers
`{base}/v1beta/files/{id}:download?alt=media` under `x-goog-api-key`, the Vertex line
`{base}/v1/projects/{p}/locations/{l}/files/{id}:download?alt=media` under the minted bearer (fixtures in
`fixtures-veo-task-v2`).

The server fetches those with its own HTTP client (`server:gateway/src/modules/inference/engine/endpoint_fetch.rs`,
`fetch_with_endpoint_credential`): the credential goes only to a URL with the bound endpoint's origin, the keyed hop
does not follow redirects, and a redirect that leaves the origin is followed by the host's egress guard without the
credential. Every other credentialed upstream call goes through South's transport, which owns the request shape,
the auth arms and the no-redirect rule; this one is the exception, and the server's interface review (M2) traced it
to a missing shape: **South has no GET with a binary response**.

The buffered-binary record ruled that out on 2026-09-10 (`2026-09-09-buffered-binary-response.md` D6): a binary GET
had no consumer, because the two binary GETs of that time fetched absolute, non-provider URLs that a `GetRequestV1`
(a relative path under the bound base) cannot express. A credential-gated artifact is different: the host refuses
it unless it sits under the bound endpoint, so it **is** a relative path under the bound base, and it has a consumer.

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
    /// streaming POST is (`StreamingCallV1`).
    Body(StreamingCallV1),
    /// A 301, 302, 303, 307 or 308 with a `Location`. Never followed.
    Redirect(ArtifactRedirectV1),
}

pub struct ArtifactRedirectV1 {
    status: StatusCode,
    /// The target resolved against the request URL, at most 2048 bytes, `https` only.
    location: Url,
}

pub async fn open_artifact_get_raw_call_v1<R, T>(
    raw: &RawGetProviderCallV1<'_>, resolver: &R, transport: &T,
    deadline: Option<Instant>, cancellation: &CancellationToken,
) -> Result<ArtifactGetOutcomeV1, RawProviderCallErrorV1>;
```

- The request is the buffered GET's: no body, a relative path under the bound endpoint, the same slot and auth arms
  (header auth and bearer), the same controlled query. A non-2xx that is not a redirect is `Rejected`, with a bounded
  error body, as on the streaming POST.
- South **never follows** the redirect (AQ-2). The host decides: a target under the bound endpoint is another
  `open_artifact_get_raw_call_v1` with the credential; any other target is fetched by the host's egress guard without
  it. The hop budget stays the host's.
- A redirect without a `Location`, with a non-`https` target, or with a target over the length bound is a transport
  error, not an outcome.
- The total size is the host's to enforce while it pulls the stream (it already caps at `max_artifact_bytes`); South
  keeps its per-chunk and idle-timeout behaviour of the streaming POST.

### 2.2 `alt=media`

The controlled query's `alt` admits `sse` and `json` (`QueryParameterV1::Alt`). Both download URLs above carry
`alt=media`. The grammar widens to `sse`, `json` and `media` (AQ-3), with an HTTP contract version bump.

### 2.3 What does not change

- Which artifacts are credential-gated stays the component's declaration (`fetch_with_credential`, contract 6).
- Uncredentialed second hops (image and speech artifacts, ungated task artifacts) keep going through the host's
  `safe_fetch`; they are absolute URLs elsewhere and not provider calls (AQ-5).
- No component, package or world changes; this is host transport surface.

## 3. Conformance

A host-facing suite `south.artifact-get.v1`, run by the host against its transport adapter as the provider-get and
provider-binary suites are:

- a body-less GET under the bound endpoint, with each auth arm and with `alt=media`;
- a 2xx body streamed in several chunks equals the upstream bytes, with the bounded `content-type`;
- each redirect status yields `Redirect` with the resolved target, relative and absolute `Location` alike, and the
  transport sends one request only;
- a redirect without `Location`, to `http:`, or over the bound is a transport error;
- a 4xx or 5xx is `Rejected` with a truncated body;
- the credential header appears on the bound request and on no other.

`compatibility.json` gains `artifact_get` under `host_capabilities`, `not_verified` until the host runs the suite.

## 4. Host adoption (for reference)

The server replaces the keyed hop of `fetch_with_endpoint_credential` with `open_artifact_get_raw_call_v1`, keeps its
egress-guarded path for cross-origin targets, deletes its own credentialed client, and registers the suite result.

## 5. Open questions

| # | Question | Recommendation |
|---|---|---|
| AQ-1 | Streamed GET, or a buffered binary GET like the binary POST | Streamed: artifacts are up to 512 MiB on the host and videos exceed the 32 MiB buffered cap |
| AQ-2 | Surface a redirect to the host, or let South follow redirects that stay under the bound endpoint | Surface every redirect: South's transport never follows one, and the host already owns the cross-origin decision and the egress guard |
| AQ-3 | Widen `alt` to admit `media`, or add a separate sanctioned parameter | Widen `alt`: it is the same parameter name with one more value |
| AQ-4 | Bound on the redirect target | 2048 bytes, `https` only; a longer or plain-`http` target is a transport error |
| AQ-5 | Scope | Credential-gated task artifacts only; uncredentialed second hops stay on the host's `safe_fetch` |
