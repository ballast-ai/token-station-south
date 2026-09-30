# Image World: Synchronous Generation and Edit Share One World (image-adapter-v1)

Status: **proposed — drafted for review by the host team (token-station-server P21/P22/P23), not accepted**

Date: 2026-09-30

Rulings: on 2026-09-30 the host owner (lv) ruled on Q4, Q7, Q10 and Q13 (§17), each as recommended. Q4 also needs
the south maintainers. Questions decided by the south maintainers remain open.


Baseline: south `origin/main` = `3135e36` (v0.42.0). Server line numbers come from the local checkout `8b2a1976`
(dev-v2 merged in up to `5e7816a3`); P22 / P18 were written at `d672b945` / `b660ea3f`, so their line numbers have
drifted — re-verify in place before citing. Kernel line numbers come from the `token-station-protocol` revision south
pins, `f585bc8` (`Cargo.toml:22`).

Citation convention: a south file name without a directory refers to this repository's crate sources — `lib.rs` and
`task_v2.rs` are in `crates/south-contracts/src/`; `manifest.rs` is in `crates/south-provider-api/src/` and `*.wit` in
`crates/south-provider-api/wit/`; `component.rs`, `runtime.rs`, `bindings.rs` and `loader.rs` are in
`crates/south-provider-runtime/src/`; `task_suite_v2.rs`, `report.rs`, `component_v2.rs` and `reference_*.rs` are in
`crates/south-component-conformance/src/`. The kernel's `http.rs` and `usage.rs` are in `token-station-protocol`'s
`crates/protocol/src/`. Server file names are abbreviated relative to `gateway/src/modules/`: `handlers.rs`,
`precheck.rs`, `execute.rs`, `durable.rs`, `gemini.rs`, `azure.rs`, `minimax.rs`, `bailian.rs`, `ideogram.rs`,
`stability.rs`, `xai.rs` and `openai_compat.rs` are in `inference/handler/images/`; `reve.rs` is in
`inference/handler/`; `media.rs` is in `inference/engine/token_counter/`; `capabilities.rs` is in `inference/engine/`;
`webhook_sender.rs`, `domain.rs` and `repo/reconcile.rs` are in `tasks/`; `usage_types.rs` is in
`crates/gateway-provider-protocol/src/`, `models.rs` in `gateway/src/infra/config/`, and `core/limits.rs` in
`gateway/src/`.

Predecessors:
`2026-09-09-multipart-request-body.md` (0.25.0: multipart is only opaque bytes; South has no part model and does not
encode),
`2026-09-09-buffered-binary-response.md` (0.26.0: binary responses; an absolute-URL second hop "is the host's artifact
fetch, not a provider call"),
`2026-09-18-task-adapter-world.md` (the precedent for a separate WIT package and a capability vocabulary),
`2026-09-19-runtime-second-world.md` (how the runtime loads more than one world),
`2026-09-27-task-contract-v6-facts.md` (request estimate facts, `immutable_body_paths`),
`2026-09-28-task-contract-v7-artifact-role.md` (no reservation for values without a consumer),
`2026-09-30-host-zero-vendor-boundary.md` (this record relies on its §3 credential recipe, §4.2 descriptor auth
admission, §6.3 host checks and undetectable zone, and §8 compatibility range).

Origin: token-station-server P21 (DP0 / DP1 decided; group-B decisions "as recommended"), P22 (DI1–DI6), P18 (D1–D6).
Sibling records: `2026-09-30-speech-world.md` (reuses this record's §6 media vocabulary and §11 safe fetch executor
without repeating them); `2026-09-30-embeddings-contract.md` (the third new world, `embeddings-adapter-v1`, from P24).

## 1. Problem

The server's `/v1/images/generations` and `/v1/images/edits` are translated by the host with hand-written
per-provider code. The generation surface has 9 synchronous arms (Vertex, Gemini, Bailian Qwen-Image, MiniMax, Azure
MAI, Ideogram, Stability, the Reve bridge, and the OpenAI-compatible fallback; xAI enters through the fallback and is
told apart by `provider_config.name == "xai"`, `handlers.rs:143`, `openai_compat.rs:224-228`), and the edit surface
has four providers with translation logic (Gemini, Reve, Azure Foundry, xAI JSON edit) plus an OpenAI-compatible byte
pass-through. Before dispatch there is also per-provider admission: the Nano Banana tier gate (`precheck.rs:34-62`),
the mapping from input-image keys to roles (`precheck.rs:88-155`), and the xAI upper-bound branch
(`execute.rs:156-190`). P21 DP0 requires zero provider logic in the host, so all of these must move into components.

South today has no world that can hold them:

| Need | South today | Source |
|---|---|---|
| Non-chat request / response | The provider world has only chat body building, parsing, stream chunks and error mapping, plus metadata and model capabilities; the capability words are closed to `chat / stream / tool_call / json_schema` | `provider-adapter.wit:80-133`; `manifest.rs:74` |
| Inline byte artifacts (Azure, Stability and Gemini return only b64) | Task-world artifacts can only be a URL, a file id, or empty | `task_v2.rs:296-303` |
| Multipart requests, non-JSON request bodies | The descriptor a component hands the host is the kernel `HttpRequestDescriptor`, whose `body: Option<Value>` can only be JSON | kernel `http.rs:350-361` |
| Handing binary responses to the component for parsing | The kernel `HttpResponseParts.body` is a `String`; its comment admits binary needs a "-v2 field" | kernel `http.rs:382-391` |
| Image token buckets, image counts, credits | The kernel `Usage` has only input / output / cache buckets — no modality, no image count, no credits | kernel `usage.rs` |

Multipart and binary **transport** types already exist (`MultipartPostRequestV1`, `lib.rs:1585`;
`BufferedBinaryResponseV1`, `lib.rs:2331`), but they are host-side sending primitives; a component cannot use them to
describe a request.

One more hard constraint is often overlooked: by default the runtime limits each call to **a single JSON payload of
16 MiB, 64 MiB of linear memory and 2 seconds of wall clock** (`runtime.rs:26-34`), and ordinary calls into one loaded
component are serialized through the same instance (`component.rs:136`, `:674-686`). A gpt-image-class b64 response
with n=10 exceeds 16 MiB, and a high-resolution Gemini `inlineData` approaches it; the request body limit for
multipart edits is 100 MiB (`lib.rs:115`). "Hand the bytes to the component" is not solved by changing the WIT alone.

## 2. Scope and boundary statement

**In scope**: world shape and functions; byte handling for requests and responses; artifact forms and delivery order;
metering facts and host generic checks; pre-dispatch facts; the division of labour on pricing form; how the
GatewayHeld path connects; the conformance suite and host obligations; migration order and dual-run acceptance.

**Out of scope**: the host executor's implementation; the values of any price or reservation formula; the two
asynchronous arms Wan / GMI (task world, P13 S14); Bailian's native image surface and Reve's three native routes
(P25); streaming image output (partial images); Vertex service-account minting itself (P21 S3; specified as credential
recipe v1 in the boundary record §3, phase B4 there — this world only consumes the minted slot, §4).

**Boundary statement**: South still does not own the network, the clock, prices, reservations, persistence or
credential sources. This world only lets a component say, as a **pure function**, "what the request looks like and
what facts the response contains". The host holds the bytes; the component describes, by **reference**, where the
bytes are and which public standard encodes them (§6). That confines the host's new responsibilities to what P21 §1.1
allows: "generic executors for public standards, selected by component declaration".

## 3. D1 — Generation and edit share one world (DI1)

Generation and edit differ only in their inputs: edit adds input images and a mask, and both are "binary inputs in the
request" — the same kind of thing as the generation surface's reference images (Gemini `image_url`, MiniMax
`subject_reference`, `precheck.rs:88-115`). Artifacts, metering and delivery are identical. Splitting them into two
worlds would put one provider's generation and edit into two packages, each running its own conformance suite, with
no difference in function shape.

