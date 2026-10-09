# Image World: Synchronous Generation and Edit Share One World (image-adapter-v1)

Status: **accepted 2026-10-08** (rulings in §17; reconciled with south 0.50.0 in §18; implementation steps in §19);
proposed 2026-09-30 by the host team (token-station-server P21/P22/P23)

Date: 2026-09-30

Revised: 2026-10-01 after independent review; 2026-10-08 rulings and 2026-10-09 reconciliation with 0.50.0 (see the
revision notes at the end).

Rulings: on 2026-09-30 the host owner (lv) ruled on Q4, Q7, Q10 and Q13 (§17), each as recommended. On 2026-10-08 lv,
acting for the south maintainers, ruled every question tagged for them as recommended (Q1, Q2, Q3, Q6, Q9, Q11, Q14,
Q15, and the south half of Q4), ruled Q8, Q12 and Q16 as recommended for the south maintainers and for the server, and
ruled Q5 the same way as embeddings E-Q5; on the same day lv ruled DI6 = A in the host plan P22 (§10.2). Each ruling is
recorded under its question in §17. No ruling is reversed. §18 records three reconciliation findings: the contract
numbers in the Q14 ruling are stale (its substance stands; the bump is 11 → 12), and two points the rulings did not
cover (R-1, R-2: the model catalog of 0.50.0 overlaps the capability facts of §7/§8, and the spelling of tier words).
lv ruled both on 2026-10-09: R-1 = A (the catalog is authoritative for per-role limits and default words) and R-2
(tier words are case-sensitive, matched exactly, grammar widened to `[A-Za-z0-9_.-]{1,32}`); §7, §8, §12.1 and §19 are
revised accordingly and marked "revised 2026-10-09 per R-1" or "per R-2".


Baseline: south `main` = `3c1501a` (its code is identical to v0.42.0, `3135e36`; the merge added only design
records). Server line numbers come from host `a82c852b`; P22 / P18 were written at `d672b945` / `b660ea3f`, so their
line numbers have drifted — re-verify in place before citing. Kernel line numbers come from the
`token-station-protocol` revision south pins, `f585bc8` (`Cargo.toml:22`).

Reconciliation baseline (2026-10-09): south `origin/main` = `6aa2813` = v0.50.0. The citations above and in §1–§17 are
those of the baseline above; §18 lists the ones that moved and every assumption that changed. Host plans were read at
token-station-server `fbee0ed7`; host line numbers were not re-verified.

Citation convention: a south file name without a directory refers to this repository's crate sources — `lib.rs` and
`task_v2.rs` are in `crates/south-contracts/src/`; `manifest.rs` is in `crates/south-provider-api/src/` and `*.wit` in
`crates/south-provider-api/wit/`; `component.rs`, `runtime.rs`, `bindings.rs` and `loader.rs` are in
`crates/south-provider-runtime/src/`; `task_suite_v2.rs`, `report.rs`, `component_v2.rs` and `reference_*.rs` are in
`crates/south-component-conformance/src/`. `south-core`'s own `lib.rs` is always written `south-core/src/lib.rs`. The
kernel's `http.rs` and `usage.rs` are in `token-station-protocol`'s `crates/protocol/src/`. Server file names are
abbreviated relative to `gateway/src/modules/`: `handlers.rs`, `precheck.rs`, `execute.rs`, `durable.rs`, `gemini.rs`,
`azure.rs`, `minimax.rs`, `bailian.rs`, `ideogram.rs`, `stability.rs`, `xai.rs` and `openai_compat.rs` are in
`inference/handler/images/`; `reve.rs` is in `inference/handler/`; `media.rs` is in `inference/engine/token_counter/`;
`capabilities.rs`, `body_cap.rs` and `south_adapter.rs` are in `inference/engine/`; `webhook_sender.rs`, `domain.rs`
and `repo/reconcile.rs` are in `tasks/`; `usage_types.rs` is in `crates/gateway-provider-protocol/src/`, `models.rs`
in `gateway/src/infra/config/`, and `core/limits.rs` in `gateway/src/`.

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
admission, §6.3 host bounds, host checks and undetectable zone, its §6.4 rule that a `rejected` outcome releases
the reservation, and §8 compatibility range).

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
(`execute.rs:181-187`). P21 DP0 requires zero provider logic in the host, so all of these must move into components.

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
The transport has limits of its own, below the host's (§6.3a).

## 2. Scope and boundary statement

**In scope**: world shape and functions; byte handling for requests and responses; the transport sizing the image
surface needs; artifact forms and delivery order; metering facts and host generic checks; pre-dispatch facts; the
division of labour on pricing form; how the GatewayHeld path connects; the conformance suite and host obligations;
migration order and dual-run acceptance.

**Out of scope**: the host executor's implementation; the values of any price or reservation formula; the two
asynchronous arms Wan / GMI (task world, P13 S14); Bailian's native image surface and Reve's three native routes
(P25); streaming image output (partial images); Vertex service-account minting itself (P21 S3; specified as credential
recipe v1 in the boundary record §3, phase B4 there — this world only consumes the minted slot, §4).

**Boundary statement**: South still does not own the network, the clock, prices, reservations, persistence or
credential sources. This world only lets a component say, as a **pure function**, "what the request looks like and
what facts the response contains". The host holds the bytes; the component describes, by **reference**, where the
bytes are and which public standard encodes them (§6). That confines the host's new responsibilities to what P21 §1.1
allows: "generic executors for public standards, selected by component declaration". Every transform two hosts must
execute identically is a south pure function with golden vectors (§6.7), so "generic executor" never means "each host
writes its own".

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
presented as `bearer`, and the host admits the descriptor's auth by the rules of the boundary record §4.2, applied to
this world's descriptor type (§6.3). This world does not admit the `oauth` arm, which the boundary record proposes to
deprecate (§3.8 there); `host_signed` has no consumer and is not admitted. Runtime loading needs one change with it:
the host import is currently linked on `api_version != TASK_WORLD_V2` (`component.rs:182`). This world, the speech
world and the embeddings world all come without a `host` import, so the condition should become a world property
rather than an enumerated exclusion, and the import scan should refuse any `host` namespace for these worlds, as
`loader.rs:216-224` does for task-v2 (the embeddings record §4 does the same).

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
- Multipart northbound request (the edit surface, ASR): the host splits it into an **ordered** part list
  `parts: [{name, kind: "text", value} | {name, kind: "file", blob, filename?, media_type?, bytes}]`, keeping
  repeated fields in order of appearance. File parts appear only as a `blob` reference; their bytes do not enter the
  sandbox. A text part longer than the fallback threshold (§6.2) is elided the same way — it becomes
  `{name, kind: "text", blob, bytes, head}` — so a huge text field cannot push the view past the runtime payload
  limit. The host's multipart tooling today only scans and replaces in place and never splits out file parts
  (`inference/handler/audio/multipart.rs:560-683`). The splitter is a parser both hosts must run identically, so south
  supplies it: `parse_multipart_parts_v1` in `south-contracts::media` (§6.7).
- The host must not hand over any key starting with `$south.`: its appearance in northbound JSON is a 400 (placeholder
  namespace, §6.2).

### 6.2 Elision rules and `elide_v1` (deterministic; both hosts produce the same view byte for byte)

Both hosts must produce the same view from the same document, so the rules below are not a specification each host
implements: south supplies them as one pure function in `south-contracts::media`,

```text
elide_v1(document: &[u8], declared: &[PathPatternV1], limits: &MediaLimitsV1)
    -> Result<(ElidedViewV1, Vec<BlobV1>), ElisionErrorV1>
```

with golden vectors shipped next to it and the same fuzz obligation as every other grammar in `south-contracts`. The
host calls it (or, if it cannot link Rust, reproduces every golden vector byte for byte — host obligation §12.3).

A JSON string is replaced by the placeholder `{"$south.blob": {"id": …, "bytes": …, "head": …}}` in two cases:

1. **Declared paths**: path patterns the component declares in `model-capabilities` (request side) and in the result of
   `prepare` (response side) — RFC 6901 JSON Pointers in which a `*` segment matches any array index or key, e.g.
   `/image_url`, `/images/*/url`, `/data/*/b64_json`, `/candidates/*/content/parts/*/inlineData/data`. At most 32
   entries, each ≤ 256 bytes. A match is elided **regardless of length**.
2. **Fallback threshold**: any string value, at any position, longer than `MEDIA_MAX_INLINE_STRING_BYTES` (suggested
   1 MiB). It guarantees that the view stays within the runtime payload limit, and gives the component a deterministic
   shape to refuse when a huge string turns up at an undeclared position (for example, a prompt above 1 MiB is refused
   as `invalid_request`).

What `elide_v1` fixes, so that nothing is left to a host's choice:

- **Traversal**: depth first, in source order; object members keep their source order (the view is not re-sorted).
  A document with a duplicate object key is refused (`ElisionErrorV1::DuplicateKey`; the host answers 400 on the
  request side and `unknown` on the response side).
- **Ids**: `b0`, `b1`, … in traversal order; on the multipart side, one sequence over file parts and elided text parts
  in part order. Ids are local to one view.
- **`bytes`**: the length in bytes of the **decoded** string value (UTF-8, after JSON unescaping). **`head`**: the first
  64 bytes of the decoded value, truncated back to a UTF-8 boundary, so the component can recognise a prefix such as
  `data:image/png;base64,` without getting the content.
- **Keys** are never elided; an object key longer than the threshold refuses the document.
- **Serialization of the view**: compact (no insignificant whitespace), members in source order, and every number
  copied as its source text, so a number is never re-rounded on the way through.

If the elided view still exceeds the runtime `max_payload_bytes`: on the request side the host returns 413 / 400
before admission; on the response side the round is `unknown` (§9.2) — no settlement, no refund. The threshold is
1 MiB rather than smaller because legitimate text approaches tens of KiB (a 32,000-character CJK prompt is about
96 KB): eliding text the component needs to read would break the request.

A component that passes a value through unchanged (the OpenAI-compatible fallback copies the whole request) re-emits
each elided node as `{"$south.ref": {"blob": "<id>", "transform": "as_is"}}` (§6.3). A `$south.blob` node in any
component output is refused (`reference_integrity`, §12.1).

### 6.3 The request descriptor the component gives the host: `MediaRequestDescriptorV1`

Same role as the task world's `HttpRequestDescriptor`, but defined by south itself, because the kernel's can only
carry a JSON body:

| Field | Form |
|---|---|
| `method` | `POST` only. The earlier draft also allowed `GET`; no image or speech call needs it (second hops are host fetches, §11), and there is no binary-response GET to execute it with (`south-core/src/lib.rs:845-850`), so by task contract 7's rule it is not reserved |
| `path` | A relative path following the `RelativePathV1` grammar (`lib.rs:481`); the host authorizes it against the configured endpoint (EndpointConfinement) |
| `query` | Only the closed `QueryParameterV1` set (`lib.rs:1064-1090`; the image surface uses `GroupId` and `api-version`) |
| `headers` | `SafeHeaders` rules; a multipart body must not carry `content-type` (same rule as `lib.rs:1615`) |
| `auth` | `MediaAuthV1`: `{"arm": "bearer", "slot": "<slot>"}` or `{"arm": "header_secret", "header": "<name>", "slot": "<slot>"}`; the arm must be one the manifest declares, and the header one of `SecretHeaderV1` |
| `body` | `json {template}` / `multipart {parts}` / `text {media_type, text}` / `empty` (`text` is the speech world's SSML body; it is part of `contracts.media` v1, §6.4, and no image component uses it) |

**Auth admission.** `MediaAuthV1` maps one to one onto the contract's `ProviderAuthV1::Bearer` / `HeaderSecret`
(`lib.rs:1309-1323`). The boundary record §4.2 specifies admission as a function over the kernel descriptor's `Auth`,
which this descriptor does not carry (the kernel descriptor also carries an absolute `url`, kernel `http.rs:350-361`).
South therefore provides a twin, `admit_media_descriptor_auth(manifest, config, &MediaRequestDescriptorV1)`, in the
same crate and built on the same rule implementation, so the two cannot drift: the arm must be declared, the header
must be admitted, a `minted` slot is presented as `bearer`, and any mismatch is refused before admission.

`json.template` is plain JSON in which reference nodes `{"$south.ref": {"blob": "<id>", "transform": "<word>"}}` may
appear; the host replaces each with the transformed JSON string. `multipart.parts` is an ordered list:
`{name, value: "<text>"}` or `{name, blob, transform, filename?, media_type?}`. The host encodes the list with south's
encoder into `MultipartBodyV1` (the boundary is supplied by the host; contracts do not touch randomness, §6.7).

**Expansion happens before admission.** On both paths — including the GatewayHeld path, before its 202 — the host
expands every reference, encodes the body and builds the transport request (`JsonBodyV1` / `MultipartBodyV1`) before
admission, so every size refusal in §6.3a is a pre-admission 413 that moves no money.

### 6.3a Transport sizing: one new shape, HTTP contract 9 → 10

The earlier draft said every image request falls within the three shapes HTTP contract 9 already has and that no new
transport shape is needed. **That is withdrawn.** The shapes exist; their size limits do not fit the image surface:

| Limit | South | Host today | Consequence |
|---|---|---|---|
| JSON request body | 32 MiB (`MAX_JSON_REQUEST_BODY_BYTES`, `lib.rs:106`, enforced at `lib.rs:620`) | multipart edits up to 100 MiB (`core/limits.rs:12`) | an edit whose component inlines input images as base64 JSON (Gemini, Reve) can exceed 32 MiB |
| UTF-8 response body | 32 MiB (`MAX_RESPONSE_BODY_BYTES`, `lib.rs:121`) | 64 MiB success-body cap (`UPSTREAM_JSON_BODY_CAP`, `body_cap.rs:27`) | a b64 response between 32 and 64 MiB is refused **after** dispatch |
| Binary response body | 64 MiB (`MAX_BINARY_RESPONSE_BODY_BYTES`, `lib.rs:131`), JSON POST only (`execute_binary_call_v1`, `south-core/src/lib.rs:858`) | — | no multipart POST can read its response as bytes |

The response side is the serious one. The host already recorded why: a south refusal in the 32–64 MiB window happens
after dispatch, becomes `delivery_unknown`, and the manual review's answer is foregone — the upstream succeeded and
charged (`south_adapter.rs:833-841`). That is exactly the case "a failed delivery the customer is charged for" that
§10.2 exists to avoid.

**Rules for this world:**

1. **Every media-world call reads its response as bytes.** The host builds the response view itself (§6.5) and
   decodes UTF-8 / JSON outside the sandbox, so the UTF-8 entry points are never used here and the 32 MiB response
   window disappears for JSON POST, which already has `execute_binary_call_v1`.
2. **New shape: multipart POST with a binary response.** `execute_multipart_call_v1` reads UTF-8
   (`south-core/src/lib.rs:811-831`), and 0.26.0 declined a binary multipart twin because "no multipart call site
   answers in bytes" (`south-core/src/lib.rs:845-850`). The image edit surface is such a call site in effect: the
   OpenAI-compatible, Azure and Stability edits are multipart requests whose JSON answer carries up to ten b64 images.
   **Proposal**: add `execute_multipart_binary_call_v1` (same request type, response buffered as bytes up to
   `MAX_BINARY_RESPONSE_BODY_BYTES`) and its raw twin, following the 0.25.0 / 0.26.0 additive precedents.
   **One bump, `HTTP_CONTRACT_VERSION` 9 → 10, released in this world's minor, carries both new shapes**: this twin
   and the speech record's D3a `TextPostRequestV1` with its binary execution entry point. The ASR arms use this twin
   too (speech §5), since every media-world call reads its response as bytes (rule 1). Open question Q14 (S).
3. **Request side: no new shape.** A JSON body above 32 MiB is refused before admission (413) after expansion. The
   32 MiB bound is below the northbound 100 MiB, so some large inlined edits that a native arm without a south plan
   would send are refused; the upstreams concerned document inline request limits well below 32 MiB (inferred from
   their public documentation, to be confirmed per component), so raising the JSON limit buys nothing. Any refusal of
   this kind seen in the dual run is recorded (§14).

Raising `MAX_RESPONSE_BODY_BYTES` to 64 MiB instead was considered and rejected (§16): it would change the bound on
every text path to fix a media-only need.

### 6.4 Closed transforms `MediaTransformV1`

All are public standards, named by the component and implemented once, by south, as pure functions (§6.7):

| Word | Input → output | Use on the image surface |
|---|---|---|
| `as_is` | original string / original bytes | the OpenAI-compatible edit passes file parts through; xAI forwards the client's data URL unchanged |
| `base64` | bytes → base64 string | Gemini `inlineData.data`, Reve `reference_images` |
| `data_url` | bytes → `data:<media_type>;base64,…` | reference images for upstreams that only accept data URLs |
| `from_data_url` | data URL string → bytes | the client sends a data URL, the upstream wants a multipart file part |
| `from_base64` | base64 string → bytes | same, with the client sending bare base64 |

**`contracts.media` v1 carries the whole vocabulary both media worlds need, released once**: the five words above,
the speech words `from_hex`, `concat` and `wav_pcm_s16le{sample_rate, channels}` (speech record §6), the `text`
request body (§6.3; speech D3a) and the `sse` response body form (§6.5). The set is closed: adding a word after that
release is a `contracts.media` version change. The earlier draft declared `media: 1` in both records while speech
added words to it; releasing the full vocabulary together removes that contradiction.

### 6.5 The response view `MediaResponseViewV1` and `response_body_form`

In `prepare` the component declares this response's body form, `response_body_form`: `json | binary | text | sse`
(`sse` is used by the speech world). The earlier draft called this "framing"; it is renamed so it cannot be confused
with the provider world's `stream_framing` (boundary record §5.2: `bytes | aws-eventstream`). It follows the rule of
P21 S4 — the component names a form from a closed set and the host implements each form once. That record declines
`sse` because provider-world components receive raw chunks and split SSE themselves; in this world the body stays out
of the sandbox, so the host builds the view and therefore does the splitting, with south's `decode_sse_v1`, which
lives in `south-contracts` as the SSE sibling of the eventstream deframer (boundary record §5.2) and is released with
this world's minor.

From the declaration the host provides `{status, headers, body}`, where `body` is `{"json": <elided view>}`,
`{"text": "…"}` (≤ the fallback threshold), or `{"opaque": {"blob", "bytes", "media_type"}}` (binary, or over the
limit). A non-2xx body is tried as UTF-8 to give `json` / `text`, otherwise `opaque`. If a key prefixed with `$south.`
appears in the upstream body, the host judges the round `unknown` and does not hand it to the component (so an
upstream cannot forge references). `headers` follows the exclusion rules of `ResponseTranscriptV1` (`lib.rs:193`,
`RESPONSE_TRANSCRIPT_DENIED_HEADERS`) and contains no credential-class headers.

### 6.6 Relation to the 0.25.0 ruling: what agrees and what is revised

P22 I1 says option B "agrees with south 0.25.0's ruling that 'South does not encode multipart'". **That is only half
right**:

- **Agrees**: the transport contract is unchanged in kind. `MultipartBodyV1` is still encoded opaque bytes, and "This
  contract does not parse multipart" (`lib.rs:745`) still holds for the transport type.
- **Revised**: one of 0.25.0 D2's reasons was "South also has no business learning what a form field is"
  (`2026-09-09-multipart-request-body.md:201-207`). Under option B the component must state part names, file names and
  media types in the world vocabulary, and south supplies the part parser and the encoder (§6.7) — so South's
  **world layer** starts knowing what a form field is. This record explicitly narrows that sentence's scope: it
  continues to bind the transport layer, not the world layer.

One more argument 0.25.0 made does not hold here: it required **byte-for-byte** agreement with the host's old path,
because the host already had a correctly encoded body and only replaced in place. Once the component builds the body,
the host has changed encoders and the boundary necessarily differs, so the dual run can only compare **decoded part
lists** (§14).

### 6.7 D3b — Where the parser, the encoder, the elider and the transforms live

- **A — each host writes its own**: matches the letter of 0.25.0's "South ships no encoder", but the community host
  and the server each write one, the details (part order, part header order, line breaks, file-name escaping, elision
  ids) drift, and the conformance suite cannot assert anything about the results.
- **B — south provides host-side pure functions** (recommended): `south-contracts::media` provides
  `parse_multipart_parts_v1` (§6.1), `elide_v1` (§6.2), `encode_multipart_v1(parts, resolved_blobs, boundary) ->
  MultipartBodyV1` and the transforms of §6.4, with no I/O and no randomness (the caller supplies the boundary). They
  go in `south-contracts`, not `south-core`, because `south-core` introduces no parsing grammar and every grammar
  lives in `south-contracts` under its fuzz obligations (`south-core/src/raw.rs:11-13`). They run **outside** the
  sandbox, so large bytes still never enter wasm; both hosts share one implementation, and golden vectors let the
  suite assert byte for byte on every result. Cost: it overturns the half-sentence "South gains no … encoder" of
  0.25.0 §2 and adds a multipart parser, which needs a ruling from the south maintainers (§17 Q1). The SSE
  decoder `decode_sse_v1` that the speech world needs follows the same reasoning and also lives in `south-contracts`
  (boundary record §5.2).

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

    // ProviderConfig -> list<ImageModelCapabilitiesV1>.
    // Deterministic in (package digest, provider config).
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

- **`model-capabilities`** *(revised 2026-10-09 per R-1)*: static per-model declarations of what is bound to the
  dialect — supported operations, the mapping from request keys to input roles (§8), renderable `response_format`s,
  the metering forms and token buckets it will report (§9.1), the **names** of the tier dimensions it may report (§8),
  `repeat`, and request-side elision paths. It does **not** return per-role limits, parameter ranges or default tier
  words: those are catalog data (below). The result depends on the
  provider config as well as on the package (the catalog's `supported_parameters` words and other non-secret
  configuration), so the host caches it under the key **(package digest, digest of the canonical provider-config
  JSON)**, never under the package digest alone. When the host routes aliases or private deployments and the
  component cannot tell from the model name, follow the precedent of `2026-09-29-claude-model-dialect.md`: the
  catalog declares words in `ProviderConfig.models[].supported_parameters` and the component reads them (the
  dialect-word mechanism of the boundary record §7.4).
  *(Added 2026-10-09 per R-1.)* **Limits and default words live in the model catalog** (`south.model-catalog.v1`,
  boundary record §13.11): per-role maxima (`media.roles.<role>.max`), parameter options and the default word of a tier
  dimension (`params.resolution.default`). The host reads them by upstream model id and enforces them before `prepare`;
  South carries the document and does not interpret it. A model that is absent from the catalog has **no declaration**
  (boundary record §13.11: "no declaration", not "no capability"), and the host applies no catalog limit or default to it.
- **`prepare`**: a pure function the host calls **before admission and reservation**; it returns the pre-dispatch
  facts (§8), the request descriptor (§6.3), `repeat`, the `response_body_form` and response elision paths,
  `immutable_body_paths` (same semantics as task contract 6 §4), and a bounded `state` (≤ 8 KiB, no secrets, for use
  by the two later functions).
- **`parse-response`**: called once per upstream round; returns one of the four outcomes of §9.2. Error mapping is
  merged in here, with no separate `map-provider-error`: on the synchronous image surface only the component can tell
  "failed but charged" from "failed and not charged" (MiniMax's errors arrive as HTTP 200 + `base_resp`,
  `minimax.rs:356-369`). Because error mapping lives here, the auth-error rule of the provider world applies here too
  (§12.1).
- **`render`**: takes the outcomes of all succeeded rounds and the render context the host provides (the `created`
  timestamp and so on — the component has no clock), and produces a client-body template in which artifact positions
  are written as `{"$south.artifact": {"index": i, "as": "b64_json" | "url"}}` for the host to fill in. It lives in the
  component rather than in generic host rendering for two reasons: the OpenAI-compatible fallback today returns the
  upstream body to the client unchanged (`handlers.rs:650-656`), and generic host rendering would lose fields such as
  `usage`, `background` and `output_format`; and Gemini produces only one image per call, so n images take n loops
  (`gemini.rs:323`), and aggregating n rounds must happen in one function. Same precedent as the task world's
  `render-success` (`task-adapter-v2.wit:53-58`).

**Allowed artifact deliveries** (closed; §10.1 defines the forms):

| Artifact form | `as: "b64_json"` | `as: "url"` |
|---|---|---|
| `inline` | yes (host decodes and re-encodes as plain base64) | **no** in v1 |
| `body` | yes | **no** in v1 |
| `url` | yes (host fetches through §11) | yes (host passes the URL through) |

`inline` / `body` → `url` would need the host to store the bytes and mint a URL on the synchronous path. No arm does
that today — Azure and Stability refuse a `url` response format outright (`azure.rs:200-227`,
`stability.rs:119-145`) — so by task contract 7's rule it is not reserved. A component whose upstream cannot serve the
requested `response_format` refuses in `prepare` (`invalid_request`), as those arms do today.

**`render` failing after the upstream succeeded** (an `Err`, a template that violates the table above, or one that
fails `reference_integrity`): the upstream has produced and may have charged, so the call is `unknown` — the
synchronous path records `delivery_unknown`, the held path parks it, and there is **no** `finalize`. It is never
treated as a pre-dispatch refusal.

**`repeat`**: `prepare` may declare that one descriptor is sent k times (1 ≤ k ≤ 10), only for "one image per call"
upstreams such as Gemini / Vertex. The host executes the rounds sequentially and stops at the first non-succeeded
round. The aggregation rule is part of the contract: round 1 `rejected` → the whole call is `rejected`; any
non-success from round 2 on → the whole call is `unknown` (earlier rounds were already charged; consistent with
today's `DispatchProbe` being set only when `i==0`, `gemini.rs:394-396`). "k different descriptors" is not offered:
there is no consumer.

## 8. D5 — Pre-dispatch facts (each item P22 I1 lists, landed)

`PreparedImageCallV1.facts` — all request-side facts, no prices:

| Fact | Form | Which host provider logic it replaces |
|---|---|---|
| `operation` | `generate` / `edit` | — |
| `inputs` | counts per role `{input_image, reference_image, mask}`; closed vocabulary | `precheck.rs:88-155` interpreting keys and roles by `provider_type` |
| `requested_outputs` | image count sent upstream × `repeat` | the per-arm branches where the host parses `n` itself |
| `size` | `{width, height}`, when it can be determined | `image_size_hint` in `media.rs:31-36` |
| `tier` | tier words by dimension `{resolution?, quality?}`, grammar `[A-Za-z0-9_.-]{1,32}` (revised 2026-10-09 per R-2: words are case-sensitive and matched exactly; the component reports the upstream's spelling, such as `1K`, `720P` or xAI's `1k`) | xAI's `size→resolution` normalisation (`xai.rs:338-409`), the Nano Banana resolution gate (`precheck.rs:34-62`) |
| `tier_candidates` | `{candidates: [...], default}`: the candidate set when the upstream decides the tier | the xAI candidate price cards (`media.rs:994-1025`) |
| `metering_forms` | the metering forms this call will report on success (§9.1); must include every form in `context.metering_required` | half the job of the four copies of "is this token-priced" (§9.3) |
| `bounds` | optional **tightening** bounds: `max_images`, `max_tokens{text_input, image_input, output}`, `max_credits` | the xAI branch in `execute.rs:181-187`; the `body_len` approximation in the token bound |

**Bounds: this world's instance of the boundary record §6.3 rule.** The host computes every bound it checks from the
northbound request, with no provider knowledge. The image instance:

| Bound | Host computation | Do media bytes count? |
|---|---|---|
| text input tokens | bytes of the northbound request **excluding** media bytes (the byte length of the elided view, §6.2, plus the multipart text parts) | no |
| image input tokens | the host-configured allowance × the number of media parts (the view's file parts and elided blobs) | no — each media part counts once, whatever its size |
| output tokens | the host's authorized output cap × the northbound `n` | — |
| images | the northbound `n` (the host's own API field, already range-checked at `handlers.rs:133-138`) | — |
| credits | none: the host has no request-derived credit bound; credit-priced calls are checked only by §9.4 item 5 | — |

Media bytes are excluded because a 20 MiB image counted as text would make the text-input bound meaningless; the
per-part allowance is what bounds image input. A component may supply `bounds` only to **tighten** them. The
reservation uses `min(host bound, component bound)`; **every check in §9.4 compares against the host bound only**, so
a component-supplied number is never used both to reserve and to check that same component. A component that
tightens too far and then reports more is caught by check 5 (settlement ≤ reservation), not waved through. The
earlier draft let `prepare.reservation` set both the reservation and the check bound; that made the check exactly as
strong as the component's honesty. The host defaults today (`image_input_tokens: 0` on
generation, `execute.rs:175`; `cap` on edits, `handlers.rs:980`) are provider-independent but do not count reference
images on generation; the allowance per media part replaces them, so a token-priced generation with a reference image
is not sent to review for reporting image-input tokens.

**Input roles and limits** *(revised 2026-10-09 per R-1)*: the component maps keys to roles (keeping today's "take the
first key that appears" semantics, `precheck.rs:88-115`) and reports the counts in `inputs`; the host **no longer**
interprets key names. The per-role **limits** are catalog data, not component declarations: the model catalog carries
them per upstream model id (`media.roles.<role>.max`: gpt-image-1 input images 16 / mask 1, xAI, Stability 1, Nano Banana
reference images 1, …; today's source is the host's capability table, `capabilities.rs:1569-1620`, which the catalog
was exported from), and the host enforces them against the `inputs` fact, as it does today against its profile. A
model absent from the catalog has no declaration and no catalog limit applies. The component refuses in `prepare`
(`invalid_request`) only what is bound to its dialect and cannot be expressed as a catalog range (for example an edit
with a mask on Gemini). The host also keeps the provider-independent checks: total bytes within the northbound limit
(100 MiB, `core/limits.rs:12`).

**What a tier word is.** A tier word names a **request-time choice** that can differ between two requests to the same
model row, and that the upstream serves (and usually bills) differently: `resolution` (`1k`, `2k`, `4k`, …) and
`quality` (`low`, `medium`, `high`, …). It is a fact about the request, not a price, so it stays on the fact side of
ARCHITECTURE.md's line (`ARCHITECTURE.md:103-116`, which puts "tiers" among host pricing: the host still owns what
each tier costs). A value **fixed by the model row** is not a tier word: Ideogram's rendering speed is pinned by the
SKU, never a request parameter (`ideogram.rs:55-72`), so the component derives it from the model and reports no tier.
The earlier draft listed a `speed` dimension for Ideogram; it is removed, and with it the case in which a generic tier
rule would have refused every Ideogram request on its single-price row.

**Tier words against today's price columns.** Today no price is keyed by a tier word. Per-image prices are keyed by
the long edge of the `size` hint (`image_price`, `image_price_1k`, `image_price_2k`, `media.rs:38-50`); xAI's cards
pick between primary and alternative columns by upstream model family (`media.rs:920-929`). A host price list keyed by
tier words is therefore a **host schema change** (§17 Q8), and it is a **prerequisite** of every migration step that
relies on tier words: I2's xAI and I3-2's Nano Banana per-image rows (§14). Until it lands, those rows stay on their
native arms.

**Tier refusal for per-image pricing** (replaces `nano_banana_flat_tier_guard`, once Q8 has landed; *revised
2026-10-09 per R-1 and R-2*): `model-capabilities` declares the **names** of the tier dimensions a model may report
(Nano Banana: `resolution`); the default word of a dimension is read by the host from the model catalog
(`params.resolution.default`: `1K` for the Nano Banana entries), not declared by the component. Words are compared
exactly, as spelled (R-2). The host rule becomes generic: "for a per-image-priced row, every tier word `prepare`
reports must have a price in that row's tier-keyed list; a row with **no** tier-keyed list accepts only requests whose
tier words all equal the catalog default word of that dimension for the model; anything else is a 400 before
admission". A dimension for which the catalog has no default word (or a model the catalog does not describe) has no
declaration, and this rule has nothing to compare with there (§18.5, open point O-1). That reproduces today's gate — a
single-price Nano Banana row refuses `2k` and accepts `1k` or no resolution (`precheck.rs:47-60`; today's gate compares
case-insensitively, while words are now matched exactly, so a component that accepts a client's `1k` reports the
upstream's spelling `1K`) — without the host
knowing which model is Nano Banana, and a model with no tier dimensions (MiniMax, Ideogram) is never refused by it.
When a model declares a `resolution` dimension, the price is selected by the tier word and `size` is not used for
price selection, so the two facts never pick different columns. **Cost**: the refusal text becomes generic host text,
and today's text mentioning "Gemini image token pricing" (`precheck.rs:53-58`) cannot be kept verbatim; P22 I4's "400
texts identical item by item" gives way here (accepted by lv on 2026-09-30, §17 Q10).

**Upstream refused before producing output**: this is not a pre-dispatch fact but an outcome in §9.2, judged by the
component per dialect. It replaces the host's status-code heuristic of today: `DispatchProbe::upstream_rejected`
treats only 4xx as "refused before output" (`execute.rs:204-222`), while MiniMax's refusals arrive as HTTP 200.

**Request body normalisation**: `prepare` produces the upstream request body directly, so normalisation happens inside
the component; the host reserves and settles using only the `tier` / `size` facts.

**Late refusals on the multipart edit surface**: the earlier draft listed two refusals that happened after admission
without releasing the hold (`n` failing to parse and the refusal of `stream`). The host fixed this on 2026-09-30
(P22-F6, `b8f63414`): all of them now go through `cancel_pre_dispatch` (`handlers.rs:1150`). Moving these validations
into `prepare` keeps that result and moves them before admission altogether.

## 9. D6 — Metering facts, pricing form and host generic checks (per P21 DP1)

### 9.1 Metering forms, required facts and evidence facts `ImageMeteringV1`

Closed vocabulary of metering forms — the unit a row can be billed in: `tokens`, `images`, `credits`, `requests`.
Every fact is nullable; **null and 0 are different facts**. Facts come in two kinds, and the difference decides what
a missing fact does:

- A **required** fact is one the bill is computed from. Missing on a 2xx → the round is `unknown` (never 0).
- An **evidence** fact is something the upstream may report that the host uses to check or refine a bill it can
  compute without it. Missing → `null`, and the host applies its documented fallback. A component must never invent
  an evidence fact to fill the gap.

| Form | Required facts on `succeeded` | Evidence facts (missing → host fallback) | Required on `charged_failure` |
|---|---|---|---|
| `tokens` | every token bucket the model declares in `model-capabilities` (below) | — | the same buckets |
| `images` | none — the host counts delivered primary artifacts | `images_reported` (fallback: the delivered count, as today: `minimax.rs:186-197`, `bailian.rs:290-305`) | `images_reported` (nothing was delivered, so it is the only count) |
| `credits` | `credits` (decimal string, ≤ 6 decimal places) | — | `credits` |
| `requests` | none — the host counts succeeded rounds | — | none |

One more evidence fact belongs to no form: `upstream_cost` `{currency: "USD", amount: "<decimal string>"}`, the
upstream's own quote (xAI `cost_in_usd_ticks`, 1 µ$ = 10,000 ticks, `media.rs:891`, converted by the component into
decimal dollars with **up to 10 decimal places**, so one tick is representable and the host's exact-match rule in
§9.4 item 4 loses nothing). It is used only to recognise the served tier; when it is missing the host bills the
default candidate, as today (`XaiImageTierEvidence::NoReport`, `media.rs:1082`). The earlier draft listed
`upstream_cost` as a metering form and treated its absence as `unknown`, which would have parked xAI calls that are
billed correctly today; it was never a unit anything is billed in.

**Token buckets**: `{text_input, image_input, cached_text_input, cached_image_input, cached_input, text_output,
image_output, total_input, total_output}`. `cached_input` is new: the cached count the upstream reports **without**
saying whether it is text or image. OpenAI reports exactly that — one undifferentiated cached count
(`usage_types.rs:363-373`, read at `:393-396`) — and deciding where it goes is a pricing decision the host has already
made (it is deducted from the text bucket and billed at the cached-text rate, so it can never under-bill). Without the
bucket, the component would have to make that decision itself. The component reports what the upstream reports and
the host folds it; how to fold is pricing policy.

**Declared buckets**: in `model-capabilities`, each model declares which buckets its upstream reports (for example
Gemini: `text_input`, `image_input`, `image_output`, `cached_input`; OpenAI: `text_input`, `image_input`,
`cached_input`, `total_output`). On a 2xx, **every declared bucket must be non-null**, or the round is `unknown`. This
closes the partial-report gap the host still has on the image surface: its gates accept any non-zero bucket
(`ImageTokenUsage::has_usage`, `usage_types.rs:414-419`, used by `require_nano_banana_usage`, `gemini.rs:1001`, and
the OpenAI-compatible gate, `openai_compat.rs:206`), so a response missing only its output count still settles low —
the same class the host fixed for token-priced TTS in `a82c852b`. It is a behaviour change, listed in §14.

**The delivered image count is counted by the host, not reported by the component**: the host counts primary
artifacts. Rules such as "settled count ≤ delivered + 1, otherwise use the delivered count" are host funds policy;
they stay in the host and apply uniformly to every component. **This is inconsistent with the existing
`task-wan-image-v2`**: it writes "≤ delivered + 1" into the component (`reference_wan_image_task_v2.rs:10-14`,
`:129-138`), whereas the contract 6 record says explicitly that this is server policy and stays out of the contract
(`2026-09-27-task-contract-v6-facts.md:41`). This world follows contract 6's intent; whether the task side reclaims the
rule is a separate question (§17 Q9).

### 9.2 Outcome `ImageOutcomeV1` (per round)

| Outcome | Meaning | How the host uses it (today's counterpart) |
|---|---|---|
| `succeeded {artifacts, metering, extras}` | there are deliverable artifacts; `metering` carries every required fact (§9.1) of every form in `metering_forms` | verify the artifacts, then settle (§10) |
| `rejected {error}` | the upstream explicitly refused: **no output, no charge** | replaces `DispatchProbe.rejected_before_output`; releases the reservation under the boundary record §6.4 rule, after the dual run (below) |
| `charged_failure {error, metering}` | the upstream charged but there is no deliverable artifact | Reve settles a policy violation first and then turns it into a 400 (`reve.rs:703-733`) |
| `unknown {reason, error?}` | cannot be decided (5xx, unparsable body, a required fact missing, a failed `render`) | `delivery_unknown` / parked for manual review |

**Ideogram "every result removed by the safety filter"** is `rejected`: the upstream says nothing was produced and the
host today answers 422 with "nothing was billed" (`ideogram.rs:366-381`). Today that 422 is built by the host after a
2xx, without setting the rejection probe (the probe is set only on the upstream status, `ideogram.rs:340`), so on the
held path — which Ideogram uses (`durable.rs:767`) — it is parked, not released (`durable.rs:1287-1291`). Under
`rejected` it would be released. During the dual run each path keeps today's funds behaviour (below), so the dual run
compares equal; the change takes effect only afterwards and is accepted by a fixture of its own (§14). The
boundary record §6.4 describes `rejected` as "an upstream 4xx that proves nothing was produced"; here the upstream
answer is a 2xx that proves the same, which the component recognises per dialect. The outcome is the same, and the
§6.4 rule applies to it unchanged.

**Missing metering is `unknown`, never 0**: if a required fact is missing on a 2xx, the component returns `unknown`.
This turns the host's own gates into contract: P22-F2 (OpenAI-compatible with missing usage → 502,
`openai_compat.rs:206-222`) and P22-F5 (token-priced Gemini image output without `usageMetadata` → 502; fixed in
`b8f63414`, `gemini.rs:418` for generation and `:789` for edit — the earlier draft still described this as an open
$0 hole). The declared-bucket rule of §9.1 goes one step further than those gates.

**Refunds**: telling `rejected` from `unknown` is left to the component; **whether a path refunds on it is host funds
policy**. Today the synchronous path records any error after sending as `delivery_unknown` (`handlers.rs:662-672`),
while the held path releases the reservation on a first-round 4xx only (`durable.rs:1287-1291`). During the dual run
each path keeps that behaviour, which the host can express without provider knowledge (the held path releases a
`rejected` round only when its upstream status was 4xx). Afterwards the boundary record §6.4 rule applies: a `rejected`
outcome releases the reservation, on every path and in every world that has the outcome (ruled by lv on 2026-09-30,
§17 Q7; stated once in the boundary record §6.4).

### 9.3 Pricing form: the component declares "what it will report", the host keeps a single decision point

The requirement as written asks the component to "declare the pricing form, so that the host stops maintaining four
copies of 'is this token-priced'". **Taken literally, that crosses south's vocabulary line**: ARCHITECTURE.md says
South carries only what the upstream **reported**, while prices, tiers and price lists stay in the host
(`ARCHITECTURE.md:103-116`). The server's own comment frames it the same way: `image_model_token_priced` "is a decision
about the **host's pricing form**, not an Azure dialect" (`media.rs:1118-1119`). For the same upstream model the host
can perfectly well choose to sell by token or by image (Nano Banana does exactly that: the upstream bills by token,
and the row can be configured with a per-image price).

So the division is:

- The **component** declares, in `model-capabilities`, **which metering forms and token buckets it can report** — an
  upstream fact — and echoes the forms for this call in `prepare.facts.metering_forms`.
- The **host** keeps **one single** pricing-form decision function, whose inputs are the catalog row and the
  component's declaration and whose output is per token / per image / per credit / per request; it passes the
  conclusion to `prepare` as `context.metering_required`. If the component cannot report the required form, `prepare`
  returns a capability error; if `metering_forms` does not contain every form in `metering_required`, the host refuses
  before admission.
- Collapsing the four copies (`execute.rs:137-139`, `handlers.rs:972-974`, `media.rs:1120-1132`, `gemini.rs:982-990`)
  into one is the **host's** job. Nano Banana's copy is deliberately narrower (no generic-output fallback,
  `media.rs:1116-1117`), so collapsing it changes which Nano Banana rows count as token-priced; the host has to decide
  that explicitly. P22-F1's counterexample (a row configured only with `image_cached_image_input_price_per_million`)
  can also only be solved in the host: `cached_image_input` can be reported on its own in the component's facts;
  whether to use it, and at what price, is a reading the host pricing function has to add.

`metering_required` has one more use: it lets the component ask the upstream for enough evidence when needed (on the
speech surface, OpenAI TTS relies on it to decide whether to add `stream_format: "sse"`).

### 9.4 Host generic checks (dialect-independent; DP1's "bounds")

Violating any one → the request is routed to manual review (synchronous path `delivery_unknown`, held path
`park_held_uncertain`) and is not settled automatically:

1. Structure: `succeeded` must have at least one primary artifact; `metering` carries every required fact of every
   form in `metering_required` (§9.1).
2. Internal consistency: `cached_text_input ≤ text_input`, `cached_image_input ≤ image_input`,
   `cached_text_input + cached_image_input + cached_input ≤ text_input + image_input`; when buckets and `total_*` both
   appear, they are equal.
3. Upper bounds: each token bucket ≤ the **host** bound of §8 (never the component's own `bounds`); `credits` have
   no request-derived host bound and are checked by item 5 alone. An out-of-bound `images_reported` is **not** routed
   to review: it is an evidence fact, and per the host's image-count policy it falls back to the delivered count (as
   MiniMax and Bailian do today, `minimax.rs:186-197`).
4. Tier: `upstream_cost`, when present, is used to pick a tier among `tier_candidates` — the default candidate first,
   then in candidate order, taking the first tier whose upstream list price in the host price list equals the quoted
   cost; if none match, or `upstream_cost` is null, take `default` and log a warning. This is xAI's behaviour today
   (`media.rs:1066-1097`, called from `xai.rs:260-293`), written as a provider-independent rule. The bill always follows
   the host price list, never the upstream quote.
5. Amount: settlement ≤ reservation (the existing `authorized_max_cost`).

These out-of-bound and internal-consistency checks catch only out-of-bound values and self-contradiction; **they
cannot catch under-reporting or deviation within the bounds** — the trust boundary DP1 has accepted (the same
undetectable zone as the boundary record §6.3), covered instead by pinned digests, §12's metering samples and the
pre-cutover dual run. Because the bounds are the host's own, a component cannot widen the zone by declaring a larger
bound.

## 10. D7 — Artifact forms, delivery order and GatewayHeld

### 10.1 Artifact forms `ImageArtifactV1`

| Form | Description | Example |
|---|---|---|
| `inline {pointer, encoding, media_type}` | a string in the response view (usually already elided); `encoding` is `base64` / `data_url` | Azure, Stability, Gemini, OpenAI b64 |
| `body {media_type}` | the whole response body is the image (`binary` body form) | an upstream returning image bytes per `Accept` (no current arm on this surface; the Reve bridge pins `Accept: application/json`, `reve.rs:572`) |
| `url {pointer, media_type?}` | a JSON Pointer into the **unelided** upstream response view, landing on a string: an absolute https URL ≤ 8 KiB (same value as `MAX_ARTIFACT_REF_BYTES`) | MiniMax, Bailian, Ideogram |

**Why `url` is a pointer, not a URL.** The earlier draft let the component write the URL itself. The host then fetches
that URL (b64 delivery, held storage), so a component could have made the host request
`https://attacker.example/?q=<prompt>` — a network egress channel out of a sandbox that has no network, which
`endpoint_confinement` (it checks only the descriptor path) does not close and the safe fetch rules of §11 (which stop
SSRF, not exfiltration) do not either. With a pointer, the host reads the string from the upstream's own response; the
component chooses which of the upstream's URLs is the artifact, never what the URL is. Every image arm that answers
with a URL carries it verbatim in its JSON body, so nothing is lost. The speech world uses the same form,
`url {pointer, media_type}` (speech §6), so the rule holds in every media world. The same risk exists wherever a
task-world component names a URL the host later fetches; that is outside this record (inferred; not assessed here).

The component declares `media_type`. Today the held path sniffs b64 artifacts from their leading bytes and falls back
to `image/png` (P22-F7, `b8f63414`, `durable.rs:1418`), and URL artifacts without a content-type also fall back to
`image/png` (`durable.rs:1456-1459`); Gemini's `inlineData` carries its own `mimeType`, which is dropped today. The
component's declaration replaces the fallback; the host may keep sniffing as a generic consistency check (a mismatch is
logged; the declared type is delivered). `sniff_image_mime` lives in `gemini.rs` today and has to move to a neutral
place before the Gemini arm is deleted. The first version has no artifact roles: the image surface has no companion
artifacts, and task contract 7's rule is not to reserve values that have no consumer.

### 10.2 Delivery order: verify-then-settle (DI6 = A)

Ruled by lv on 2026-10-08 (host plan P22, DI6 = A); the host-side obligations below are unchanged by the ruling.

`render`'s template decides whether each artifact is delivered as `b64_json` or as `url` (within the table of §7).
Host obligations:

1. Artifacts the template delivers as **bytes** must be obtained and verified before settlement: an `inline` artifact
   decodes successfully and is non-empty; a `url` artifact is fetched successfully by the safe fetch executor of §11,
   non-empty and within the limit.
2. Any failure — including a `render` failure after the upstream succeeded (§7) → `delivery_unknown` (parked for
   manual review; neither charged in full nor refunded automatically), and **no** `finalize`.
3. Only after all pass: `finalize`, then fill in the template and deliver.

This changes Ideogram's behaviour today: it `finalize`s first (`ideogram.rs:473`) and downloads afterwards (`:493`); if
the download fails, the client gets a 502 and the charge stands (`:488-491`). Agreement with the old behaviour in the
dual run cannot prove delivery is correct, so I3 acceptance adds one fixture each for download 404 / timeout / empty
body, asserting the new ledger state (P22 DI6). Artifacts delivered as `url` are not fetched on the synchronous path,
as today.

### 10.3 GatewayHeld: the same functions, the same executor (P22 I4)

Today the held path has an in-process background task call **the same native arms** (`prepare_held_arm` / `HeldArm`,
`durable.rs:726-737`, `:802-884`), with a 120-second lease renewed every 30 seconds (`:711-712`), and orphans turned
into `status_unknown` by `sweep_gateway_held_orphans` (`repo/reconcile.rs:90`). This world's commitments to it:

- `model-capabilities`, `prepare`, `parse-response` and `render` are all pure functions with no clock and no
  randomness, and `state` is bounded and serializable: the host can write `prepare`'s result together with its facts
  into the task snapshot before returning 202, and the background task then sends, parses and renders **step for step
  the same** as the synchronous path. Reference expansion and body encoding also happen before the 202 (§6.3).
- The mapping from outcomes to held terminal states is host-generic: `succeeded` → `finish_held_success` (store first,
  then write the terminal state); `rejected` → `finish_held_failure` (during the dual run only when the round's
  upstream status was 4xx, otherwise `park_held_uncertain`, §9.2); `charged_failure` → a failed terminal state after
  settlement; `unknown` or a failed check → `park_held_uncertain`.
- Held storage always needs the bytes, so every artifact on the held path goes through §10.2's verification. Storing
  `url` artifacts today already uses the safe fetch client, sends `Accept: image/*`, and writes to disk within
  `max_artifact_bytes` (`durable.rs:1390-1487`).
- Held input bytes live only in process memory: a process restart leads to the orphan sweep; no blob persistence is
  needed.

The per-type list in `held_arm_is_wired` (`durable.rs:761-777`) becomes "the model row has an image component route";
which comes first between it and the task world's component route (`component_dispatch_for`, called at
`handlers.rs:169-173`) must be pinned by host tests — a host obligation, listed in §12.3.

## 11. D8 — The safe fetch executor (shared by the image and speech second hops, DV2)

An absolute-URL second hop is not a provider call. South's `GetRequestV1` accepts only relative paths under the bound
endpoint, and 0.26.0 already states that such fetches "neither can go through South at all … host artifact fetches"
(`2026-09-09-buffered-binary-response.md:180-182`). So execution is in the host; this world sets the **rules**, and the
component supplies only a pointer to the upstream's URL (§10.1) and the expected media type.

**Host obligations (a single executor, shared by image and speech)**:

1. `https` only; refuse userinfo; a host name is required; refuse `localhost` and `*.localhost`.
2. Resolve DNS before connecting (with a timeout); if **any** resolved address falls in a forbidden range, refuse
   outright; then pin the connection to the checked address.
3. The forbidden ranges include at least: IPv4 loopback, RFC 1918 private, link-local (including the
   `169.254.169.254` metadata address), unspecified, broadcast, multicast, `100.64/10`, `0/8`, `192.0.0/24`,
   `198.18/15`, `240/4`; IPv6 loopback, unspecified, multicast, `fc00::/7`, `fe80::/10`, the deprecated site-local
   `fec0::/10`; and every IPv6 form that **embeds** an IPv4 address, whose embedded address is rechecked as IPv4:
   v4-mapped `::ffff:0:0/96`, v4-compatible `::/96`, NAT64 `64:ff9b::/96` and `64:ff9b:1::/48`, and 6to4 `2002::/16`
   (the IPv4 address in bits 16–47). On a network with a NAT64 gateway, `64:ff9b::a00:1` reaches `10.0.0.1`.
4. Do not follow redirects (3xx is a failure); **disable the system proxy** (behind a proxy, the proxy re-resolves and
   the address pin is void — exactly the server E-1 gap).
5. Attach no credential headers and no cookies; send only `Accept`.
6. A total timeout and a byte limit; an empty body is a failure. The limit is the minimum of (the host limit,
   `MAX_BINARY_RESPONSE_BODY_BYTES` 64 MiB).
7. The result's media type is the one the component declared; the upstream's `content-type` is only recorded (today
   neither the image second hop nor the Bailian TTS second hop validates it).

The server's current `guarded_asset_client` (`webhook_sender.rs:491-513`, forbidden ranges `domain.rs:1346-1378`)
satisfies 1, 2, 4, 5 and 6, and additionally allows only ports 443 / 8443. For item 3 it rechecks v4-mapped addresses
only (`domain.rs:1366-1369`); the v4-compatible, NAT64, 6to4 and `fec0::/10` rows are new host work before the
`south.safe-fetch.v1` suite can pass. It has a development escape hatch,
`asset_fetch_unsafe_allow_private_destinations`: when enabled it allows http and skips DNS pinning. That is host
configuration; this world's obligation is that **production must not enable it**, and the host is advised to refuse
it for the production profile in its startup gate. South's own reqwest transport has long used `.no_proxy()` +
`Policy::none()` (`south-transport-reqwest/src/lib.rs:95-96`); the rules point in the same direction.

**D8b — what South provides (recommended)**: two pure functions plus a set of test vectors; execution stays in the
host. `ArtifactUrlV1::parse` (scheme, userinfo, host, length — the same shape as `ProviderEndpointV1::parse`'s checks
on an endpoint, `lib.rs:427-458`) and `is_forbidden_egress_address(IpAddr) -> bool`; plus a host-obligation suite
`south.safe-fetch.v1` (gate ③) that uses an injectable resolver and transport to assert that these vectors are all
refused: http, userinfo, `127.0.0.1`, `169.254.169.254`, a domain resolving to a private range, `::ffff:10.0.0.1`,
`::10.0.0.1`, `64:ff9b::a00:1`, `2002:a00:1::1`, `fec0::1`, a 302 to an internal address, and a proxy environment.
Both hosts then execute one rule set, instead of each writing one and each missing something. Acceptance is for the
south maintainers to decide (§17 Q6).

## 12. D9 — Conformance suite `south.image-component.v1` and host obligations

### 12.1 Component suite

Fixture naming follows `image-v1.<family>.<case>.{input,expected}.json`, with the families `capabilities`, `prepare`,
`response` and `render`. The four check kinds of the task v2 suite carry over (coverage, fixture equality,
determinism, unknown-field tolerance, `task_suite_v2.rs:125-218`), plus these **required rows**:

| Check | Requirement |
|---|---|
| `metering_sample` | for every metering form the component declares, at least one `response` fixture gives **exact** metering facts with `succeeded` |
| `missing_meter_is_not_zero` | for every **required** fact the component can report (`credits`, `images_reported` on `charged_failure`) and for every declared token bucket, at least one 2xx fixture missing it, whose outcome must be `unknown` |
| `evidence_absent_is_null` | for every **evidence** fact the component can report (`images_reported` on `succeeded`, `upstream_cost`), at least one 2xx fixture missing it, whose outcome is `succeeded` with that fact `null` — never a filled-in number |
| `terminal_only_from_the_wire` | at least one non-2xx fixture; no non-2xx may produce `succeeded` |
| `auth_errors_are_not_retriable` | a `401` / `403` fixture whose outcome's error is not retriable (the existing `AuthErrorsAreNotRetriable` variant, `report.rs:57-63`); it applies here because error mapping is merged into `parse-response` |
| `pre_dispatch_refusal` *(revised 2026-10-09 per R-1)* | for every refusal the component itself makes in `prepare` (a combination its dialect cannot express, such as an edit with a mask on Gemini), at least one fixture that returns `invalid_request` and produces no descriptor. Per-role maxima are catalog data enforced by the host and are not a suite row |
| `reference_integrity` (a structural check, run on every output) | every `$south.ref` points to a blob that exists in the view; no `$south.blob` node appears in any output; every `$south.artifact` points to an existing artifact and uses an allowed delivery (§7); every `inline` and `url` pointer lands on a string in the response view; no output string exceeds the fallback threshold (the component must not smuggle bytes out of the sandbox) |
| `endpoint_confinement` | the descriptor's `path` is relative and passes the grammar |

`CheckV1` is a closed enum (`report.rs:19-64`); `TerminalOnlyFromTheWire`, `EndpointConfinement` and
`AuthErrorsAreNotRetriable` already exist, and each other check is a new variant, an additive change to the
conformance crate.

**On "south has not a single usage sample"**: P21 S5's statement is inaccurate — existing fixtures under
`crates/south-component-conformance/` do carry usage expectations (e.g. `fixtures/provider.response.cached-usage.*`,
`fixtures/provider.stream.no-usage.*`, `fixtures-wan-image-task-v2/task-v2.observation.caps-absurd-count.*`). What is
missing is **enforcement**: no existing check requires a component to carry metering samples (`report.rs:19-64` has no
such variant). The metering rows above add exactly that. The boundary record reaches the same finding for the
provider world (§6.1 there) and enforces usage rows by name in gate ② (§6.2 there).

### 12.2 Metering samples are DP1's "usage samples"

The `metering_sample`, `missing_meter_is_not_zero` and `evidence_absent_is_null` rows are exactly what P21 DP1
requires as "passing conformance with usage samples". The samples are transcribed from the server's native arms (the
P13 S8 pattern: native arm → hand-written frozen expectation → real wasm comparison), never back-derived from component
output.