**Decision**: one world. The operation is distinguished by `operation` (`generate | edit`) in the request context; the
manifest capability words are `generate` and `edit`, at least one of which must be declared; which operations each
model supports is declared per model by `model-capabilities` (§7). The host refuses an undeclared operation before
admission and does not call `prepare`.

## 4. D2 — A separate WIT package, no `host` import (DI2)

**Package**: `token-station:image-adapter@1.0.0`, world `image-adapter-v1`, suite `south.image-component.v1` — the same
`token-station:*-adapter@1.0.0` / `*-adapter-v1` naming as the speech world (`speech-adapter-v1`) and the embeddings
world (`embeddings-adapter-v1`, embeddings record §2 D2). The reasoning is the same as the task world's D1: a package
version is shared by every world in the package, so putting this into `token-station:adapter@2.0.0` would make the
chat side and the image side send each other false version signals; and adding an export to adapter@2 means old
components cannot instantiate against the new bindings — effectively a major, and all four text components would have
to be rebuilt.

**No `host` import**: there is no signing consumer. What Vertex needs is a bearer token minted by the host, not
`host.sign`: the component declares the service-account credential recipe in its manifest, the host executes it, and
the product is presented through a `minted` slot as `bearer` (boundary record §3.3; P21 S3).
`task-adapter-v2.wit:63-66` already set the precedent "no accepted signing consumer, no import".

**Auth arms**: the first version admits `bearer` and `header_secret`. The header names required are all already in
the closed set: `api-key` (Azure Foundry, Ideogram; header names are case-insensitive) and `x-goog-api-key` (Gemini)
(`lib.rs:884-894`). Minted credentials (Vertex) need no arm of their own: they arrive through a `minted` slot and are
presented as `bearer`, and the host admits the descriptor's auth by the descriptor auth rule of the boundary record
§4.2. This world does not admit the `oauth` arm, which the boundary record proposes to deprecate (§3.7 there);
`host_signed` has no consumer and is not admitted. Runtime loading needs one change with it: the host import is
currently linked on `api_version != TASK_WORLD_V2` (`component.rs:182`). This world, the speech world and the
embeddings world all come without a `host` import, so the condition should become a world property rather than an
enumerated exclusion, and the import scan should refuse any `host` namespace for these worlds, as `loader.rs:216-224`
does for task-v2 (the embeddings record §4 does the same).

**Not recommended, but awaiting a ruling: one "synchronous media world" holding both image and speech (and even
embeddings).** This is the alternative that P22 / P23 / P24 never compared against one another when each decided to
"open a new world"; it is listed in §16 and §17 Q4. lv ruled on 2026-09-30 for separate worlds; the south
maintainers' ruling is still needed. This record is written as separate worlds; the only thing shared is §6's JSON
vocabulary.

## 5. Additional reasoning for DI2: why not reuse the task world's "terminal on submit"

P22 §2 already lists: task artifacts can only be URLs (`task_v2.rs:296-303`); task components parse the response as a
string; and every synchronous request would add an `async_tasks` row. One more: the task world's `SubmitOutcomeV2`
distinguishes only `Rejected` and `Unknown` (`component_v2.rs:28-37`) and cannot express "the upstream charged but
there is no deliverable artifact" (Reve charges for a policy violation and then turns it into a 400,
`reve.rs:703-733`), which is exactly what the synchronous image surface needs (§9.2).

## 6. D3 — Bytes stay out of the sandbox: the host holds the bytes, the component describes them by reference (DI3 = P18 D3 B, extended to the response side)