### 12.3 Host obligations (gate ③; a host suite, not part of the component suite)

1. The host produces request and response views with south's `elide_v1`, or reproduces every golden vector of it byte
   for byte (§6.2); refusal of `$south.` keys and of duplicate keys. Self-consistency alone is not enough: the
   obligation is agreement with the vectors, which is what makes two hosts agree with each other.
2. The §6.7 parser and encoder: if D3b-B is adopted, the host uses south's; if A, the host's results match south's
   golden vectors after decoding.
3. Every §11 safe fetch vector is refused.
4. §10.2's verify-then-settle ordering: no `finalize` when a fetch fails or `render` fails after upstream success.
5. No extra configuration is injected into `immutable_body_paths`, their ancestors or their descendants (same rule as
   task contract 6 §4).
6. Undeclared operations and undeclared metering forms are refused before admission.
7. The precedence between image component routes and task component routes is pinned by a test; a model never hits
   both.
8. Reference expansion, body encoding and every §6.3a size refusal happen before admission (before the 202 on the
   held path); every response is read through a binary-response entry point.
9. The bounds of §9.4 item 3 are the host's own; a component `bounds` value lowers the reservation and is never used
   as the check bound.

## 13. Mapping of the existing execution arms

| Execution arm | Request | Auth | Artifact | Metering form | World capability needed |
|---|---|---|---|---|---|
| Gemini (generate / edit) | JSON `:generateContent`; reference / edit images inlined via `base64`; an edit with a mask → refused in `prepare` | `header_secret` `x-goog-api-key` | `inline base64` | `tokens` (declared buckets) or `images` | `repeat`; response elision paths; tier word `resolution` (Nano Banana) after Q8 |
| Vertex (Nano Banana) | same, with project / region in the URL, taken from an exported credential attribute or non-secret config (boundary record §3.3, §7.3) | `bearer`; the slot is `minted` by the service-account credential recipe (boundary record §3.3) | same | same | same + credential recipe (boundary record §3, phase B4) |
| Azure MAI / Foundry | JSON, `size → width/height`; n=1, b64 only (`azure.rs:14-39,200-227`); edit is multipart | `header_secret` `api-key` | `inline base64` | `tokens` (a missing declared bucket → `unknown`) or `images` | edit: multipart with binary response (§6.3a) |
| MiniMax | JSON, `GroupId` query; `b64_json → base64` | `bearer` | `url` (pointer) or `inline base64` | `images` (evidence `images_reported = success_count`) | error detection on HTTP 200 |
| Bailian Qwen-Image | JSON, `size → W*H`, n 1..=6 | `bearer` | `url` (pointer) | `images` (evidence `images_reported = image_count`) | — |
| Ideogram | **multipart** described by the component (`text_prompt`, `rendering_speed`, `resolution`), n=1; the speed is derived from the SKU, not a tier word | `header_secret` `api-key` | `url` (pointer); b64 delivery goes through the second hop | `images` | multipart encoding; safe fetch; §10.2 |
| Stability | **multipart**, `Accept: application/json`, n=1, b64 only | `bearer` | `inline base64` | `images` (`finish_reason = CONTENT_FILTERED` is also charged today, `stability.rs:548-583`) | multipart encoding; multipart with binary response (§6.3a) |
| xAI generate | JSON, `size → resolution` inside the component | `bearer` | as the upstream returns it | `images` + `tier_candidates` + evidence `upstream_cost` | tier evidence; tier-keyed price list (Q8) |
| xAI edit (P18) | JSON, `images` / `image` → `input_image`; the client's data URL `as_is` | `bearer` | as the upstream returns it | same; the input image count goes into `inputs` | request-side elision paths; Q8 |
| OpenAI-compatible fallback (DI4) | JSON with only `model` changed; edit is a multipart part list, each part `as_is`, only `model` changed | `bearer` / `header_secret` | `inline` or `url` (pointer) | `tokens` (declared buckets, `cached_input`) or `images` | render fidelity (§7); edit: multipart with binary response (§6.3a) |
| Reve bridge | JSON `/v1/image/create`, n=1, png only, `Accept: application/json` (`reve.rs:572`) | `bearer` | `inline` | `credits` | `charged_failure` |