P18 D3 discussed only the **request side** (the component outputs "request structure + blob references + encoding
directives", and the host generically encodes multipart or inline base64). The response side needs the same: b64
images sit inside the upstream JSON; the component has to read usage, but should not read tens of MiB of base64 into
the sandbox only to emit it again unchanged. This section applies one mechanism in both directions. These types go
into a new `south-contracts` module, `media` (shared by image and speech; contract counter `contracts.media`).

### 6.1 The request view the host gives the component: `MediaRequestViewV1`

- JSON northbound request: the host gives the **elided** JSON (§6.2).
- Multipart northbound request (the edit surface, ASR): the host uses a generic parser to split it into an **ordered**
  part list `parts: [{name, kind: "text", value} | {name, kind: "file", blob, filename?, media_type?, bytes}]`, keeping
  repeated fields in order of appearance. File parts appear only as a `blob` reference; their bytes do not enter the
  sandbox. The host's multipart tooling today only scans and replaces in place and never splits out file parts
  (`inference/handler/audio/multipart.rs:560-683`); a complete generic part parser is needed here — P21 already
  classifies "multipart parsing" as host generic logic.
- The host must not hand over any key starting with `$south.`: its appearance in northbound JSON is a 400 (placeholder
  namespace, §6.2).

### 6.2 Elision rules (deterministic; both hosts must execute them identically, byte for byte)

A JSON string is replaced by the placeholder `{"$south.blob": {"id": …, "bytes": …, "head": …}}` in two cases (`head`
is the first 64 bytes, truncated at a UTF-8 boundary, so the component can recognise a prefix such as
`data:image/png;base64,` without getting the content):

1. **Declared paths**: path patterns the component declares in `model-capabilities` (request side) and in the result of
   `prepare` (response side) — RFC 6901 JSON Pointers in which a `*` segment matches any array index or key, e.g.
   `/image_url`, `/images/*/url`, `/data/*/b64_json`, `/candidates/*/content/parts/*/inlineData/data`. At most 32
   entries, each ≤ 256 bytes. A match is elided **regardless of length**.
2. **Fallback threshold**: any string, at any position, longer than `MEDIA_MAX_INLINE_STRING_BYTES` (suggested
   1 MiB). It guarantees that the view stays within the runtime payload limit, and gives the component a deterministic
   shape to refuse when a huge string turns up at an undeclared position (for example, a prompt above 1 MiB is refused
   as `invalid_request`).

If the elided view still exceeds the runtime `max_payload_bytes`: on the request side the host returns 413 / 400
before admission; on the response side the round is `unknown` (§9.2) — no settlement, no refund. The threshold is
1 MiB rather than smaller because legitimate text approaches tens of KiB (a 32,000-character CJK prompt is about
96 KB): eliding text the component needs to read would break the request.

### 6.3 The request descriptor the component gives the host: `MediaRequestDescriptorV1`

Same role as the task world's `HttpRequestDescriptor`, but defined by south itself, because the kernel's can only
carry a JSON body:

| Field | Form |
|---|---|
| `method` | `POST` or `GET` |
| `path` | A relative path following the `RelativePathV1` grammar (`lib.rs:479`); the host authorizes it against the configured endpoint (EndpointConfinement) |
| `query` | Only the closed `QueryParameterV1` set (`lib.rs:1064-1090`; the image surface uses `GroupId` and `api-version`) |
| `headers` | `SafeHeaders` rules; a multipart body must not carry `content-type` (same rule as `lib.rs:1615`) |
| `auth` | One of the auth arms the manifest declares; the host admits it by the descriptor auth rule of the boundary record §4.2 |
| `body` | `json {template}` / `multipart {parts}` / `empty` |

`json.template` is plain JSON in which reference nodes `{"$south.ref": {"blob": "<id>", "transform": "<word>"}}` may
appear; the host replaces each with the transformed JSON string. `multipart.parts` is an ordered list:
`{name, value: "<text>"}` or `{name, blob, transform, filename?, media_type?}`. The host encodes the list
**generically** into `MultipartBodyV1` (the boundary is supplied by the host; contracts do not touch randomness), then
goes through the existing `MultipartPostRequestV1` / `execute_multipart_call_v1`. Every request shape on the image
surface falls within the three that HTTP contract 9 already has (JSON POST, multipart POST, JSON POST with a binary
response); **no** new transport shape is needed.

### 6.4 Closed transforms `MediaTransformV1`

All are public standards, named by the component and implemented once by each host (P21 §1.1):

| Word | Input → output | Use on the image surface |
|---|---|---|
| `as_is` | original string / original bytes | the OpenAI-compatible edit passes file parts through; xAI forwards the client's data URL unchanged |
| `base64` | bytes → base64 string | Gemini `inlineData.data`, Reve `reference_images` |
| `data_url` | bytes → `data:<media_type>;base64,…` | reference images for upstreams that only accept data URLs |
| `from_data_url` | data URL string → bytes | the client sends a data URL, the upstream wants a multipart file part |
| `from_base64` | base64 string → bytes | same, with the client sending bare base64 |

The speech surface adds `from_hex`, `concat` and `wav_pcm_s16le{rate, channels}` (see the speech record §6). The set
is closed: adding a word is a `contracts.media` version change.

### 6.5 The response view `MediaResponseViewV1` and the response framing declaration

In `prepare` the component declares this response's framing: `json | binary | text` (the speech surface adds `sse`).
It follows the rule of P21 S4 — the component names a form from a closed set and the host implements each form once —
but it is a media-world vocabulary carried in `contracts.media`, not the provider world's stream framing
(`stream_framing`, boundary record §5.2: `bytes | aws-eventstream`). That record declines `sse` because provider-world
components receive raw chunks and split SSE themselves; in this world the body stays out of the sandbox (§6), so the
host builds the view and therefore does the splitting.

From the declaration the host provides `{status, headers, body}`, where `body` is `{"json": <elided view>}`,
`{"text": "…"}` (≤ the fallback threshold), or `{"opaque": {"blob", "bytes", "media_type"}}` (binary, or over the
limit). A non-2xx body is tried as UTF-8 to give `json` / `text`, otherwise `opaque`. If a key prefixed with `$south.`
appears in the upstream body, the host judges the round `unknown` and does not hand it to the component (so an
upstream cannot forge references). `headers` follows the exclusion rules of `ResponseTranscriptV1` (`lib.rs`
`RESPONSE_TRANSCRIPT_DENIED_HEADERS`) and contains no credential-class headers.

### 6.6 Relation to the 0.25.0 ruling: what agrees and what is revised

P22 I1 says option B "agrees with south 0.25.0's ruling that 'South does not encode multipart'". **That is only half
right**:

- **Agrees**: the transport contract is unchanged. `MultipartBodyV1` is still encoded opaque bytes, and "This contract
  does not parse multipart" (`lib.rs:740`) still holds.
- **Revised**: one of 0.25.0 D2's reasons was "South also has no business learning what a form field is"
  (`2026-09-09-multipart-request-body.md:203-208`). Under option B the component must state part names, file names and
  media types in the world vocabulary — so South's **world layer** starts knowing what a form field is. This record
  explicitly narrows that sentence's scope: it continues to bind the transport layer, not the world layer.

One more argument 0.25.0 made does not hold here: it required **byte-for-byte** agreement with the host's old path,
because the host already had a correctly encoded body and only replaced in place. Once the component builds the body,
the host has changed encoders and the boundary necessarily differs, so the dual run can only compare **decoded part
lists** (§14).

### 6.7 D3b — Where the encoder lives

- **A — each host writes its own**: matches the letter of 0.25.0's "South ships no encoder", but the community host
  and the server each write one, the encoding details (part header order, line breaks, file-name escaping) drift, and
  the conformance suite cannot assert anything about the encoded result.
- **B — south provides host-side pure functions** (recommended): `south-core` (or `south-contracts::media`) provides
  `encode_multipart_v1(parts, resolved_blobs, boundary) -> MultipartBodyV1`, with no I/O and no randomness (the caller
  supplies the boundary). It runs **outside** the sandbox, so large bytes still never enter wasm; both hosts share one
  implementation, and the suite can assert byte for byte on the encoded result. Cost: it overturns the half-sentence
  "South gains no … encoder" of 0.25.0 §2, which needs a ruling from the south maintainers (§17 Q1). Transforms such
  as `base64` / `data_url` likewise; the suggestion is to provide them as pure functions in the same place.

## 7. D4 — Function set

```wit
// Synchronous image generation and edit. All exports are pure translation.
// The host owns bytes, credentials, HTTP, clock, pricing, persistence and delivery.
package token-station:image-adapter@1.0.0;

interface image-adapter {
    record adapter-metadata { name: string, version: string, api-version: string }
    enum health-status { ready, degraded, unavailable }
    record adapter-health { status: health-status, detail: option<string> }

    type json = string;

    metadata: func() -> adapter-metadata;
    healthcheck: func() -> adapter-health;

    // ProviderConfig -> list<ImageModelCapabilitiesV1>. Static per package digest.
    model-capabilities: func(provider-config: json) -> result<json, json>;

    // ProviderConfig, MediaRequestViewV1, ImageCallContextV1 -> PreparedImageCallV1.
    // Err(ErrorEnvelope) is a pre-dispatch refusal: nothing was sent, nothing may be reserved.
    prepare: func(provider-config: json, request: json, context: json) -> result<json, json>;

    // Prepared state, MediaResponseViewV1 (one upstream round) -> ImageOutcomeV1.
    parse-response: func(state: json, response: json) -> result<json, json>;

    // Prepared state, the succeeded round outcomes, ImageRenderContextV1 -> ImageRenderedV1.
    render: func(state: json, outcomes: json, render-context: json) -> result<json, json>;
}

// No host import: this world has no signing consumer.
world image-adapter-v1 {
    export image-adapter;
}
```

- **`model-capabilities`**: static per-model declarations — supported operations, input roles and limits (§8),
  renderable `response_format`s, the metering forms it will report (§9.1), the tier words that may appear, and
  request-side elision paths. The host may cache the result by package digest. When the host routes aliases or
  private deployments and the component cannot tell from the model name, follow the precedent of
  `2026-09-29-claude-model-dialect.md`: the catalog declares words in `ProviderConfig.models[].supported_parameters`
  and the component reads them (the dialect-word mechanism of the boundary record §7.4).
- **`prepare`**: a pure function the host calls **before admission and reservation**; it returns the pre-dispatch
  facts (§8), the request descriptor (§6.3), `repeat`, the response framing and elision paths, `immutable_body_paths`
  (same semantics as task contract 6 §4), and a bounded `state` (≤ 8 KiB, no secrets, for use by the two later
  functions).
- **`parse-response`**: called once per upstream round; returns one of the four outcomes of §9.2. Error mapping is
  merged in here, with no separate `map-provider-error`: on the synchronous image surface only the component can tell
  "failed but charged" from "failed and not charged" (MiniMax's errors arrive as HTTP 200 + `base_resp`,
  `minimax.rs:356-369`).
- **`render`**: takes the outcomes of all succeeded rounds and the render context the host provides (the `created`
  timestamp and so on — the component has no clock), and produces a client-body template in which artifact positions
  are written as `{"$south.artifact": {"index": i, "as": "b64_json" | "url"}}` for the host to fill in. It lives in the
  component rather than in generic host rendering for two reasons: the OpenAI-compatible fallback today returns the
  upstream body to the client unchanged (`handlers.rs:650-656`), and generic host rendering would lose fields such as
  `usage`, `background` and `output_format`; and Gemini produces only one image per call, so n images take n loops
  (`gemini.rs:314`), and aggregating n rounds must happen in one function. Same precedent as the task world's
  `render-success` (`task-adapter-v2.wit:53-58`).

**`repeat`**: `prepare` may declare that one descriptor is sent k times (1 ≤ k ≤ 10), only for "one image per call"
upstreams such as Gemini / Vertex. The host executes the rounds sequentially and stops at the first non-succeeded
round. The aggregation rule is part of the contract: round 1 `rejected` → the whole call is `rejected`; any
non-success from round 2 on → the whole call is `unknown` (earlier rounds were already charged; consistent with
today's `DispatchProbe` being set only when `i==0`, `gemini.rs:385`). "k different descriptors" is not offered: there
is no consumer.

## 8. D5 — Pre-dispatch facts (each item P22 I1 lists, landed)

`PreparedImageCallV1.facts` — all request-side facts, no prices:

| Fact | Form | Which host provider logic it replaces |
|---|---|---|
| `operation` | `generate` / `edit` | — |
| `inputs` | counts per role `{input_image, reference_image, mask}`; closed vocabulary | `precheck.rs:88-155` interpreting keys and roles by `provider_type` |
| `requested_outputs` | image count sent upstream × `repeat` | the per-arm branches where the host parses `n` itself |
| `size` | `{width, height}`, when it can be determined | `image_size_hint` in `media.rs:31-36` |
| `tier` | tier words `{resolution?, quality?, speed?}`, grammar `[a-z0-9_.-]{1,32}` | xAI's `size→resolution` normalisation (`xai.rs:338-409`), Ideogram's speed tier (`ideogram.rs:62-72`) |
| `tier_candidates` | `{candidates: [...], default}`: the candidate set when the upstream decides the tier | the xAI candidate price cards (`media.rs:994-1040`) |
| `metering_forms` | the metering forms that must be reported on success (§9.1) | half the job of the four copies of "is this token-priced" (§9.3) |
| `reservation` | quantity upper bounds: `max_images`, `max_tokens{text_input, image_input, output}?`, `max_credits?` | the xAI branch in `execute.rs:156-190`; the `body_len` approximation in the token bound |

**Input roles and limits**: the component maps keys to roles (keeping today's "take the first key that appears"
semantics, `precheck.rs:88-115`) and refuses in `prepare` (`invalid_request`) against the per-role limits declared in
`model-capabilities`; the host **no longer** interprets key names. Today's limits come from the host's capability
table (`capabilities.rs:1569-1620`: gpt-image-1 input images 16 / mask 1, xAI 3 or 5, Stability 1, Nano Banana
reference images 1, …) and move into the component with it. The host keeps only provider-independent checks: total
bytes within the northbound limit (100 MiB, `core/limits.rs:12`).

**Tier refusal for per-image pricing** (replaces `nano_banana_flat_tier_guard`): the host rule becomes generic — "for
a per-image-priced row, if a tier word reported by `prepare` has no price in that row's price list, 400 before
admission". The host no longer needs to know which model is Nano Banana. **Cost**: the refusal text becomes generic
host text, and today's text mentioning "Gemini image token pricing" (`precheck.rs:53-58`) cannot be kept verbatim;
P22 I4's "400 texts identical item by item" gives way here (accepted by lv on 2026-09-30, §17 Q10).

**Upstream refused before producing output**: this is not a pre-dispatch fact but an outcome in §9.2, judged by the
component per dialect. It replaces the host's status-code heuristic of today: `DispatchProbe::upstream_rejected`
treats only 4xx as "refused before output" (`execute.rs:204-222`), while MiniMax's refusals arrive as HTTP 200.

**Request body normalisation**: `prepare` produces the upstream request body directly, so normalisation happens inside
the component; the host reserves and settles using only the `tier` / `size` facts.

**A class of defect removed along the way**: the multipart edit surface today has two refusals that happen **after**
admission without calling `cancel_pre_dispatch` — `n` failing to parse (`handlers.rs:1103-1113`) and the refusal of
`stream` (`:1137-1142`) — so the reservation is reclaimed only by the stale sweep after about 120 seconds (the comment
at `reve.rs:845-846`). Once these validations move into `prepare`, all of them happen before admission. The host
should still fix them before migration (I0), or the dual run would pin "late refusal" as correct.

## 9. D6 — Metering facts, pricing form and host generic checks (per P21 DP1)

### 9.1 Metering forms and facts `ImageMeteringV1`

Closed vocabulary of metering forms: `tokens`, `images`, `credits`, `requests`, `upstream_cost`. Facts (all nullable;
**null and 0 are different facts**):

| Field | Meaning | Source today |
|---|---|---|
| `images_reported` | the output / billed image count the upstream **itself reports**, raw and uncapped | MiniMax `metadata.success_count` (`minimax.rs:186-197`), Bailian `usage.image_count` (`bailian.rs:290-305`) |
| `tokens` | `{text_input, image_input, cached_text_input, cached_image_input, text_output, image_output, total_input, total_output}` | Gemini `usageMetadata` by modality (`media.rs:146-212`), OpenAI / Azure `usage` (`azure.rs:101-150`) |
| `credits` | decimal string, ≤ 6 decimal places | Reve `credits_used` (`reve.rs:666-697`) |
| `upstream_cost` | `{currency: "USD", amount: "<decimal string>"}` | xAI `cost_in_usd_ticks` (1 µ$ = 10,000 ticks, `media.rs:891`), converted by the component into decimal dollars |

`requests` needs no count from the component: the host counts succeeded rounds itself. It appears only as a form,
saying that the upstream bills per call (the `price_per_request` column GMI uses today, `models.rs:204-210`).

**The delivered image count is counted by the host, not reported by the component**: the host counts primary
artifacts. Rules such as "settled count ≤ delivered + 1, otherwise use the delivered count" are host funds policy;
they stay in the host and apply uniformly to every component. **This is inconsistent with the existing
`task-wan-image-v2`**: it writes "≤ delivered + 1" into the component (`reference_wan_image_task_v2.rs:10-14`,
`:129-138`), whereas the contract 6 record says explicitly that this is server policy and stays out of the contract.
This world follows contract 6's intent; whether the task side reclaims the rule is a separate question (§17 Q9).

The token buckets are finer than the host's today: the host's `ImageTokenUsage` has only text / image / cached (a
single bucket) / output (`usage_types.rs:360-381`). The component reports what the upstream reports and the host folds
it itself; how to fold is pricing policy.

### 9.2 Outcome `ImageOutcomeV1` (per round)

| Outcome | Meaning | How the host uses it (today's counterpart) |
|---|---|---|
| `succeeded {artifacts, metering, extras}` | there are deliverable artifacts; `metering` must contain every form `prepare` declared | verify the artifacts, then settle (§10) |
| `rejected {error}` | the upstream explicitly refused: **no output, no charge** | replaces `DispatchProbe.rejected_before_output`; the held path releases the reservation on it (`durable.rs:1283-1292`) |
| `charged_failure {error, metering}` | the upstream charged but there is no deliverable artifact | Reve settles a policy violation first and then turns it into a 400 (`reve.rs:703-733`); when every Ideogram result is removed by the safety filter, the host today returns 422 and declares no charge, which corresponds to `rejected` (`ideogram.rs:366-381`) |
| `unknown {reason, error?}` | cannot be decided (5xx, unparsable body, declared metering missing) | `delivery_unknown` / parked for manual review |

**Missing metering is `unknown`, never 0**: if `prepare` declared `tokens` and the upstream returns 2xx without
usage, the component must return `unknown`. This turns P22-F2's fix (OpenAI-compatible with missing usage → 502,
`openai_compat.rs:206-222`) into contract, and closes a hole that still settles at $0 today: token-priced Gemini image
output gets all-zero usage when `usageMetadata` is missing (`media.rs:147-149`; neither of the two call sites,
generation `gemini.rs:406` and edit `:778`, has a gate).

Telling `rejected` from `unknown` is left to the component, but **whether the synchronous path refunds on it is host
funds policy**. Today the synchronous path records any error after sending as `delivery_unknown`
(`handlers.rs:662-672`), while the held path releases the reservation on 4xx — the two paths already disagree. This
world only supplies the facts; during the dual run each path keeps today's behaviour, and afterwards the two are
unified as "`rejected` releases the reservation" (ruled by lv on 2026-09-30, §17 Q7).

### 9.3 Pricing form: the component declares "what it will report", the host keeps a single decision point

The requirement as written asks the component to "declare the pricing form, so that the host stops maintaining four
copies of 'is this token-priced'". **Taken literally, that crosses south's vocabulary line**: ARCHITECTURE.md says
South carries only what the upstream **reported**, while prices, tiers and price lists stay in the host
(`ARCHITECTURE.md:103-116`). The server's own comment frames it the same way: `image_model_token_priced` "is a decision
about the **host's pricing form**, not an Azure dialect" (`media.rs:1118-1119`). For the same upstream model the host
can perfectly well choose to sell by token or by image (Nano Banana does exactly that: the upstream bills by token,
and the row can be configured with a per-image price).

So the division is:

- The **component** declares, in `model-capabilities` and `prepare.facts.metering_forms`, **which metering forms it can
  report** — an upstream fact.
- The **host** keeps **one single** pricing-form decision function, whose inputs are the catalog row and the
  component's declaration and whose output is per token / per image / per credit / per request; it passes the
  conclusion to `prepare` as `context.metering_required`. If the component cannot report the required form, `prepare`
  returns a capability error and the host refuses before admission.
- Collapsing the four copies (`execute.rs:133-137`, `handlers.rs:972-974`, `media.rs:1120-1132`, `gemini.rs:979-986`)
  into one is the **host's** job. P22-F1's counterexample (a row configured only with
  `image_cached_image_input_price_per_million`) can also only be solved in the host: `cached_image_input` can be
  reported on its own in the component's facts; whether to use it, and at what price, is a reading the host pricing
  function has to add.

`metering_required` has one more use: it lets the component ask the upstream for enough evidence when needed (on the
speech surface, OpenAI TTS relies on it to decide whether to add `stream_format: "sse"`).

### 9.4 Host generic checks (dialect-independent; DP1's "bounds")

Violating any one → the request is routed to manual review (synchronous path `delivery_unknown`, held path
`park_held_uncertain`) and is not settled automatically:

1. Structure: `succeeded` must have at least one primary artifact; `metering` covers every form in `metering_forms`.
2. Internal consistency: `cached_text_input ≤ text_input`, `cached_image_input ≤ image_input`; when buckets and
   `total_*` both appear, they are equal.
3. Upper bounds: each token bucket ≤ the bound actually used when reserving (`prepare.reservation.max_tokens` if
   given, otherwise the host default); `credits ≤ max_credits`. An out-of-bound `images_reported` is **not** routed to
   review: per the host's image-count policy it falls back to the delivered count (as MiniMax and Bailian do today,
   `minimax.rs:186-197`).
4. Tier: `upstream_cost` is used to pick a tier among `tier_candidates` — the default candidate first, then in
   candidate order, taking the first tier whose upstream list price in the host price list equals the quoted cost; if
   none match, take `default` and log a warning. This is xAI's behaviour today (`xai.rs:260-293`), written as a
   provider-independent rule. The bill always follows the host price list, never the upstream quote.
5. Amount: settlement ≤ reservation (the existing `authorized_max_cost`).

These out-of-bound and internal-consistency checks catch only out-of-bound values and self-contradiction; **they
cannot catch under-reporting or deviation within the bounds** — the trust boundary DP1 has accepted (the same
undetectable zone as the boundary record §6.3), covered instead by pinned digests, §12's metering samples and the
pre-cutover dual run.

## 10. D7 — Artifact forms, delivery order and GatewayHeld

### 10.1 Artifact forms `ImageArtifactV1`

| Form | Description | Example |
|---|---|---|
| `inline {pointer, encoding, media_type}` | a string in the response view (usually already elided); `encoding` is `base64` / `data_url` | Azure, Stability, Gemini, OpenAI b64 |
| `body {media_type}` | the whole response body is the image (`binary` framing) | Reve returning binary per `Accept` |
| `url {url, media_type?}` | an absolute https URL from the upstream, ≤ 8 KiB (same value as `MAX_ARTIFACT_REF_BYTES`) | MiniMax, Bailian, Ideogram |

The component declares `media_type`. This fixes a host hard-code along the way: when the held path stores b64
artifacts, the content-type is always written as `image/png` (`durable.rs:1416`), and URL artifacts without a
content-type also fall back to `image/png` (`:1456`); Gemini's `inlineData` carries its own `mimeType`, which is
dropped today. The first version has no artifact roles: the image surface has no companion artifacts, and task
contract 7's rule is not to reserve values that have no consumer.

### 10.2 Delivery order: verify-then-settle (DI6 = A)

`render`'s template decides whether each artifact is delivered as `b64_json` or as `url`. Host obligations:

1. Artifacts the template delivers as **bytes** must be obtained and verified before settlement: an `inline` artifact
   decodes successfully and is non-empty; a `url` artifact is fetched successfully by the safe fetch executor of §11,
   non-empty and within the limit.
2. Any failure → `delivery_unknown` (parked for manual review; neither charged in full nor refunded automatically),
   and **no** `finalize`.
3. Only after all pass: `finalize`, then fill in the template and deliver.

This changes Ideogram's behaviour today: it `finalize`s first (`ideogram.rs:473`) and downloads afterwards (`:493`); if
the download fails, the client gets a 502 and the charge stands (`:488-491`). Agreement with the old behaviour in the
dual run cannot prove delivery is correct, so I3 acceptance adds one fixture each for download 404 / timeout / empty
body, asserting the new ledger state (P22 DI6). Artifacts delivered as `url` are not fetched on the synchronous path,
as today.

### 10.3 GatewayHeld: the same functions, the same executor (P22 I4)

Today the held path has an in-process background task call **the same native arms** (`prepare_held_arm` / `HeldArm`,
`durable.rs:726-737`, `:802-884`), with a 120-second lease renewed every 30 seconds (`:711-712`), and orphans turned
into `status_unknown` by `sweep_gateway_held_orphans` (`repo/reconcile.rs:79-133`). This world's commitments to it:

- `model-capabilities`, `prepare`, `parse-response` and `render` are all pure functions with no clock and no
  randomness, and `state` is bounded and serializable: the host can write `prepare`'s result together with its facts
  into the task snapshot before returning 202, and the background task then sends, parses and renders **step for step
  the same** as the synchronous path.
- The mapping from outcomes to held terminal states is host-generic: `succeeded` → `finish_held_success` (store first,
  then write the terminal state); `rejected` → `finish_held_failure`; `charged_failure` → a failed terminal state after
  settlement; `unknown` or a failed check → `park_held_uncertain`.
- Held storage always needs the bytes, so every artifact on the held path goes through §10.2's verification. Storing
  `url` artifacts today already uses the safe fetch client, sends `Accept: image/*`, and writes to disk within
  `max_artifact_bytes` (`durable.rs:1390-1487`).
- Held input bytes live only in process memory: a process restart leads to the orphan sweep; no blob persistence is
  needed.

The per-type list in `held_arm_is_wired` (`durable.rs:761-777`) becomes "the model row has an image component route";
which comes first between it and the task world's `component_dispatch_for` (`handlers.rs:169-202`) must be pinned by
host tests — a host obligation, listed in §12.3.

## 11. D8 — The safe fetch executor (shared by the image and speech second hops, DV2)

An absolute-URL second hop is not a provider call. South's `GetRequestV1` accepts only relative paths under the bound
endpoint, and 0.26.0 already states that such fetches "neither can go through South at all … host artifact fetches"
(`2026-09-09-buffered-binary-response.md:180-182`). So execution is in the host; this world sets the **rules**, and the
component supplies only the URL and the expected media type.

**Host obligations (a single executor, shared by image and speech)**:

1. `https` only; refuse userinfo; a host name is required; refuse `localhost` and `*.localhost`.
2. Resolve DNS before connecting (with a timeout); if **any** resolved address falls in a forbidden range, refuse
   outright; then pin the connection to the checked address.
3. The forbidden ranges include at least: loopback, RFC 1918 private, link-local (including the `169.254.169.254`
   metadata address), unspecified, broadcast, multicast, `100.64/10`, `0/8`, `192.0.0/24`, `198.18/15`, `240/4`; IPv6
   loopback, unspecified, multicast, `fc00::/7`, `fe80::/10`; v4-mapped addresses are rechecked as IPv4.
4. Do not follow redirects (3xx is a failure); **disable the system proxy** (behind a proxy, the proxy re-resolves and
   the address pin is void — exactly the server E-1 gap).
5. Attach no credential headers and no cookies; send only `Accept`.
6. A total timeout and a byte limit; an empty body is a failure. The limit is the minimum of (the host limit,
   `MAX_BINARY_RESPONSE_BODY_BYTES` 64 MiB).
7. The result's media type is the one the component declared; the upstream's `content-type` is only recorded (today
   neither the image second hop nor the Bailian TTS second hop validates it).

The server's current `guarded_asset_client` (`webhook_sender.rs:491-513`, forbidden ranges `domain.rs:1346-1378`)
already satisfies 1–6, and additionally allows only ports 443 / 8443. It has a development escape hatch,
`asset_fetch_unsafe_allow_private_destinations`: when enabled it allows http and skips DNS pinning. That is host
configuration; this world's obligation is that **production must not enable it**, and the host is advised to refuse
it for the production profile in its startup gate. South's own reqwest transport has long used `.no_proxy()` +
`Policy::none()` (`south-transport-reqwest/src/lib.rs:92-96`); the rules point in the same direction.

**D8b — what South provides (recommended)**: two pure functions plus a set of test vectors; execution stays in the
host. `ArtifactUrlV1::parse` (scheme, userinfo, host, length — the same shape as `ProviderEndpointV1::parse`'s checks
on an endpoint, `lib.rs:427-458`) and `is_forbidden_egress_address(IpAddr) -> bool`; plus a host-obligation suite
`south.safe-fetch.v1` (gate ③) that uses an injectable resolver and transport to assert that these vectors are all
refused: http, userinfo, `127.0.0.1`, `169.254.169.254`, a domain resolving to a private range, `::ffff:10.0.0.1`, a
302 to an internal address, and a proxy environment. Both hosts then execute one rule set, instead of each writing one
and each missing something. Acceptance is for the south maintainers to decide (§17 Q6).

## 12. D9 — Conformance suite `south.image-component.v1` and host obligations

### 12.1 Component suite

Fixture naming follows `image-v1.<family>.<case>.{input,expected}.json`, with the families `capabilities`, `prepare`,
`response` and `render`. The four check kinds of the task v2 suite carry over (coverage, fixture equality,
determinism, unknown-field tolerance, `task_suite_v2.rs:125-218`), plus these **required rows**:

| Check | Requirement |
|---|---|
| `metering_sample` | for every metering form the component declares, at least one `response` fixture gives **exact** metering facts with `succeeded` |
| `missing_meter_is_not_zero` | for every form carried in the response body (`tokens` / `credits` / `images_reported` / `upstream_cost`), at least one 2xx fixture missing that field, whose outcome must be `unknown` |
| `terminal_only_from_the_wire` | at least one non-2xx fixture; no non-2xx may produce `succeeded` |
| `pre_dispatch_refusal` | for every input role with a declared limit, at least one over-limit `prepare` fixture that returns `invalid_request` and produces no descriptor |
| `reference_integrity` (a structural check, run on every output) | every `$south.ref` points to a blob that exists in the view; every `$south.artifact` points to an existing artifact; an `inline` pointer lands on a string; no output string exceeds the fallback threshold (the component must not smuggle bytes out of the sandbox) |
| `endpoint_confinement` | the descriptor's `path` is relative and passes the grammar |

`CheckV1` is a closed enum (`report.rs:19-64`); a new check is a new variant, an additive change to the conformance
crate.

**On "south has not a single usage sample"**: P21 S5's statement is inaccurate — existing fixtures under
`crates/south-component-conformance/` do carry usage expectations (e.g. `fixtures/provider.response.cached-usage.*`,
`fixtures/provider.stream.no-usage.*`, `fixtures-wan-image-task-v2/task-v2.observation.caps-absurd-count.*`). What is
missing is **enforcement**: no existing check requires a component to carry metering samples (`report.rs:19-64` has no
such variant). The first two rows above add exactly that. The boundary record reaches the same finding for the
provider world (§6.1 there) and enforces usage rows by name in gate ② (§6.2 there).

### 12.2 Metering samples are DP1's "usage samples"

The `metering_sample` and `missing_meter_is_not_zero` rows are exactly what P21 DP1 requires as "passing conformance
with usage samples". The samples are transcribed from the server's native arms (the P13 S8 pattern: native arm →
hand-written frozen expectation → real wasm comparison), never back-derived from component output.

### 12.3 Host obligations (gate ③; a host suite, not part of the component suite)

1. Determinism of §6.2's elision rules (same document, same declarations → same view); refusal of `$south.` keys.
2. The §6.7 encoder: if D3b-B is adopted, the host uses south's encoder; if A, the host's encoded result decodes back
   to a part list equal to the original part list.
3. Every §11 safe fetch vector is refused.
4. §10.2's verify-then-settle ordering: no `finalize` when a fetch fails.
5. No extra configuration is injected into `immutable_body_paths`, their ancestors or their descendants (same rule as
   task contract 6 §4).
6. Undeclared operations and undeclared metering forms are refused before admission.
7. The precedence between image component routes and task component routes is pinned by a test; a model never hits
   both.

## 13. Mapping of the existing execution arms

| Execution arm | Request | Auth | Artifact | Metering form | World capability needed |
|---|---|---|---|---|---|
| Gemini (generate / edit) | JSON `:generateContent`; reference / edit images inlined via `base64`; an edit with a mask → refused in `prepare` | `header_secret` `x-goog-api-key` | `inline base64` | `tokens` or `images` | `repeat`; response elision paths |
| Vertex (Nano Banana) | same, with project / region in the URL, taken from an exported credential attribute or non-secret config (boundary record §3.3, §7.3) | `bearer`; the slot is `minted` by the service-account credential recipe (boundary record §3.3) | same | same | same + credential recipe (boundary record §3, phase B4) |
| Azure MAI / Foundry | JSON, `size → width/height`; n=1, b64 only (`azure.rs:14-39,204-227`); edit is multipart | `header_secret` `api-key` | `inline base64` | `tokens` (missing → `unknown`) or `images` | — |
| MiniMax | JSON, `GroupId` query; `b64_json → base64` | `bearer` | `url` or `inline base64` | `images` (`images_reported = success_count`) | error detection on HTTP 200 |
| Bailian Qwen-Image | JSON, `size → W*H`, n 1..=6 | `bearer` | `url` | `images` (`image_count`) | — |
| Ideogram | **multipart** described by the component (`text_prompt`, `rendering_speed`, `resolution`), n=1 | `header_secret` `api-key` | `url`; b64 delivery goes through the second hop | `images`, tier word `speed` | multipart encoding; safe fetch; §10.2 |
| Stability | **multipart**, `Accept: application/json`, n=1, b64 only | `bearer` | `inline base64` | `images` (`finish_reason = CONTENT_FILTERED` is also charged today, `stability.rs:548-583`) | multipart encoding |
| xAI generate | JSON, `size → resolution` inside the component | `bearer` | as the upstream returns it | `images` + `upstream_cost` + `tier_candidates` | tier evidence |
| xAI edit (P18) | JSON, `images` / `image` → `input_image`; the client's data URL `as_is` | `bearer` | as the upstream returns it | same; the input image count goes into `inputs` | request-side elision paths |
| OpenAI-compatible fallback (DI4) | JSON with only `model` changed; edit is a multipart part list, each part `as_is`, only `model` changed | `bearer` / `header_secret` | `inline` or `url` | `tokens` or `images` | render fidelity (§7) |
| Reve bridge | JSON `/v1/image/create`, n=1, png only | `bearer` | `inline` or `body` | `credits` | `charged_failure` |