The OpenAI-compatible fallback moves into a component (DI4), overturning P18 D5's "stays in the host". P18 worried
about "one more copy and more latency": in this design that copy happens in the host-side encoder and the bytes do not
enter the sandbox, so the cost is one memory copy, not a round trip across the wasm boundary. The host has to keep GMI
from silently falling into this fallback once its arm is deleted (P22 §8); that is the job of host routing.

## 14. Migration order and dual-run acceptance

**Order** (P22 I2–I4 merged with P18 I3; the first batch of one south minor ships the world, `contracts.media` v1 with
the full vocabulary of §6.4, the parser / elider / encoder, HTTP contract 10 (§6.3a) and the Azure and xAI
components):

1. **I2**: Azure (generate + edit), xAI (generate + edit). Azure covers "a missing declared bucket is `unknown`" and
   needs HTTP contract 10 for its multipart edit; xAI covers tier evidence and request-side elision.
   **Prerequisite for xAI**: the host's tier-keyed price list (§8, Q8). If Q8 has not landed, I2 ships Azure alone
   and xAI moves after it.
2. **I3-1**: MiniMax, Bailian Qwen-Image, the OpenAI-compatible fallback (its edit needs HTTP contract 10).
3. **I3-2**: Gemini. The earlier prerequisite ("the host first fixes missing `usageMetadata` settling at $0") is done
   (P22-F5, `b8f63414`). **New prerequisite for Nano Banana per-image rows**: Q8, as for xAI. Token-priced rows do not
   need Q8.