The OpenAI-compatible fallback moves into a component (DI4), overturning P18 D5's "stays in the host". P18 worried
about "one more copy and more latency": in this design that copy happens in the host-side encoder and the bytes do not
enter the sandbox, so the cost is one memory copy, not a round trip across the wasm boundary. The host has to keep GMI
from silently falling into this fallback once its arm is deleted (P22 §8); that is the job of host routing.

## 14. Migration order and dual-run acceptance

**Order** (P22 I2–I4 merged with P18 I3; the first batch of one south minor ships the world, the contracts, the
encoder and the Azure and xAI components):

1. **I2**: Azure (generate + edit), xAI (generate + edit). Pure JSON, lowest risk; Azure covers "missing usage is
   `unknown`", xAI covers tier evidence and request-side elision.
2. **I3-1**: MiniMax, Bailian Qwen-Image, the OpenAI-compatible fallback.
3. **I3-2**: Gemini. **Prerequisite**: the host first fixes "missing `usageMetadata` settles at $0" per §9.2, or the
   dual run would pin this defect as correct (the same risk as P21 §6).
4. **I3-3**: Vertex, waiting for credential recipe v1 (boundary record §3; phase B4 there, which unlocks P21 S3 and
   P22 Vertex).
5. **I3-4**: Stability, needs the host-side encoder (§6.7).
6. **I3-5**: Ideogram, needs the safe fetch executor and §10.2; acceptance adds the three download-failure fixtures.
7. **I3-6**: the Reve bridge, in the same batch as P18's Reve edit, so that neither side migrates twice.