4. **I3-3**: Vertex, waiting for credential recipe v1 (boundary record §3; phase B4 there, which unlocks P21 S3 and
   P22 Vertex).
5. **I3-4**: Stability, needs the encoder (§6.7) and HTTP contract 10.
6. **I3-5**: Ideogram, needs the safe fetch executor (including the new §11 item 3 rows) and §10.2; acceptance adds the
   three download-failure fixtures.
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
| Reservation bound | equal where the component supplies no tightening `bounds` and the host bound formula is unchanged; otherwise recorded and explained |
| Settlement | metering kinds, values and amounts equal |
| Held path | 202 → terminal state → artifact → settlement, equal item by item (each path keeps today's funds behaviour, §9.2) |

**What the dual run cannot prove**: the same as the old behaviour ≠ correct. These need fixtures written against the
new ledger state and must not be accepted as "matches native":

- the Ideogram download failure (§10.2);
- a response missing one declared token bucket — today it settles low, afterwards it is `unknown` (§9.1);
- Ideogram's all-filtered result on the held path after the dual run — today parked, afterwards released (§9.2);
- a response between 32 and 64 MiB on a multipart edit — today delivered by the native arm, and the proof that the
  binary-response path delivers it too (§6.3a);
- a JSON request body above 32 MiB after expansion — refused before admission (§6.3a); every such case the dual run
  sees is recorded.

In addition, run a negative case with images on a synthetic provider to prove that host precheck does not recognise
provider names (J2) — the image-world counterpart of the boundary record's unseen-provider guest (T21), §12 there.

## 15. Versioning

A South **minor**. Released worlds and contracts are unchanged; one transport shape is added:

- A new WIT file `crates/south-provider-api/wit/image-adapter.wit`; `manifest.rs` gains `IMAGE_WIT_PACKAGE`,
  `IMAGE_WORLD`, `IMAGE_BEHAVIOR_SUITE`, `IMAGE_CAPABILITIES` (`generate`, `edit`) and `IMAGE_WORLD_SCHEMA`, one row in
  `KNOWN_WORLDS` (`manifest.rs:151-152`), and a branch in `validate_role`: "at least one operation word"
  (`:401-430`).
- Runtime: `bindings.rs` gains one `bindgen!` module, `InstanceKind` gains one variant (`component.rs:73-77`), and the
  link condition for the host import becomes a world property (`:182`). **The runtime limits are unchanged** —
  exactly the payoff of §6.
- `south-contracts`: new modules `media` (request view, descriptor and `MediaAuthV1`, transforms, response view,
  `response_body_form`, `parse_multipart_parts_v1`, `elide_v1`, `encode_multipart_v1`, the safe fetch pure
  functions, and their golden vectors) and `image` (facts, outcomes, artifacts). The SSE decoder `decode_sse_v1`
  that the speech world needs also lives in `south-contracts`, as the SSE sibling of the eventstream deframer
  (boundary record §5.2), and is released with this minor. JSON codecs and
  types containing `ErrorEnvelope` go into conformance, following the task v2 precedent (`component_v2.rs:13-37`), as
  does `admit_media_descriptor_auth`.
- `south-core`: `execute_multipart_binary_call_v1` and its raw twin (§6.3a). `HTTP_CONTRACT_VERSION` 9 → 10
  (`lib.rs:59`): one bump, in this minor, carrying both this twin and the speech record's D3a
  `TextPostRequestV1` with its binary execution entry point.
- `compatibility.json`: `contracts.media: 1` (the full vocabulary of §6.4, released once), `contracts.image: 1`,
  `media_limits` (fallback threshold, count and length of elision paths, `repeat` limit, artifact URL length, part
  count limit), `conformance.image_component_v1_suite_id`, the per-crate capability strings, and a host verification
  block (`not_verified` at first release).
- If this lands together with the compatibility range of the boundary record §8, image packages declare
  `contracts: {"media": 1, "image": 1}` (as embeddings packages declare `{"media": 1, "embeddings": 1}`, embeddings
  record §12).

## 16. Rejected alternatives

- **Reuse the task world's "terminal on submit"** (rejected by DI2): §5.
- **Add exports to provider-adapter-v2**: effectively a major, rebuilding the four text components (P18 §3,
  architecture).
- **Bytes into the sandbox with relaxed runtime limits** (option A of P18 D3): the payload limit would rise from
  16 MiB to the hundreds-of-MiB range and memory from 64 MiB to hundreds of MiB, while ordinary calls go through a
  single instance serially (`component.rs:136`), so large image requests would queue behind one another; for
  concurrency the host could only open more instances, multiplying memory by the instance count. Option B keeps all of
  this outside the sandbox, at the cost of one closed transform table.
- **Raise `MAX_RESPONSE_BODY_BYTES` from 32 to 64 MiB instead of adding a multipart binary twin** (§6.3a): it would
  change the bound, and the buffer a host must be ready to hold, on every text path to serve a media-only need; the
  additive twin follows the 0.26.0 precedent and leaves released bounds alone.
- **Generic host rendering of the northbound body**: loses the field fidelity of the OpenAI-compatible pass-through,
  and cannot aggregate Gemini's multiple rounds (§7).
- **The component declares "token-priced"**: crosses the vocabulary line (§9.3).
- **The component infers the xAI tier from `cost_in_usd_ticks` on its own**: requires the component to know upstream
  price lists, which is pricing knowledge; instead the component reports the upstream quote and the host matches it
  against the candidate tiers (§9.4 item 4).
- **`upstream_cost` as a metering form** (the earlier draft): nothing is billed in it; treating its absence as
  `unknown` would park correctly billed xAI calls (§9.1).
- **A component-written artifact URL** (the earlier draft): an egress channel out of the sandbox (§10.1).
- **A component-supplied bound used for both reservation and check** (the earlier draft): makes the check as strong as
  the component's honesty (§8, boundary record §6.3).
- **An `oauth` auth arm for Vertex, added in a later minor** (this record's earlier draft): superseded by the boundary
  record §3 — the component declares a credential recipe, the host executes it and presents the minted slot as
  `bearer`. The boundary record proposes deprecating the `oauth` arm (§3.8 there), so this world never admits it.
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
| Q1 | Multipart part parser, elider, encoder and byte transforms as south host-side pure functions in `south-contracts::media`, or written by each host (§6.7) | South provides them with golden vectors and fuzz obligations; revise the "no encoder" half-sentence of 0.25.0 §2 and narrow "South has no business learning what a form field is" to the transport layer | south maintainers — **ruled 2026-10-08** (below) |
| Q2 | Elision rules: declared paths + fallback threshold; the threshold value; `unknown` when the `$south.` namespace collides with the upstream; duplicate keys refused (§6.2) | As in §6.2, threshold 1 MiB | south maintainers — **ruled 2026-10-08** (below) |
| Q3 | First-version contents of the closed transform table and body forms: `contracts.media` v1 carries the image and speech words, the `text` request body and the `sse` body form, released once (§6.4) | As listed, nothing reserved beyond them | south maintainers — **ruled 2026-10-08** (below) |
| Q4 | Two worlds, or one synchronous media world (§16) | Separate, sharing `contracts.media` | lv — **ruled by lv, 2026-09-30: as recommended**; south maintainers — **ruled by lv for them, 2026-10-08: as recommended** (below) |
| Q5 | ARCHITECTURE.md's "a metering vocabulary must have a second consumer in sight" (`ARCHITECTURE.md:114-115`). **Not met today**: the release notes of 0.25.0 / 0.26.0 both state that the community host has no multipart surface and no byte-returning surface (`2026-09-09-multipart-request-body.md:189`, `2026-09-09-buffered-binary-response.md:302`). The same question is open as the boundary record Q9, the speech record Q9 and the embeddings record E-Q5 | Write P21 §7's "synchronous implementation recommended" into the release record as a written commitment, and mark `media_component_capabilities` `not_verified` until the community host lands; otherwise this vocabulary serves only one host and, under the current rules, should not be admitted | lv + south maintainers — **ruled 2026-10-08** (below) |
| Q6 | Safe fetch: South provides the URL / address pure functions and the `south.safe-fetch.v1` host suite, including the IPv4-embedding IPv6 ranges (§11 D8b) | Provide them | south maintainers — **ruled 2026-10-08** (below) |
| Q7 | Does the synchronous path release the reservation on `rejected` (today the synchronous path is always `delivery_unknown`, while held releases on a first-round 4xx) | Keep the status quo during the dual run; afterwards unify as "`rejected` releases" | lv — **ruled by lv, 2026-09-30: as recommended**. Note (2026-10-01): the rule is now stated once in the boundary record §6.4 for every world with a `rejected` outcome |
| Q8 | Tier words: are the dimensions (`resolution` / `quality`) closed; host price lists keyed by component tier words, with a per-model default word per dimension (a host schema change). **Now a prerequisite** of I2's xAI and I3-2's Nano Banana per-image rows (§8, §14) | Two closed dimensions; a new dimension goes through a `contracts.image` version; values fixed by the model row (Ideogram speed) are not tier words | south maintainers + server — **ruled 2026-10-08** (below) |
| Q9 | "Settled count ≤ delivered + 1" stays in the host; the released `task-wan-image-v2` writes it into the component — reclaim it? | The image world keeps it in the host; the task side reclaims it at the next contract upgrade | south maintainers — **ruled 2026-10-08** (below) |
| Q10 | The generalised 400 text differs from native (the Nano Banana tier gate) | Accept; P22 I4 acceptance compares status code and timing instead | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q11 | Vertex authentication is settled by the boundary record §3 (credential recipe; the `minted` slot is presented as `bearer`; this world admits no `oauth` arm, §4). What remains: is `host_signed` never admitted in this world | `host_signed` has no consumer and is not admitted | south maintainers — **ruled 2026-10-08** (below) |
| Q12 | Is the OpenAI-compatible fallback one component covering OpenAI / Azure OpenAI / DeepInfra / BytePlus / GLM, or one per provider | One component; differences via `supported_parameters` words | server + south — **ruled 2026-10-08** (below) |
| Q13 | Streaming image output (partial images) | Not included; a separate world version later | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q14 | Add `execute_multipart_binary_call_v1` (multipart POST, bytes response up to 64 MiB), taking HTTP contract 9 → 10 in one bump in this world's minor that also carries speech D3a's `TextPostRequestV1`; or raise the UTF-8 response limit (§6.3a, §16) | Add the twin; one bump carrying both new shapes | south maintainers — **ruled 2026-10-08** (below) |
| Q15 | `url` artifacts as pointers into the upstream response rather than component-written URLs (§10.1), in every media world — the speech world's `url {pointer, media_type}` (speech §6) is covered by the same rule; and whether the task world should adopt it for URLs the host fetches | Pointers in both media worlds; assess the task world separately | south maintainers — **ruled 2026-10-08** (below) |
| Q16 | Declared token buckets: every bucket a model declares must be present on a 2xx, or the round is `unknown` (§9.1); this parks responses the host settles low today | Adopt; the change is accepted by its own fixture, not by the dual run | south maintainers + server — **ruled 2026-10-08** (below) |

### Rulings of 2026-10-08

lv, the host owner, ruled on 2026-10-08 on every question tagged for the south maintainers, acting for them. Each
ruling is as recommended in the table; the line after it states the consequence. Q7, Q10 and Q13 were ruled on
2026-09-30 and are unchanged.

- **Q1** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. `south-contracts::media` provides
  `parse_multipart_parts_v1`, `elide_v1`, `encode_multipart_v1` and the byte transforms, with golden vectors and fuzz
  obligations. The "no encoder" half-sentence of `2026-09-09-multipart-request-body.md` §2 is revised and "South has no
  business learning what a form field is" is narrowed to the transport layer; that edit to the 0.25.0 record is made
  when S-I-1 lands (§19), not by this acceptance.
- **Q2** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. §6.2 is the rule: declared paths plus a
  1 MiB fallback threshold, `unknown` when the `$south.` namespace collides with the upstream, duplicate keys refused.
- **Q3** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. `contracts.media` v1 carries the five image
  words, the speech words `from_hex`, `concat` and `wav_pcm_s16le`, the `text` request body and the `sse` response
  body form, released once; nothing is reserved beyond them, and a later word is a `contracts.media` version change.
- **Q4** — Ruled (lv for the south maintainers, 2026-10-08): separate worlds sharing `contracts.media`, as recommended.
  With lv's ruling of 2026-09-30 both halves are decided, and the "one synchronous media world" alternative (§16) is
  closed.
- **Q5** — Ruled (lv, 2026-10-08), the same ruling as embeddings E-Q5: the release record of the first media release
  states in writing that a synchronous implementation in the community host is recommended (P21 §7), and the media
  vocabulary capabilities are marked `not_verified` for the community host in `compatibility.json` until that host
  lands it (the server stays `not_verified` until its own gate ③ run, as for embeddings). ARCHITECTURE.md's
  "second consumer in sight" rule is thereby met by written commitment, not by a consumer. The table is named
  `media_component_capabilities` here and carries one entry per world; its exact shape is fixed in S-I-8.
- **Q6** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. South provides `ArtifactUrlV1::parse`,
  `is_forbidden_egress_address` and the host suite `south.safe-fetch.v1`, including the IPv4-embedding IPv6 ranges and
  `fec0::/10`; execution stays in the host.
- **Q8** — Ruled (lv for the south maintainers and, as host owner, for the server, 2026-10-08): as recommended. Two
  closed dimensions, `resolution` and `quality`; a new dimension goes through a `contracts.image` version; a value
  fixed by the model row (Ideogram speed) is not a tier word. The host's tier-keyed price list remains a prerequisite
  of the xAI and Nano Banana per-image rows; as of 2026-10-09 the host has not started it (§18.5). The ruling does not
  say where a model's default word and role limits are declared, or how a tier word is spelled; lv ruled both on
  2026-10-09 (R-1 = A, R-2 exact and widened, §18.5), and the "per-model default word" of the Q8 table is the catalog's
  `params.resolution.default` *(revised 2026-10-09 per R-1)*.
- **Q9** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. The image world keeps "settled count ≤
  delivered + 1" in the host; the task side reclaims the rule at its next contract upgrade, which is outside this
  acceptance.
- **Q11** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. `host_signed` has no consumer in this
  world and is not admitted; the `oauth` arm is not admitted either (§4).
- **Q12** — Ruled (lv, as host owner and for the south maintainers, 2026-10-08): as recommended. One OpenAI-compatible
  component covers OpenAI, Azure OpenAI, DeepInfra, BytePlus and GLM; differences travel as `supported_parameters`
  words.
- **Q14** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. South adds
  `execute_multipart_binary_call_v1` and does not raise the UTF-8 response limit; one HTTP contract bump in the media
  minor carries both it and the speech record's `TextPostRequestV1` with its binary entry point. **Reconciliation
  finding: the contract numbers in this ruling no longer stand as stated.** Contract 10 was taken by B7a in 0.43.0 and
  11 by SF26 in 0.47.0, so the bump is 11 → 12 (§18.1). The substance of the ruling is unchanged. The "raw twin" the
  record mentions has a further caveat, also in §18.1.
- **Q15** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. `url` artifacts are pointers into the
  upstream response in both media worlds; the task world's exposure is assessed separately, outside this acceptance.
- **Q16** — Ruled (lv, as host owner and for the south maintainers, 2026-10-08): as recommended. Every declared token
  bucket must be present on a 2xx or the round is `unknown`; the change is accepted by its own fixture, not by the dual
  run (§14).

## 18. Reconciliation with 0.50.0 (2026-10-09)

This record was written at v0.42.0. South shipped 0.43.0 (B7a: package-declared secret headers, query parameters, quota
headers and user-agent; HTTP contract 10), 0.44.0 (S3b), 0.45.0 (S2/S4 prerequisites), 0.46.0 (kernel protocol 0.5.0,
the value channel of boundary record §13.8, B7b), 0.47.0 (the embeddings world; SF26, HTTP contract 11), 0.48.0 (the
embeddings value channel, `embeddings-vertex`), 0.49.0 (SF27) and 0.50.0 (the model catalog, Q47). Everything below was
read from `origin/main` at `6aa2813`. Nothing of this world exists in the code yet: a search for `elide_v1`,
`parse_multipart_parts_v1`, `decode_sse_v1`, `ArtifactUrlV1`, `is_forbidden_egress_address`, `safe-fetch` and
`TextPostRequestV1` in `crates/`, `scripts/` and `compatibility.json` finds none. To avoid confusing two numbered
questions, "Q14" below always means this record's Q14 (the multipart twin); the value channel is "boundary record
§13.8".

### 18.1 HTTP contract number and the multipart twin

| Record assumes | Fact on main | Effect |
|---|---|---|
| `HTTP_CONTRACT_VERSION` is 9 and the media minor takes it to 10 (header of §6.3a, §14 first batch, §15, Q14; speech §5, §13, §14, Q1) | It is **11**: `crates/south-contracts/src/lib.rs:106`, `compatibility.json` `contracts.http`. Version 10 is B7a (#136, 0.43.0: `QueryParameterV1::Declared`, declared user-agent, quota headers). Version 11 is SF26 (#157, 0.47.0: `%2F` inside one relative-path segment, `lib.rs:99-105`). Two tests pin 11: `tests/http_contract_v1.rs:54`, `tests/declared_instances_v1.rs:26` | The one bump of this world is **11 → 12**, in the media minor, carrying `execute_multipart_binary_call_v1` and the speech `TextPostRequestV1` with its binary entry point. Every "contract 10" for these shapes in this record and in the speech record reads "contract 12". S-I-2 changes the two pinned tests |
| "`execute_multipart_binary_call_v1` ... and its raw twin" (§6.3a rule 2, §15) | No raw twin of any binary entry point exists: `south-core/src/raw.rs` has none, and 0.26.0 decided "D7 — a raw twin. None for now" (`2026-09-09-buffered-binary-response.md:283-285`) | Open item for S-I-2, not a ruling change: the default is the 0.26.0 rule (typed entry point only, a raw form if an adopting host asks). The Q14 ruling is satisfied by the typed entry point |
| The twin needs transport work | `AsyncBinaryHttpTransport::execute_binary` takes any `PreparedHttpRequestV1` (`south-core/src/lib.rs:650-657`); the text and binary entry points share `execute_buffered` (`:897`); the reqwest implementation is the shared `fetch_buffered` (`south-transport-reqwest/src/lib.rs:145-162`). 0.26.0 declined the *entry point* ("no multipart call site answers in bytes", `lib.rs:860-862`) | The twin is a thin entry point beside `execute_multipart_call_v1` (`:826`); the transport trait does not change. `TextPostRequestV1` is different: it adds a request-body shape and touches `south-contracts`, `south-core` and the reqwest transport |
| "`Raising MAX_RESPONSE_BODY_BYTES`" and the other limits | Unchanged: 32 MiB JSON request (`lib.rs:156`), 32 MiB UTF-8 response (`:171`), 64 MiB binary response (`:181`), 100 MiB multipart request (`:165`) | none |

**Implemented (S-I-2).** `HTTP_CONTRACT_VERSION` is 12. The two open items were settled on the defaults: typed
entry points only (`execute_multipart_binary_call_v1`, `execute_text_binary_call_v1`), no raw, signed or streaming
twin; and the new rows are their own suite, `south.provider-media-binary.v1` (six cases), so the case counts of
`south.provider-multipart.v1` (5) and `south.provider-binary.v1` (6) that hosts verified are unchanged. Two additions
the records did not name: the closed media type is a contract enum, `TextMediaTypeV1` (`application/ssml+xml` only,
parsed by exact spelling), and an unknown one is refused with a new `ContractErrorV1::UnsupportedTextMediaType`
(`ContractErrorV1` has been `#[non_exhaustive]` since 0.25.0). The text body bound is
`MAX_TEXT_REQUEST_BODY_BYTES` = 1 MiB, equal to `media::MAX_MEDIA_TEXT_BODY_BYTES`.

### 18.2 `contracts.media`, `contracts.image` and embeddings contract 2

`compatibility.json` `contracts` keys on main: `reserved_header_policy` 2, `http` 11, `auth` 5, `error` 2, `stream` 2,
`provider_quota_metadata` 2, `response_diagnostic` 1, `response_transcript` 1, `canonical_ir` null, `task` 7,
`embeddings` 1, and the limit tables `header_limits`, `provider_quota_metadata_limits`, `response_diagnostic_limits`,
`response_transcript_limits`, `task_limits`. There is no `media`, `image` or `speech` key and no `media_limits` table.

| Record assumes | Fact | Effect |
|---|---|---|
| Embeddings packages declare `contracts: {"media": 1, "embeddings": 1}` (§15, last bullet; speech §14) | The three shipped embeddings packages declare `{"embeddings": 1}` only (e.g. `components/embeddings-vertex/manifest.json`), because contract 1 carries no media (embeddings record §15). The host range is a map of version *sets* (`HostRangeV1.contracts`, `south-provider-api/src/manifest.rs:1350`), so a package may declare a key a host does not list only if the host knows it | The sentence stays true for contract 2 packages only. Image packages declare `{"media": 1, "image": 1}` |
| The embeddings world depends on `contracts.media` v1 and ships in or after the image minor (embeddings record §12, E-Q8) | Embeddings contract 1 shipped in 0.47.0 **without** media (§15 there). Contract 2 adds `Media`, `TextBlob`, the `media` capability word, the `request.media` row and `ReferenceIntegrity`, using `contracts.media` v1 | The image minor is the prerequisite of embeddings contract 2, not the reverse. Order: S-I-1 (the `media` module) first; embeddings contract 2 may ship in the same minor as the image world or after it, and needs only the blob types, `elide_v1` and the `as_is` transform from it. Contract 1 packages are untouched: a host range that lists `embeddings` {1, 2} keeps serving them. Embeddings contract 2 is its own work item (S-I-9), not part of the first batch. **Superseded for embeddings on 2026-10-09 (lv):** contract 2 carries media inputs inline and bounded and uses neither `contracts.media` nor its blob types, so it does not follow the image minor; see the embeddings record §17 |
| `contracts.image: 1`, `media_limits`, `conformance.image_component_v1_suite_id` are added (§15) | Embeddings added the same kinds of field and moved `schema_version` to 6 together with a new top-level `embeddings_component_capabilities` table (release 0.47.0 §2) | Same shape here, with `media_component_capabilities` (Q5); expect `schema_version` 6 → 7, to be confirmed in S-I-8 |

### 18.3 The value channel (boundary record §13.8) and the media worlds

Kernel protocol 0.5.0 adds `ProviderConfig.declared` (`token-station-protocol` `provider.rs:371` at the pinned
`c2581f3`, `Cargo.toml:26`), and `ComponentValues` keys of 1 to 64 bytes of `[a-z0-9_]` with values of 1 to 4096 bytes of
printable ASCII. South shipped the channel in 0.46.0 for the provider world and widened it to the embeddings world in
0.48.0 (embeddings record §16). A media world's `prepare` and `model-capabilities` receive a `ProviderConfig` (§7), so
they can read `declared` once gate ① admits its two sources there.

| Record assumes | Fact | Effect |
|---|---|---|
| Vertex project and region come from "an exported credential attribute or non-secret config" (§13, Vertex row) | Gate ① refuses both in every world but the provider and embeddings worlds: `config_schema` at `manifest.rs:867` (`world.world != EMBEDDINGS_WORLD`), credential attributes at `values.rs:72` (`!= PROVIDER_WORLD && != EMBEDDINGS_WORLD`). `host_values` is provider-only by design (it lives on `ChatRequest`) | The image world needs the same widening embeddings got in 0.48.0: admit a family's `config_schema` and exported credential attributes, and nothing else (no `endpoint`, `host_values`, instance declarations, `signing`, `stream_framing`, `usage_evidence`, `request_facts`). Touch points: `validate_role` (`manifest.rs:849-905`; the `endpoint`/`config_schema` branch and a new image block that also calls `validate_endpoints` for the key rules, as `:890-901` does for embeddings), `validate_component_values` (`values.rs:59-`), and the host import link (`component.rs:214`) and import scan (`loader.rs:249-258`). Better than adding a third enumerated exclusion: a world property on `WorldSchemaV1` (§4 already asks for it for the host import). Gate ② gains `UndeclaredValuesIgnored` for the world, as embeddings did. Any package that uses the widening declares the `south_runtime` of the minor that ships it |
| Vertex rows get `base_url` from the host | The embeddings-vertex pattern applies as is: the host passes the location's API origin (`https://{region}-aiplatform.googleapis.com`, or `https://aiplatform.googleapis.com` for `global`) and the component appends `/v1/projects/{project}/locations/{region}/publishers/google/models/{model}:generateContent`, each value one percent-encoded segment, so `EndpointConfinement` holds (embeddings record §16; `RelativePathV1` admits `%2F` inside a segment since contract 11) | none for the contract; the Vertex image component copies the rule, including the refusal when `base_url` is a Vertex origin the region does not select |
| The media worlds may use the §10 instance declarations the speech record relies on (`query_parameters` for ElevenLabs `output_format`, speech D3b) | `validate_instances` (`south-provider-api/src/instances.rs:244-254`) refuses `query_parameters`, `quota_headers` and `user_agent` in every non-provider world. The image surface needs none of them: `GroupId` and `api-version` are sanctioned names (`lib.rs:1179-1204`), and a package-declared secret header needs only the `header_secret` arm, which every world with that arm already allows (`manifest.rs`, `validate_secret_headers`) | none for the image world. The speech world needs `query_parameters` admitted (speech §17) |

### 18.4 Credential recipes and Vertex

| Record assumes | Fact | Effect |
|---|---|---|
| Vertex waits for credential recipe v1 (B4) (§14 I3-3, §13) | B4 shipped in 0.43.0 (#135); the host passes the twelve-case recipe suite (`compatibility.json` `host_capabilities.token-station-server.credential_recipe` verified, 12 cases). `embeddings-vertex` declares the working Vertex recipe | The south-side prerequisite of I3-3 is met. What remains for Vertex images is the gate ① widening of §18.3 |
| A Vertex arm needs a recipe of its own | `components/embeddings-vertex/manifest.json` declares recipe `vertex_sa`: an RS256 `jwt_sign` step over the key file's `/private_key` (`iss` = `client_email`, scope `https://www.googleapis.com/auth/cloud-platform`, `aud` = the token step's endpoint, `iat` now, `exp` now + 600), then an `oauth2_token` form step to `https://oauth2.googleapis.com/token`, presenting `access_token`, `refresh_margin_seconds` 300, `min_ttl_seconds` 301, and attribute `project_id` exported from the non-secret field imported from `/project_id`. Recipes are manifest data, not a shared artifact | Reusable as is for Vertex image generation (the scope covers `generateContent`): the image package copies the `credentials` section byte for byte, with its own `region` and `project` config keys (syntaxes `aws_region` and `gcp_project_id`). Each package also carries its own `credential.*` fixture pack (the four files under `fixtures-embeddings-vertex/` are the model), and the world's suite must call `credential_recipe_checks_v1`, as `embeddings_suite.rs` does |
| `MediaAuthV1` header arm names one of `SecretHeaderV1` (§6.3) | The closed set is five names (`api-key`, `x-api-key`, `x-goog-api-key`, `xi-api-key`, `ocp-apim-subscription-key`; `lib.rs:993`), and auth contract 5 adds `ProviderAuthV1::DeclaredHeaderSecret` for a name a package declares in `secret_headers` (`lib.rs:1423-`). `admit_descriptor_auth` already admits both (`south-component-conformance/src/descriptor_auth.rs:140`) and a minted slot as Bearer (`Auth::OAuth`) | `admit_media_descriptor_auth` (§6.3) is built on that rule implementation, so it admits a declared name too; "`SecretHeaderV1`" in §6.3 reads "a sanctioned or package-declared secret header". The shared rule must be factored out of the kernel-typed function in S-I-5. This does not touch the Q11 ruling |

### 18.5 The model catalog (0.50.0) against §7, §8 and Q8

`catalogs/model-catalog.json` is `south.model-catalog.v1`: 37 entries, 60 rules, image and video models only, keyed by
upstream model id with `exact`, `prefix` and `contains` rules, first match wins, and a `capabilities` object in **the
host's** vocabulary that South carries verbatim and does not interpret (boundary record §13.11; reader
`south_provider_runtime::ModelCatalogV1`). It has no prices and no provider names. It is data the host loads and pins;
no component sees it, and no suite checks against it.

The vocabulary it uses for the image models overlaps this record's capability facts:

| This record | Catalog (`catalogs/model-catalog.json`) |
|---|---|
| Per-role input limits "move into the component" (§8: gpt-image-1 input images 16 / mask 1, xAI 3 or 5, Stability 1, Nano Banana reference images 1) | `media.roles.<role>.max`, with roles `input_image`, `reference_image`, `mask`: `gpt-image-1` input_image 16 and mask 1, `grok-imagine-image*` input_image, `stable-image-*` input_image 1, `gemini-3-pro-image` reference_image 1, `wan2.7-image` input_image 9 |
| Tier dimension `resolution` and "a default word per dimension (Nano Banana: `resolution`, default `1k`)" declared in `model-capabilities` (§8, Q8) | `params.resolution.options` and `.default`: `gemini-3-pro-image` options `1K 2K 4K`, default `1K`; `gemini-3.1-flash-lite-image` options `1K`; `grok-imagine-image` `1k 2k`, default `1k` |
| `requested_outputs`, `size` | `params.n.max/default`, `params.size.options`, `params.pixels.*` |

The host, per its S6 plan (read-only, `fbee0ed7`), loads the catalog (loader, `south_package_set.rs:353`) and uses it for
pre-dispatch admission on images and video, for its public model listing and for the playground form; it keeps its
built-in profiles only as a fallback until a catalog is installed, and has not re-pinned to 0.50.0 (pinned to 0.49.0).
The S6 plan hands "the per-`provider_type` input-role readers, `nano_banana_flat_tier_guard` and the no-tier fallback" to
P22 and P25, and the host plans say nothing about how the catalog and an image component's `model-capabilities` divide
the facts above. Today the host enforces `media.roles.*.max` and `params.resolution.options` before dispatch
(`images/precheck.rs`, `engine/capabilities.rs`), does not enforce `resolution.default` anywhere it was found, and the
Nano Banana gate (`precheck.rs:34-62`) still exists, keyed on provider type, not on the catalog.

**What the catalog does not change.**

- The tier price list (Q8) stays a host schema change: the catalog carries no price, and a tier word is a request-time
  choice whose *price* is the host's (ARCHITECTURE.md:129-141). As of 2026-10-09 nothing of it is implemented or
  scheduled in the host (price columns are fixed names, `image_price_1k` and `image_price_2k`, and `image_tier_price`
  reads the `size` hint, not `resolution`). The xAI and Nano Banana per-image rows therefore still wait for it (§14).
- `prepare`'s role *mapping* ("take the first key that appears", `precheck.rs:88-155`), the metering forms and token
  buckets, response formats, elision paths and `repeat` are dialect facts the catalog does not carry.
- The speech record is not affected (§17 there): the catalog is image and video only, and text was ruled out of it.

**Reconciliation finding R-1: two places now declare the same facts.** *(Ruled 2026-10-09: A, below.)* §7 had `model-capabilities` return per-role
limits, and §8 had it declare each model's tier dimensions with a default word, while the catalog already carries role
maxima, `resolution` options and defaults for the same models, South maintains it, and the host enforces it. If both stay
authoritative they will drift, and the conformance row `pre_dispatch_refusal` ("for every input role with a declared
limit, an over-limit `prepare` fixture", §12.1) cannot hold for limits that live in data South does not interpret.
No ruling covers this; Q8 decides the dimensions and the price list, not where limits and default words are declared.
Options for lv, with the recommendation of this reconciliation:

- **A (recommended; ruled 2026-10-09).** Ranges, counts and default words live in the catalog, the host enforces them and reads the
  default word for the tier-refusal rule of §8 from `params.resolution.default`. `model-capabilities` declares what is
  dialect-bound: operations, the role-key mapping, metering forms and buckets, renderable response formats, elision
  paths, `repeat`, and the *names* of the tier dimensions the component reports. `pre_dispatch_refusal` then covers only
  refusals the component itself makes (for example an edit with a mask on Gemini). One source of truth; the catalog
  needs an entry for every model the image components serve; it covers 37 ids today, and a model absent from it has
  no declaration (boundary record §13.11: `None` means "no declaration", not "no capability").
- **B.** The component stays authoritative as written in §7/§8 and the catalog's image entries drop those fields. Not
  viable as stated: the catalog is shared with video models, which have no component world, and the host's S6 design
  reads those fields.
- **C.** Both, and a conformance cross-check that a component's declared limits never exceed the catalog entry for the
  same model id. More machinery, and the catalog is not visible to the suite.

**Reconciliation finding R-2: tier-word spelling.** *(Ruled 2026-10-09, below.)* §8 gave the grammar `[a-z0-9_.-]{1,32}` for tier words, but the
catalog's resolution words are `1K`, `2K`, `4K` and `480P`, `720P`, `1080P`, `768P` in upper case for most models and
`1k`, `2k` for xAI. A host price list keyed by the component's word must map to the catalog's spelling, or the
component must report the word as the upstream and the catalog spell it. This is a spelling decision (lower-case
normalization in the component, a case-insensitive comparison in the host, or widening the grammar) and not a design
change; it had to be settled in S-I-1, before `ImageFactsV1` is frozen.

**Ruled (lv, 2026-10-09): R-1 = A.** The catalog is authoritative for per-role maxima, parameter options and default
words; `model-capabilities` declares only what is dialect-bound, and §7, §8 and §12.1 are revised accordingly. The
consequence for the catalog: it must carry an entry for every model an image component serves (37 ids today), and
the host's S6 loader already reads it.

**Ruled (lv, 2026-10-09): R-2.** Tier words are case-sensitive and matched exactly, the grammar is widened to
`[A-Za-z0-9_.-]{1,32}`, the component reports the upstream's spelling (`1K`, `720P`, xAI `1k`), and a host price list is
keyed by the exact word. No case folding anywhere.

**Open point O-1 (found while applying R-1; needs a decision, not guessed here).** The tier-refusal rule of §8 compares
a reported word with the default word of the dimension. Under the earlier text the component declared that default for
every model it served, so the rule always had a value to compare with. Under A the value exists only where the catalog
has an entry with `params.resolution.default`, and the catalog is "no declaration" for a model it does not describe. For
a per-image-priced row on a model the component says has a `resolution` dimension but the catalog has no default for (or
no entry at all), the host has nothing to compare with: it may not refuse (under-charging if the upstream serves a
higher tier than the flat price covers), or it may refuse every non-default word (but it does not know the default).
Which of the two, or a requirement that such a row cannot be listed per image, is lv's decision before the host builds
the rule; it does not affect any south step, because the rule is the host's.

### 18.6 `WorldSchemaV1`, `validate_role`, runtime and limits

| Record assumes | Fact on main | Effect |
|---|---|---|
| One new row in `KNOWN_WORLDS` and a branch in `validate_role` (§15) | `WorldSchemaV1` has five fields (`world`, `wit_package`, `behavior_suite`, `capabilities`, `auth_arms`; `manifest.rs:69-80`); `KNOWN_WORLDS` has four rows (`:301-302`: provider, task-v1, task-v2, embeddings); `validate_role` (`:849`) is an if-chain on the world name with the embeddings carve-outs at `:867` and `:890-901`; `validate_component_values` and the host-import link repeat the enumeration (`values.rs:72`, `component.rs:214`, `loader.rs:249-258`), and `validate_instances` refuses the three instance declarations outside the provider world (`instances.rs:244-254`). `EMBEDDINGS_CAPABILITIES` (`:285`) is the model for a closed vocabulary with one mandatory word | The image row is `IMAGE_WORLD_SCHEMA` with capabilities `generate`, `edit` and auth arms `bearer`, `header_secret`; `validate_role` adds "at least one of `generate`, `edit`" and requires a provider family. The five enumerations above should become world properties in the same change (S-I-3) |
| The host import is linked on `api_version != TASK_WORLD_V2` (`component.rs:182`) | The condition is now `!= TASK_WORLD_V2 && != EMBEDDINGS_WORLD` (`component.rs:214`); the import scan refuses any `token-station:` or `host` import for embeddings at `loader.rs:249-258` (task-v2: `:243-248`). `InstanceKind` has four variants (`:75-80`); bindings are one `bindgen!` module per world (`bindings.rs:47-53` for embeddings) | The image world becomes the third world without a host import; the scan refuses both namespaces for it too |
| Runtime limits are unchanged: payload 16 MiB, memory 64 MiB, 2 s (`runtime.rs:26-34`); calls serialize through one instance (`component.rs:136`, `:674-686`) | Defaults unchanged (`runtime.rs:26-33`); the instance lock is `main: Mutex<InstanceHandle>` (`component.rs:165`), taken in `call` (`:788`). The embeddings measurement (release 0.47.0 §2.3) shows the host-side erase-and-skeleton pattern passes a 32 MiB base64 body in about 130 ms with the component call under 1 ms | The limits do not move. `elide_v1` has the same job as embeddings' extraction; S-I-1 records its timing on a 32 MiB base64 body in the release record, as E-Q1 did, and a result that makes elision impractical reopens Q2 |
| Kernel types: `HttpRequestDescriptor` (`http.rs:350-361`), `HttpResponseParts` (`:382-391`), `ProviderConfig` | Pinned kernel protocol 0.5.0 (`c2581f3`, `Cargo.toml:26`). `HttpRequestDescriptor` `http.rs:441-452` still has `body: Option<Value>` and an absolute `url`; `HttpResponseParts` `:475-482` still has `body: String` with the comment that binary needs a `-v2` field; `Auth` has gained `BearerAndHeader` and accepts any lowercase header name outside the never-credential list. `ProviderConfig` (`provider.rs:339`) gains `declared` (`:371`) and keeps `models[].supported_parameters` (`capability.rs:59`) | The reasons for the media descriptor and view types (§1) are unchanged. The `Auth` change is the reason `admit_media_descriptor_auth` must apply the manifest's `secret_headers` check (§18.4) |
| Q47 is not in this record | The three crates every component links (`south-contracts`, `south-provider-api`, `south-component-conformance`) carry their own versions since 0.50.0 (boundary record §13.12) | The media minor changes all three, so they take a version bump and every package takes a patch bump, as 0.47.0 and 0.48.0 did; S-I-8 includes the digest-stability test (`shipped_packages_v1`) |

### 18.7 Citations that drifted

The record's south citations moved as follows; text not listed is unchanged (`runtime.rs:26-34`, `task_v2.rs:296-303`
and `:100-105`, `component_v2.rs:13-37`, `task-adapter-v2.wit`, `provider-adapter.wit`, `reference_wan_image_task_v2.rs`).
Server citations were not re-verified.

| Cited | Now |
|---|---|
| `lib.rs:59` (`HTTP_CONTRACT_VERSION` = 9) | `lib.rs:106` (= 11) |
| `lib.rs:106`, `:115`, `:121`, `:131`, `:620` (body limits and the JSON check) | `lib.rs:156`, `:165`, `:171`, `:181`, `:729` |
| `lib.rs:193` (`RESPONSE_TRANSCRIPT_DENIED_HEADERS`) | `lib.rs:251` (now `pub(crate)`) |
| `lib.rs:427-458`, `:481` (`ProviderEndpointV1::parse`, `RelativePathV1`) | `lib.rs:519-525`, `:580-584` |
| `lib.rs:745` ("does not parse multipart") | `lib.rs:849` |
| `lib.rs:884-894` (`SecretHeaderV1`) | `lib.rs:993`; five names |
| `lib.rs:1064-1090` (`QueryParameterV1`) | `lib.rs:1179-1204`; adds `FileId` and `Declared` |
| `lib.rs:1309-1323` (`ProviderAuthV1`) | `lib.rs:1423-`; five arms (adds `BearerAndHeaderSecret`, `DeclaredHeaderSecret`) |
| `lib.rs:1585`, `:1615` (`MultipartPostRequestV1`, its `content-type` rule) | `lib.rs:1729`, `:1759` |
| `lib.rs:2331` (`BufferedBinaryResponseV1`) | `lib.rs:2519` |
| `south-core/src/lib.rs:811-831`, `:845-850`, `:858` | `south-core/src/lib.rs:826`, `:860-862`, `:873` |
| `south-transport-reqwest/src/lib.rs:95-96` | `:96-97` (and `:296-297`) |
| `manifest.rs:74`, `:151-152`, `:401-430` | `manifest.rs:87`, `:301-302`, `:849-905` |
| `component.rs:73-77`, `:136`, `:182`; `loader.rs:216-224` | `component.rs:75-80`, `:165`, `:214`; `loader.rs:243-258` |
| `report.rs:19-64` (`CheckV1`) | `report.rs:19-135`; the variants named in §12.1 still exist |
| `task_suite_v2.rs:125-218` | the runner starts at `task_suite_v2.rs:129` |
| `ARCHITECTURE.md:103-116`, `:110`, `:114-115` | `ARCHITECTURE.md:129-141`, `:136`, `:140-141` |
| Kernel `f585bc8` (`Cargo.toml:22`); `http.rs:350-361`, `:382-391` | Kernel `c2581f3` (`Cargo.toml:26`); `http.rs:441-452`, `:475-482` |

## 19. First batch and implementation steps

**What is already in main** (§18): the descriptor auth admission rule (`admit_descriptor_auth`), package-declared secret
headers, the credential recipe machinery with a working Vertex recipe, the value channel in the provider and embeddings
worlds, the pinned kernel types, the embeddings world as the structural model (contracts crate module, ABI, JSON codecs,
suite, references, sandbox parity, packages, release), and the model catalog. **What is still to build**: everything of
this world — the `media` and `image` contract modules and their pure functions, the HTTP contract 12 shapes, the WIT,
manifest row, runtime world, gate ① widening, the suite and its references, the packages, and the release plumbing.

**Content of the first batch**, applying §14 to main: the world; `contracts.media` v1 complete (§6.4, including the
speech words, `text` body and `sse` form, which no image component uses); the multipart parser, elider and encoder;
HTTP contract 12 with both new shapes; and the **Azure** component (generation and edit). The xAI component is **not**
in the first batch while the host's tier-keyed price list is unscheduled (§14 I2: "I2 ships Azure alone and xAI moves
after it"); it is S-I-7, conditional. Shipping an xAI package before the host adopts it, `not_verified` as the embeddings
packages were, is possible but is a decision for lv; this record does not take it.

Steps are in dependency order. "Host prerequisite" means work outside south that the step's *host cutover* (not its
south acceptance) waits for; the south acceptance of every step is on south alone.

| Step | Work | Files and crates | Acceptance |
|---|---|---|---|
| **S-I-1** Contract types and pure functions | New module `south-contracts::media`: request view and part list types, `MediaRequestDescriptorV1` with `MediaAuthV1`, the closed transforms of §6.4 (all of them, including `from_hex`, `concat`, `wav_pcm_s16le`), the response view and `response_body_form`, `MediaLimitsV1`, `parse_multipart_parts_v1`, `elide_v1`, `encode_multipart_v1`, `ArtifactUrlV1::parse`, `is_forbidden_egress_address`, but **not** `decode_sse_v1`: lv ruled the host's B6 plan Q-B6-6 on 2026-10-09 (release it separately and earlier, as a small minor, so the Responses record R1 does not wait for this world), so it leaves this step; the `sse` response body form stays in the `contracts.media` v1 vocabulary and its view is built by that decoder. New module `south-contracts::image`: facts, `ImageMeteringV1`, `ImageOutcomeV1`, artifact forms, tier words. Revise the 0.25.0 record's "no encoder" half-sentence (Q1). R-1 (A) and R-2 (exact, widened grammar) ruled 2026-10-09; `ImageModelCapabilitiesV1` carries only the dialect-bound fields listed in §7 | `crates/south-contracts/src/` (`lib.rs` re-exports, new `media.rs`, `image.rs`; the SSE decoder beside `eventstream.rs`), golden vectors under `crates/south-contracts/tests/`, fuzz targets in `fuzz/fuzz_targets/` (`contract_parsers.rs` pattern), `docs/design/2026-09-09-multipart-request-body.md` | Golden vectors for each function; fuzz targets build (`cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked`); `scripts/check-boundaries.sh`; timing of `elide_v1` and the multipart splitter on a 32 MiB base64 body recorded in the release record (the E-Q1 pattern); `south-contracts` version bump (Q47) |
| **S-I-2** HTTP contract 12 | `HTTP_CONTRACT_VERSION` 11 → 12 with its doc entry; `execute_multipart_binary_call_v1` beside `execute_multipart_call_v1`; `TextPostRequestV1` with a binary execution entry point (carried for the speech record, Q1 there); the raw-twin question of §18.1; testkit runners; provider-suite rows (own suite or an existing one, for the maintainers) | `crates/south-contracts/src/lib.rs`, `crates/south-core/src/lib.rs` (and `raw.rs` if a raw form is wanted), `crates/south-transport-reqwest/src/lib.rs` (text body rendering only), `crates/south-testkit/src/provider_binary.rs`, `crates/south-provider-conformance`, `compatibility.json`, the two pinned tests `http_contract_v1.rs:54` and `declared_instances_v1.rs:26` | New suite rows pass in the testkit; contract 11 requests are exactly contract 12 requests that do not use the new shapes; `compatibility.json` `contracts.http` 12 and the crate capability strings updated; the host's `provider_binary` gate ③ result is not claimed for the new rows |
| **S-I-3** WIT, manifest, gate ① | `wit/image-adapter.wit` (§7); `IMAGE_WIT_PACKAGE`, `IMAGE_WORLD`, `IMAGE_BEHAVIOR_SUITE`, `IMAGE_CAPABILITIES`, `IMAGE_WORLD_SCHEMA`, a `KNOWN_WORLDS` row, a `validate_role` branch; admit `config_schema` and credential attributes in this world (§18.3); turn the enumerated world exclusions of §18.6 into world properties; new `ManifestErrorV1` variant for "operation word required" (breaking for hosts matching it exhaustively, as 0.47.0's was) | `crates/south-provider-api/` (`wit/`, `src/manifest.rs`, `src/values.rs`, `src/lib.rs`, tests `provider_api_v2.rs`) | A manifest declaring neither `generate` nor `edit`, an unknown word, `host_values`, an `endpoint`, an instance declaration or the `oauth` arm is refused; a manifest with a family's `config_schema` and exported attributes is admitted; the existing four worlds' tests pass unchanged; `south-provider-api` version bump |
| **S-I-4** Runtime world | `bindgen!` module, `InstanceKind::Image`, `call_model_capabilities`, `call_prepare`, `call_parse_response`, `call_render`; the host import is not linked and the import scan refuses `token-station:*` and any `host` interface for this world; limits unchanged | `crates/south-provider-runtime/src/` (`bindings.rs`, `component.rs`, `loader.rs`), a test guest `tests/guests/test-image`, test `image_world_v1.rs` | The `embeddings_world_v1.rs` cases ported: a guest importing `host` is refused, a guest with the wrong world is refused, payload above 16 MiB is refused, determinism; `declared_runtime_v1` still passes |
| **S-I-5** Conformance suite `south.image-component.v1` | JSON codecs (the `image-v1.<family>.<case>` fixture format), the component trait, ABI and sandbox adapters, the suite with the required rows of §12.1 as additive `CheckV1` variants, `admit_media_descriptor_auth` built on a rule factored out of `admit_descriptor_auth`, `UndeclaredValuesIgnored`, `credential_recipe_checks_v1` for packages with a recipe, `reference_integrity`; gate ③ host suites in the style of the existing ones (`south.safe-fetch.v1` as its own host suite) | `crates/south-component-conformance/src/` (new `image_*.rs`, `abi_image.rs`, `component_image.rs`, `sandbox_image.rs`, `descriptor_auth.rs`, `report.rs`, `lib.rs`), `crates/south-testkit` for the safe-fetch host suite | A native reference passes every row; each required row has a mutation that fails it; the safe-fetch vectors of §11 are refused; `south-component-conformance` version bump |
| **S-I-6** Azure component | Reference implementation and package for the Azure MAI / Foundry family: generation JSON, edit as multipart with the binary response, `header_secret` `api-key`, `inline base64`, `tokens` (a missing declared bucket is `unknown`) or `images`. Fixtures are transcribed from the server's native arm (`azure.rs`), the P13 S8 pattern, never back-derived | new `components/image-azure/` (package name proposed; manifest, `src/lib.rs`, lockfile), `crates/south-component-conformance/src/reference_azure_image.rs` and its fixture pack, tests `azure_image_suite.rs` and `azure_image_sandbox_parity.rs`, `scripts/build-image-azure-component.sh`, `.github/workflows/release.yml` | Suite green natively and inside the sandbox with identical ABI answers; the package declares `south_runtime` of the minor; **host prerequisite for cutover**: the host's generic media executor (not started), HTTP contract 12 linkage and the multipart encoder; the model catalog must carry an entry for every model an image component serves (37 ids today; the host's S6 loader already reads it) |
| **S-I-7** xAI component (conditional) | xAI generation and edit: tier words, `tier_candidates`, evidence `upstream_cost`, request-side elision paths | `components/image-xai/`, reference and fixtures as S-I-6 | Suite green; **host prerequisite for the south release decision and for cutover**: the host's tier-keyed price list (Q8), not started and not scheduled as of 2026-10-09; until it lands the xAI rows stay on the native arm (§14) |
| **S-I-8** Release | Version bumps of the three guest-linked crates (Q47) and a patch bump of every package with `south_runtime` unchanged where nothing needs the new world; `compatibility.json` (`contracts.media`, `contracts.image`, `media_limits`, suite fields, `media_component_capabilities` `not_verified`, `schema_version`); the written Q5 commitment; release record, README and ARCHITECTURE entries; the digest-stability test; the declared-runtime check; the release index | `compatibility.json`, `Cargo.toml` files and component lockfiles, `docs/design/<date>-release-<next>.md`, `README.md`, `ARCHITECTURE.md`, `crates/south-component-conformance/tests/shipped_packages_v1.rs`, `scripts/check-declared-runtime.sh` | The checks of the 0.47.0 / 0.48.0 releases (fmt, clippy, nextest, `check-boundaries.sh`, `check-language.sh`, unit tests of `scripts/`, fuzz build, wasm clippy, release replay under the tag, `release_index.py compare`) all pass |
| **S-I-9** Embeddings contract 2 (separate work item) | `Media` and `TextBlob`, the `media` capability word, the `request.media` row, `ReferenceIntegrity`; uses the S-I-1 blob types and `elide_v1`. **Superseded for embeddings on 2026-10-09 (lv, embeddings record §17):** contract 2 is `Media` inline and bounded, the `media` capability word and the inline `request.media` row; it has no `TextBlob`, no `ReferenceIntegrity` and no use of the S-I-1 blob types, and it shipped independently of this world (release 0.51.0). A blob-based form, if ever wanted, is a future contract 3 | `crates/south-contracts/src/embeddings.rs`, `manifest.rs`, `embeddings_suite.rs`, embeddings packages | Contract 1 packages unchanged and still pass |

Later batches, from §14, after the first: I3-1 (MiniMax, Bailian Qwen-Image, the OpenAI-compatible component; its edit
needs S-I-2), I3-2 (Gemini; Nano Banana per-image rows need the host price list), I3-3 (Vertex; the south side needs
only the gate ① widening of S-I-3 and the recipe of §18.4), I3-4 (Stability), I3-5 (Ideogram, needs the safe fetch
executor in the host), I3-6 (the Reve bridge, with P18's Reve edit).

**Prerequisites outside south, in one list** (the host owns each; none blocks a south step's acceptance):

1. The host's tier-keyed price list and per-model default word handling (Q8): blocks S-I-7 and the per-image rows of
   I3-2. Not started.
2. The host's generic media executor — view building with `elide_v1`, multipart parse and encode, reference expansion
   before admission, the binary entry points, verify-then-settle, GatewayHeld wiring: blocks the cutover of every
   component. Not started (P22 I2–I4); the only host generic executor for a media world is the embeddings one.
3. The host's safe fetch executor to the `south.safe-fetch.v1` rules (the missing IPv6 ranges of §11): blocks I3-5 and
   the speech Bailian arm.
4. A re-pin of the host to the media minor, and to 0.50.0 for the catalog (R-1 = A makes the catalog authoritative; it
   must carry an entry for every model an image component serves, 37 ids today, and the host's S6 loader already reads
   it).

## Revision note (2026-10-01)

- Header: `Revised` line; baseline moved to south `3c1501a` and host `a82c852b`; line numbers re-checked.
- §6.1 / §6.2 / §6.7: the multipart splitter (`parse_multipart_parts_v1`) and the elider (`elide_v1`) become south
  pure functions in `south-contracts::media` with golden vectors; elision now fixes traversal, ids, `bytes`, `head`,
  key handling, duplicate keys and view serialization; long multipart text parts are elided too.
- §6.3: `GET` dropped (no consumer); `auth` typed as `MediaAuthV1`, admitted by a south twin of the boundary record's
  §4.2 function; reference expansion and encoding happen before admission, also before the held path's 202.
- §6.3a (new): the "no new transport shape" claim is withdrawn; every media call reads its response as bytes; a
  multipart binary-response twin is proposed with HTTP contract 9 → 10, shared with speech D3a (Q14). No change is
  proposed for the 32 MiB JSON request limit; the reason is given there.
- §6.4 / §6.5: `contracts.media` v1 carries the full image + speech vocabulary (incl. `sample_rate`), released once;
  "framing" renamed `response_body_form`.
- §7: model-capabilities cached by (package digest, provider-config digest); a closed table of artifact deliveries
  (no `inline`/`body` → `url` in v1); a `render` failure after upstream success is `unknown` with no `finalize`.
- §8: bounds follow the boundary record §6.3 — host bounds for checks, component bounds only tighten the
  reservation; tier words defined as request-time choices (Ideogram speed removed); tier-keyed prices made a
  prerequisite (Q8); flat-row rule defined with per-model default words; stale P22-F6 text updated.
- §9.1: `upstream_cost` is an evidence fact, not a form; required vs evidence facts per form; unattributed
  `cached_input` bucket; declared token buckets must all be present (Q16).
- §9.2 / §10.3: stale P22-F5 text updated; Ideogram all-filtered as `rejected` with its held-path effect after the
  dual run; the `rejected` refund rule now points to the boundary record.
- §9.4: checks use host bounds only; missing `upstream_cost` falls back to the default candidate.
- §10.1: `url` artifacts are pointers into the upstream response (Q15); stale P22-F7 text updated; Reve bridge is
  `inline` only.
- §11: IPv4-embedding IPv6 ranges and `fec0::/10` added to the forbidden list and the test vectors.
- §12: `evidence_absent_is_null` and `auth_errors_are_not_retriable` rows added; host obligations tied to golden
  vectors, pre-admission sizing and host-only check bounds.
- §13 / §14: arms remapped; Q8 and HTTP contract 10 made explicit prerequisites; the "cannot prove" list extended.
- Not adopted: no transport change for JSON request bodies above 32 MiB (§6.3a gives the reason); the task world's
  own artifact-URL exposure is noted as an open question (Q15) rather than resolved here.
- Consistency pass, bounds (§8, §9.4): the image instance of the boundary record §6.3 rule is stated as a table,
  including that media bytes do not count toward any bound and that credits have no request-derived bound; every
  check compares against the host bound only.
- Consistency pass, `url` artifacts (§10.1, Q15): the pointer rule is stated for every media world; speech's
  `url {pointer, media_type}` is named as covered.
- Consistency pass, citations: the `oauth` arm deprecation is cited as boundary record §3.8 (§4, §16); the `rejected`
  rule is cited as boundary record §6.4 (header, §9.2, Q7), with a note that a 2xx proving nothing was produced
  (Ideogram) is the same outcome.
- Consistency pass, `decode_sse_v1` (§6.5, §6.7, §15): it lives in `south-contracts` as the SSE sibling of the
  eventstream deframer (boundary record §5.2), released with this minor; it is no longer listed as part of `media`.
- Consistency pass, HTTP contract 10 (§6.3a, §15, Q14): one bump, in this world's minor, carrying both
  `execute_multipart_binary_call_v1` and speech's `TextPostRequestV1`; ASR uses the multipart binary twin.
- Consistency pass, §15: the embeddings declaration is quoted correctly as `{"media": 1, "embeddings": 1}`.

## Implementation note: S-I-1a, the `media` module (2026-10-09)

What the record left to the implementation, as built on branch `feature/image-world`, for review:

- **`decode_sse_v1` is out of S-I-1** (Q-B6-6, ruled 2026-10-09; see §19). `ResponseBodyFormV1::Sse` exists, and
  `build_media_response_view_v1` refuses it (`SseUnavailable`) until that decoder ships.
- **Limits added to `media_limits`**: at most 64 parts in a multipart request view or descriptor, at most 8 KiB of
  header block per part, `state` at most 8 KiB, JSON nesting at most 128 levels (`MAX_MEDIA_JSON_DEPTH`).
- **The JSON tree.** `elide_v1` and the template expander share one strict RFC 8259 parser that keeps source order,
  every number's and every string's source text; a view or an expanded body is that text re-emitted compactly. A
  `serde_json::Value` round trip was not used: it re-sorts keys and re-renders numbers, which a host in another
  language could not reproduce. Strings the elider writes (`head`, and an expanded reference) use `serde_json`'s
  escaping, stated in the vectors.
- **Multipart parsing** accepts what clients send and refuses the rest: no preamble, CRLF line breaks, a close
  delimiter followed by nothing, CRLF or LF; a `form-data` disposition with a non-empty `name`; `filename` or
  `filename*` makes a file part (`filename*`'s value is not decoded); a text part must be UTF-8; a
  `content-transfer-encoding` other than `binary`, `8bit` or `7bit` is refused. Repeated names keep their order.
- **Multipart encoding** writes `Content-Disposition` with `name` (and `filename`), escapes `"`, CR and LF as `%22`,
  `%0D`, `%0A` (the HTML form-data rule), writes a file part's `Content-Type` (`application/octet-stream` when the
  descriptor gives none), and refuses a content that contains the boundary, so the host retries with another.
- **Descriptor wire shape**: `{method, path, query?: [{name, value}], headers?: [{name, value}], auth?, body}`, the
  body `{"json": {"template": …}} | {"multipart": {"parts": […]}} | {"text": {"media_type", "text"}} | "empty"`.
  Query names are the five sanctioned `QueryParameterV1` names only (§18.3: this world declares none). A reference
  node is `{"$south.ref": {"blob", "transform", "media_type"?}}`; `media_type` is what `data_url` writes, and
  `concat` is never the transform of one reference.
- **Safe fetch**: `64:ff9b:1::/48` (RFC 8215 local use) is refused as a whole prefix, because a translator may place
  the embedded address anywhere after the /48; the well-known `64:ff9b::/96` is rechecked as IPv4, as §11 says.
- **Codecs** are crate-private (no `base64` runtime dependency in a crate every component links); their decisions are
  pinned against the `base64` crate by a property test.
- **Timing** (Apple M4, release build, best of five): `elide_v1` on a 33,554,483-byte JSON document carrying a 32 MiB
  base64 string at a declared path gives a 167-byte view in 55 ms; `parse_multipart_parts_v1` splits a 33,554,652-byte
  body in 9 ms (`tests/media_elision_measurement.rs`). Q2 stays closed.

## Revision note (2026-10-09)

- Header: `Status` is accepted; `Rulings` and `Revised` updated; a reconciliation baseline paragraph added.
- §10.2: the date of the DI6 = A ruling (lv, 2026-10-08, host plan P22).
- §17: rulings of 2026-10-08 recorded under their questions, with one-line consequences. No design decision changed.
- §18 (new): reconciliation with 0.50.0. The HTTP contract bump is 11 → 12, not 9 → 10 (Q14's numbers are stale, its
  substance stands); no raw twin of any binary entry point exists; embeddings contract 2 follows the image minor; gate ①
  must admit the value channel in this world; Vertex reuses the `embeddings-vertex` recipe; two findings (R-1, R-2) on
  the overlap of the model catalog with §7/§8 and on tier-word spelling (ruled the same day, below); a table of drifted
  citations.
- §19 (new): the first batch applied to main, and steps S-I-1 to S-I-9 with files and acceptance, and the prerequisites
  outside south.
- Rulings of 2026-10-09 (lv): **R-1 = A** and **R-2**. §7 `model-capabilities` no longer returns per-role limits or
  default tier words (it declares the dialect-bound facts and the names of tier dimensions), and a note states that
  limits and defaults are catalog data and that a model absent from the catalog has no declaration; §8 reads the
  default word from the catalog (`params.resolution.default`) and rewrites "Input roles and limits"; §8 `tier` grammar is
  `[A-Za-z0-9_.-]{1,32}`, case-sensitive, matched exactly; §12.1 `pre_dispatch_refusal` covers only refusals the
  component makes itself; §17 Q8 and §18.5 record the rulings; §19 S-I-1 loses its gate and S-I-6 and the host
  prerequisites list the catalog requirement. §13 and §14 assume no component-declared limit and are unchanged. Open
  point O-1 (§18.5) is recorded for lv.
- 2026-10-09 (embeddings contract 2): the §18.2 row on the embeddings world's dependency on `contracts.media` and the
  S-I-9 step are marked superseded for embeddings by the embeddings record §17, which carries media inline and
  independently of this world.