At each provider's cutover, the synchronous path and the held path switch to the component **at the same time**
(P22 I4); the native arm is deleted only after that provider's in-flight rows with `advance_mode='gateway_held'` are at
0 or have been swept and handled.

**Dual-run reconciliation** (per provider, both billing forms, per P13 D8):

| Compared item | How |
|---|---|
| Upstream request | JSON body equal by value; multipart equal as **decoded part lists** (name, order, file name, media type, byte digest), boundary not compared; headers equal |
| Client response | JSON equal by value (the host re-serializes; whitespace and key order not compared); bytes equal after b64 decoding |
| Pre-dispatch refusal | status code and timing (before admission) equal; the text may differ where §8 lists the generalisation |
| Reservation bound | equal |
| Settlement | metering kinds, values and amounts equal |
| Held path | 202 → terminal state → artifact → settlement, equal item by item |

**What the dual run cannot prove**: the same as the old behaviour ≠ correct. The Ideogram download failure (§10.2)
and Gemini's missing usage (§9.2) need fixtures written against the new ledger state and must not be accepted as
"matches native". In addition, run a negative case with images on a synthetic provider to prove that host precheck
does not recognise provider names (J2) — the image-world counterpart of the boundary record's unseen-provider guest
(T21), §12 there.

## 15. Versioning

A South **minor**. Released worlds and contracts are unchanged:

- A new WIT file `crates/south-provider-api/wit/image-adapter.wit`; `manifest.rs` gains `IMAGE_WIT_PACKAGE`,
  `IMAGE_WORLD`, `IMAGE_BEHAVIOR_SUITE`, `IMAGE_CAPABILITIES` (`generate`, `edit`) and `IMAGE_WORLD_SCHEMA`, one row in
  `KNOWN_WORLDS` (`manifest.rs:151-152`), and a branch in `validate_role`: "at least one operation word"
  (`:401-430`).
- Runtime: `bindings.rs` gains one `bindgen!` module, `InstanceKind` gains one variant (`component.rs:73-77`), and the
  link condition for the host import becomes a world property (`:182`). **The runtime limits are unchanged** —
  exactly the payoff of §6.
- `south-contracts`: new modules `media` (request view, descriptor, transforms, response view, the safe fetch pure
  functions) and `image` (facts, outcomes, artifacts). JSON codecs and types containing `ErrorEnvelope` go into
  conformance, following the task v2 precedent (`component_v2.rs:13-37`).
- `compatibility.json`: `contracts.media: 1`, `contracts.image: 1`, `media_limits` (fallback threshold, count and
  length of elision paths, `repeat` limit, artifact URL length, part count limit),
  `conformance.image_component_v1_suite_id`, the per-crate capability strings, and a host verification block
  (`not_verified` at first release).
- `HTTP_CONTRACT_VERSION` is unchanged (§6.3: the image surface falls within the three existing shapes).
- If this lands together with the compatibility range of the boundary record §8, image packages declare
  `contracts: {"media": 1, "image": 1}` (as embeddings packages declare `{"embeddings": 1}`, embeddings record §12).

## 16. Rejected alternatives

- **Reuse the task world's "terminal on submit"** (rejected by DI2): §5.
- **Add exports to provider-adapter-v2**: effectively a major, rebuilding the four text components (P18 §3,
  architecture).
- **Bytes into the sandbox with relaxed runtime limits** (option A of P18 D3): the payload limit would rise from
  16 MiB to the hundreds-of-MiB range and memory from 64 MiB to hundreds of MiB, while ordinary calls go through a
  single instance serially (`component.rs:136`), so large image requests would queue behind one another; for
  concurrency the host could only open more instances, multiplying memory by the instance count. Option B keeps all of
  this outside the sandbox, at the cost of one closed transform table.
- **Generic host rendering of the northbound body**: loses the field fidelity of the OpenAI-compatible pass-through,
  and cannot aggregate Gemini's multiple rounds (§7).
- **The component declares "token-priced"**: crosses the vocabulary line (§9.3).
- **The component infers the xAI tier from `cost_in_usd_ticks` on its own**: requires the component to know upstream
  price lists, which is pricing knowledge; instead the component reports the upstream quote and the host matches it
  against the candidate tiers (§9.4 item 4).
- **An `oauth` auth arm for Vertex, added in a later minor** (this record's earlier draft): superseded by the boundary
  record §3 — the component declares a credential recipe, the host executes it and presents the minted slot as
  `bearer`. The boundary record proposes deprecating the `oauth` arm (§3.7 there), so this world never admits it.
- **One synchronous media world holding image + speech (+ embeddings)**: the image and speech WIT signatures are
  almost identical (`model-capabilities` / `prepare` / `parse-response`, plus `render` for image; the embeddings world
  of the embeddings record §4 has a different function set), one component could serve the same upstream's image and
  speech (OpenAI-compatible, MiniMax, xAI, Bailian and Vertex all have both), and the runtime and the suites would
  each need one copy fewer. **Why it is not recommended**: P21 S2 / DP5 plans to relax the version gate from "exactly
  equal" to a compatibility range (boundary record §8); once relaxed, world version coupling becomes a real cost again
  — when speech adds a streaming export (DV4, a separate project), it would force every image component to be rebuilt.
  lv accepted this trade-off on 2026-09-30 in favour of separate worlds; the south maintainers are asked to rule on
  it as well (§17 Q4).

## 17. Open questions

| # | Question | Recommendation | Decided by |
|---|---|---|---|
| Q1 | Multipart encoder and byte transforms as south host-side pure functions, or written by each host (§6.7) | South provides them; revise the "no encoder" half-sentence of 0.25.0 §2 | south maintainers |
| Q2 | Elision rules: declared paths + fallback threshold; the threshold value; `unknown` when the `$south.` namespace collides with the upstream | As in §6.2, threshold 1 MiB | south maintainers |
| Q3 | First-version contents of the closed transform table (§6.4 + the three speech items) | As listed in the two records, nothing reserved | south maintainers |
| Q4 | Two worlds, or one synchronous media world (§16) | Separate, sharing `contracts.media` | lv — **ruled by lv, 2026-09-30: as recommended**; south maintainers — open |
| Q5 | ARCHITECTURE.md's "a metering vocabulary must have a second consumer in sight" (`ARCHITECTURE.md:114-115`). **Not met today**: the release notes of 0.25.0 / 0.26.0 both state that the community host has no multipart surface and no byte-returning surface (`2026-09-09-multipart-request-body.md:189`, `2026-09-09-buffered-binary-response.md:302`). The same question is open as the boundary record Q9, the speech record Q9 and the embeddings record E-Q5 | Write P21 §7's "synchronous implementation recommended" into the release record as a written commitment, and mark `media_component_capabilities` `not_verified` until the community host lands; otherwise this vocabulary serves only one host and, under the current rules, should not be admitted | lv + south maintainers |
| Q6 | Safe fetch: South provides the URL / address pure functions and the `south.safe-fetch.v1` host suite (§11 D8b) | Provide them | south maintainers |
| Q7 | Does the synchronous path release the reservation on `rejected` (today the synchronous path is always `delivery_unknown`, while held releases on 4xx) | Keep the status quo during the dual run; afterwards unify as "`rejected` releases" | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q8 | Are the tier word dimensions (`resolution` / `quality` / `speed`) closed; host price lists keyed by component tier words (a host schema change) | Three closed dimensions; a new dimension goes through a `contracts.image` version | south maintainers + server |
| Q9 | "Settled count ≤ delivered + 1" stays in the host; the released `task-wan-image-v2` writes it into the component — reclaim it? | The image world keeps it in the host; the task side reclaims it at the next contract upgrade | south maintainers |
| Q10 | The generalised 400 text differs from native (the Nano Banana tier gate) | Accept; P22 I4 acceptance compares status code and timing instead | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q11 | Vertex authentication is settled by the boundary record §3 (credential recipe; the `minted` slot is presented as `bearer`; this world admits no `oauth` arm, §4). What remains: is `host_signed` never admitted in this world | `host_signed` has no consumer and is not admitted | south maintainers |
| Q12 | Is the OpenAI-compatible fallback one component covering OpenAI / Azure OpenAI / DeepInfra / BytePlus / GLM, or one per provider | One component; differences via `supported_parameters` words | server + south |
| Q13 | Streaming image output (partial images) | Not included; a separate world version later | lv — **ruled by lv, 2026-09-30: as recommended** |
