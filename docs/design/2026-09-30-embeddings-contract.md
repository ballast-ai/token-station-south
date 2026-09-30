# The embeddings contract and the `embeddings-adapter-v1` world

Status: proposed — drafted for review by the host team (token-station-server P21), not accepted

Date: 2026-09-30

Revised: 2026-10-01 after independent review (see the revision note at the end).

Predecessors: `2026-09-20-task-adapter-v2-candidate.md` (the precedent of a south-local contract, with conformance
owning the IR-bearing prepared type and the single JSON codec), `2026-09-19-runtime-second-world.md` (one runtime
hosting several worlds), `2026-09-27-task-contract-v6-facts.md` (request estimates and reported usage modeled
separately; `immutable_body_paths`), `2026-09-30-host-zero-vendor-boundary.md` (this record depends on its §3
credential recipes, §4 descriptor auth admission, §6 usage discipline and host-computed bounds, §8 compatibility range
and §10 instance declarations), `2026-09-30-image-world.md` (its §6 media vocabulary: blob placeholders, reference
nodes and the `as_is` transform, reused for media inputs, §3 below).

Origin: token-station-server plan P24 (`docs/product-review-v2/plans/2026-09-30-P24-*.md`), items E1,
DE1 (the contract lives south-local, not in the kernel IR) and DE2 (a new world rather than raising the provider
world to v3) — both approved as recommended on 2026-09-30, as relayed by the host team; DE3 (whether Gemini
responses carry token counts) awaits measurement. The umbrella plan is P21 in the same directory (DP0, DP1).

Rulings: on 2026-09-30 the host owner (lv) ruled on E-Q2, E-Q3 and E-Q6 (§14), and on 2026-10-01 on E-Q10 and
E-Q11; each ruling is recorded under its question. Questions tagged S remain open for the south maintainers.


Baseline: south `origin/main` = v0.42.0 (`3135e36`); kernel `f585bc83` (protocol 0.4.0). Host line numbers refer
to token-station-server `a82c852b` and carry a `server:` prefix; kernel line numbers carry `kernel:` and refer to
`crates/protocol/src/`.

## 1. Problem

The host's `/v1/embeddings` splits into three execution arms by `ProviderType`
(server:gateway/src/modules/inference/handler/embeddings.rs:172-205), and each carries provider knowledge:

| Arm | Provider knowledge in the host |
|---|---|
| OpenAI-compatible | The auth arm, its companion headers and the user-agent are chosen by type (`south_auth_for`, `south_auth_companion_headers`, `south_user_agent_for`, :266-279, :335-341); the secret is chosen from different credential fields by type (GLM takes `oauth_token` before `api_key`, server:…/engine/south_adapter.rs:370-379); the legacy path presents GLM Coding's impersonation user-agent (server:…/engine/upstream.rs:146-153); the immutable-field table is chosen by type (:55-79); URL differences for Azure are handled by `build_upstream_url` (:224) |
| Gemini native | A single input goes to `:embedContent`, an array to `:batchEmbedContents` (:943-961); request / response translation lives in the leaf crate (server:crates/gateway-provider-protocol/src/translate_gemini.rs:46-59, :78, :136); estimates by "1 token per 4 bytes + media constants 258 / 512 / 1024" and settles by that estimate (embeddings.rs:45-53, :926-939) |
| Vertex `:predict` | Accepts only a single input (:629-671); reads `predictions[0].embeddings.statistics.token_count`, and goes to `delivery_unknown` when it cannot (:679-707); mints a Bearer from the service account (:750-767) |
| Shared entry point | Multimedia input is allowed only for Gemini, and `gemini-embedding-001` is hard-coded as text-only (:153-171) |

Two provider families are served, or refused, only by host code outside these arms: GitHub Copilot needs a minted
token and seven editor headers (south_adapter.rs:268-281, :310-332), and on this surface today it cannot be served at
all — the south path has no static secret for it (south_adapter.rs:380-384) and the legacy auth refuses minting
providers (upstream.rs:165-168). The provider-name default for NVIDIA NIM's `input_type` is no longer in the host: P24
E0 moved it to the model's request extras (embeddings.rs:211-213).

The provider world cannot hold this: its capability vocabulary is closed to chat / stream / tool_call /
json_schema (crates/south-provider-api/src/manifest.rs:69-74), and a test specifically pins that declaring
`embeddings` is refused (crates/south-provider-api/tests/provider_api_v2.rs:222-231); the kernel's `ProviderApi`
has only four kinds and no IR types for embeddings (kernel:provider.rs:137-143); and south's earliest record lists an
embedding IR as a non-goal (2026-08-16-minimal-provider-call.md:38).

## 2. Decisions

- **D1 The contract is south-local (DE1).** The IR-independent request, estimate, usage and vector-locator types go
  in `south-contracts`; the IR-bearing `PreparedEmbeddingsV1` and the single JSON codec go in
  `south-component-conformance` — the same division as task-v2 (ARCHITECTURE.md:86-90;
  2026-09-20-task-adapter-v2-candidate.md:24). The kernel IR does not change; descriptor, response, configuration and
  error reuse the kernel's `HttpRequestDescriptor`, `HttpResponseParts`, `ProviderConfig` and `ErrorEnvelope` (in
  JSON form).
- **D2 A separate WIT package and world (DE2).** Package `token-station:embeddings-adapter@1.0.0`, world
  `embeddings-adapter-v1`. The reason for a separate package is the same as for the task world: a package's version
  is shared by every world in it, so putting this in `token-station:adapter` would let chat-side changes force
  version signals on the embeddings side (manifest.rs:19-26).
- **D3 Bytes do not cross the component boundary, in either direction.** Media inputs reach the component as blob
  references from the shared media vocabulary and go back out as reference nodes (§3); vectors in the response are
  extracted by the host per the component's declaration, and the component sees only the response skeleton with the
  vectors erased (§5).
- **D4 Usage**: the component reports the numbers the upstream reported, or declares "the upstream did not report"
  (`NotReported`) and supplies an **estimated fallback** when building the request; the host settles from this and
  labels estimates truthfully (§7).
- **D5 The reservation upper bound is the host's**, computed from the northbound request with the boundary record's
  provider-agnostic rule (§7.3); the component may only tighten it.

## 3. Contract types (`south-contracts`, IR-independent)

Sketch (names are a draft):

```rust
pub const EMBEDDINGS_CONTRACT_VERSION: u16 = 1;
pub const MAX_EMBEDDING_INPUTS: usize = 2048;          // initial value taken from OpenAI's published limit
pub const MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES: usize = 8 * 1024;
pub const MAX_EMBEDDINGS_EXTRA_FIELDS: usize = 32;     // unmodelled northbound fields handed through
pub const MAX_EMBEDDINGS_EXTRA_BYTES: usize = 16 * 1024;

pub struct EmbeddingsRequestV1 {
    model: String,                    // the upstream model selected by routing
    inputs: Vec<EmbeddingInputV1>,    // 1..=MAX_EMBEDDING_INPUTS, parsed per the table below
    input_shape: InputShapeV1,        // Single | Array, per the table below
    dimensions: Option<u32>,          // > 0
    encoding_format: EncodingV1,      // Float | Base64: the format the northbound caller wants
    user: Option<String>,
    extra: serde_json::Map<String, Value>, // every other top-level northbound field, unchanged and bounded
}

pub enum EmbeddingInputV1 {
    Text(String),                                      // strings above the media fallback threshold arrive as Blob
    TextBlob { blob: BlobIdV1, bytes: u64, head: String },
    TokenIds(Vec<u32>),
    Media { media_type: String, blob: BlobIdV1, encoded_bytes: u64 },
}

pub struct EmbeddingsEstimateV1 {
    fallback_input_tokens: Option<u64>,   // null = cannot be settled if the upstream does not report
    max_input_tokens: Option<u64>,        // tightens the host's bound (§7.3); never raises it
}

pub enum VectorLocatorV1 {
    NorthIdentical,                                   // the upstream body is itself an OpenAI embeddings response
    Array { array: JsonPointer, vector: JsonPointer, index: Option<JsonPointer> },
    Single { vector: JsonPointer },
}

pub enum UsageSourceV1 { Reported, NotReported }

pub struct EmbeddingsUsageFactsV1 {
    source: UsageSourceV1,
    input_tokens: Option<u64>,            // required when Reported
    per_input_tokens: Option<Vec<u64>>,   // given when the upstream reports per input; length = input count, sum = input_tokens
}

pub struct EmbeddingsParsedV1 {
    usage: EmbeddingsUsageFactsV1,
    vector_count: u32,                    // the number of vectors the component counted in the skeleton
    upstream_model: Option<String>,       // the model the upstream echoed, for rendering only
}

pub enum EmbeddingsFailureOutcomeV1 { Rejected, Unknown }
```

`UsageSourceV1::NotReported` is a per-call fact ("this response carried no usage"). It is deliberately not called
`absent`: in the boundary record §6.2 item 4, `absent` names a family that never reports usage.

**Media inputs use the shared media vocabulary.** A media input's bytes never enter the sandbox: the host hands the
component a blob id, the media type and the length, and the component places the input in its descriptor body with a
reference node `{"$south.ref": {"blob": "<id>", "transform": "as_is"}}`, which the host replaces before sealing. The
placeholder and reference grammar, the `as_is` transform and the `reference_integrity` check are those of the image
record §6 (`contracts.media` v1); nothing is added to that vocabulary. `as_is` reproduces the client's base64 text
exactly, so the Gemini body stays byte-equal to today's (`inline_data.data` is the client's string,
translate_gemini.rs:53-57), and the host never needs to decode it. Text inputs longer than the media fallback threshold
(image record §6.2, 1 MiB) are handed the same way as `TextBlob`; a component that needs only their length (Gemini's
estimate, §7.2) reads `bytes`. If the request view still exceeds the runtime payload limit, the host answers 413
before admission, as in the image record §6.2. Without this, a Gemini multimodal request between the runtime's 16 MiB
payload limit (crates/south-provider-runtime/src/runtime.rs:32) and the host's 32 MiB JSON body limit
(server:gateway/src/core/limits.rs:9) would fail at build time, and every media input would cross the sandbox twice.

**Unmodelled northbound fields are handed through, not dropped.** Today the OpenAI-compatible arm forwards the whole
client body with only `model` replaced (embeddings.rs:208-209, :222), so a client can send `input_type` (NVIDIA NIM's
query / passage switch), `truncate` and the like. A request type with only the modelled fields would silently drop
them — and for asymmetric retrieval models, the request extras' default `passage` would then apply to queries too,
degrading results without an error. So `extra` carries every top-level field other than `model`, `input`,
`dimensions`, `encoding_format` and `user`, bounded (at most 32 keys and 16 KiB serialized; beyond that, 400 before
admission; a key starting with `$south.` is a 400). The component decides per dialect: the OpenAI-compatible
component forwards them unchanged (today's behaviour); Gemini and Vertex ignore them, as their native arms do today
(translate_gemini.rs:78 onward and embeddings.rs:629-671 read only `input` and `dimensions`). A component may refuse
a field instead of ignoring it, but that choice must be pinned by a fixture. Operator request extras are applied
after the component, outside the immutable paths, with the operator's own override / keep-existing mode deciding
against a client field of the same name, as today.

**Northbound parsing (normative; the host executes it, and both hosts must agree, because `vector_count` and the
Gemini single / batch choice depend on it):**

| Northbound `input` | Inputs | `input_shape` |
|---|---|---|
| a string | one input (text, or media when it matches the media form below) | `Single` |
| an array of strings | one input per string, in order | `Array` |
| an array of integers | **one** `TokenIds` input (OpenAI's single token-sequence form) | `Single` |
| an array of arrays of integers | one `TokenIds` input per inner array | `Array` |
| anything else: a mixed array, an empty string, an empty array, an empty inner array, a non-integer, a negative integer or one above `u32::MAX`, more than `MAX_EMBEDDING_INPUTS` inputs | — | 400 before admission |

A string is a media input when it has the form `data:<media type>;base64,<payload>`, where `<media type>` is the text
between `data:` and the **first** `;base64,` and must match `type/subtype` optionally followed by `;name=value`
parameters (RFC 2045 tokens); `media_type` is that text verbatim (today's detection and translation,
embeddings.rs:26-42, translate_gemini.rs:53-57). The payload is not decoded. Every other string, including a `data:` URI
that is not base64, is text. `dimensions` must be an integer above 0; `encoding_format` is `float` (the default) or
`base64`; any other value is a 400 before admission. South supplies this parser as a pure function with golden vectors
(`parse_embeddings_request_v1`, E-Q9).

On the `conformance` side:

```rust
pub struct PreparedEmbeddingsV1 {
    descriptor: HttpRequestDescriptor,         // kernel IR; the body may contain media reference nodes
    estimate: EmbeddingsEstimateV1,
    vectors: VectorLocatorV1,
    immutable_body_paths: Option<Vec<String>>, // syntax and semantics as in task contract 6 (south-contracts/src/task_v2.rs:48-60)
    parse_context: serde_json::Value,          // handed back unchanged by the host to parse, never interpreted; bounded
}
```

`immutable_body_paths` reuses task contract 6's definition and validation function (`validate_immutable_body_paths`)
directly: `null` = the component takes no position, and the host must not inject extra fields; `[]` = the host may
inject anywhere; non-empty = these dotted paths, their ancestors and their descendants must not be rewritten. It
replaces the host's per-type `OPENAI_/GEMINI_/VERTEX_EMBEDDINGS_OWNED_FIELDS` (server:…/embeddings.rs:63-65). Array
indices are not part of the syntax, so per-item fields in Gemini's batch form remain out of reach — the same
limitation the host has today (same file, :61).

## 4. WIT and manifest

```wit
// Embeddings component ABI, version 1. All exports are pure translation; the host owns
// credentials, HTTP, bytes, vector extraction, pricing and settlement.
package token-station:embeddings-adapter@1.0.0;

interface embeddings-adapter {
    record adapter-metadata { name: string, version: string, api-version: string }
    enum health-status { ready, degraded, unavailable }
    record adapter-health { status: health-status, detail: option<string> }

    type json = string;

    metadata: func() -> adapter-metadata;
    healthcheck: func() -> adapter-health;

    // ProviderConfig, EmbeddingsRequestV1 -> PreparedEmbeddingsV1.
    // A request the model or dialect cannot serve is a capability error here,
    // before the host admits anything.
    build-embeddings-request: func(provider-config: json, embeddings-request: json) -> result<json, json>;

    // HttpResponseParts of a 2xx whose vectors the host has replaced with null per the
    // prepared locator, plus the prepared parse-context -> EmbeddingsParsedV1.
    // A body that cannot yield the facts is an error, never a zero.
    parse-embeddings-response: func(response-parts: json, parse-context: json) -> result<json, json>;

    // HttpResponseParts of a non-2xx -> { outcome: rejected | unknown, error: ErrorEnvelope }.
    // `rejected` only when the response proves the upstream produced nothing.
    map-provider-error: func(response-parts: json) -> result<json, json>;
}

// No host import: nothing here signs, and credentials are named in the descriptor.
world embeddings-adapter-v1 {
    export embeddings-adapter;
}
```

**Manifest.** `KNOWN_WORLDS` (manifest.rs:150-152) gains one entry:

| Property | Value |
|---|---|
| world / `api_version` | `embeddings-adapter-v1` |
| WIT package | `token-station:embeddings-adapter@1.0.0` |
| Behavior suite | `south.embeddings-component.v1` |
| Capability vocabulary | `embed` (required), `batch` (accepts more than one input), `dimensions` (honors the requested dimensions), `token_ids`, `media` |
| Auth arms | `bearer`, `header_secret` (minted credentials go through the `minted` slot of the boundary record's §3 and are still presented as bearer) |

Capability words describe "the most the component can do"; per-model refusals (e.g. a model that accepts only
text) are returned from `build-embeddings-request` as capability errors, so the host answers 400 before admission,
with zero upstream calls. The host therefore no longer writes judgments such as "only Gemini accepts multimedia" or
"`gemini-embedding-001` accepts only text". Which models accept media is data: it comes from dialect words on the
model row (boundary record §7.4) or the model catalog (§7.5 there), read by the component, not from a model list
compiled into it.

**Non-2xx outcomes.** `map-provider-error` classifies each non-2xx as `rejected` (the response proves nothing was
produced: a 4xx such as an invalid request, a rejected credential, an unknown model or a rate limit) or `unknown` (5xx
and everything else). The boundary record's rule applies (§6.4 there): `rejected` releases the reservation, **after the
dual run** (lv's ruling on image record Q7); during the dual run the host keeps today's behaviour, where every non-2xx
after dispatch is `delivery_unknown` (embeddings.rs:438-453). The image and speech worlds carry the same four-way
outcome inside `parse-response`; embeddings keeps a separate function because its 2xx parse has no failure kinds of its
own.

**Runtime.** Following task-v2: add one `bindgen!` module, one instance kind and three `call_*` functions
(crates/south-provider-runtime/src/bindings.rs:25-45; component.rs:760-770); the import scan refuses any `host`
namespace for this world (as loader.rs:216-224 does for task-v2).

## 5. Vector payloads: the host extracts per declaration

**Why the component does not parse the whole response.**

- The runtime's per-call boundary payload limit is 16 MiB with a 2-second wall clock
  (crates/south-provider-runtime/src/runtime.rs:30-32). A batch of 2048 inputs × 3072 dimensions easily exceeds
  several tens of MiB as float JSON; the host's Gemini arm has already switched to the 64 MiB binary transport
  because of size (server:…/embeddings.rs:1009-1022; south-contracts/src/lib.rs:121, 131). Passing such a body into
  and out of wasm twice would hit both the size limit and the time limit.
- The host's OpenAI-compatible arm today returns the upstream bytes to the client **unchanged**
  (server:…/embeddings.rs:614-619), including base64 vectors returned by the upstream. If the component
  re-serializes the floats, the values are equal but the text may differ, so dual-run reconciliation could compare
  only values, not bytes.

**Locator declaration** (`VectorLocatorV1`, given by `build-embeddings-request` for each request — the single and
batch shapes may differ):

| Form | Meaning | Used for |
|---|---|---|
| `NorthIdentical` | The upstream body is already a northbound OpenAI embeddings response; the host validates it and, when no conversion is needed, returns it unchanged | OpenAI-compatible |
| `Array { array, vector, index }` | `array` points at the vector array, `vector` is the pointer within an element, `index` is optional (when present, sort by it) | Gemini batch `/embeddings` + `/values`; Vertex `/predictions` + `/embeddings/values` |
| `Single { vector }` | A single vector | Gemini single `/embedding/values` |

`NorthIdentical` is not provider knowledge: the northbound protocol is the host's product surface (P21 §1.1), and
the host already knows the shape of an OpenAI embeddings response.

**Encoding is detected per vector, not declared.** Many OpenAI-compatible servers ignore `encoding_format=base64` and
return floats (to be confirmed per server; the host does not depend on it); a locator that declared the encoding at
build time would then fail extraction after dispatch on every such call, where today the bytes simply pass through.
Both vector shapes are public OpenAI shapes, so the host decides per vector: a JSON array of numbers is a float
vector; a JSON string is base64 of little-endian f32 (its decoded length must be a multiple of 4); anything else is a
protocol error.

**Erasure protocol.** The host parses the 2xx body (bounded), extracts every vector per the locator, replaces each
vector position with `null`, and hands this skeleton together with `parse_context` to `parse-embeddings-response`.
The component sees structure, counts and usage fields, but no floats. The `vector_count` the component counts must
match the number of vectors the host extracted. Extraction, erasure, encoding detection and the northbound
conversions of §8 are transforms both hosts must execute identically, so south supplies them as pure functions with
golden vectors (`extract_vectors_v1`, `render_vectors_v1`, E-Q9).

**Alternative (not recommended)**: the component parses the whole response and returns IR vectors, and the host
only renders. This is the simplest to implement, but it either limits batch size and dimensions so responses stay
within 16 MiB, or raises this world's payload limit and time limit; and it gives up byte-level pass-through. If the
south maintainers consider the erasure protocol too heavy, this can be chosen instead after measurement
(§14 E-Q1).

## 6. How the three dialects land

| | OpenAI-compatible (including `azure-openai-v1`) | Gemini native | Vertex `:predict` |
|---|---|---|---|
| URL | `base_url` + `/embeddings` (built by the component; the kernel's `ProviderApi` has no embeddings, so `resolve` cannot be used); Azure's `api-version` query | `…/v1beta/models/{model}:embedContent` or `:batchEmbedContents` (by `input_shape`) | `…/publishers/google/models/{model}:predict`; project and region come from non-secret configuration or minted exported attributes (boundary record §3.3, §7.3) |
| Auth | `bearer`; Azure uses `header_secret` `api-key` | `header_secret` `x-goog-api-key` | `bearer`, with the slot minted by the service-account recipe |
| Batch | Supported, up to 2048 inputs | Supported (`batchEmbedContents`) | Accepts only one; more than one is a capability error at build time (the same rule as host plan 58 P2) |
| `dimensions` | Forwarded unchanged | Per-item `outputDimensionality` | `parameters.outputDimensionality` |
| `encoding_format` | Forwarded; the host detects what came back (§5) | The upstream returns floats only | The upstream returns floats only |
| Token-id input | Forwarded | Capability error | Capability error |
| Media input | Capability error | Per model (dialect word on the row): supported models place it as `inline_data` via a reference node; text-only models give a capability error | Capability error |
| Unmodelled fields (`extra`) | Forwarded unchanged | Ignored | Ignored |
| Vector locator | `NorthIdentical` | `Single` / `Array` | `Array` |
| Usage source | `Reported`: `usage.prompt_tokens` | `NotReported` (until DE3 is measured) | `Reported`: the sum of per-item `statistics.token_count`, each rounded half away from zero as the host does (embeddings.rs:691-697; the wire carries a float), with `per_input_tokens` given |
| Fallback estimate | None (cannot be settled if not reported) | Yes (§7.2) | None |
| Immutable paths | `["input", "model"]` | `["content", "requests", "model"]` | `["instances"]` |

The immutable paths are taken verbatim from the host's three tables today (server:…/embeddings.rs:63-65), so the
behavior of request extras (additional request configuration) is the same before and after migration.
`nvidia-nim`'s default `input_type` does not go into the component: P24 E0 already expresses it through the
operator's request extras; it is operator data, not dialect.

**Families beyond the three dialects.** The OpenAI-compatible component serves every provider whose wire is the
OpenAI embeddings shape, but several of them differ in how they authenticate, and today the host decides that by
type (§1). After migration each is a family that declares its own authentication, and none of it stays in the host:

| Family | Today in the host | After migration | Depends on |
|---|---|---|---|
| GLM (`oauth_token` before `api_key`) | secret chosen by type (south_adapter.rs:375-379) | the credential section's `fields` / `slots` name which stored field is the slot's value | boundary record §3.3 |
| GLM Coding (fixed user-agent) | user-agent chosen by type (south_adapter.rs:268-281; upstream.rs:146-153) | the family declares its user-agent value | boundary record §10 (user-agent declaration) and Q10 (DP7) |
| GitHub Copilot (minted token, seven editor headers) | not served on this surface today (§1) | a recipe mints the token; the family declares the editor headers | boundary record §3 (recipe with `http_exchange`), §10, Q10 |
| Azure OpenAI (`api-key` header, `api-version` query) | auth and URL chosen by type (:266-279, :224) | `header_secret` and the query declared by the family | already sanctioned today; boundary record §10 once it lands |

The host has no special case for Copilot (E-Q11, ruled by lv on 2026-10-01): whether Copilot rows are served on
`/v1/embeddings` after migration follows from whether the Copilot component declares embeddings support, and serving
or not serving them needs no host code.

Vertex responses also carry `metadata.billableCharacterCount` (used by older models priced by character); the host
does not read it today, and v1 of this contract does not model it.

## 7. Usage facts and estimates

### 7.1 Semantics

- `Reported`: the upstream reported the input token count. `input_tokens` is required. When the upstream reports per
  input (Vertex), `per_input_tokens` is given as well.
- `NotReported`: the upstream did not report this time. Whether the call can be settled depends on the
  `fallback_input_tokens` given at build time: if present, settle by it and label it as estimated; if not, it
  cannot be settled.
- The same discipline as the provider world (boundary record §6): **a 2xx that cannot be parsed must not be
  reported as zero**. OpenAI-compatible and Vertex must report an error when usage is missing (consistent with the
  host's rule after the P24-E-F1 fix: server:…/embeddings.rs:556-588); substituting `Reported { input_tokens: 0 }`
  is not allowed.

### 7.2 Gemini's fallback estimate

The fallback value is computed by the component when it builds the request, with the formula copied verbatim from
the host's current implementation so that dual runs agree: text by UTF-8 byte count, `(len + 3) / 4`; media by a
constant per MIME top-level type — audio 512, video 1024, everything else 258 (server:…/embeddings.rs:45-53,
:926-939). These constants are Gemini's metering approximation and therefore dialect knowledge; once they are in
the component, the host no longer holds them. The component needs only lengths and media types, which the request
view carries even for inputs handed as blobs (§3).

If DE3 (whether Gemini native responses carry token counts) measures "yes", the component switches to reporting
`Reported`, and the fallback estimate is removed: a missing report then goes to `delivery_unknown` (ruled by lv on
2026-09-30, §14 E-Q2).

### 7.3 Reservation upper bound (host, provider-agnostic)

The host computes the input-token upper bound from the **northbound** request with the boundary record's rule (§6.3
there: request bytes plus a fixed, host-configured allowance per media part). **This world's instance of that rule**
counts only the inputs, **not the bytes of media inputs**, and adds a fixed allowance per input; it is the sum over
inputs of:

- a text input: its UTF-8 byte count (a token occupies at least one byte);
- a token-id input: its number of ids (exact);
- a media input: the host's configured per-media-part allowance (its bytes are not counted: base64 bytes say nothing
  about tokens);

plus 64 per input for special tokens (the host Vertex arm's allowance, embeddings.rs:775-780). A component may
tighten only the **reservation**, with `max_input_tokens`: the host reserves at `min(host bound, max_input_tokens)`.
**Every check compares against the host bound only** (§7.4): the fallback estimate and reported counts are never
checked against the component's own `max_input_tokens`, so a component number is never both the reservation and the
thing checked against it.

The Gemini component sets `max_input_tokens` equal to its fallback estimate, so Gemini's reservation stays what it is
today (the estimate). Without that, the host bound for a media input would be the configured allowance rather than
the base64 length: a 10 MiB video would otherwise reserve about 14 million tokens against today's 1024, refusing
low-balance and quota-limited callers at admission — the reason the bound excludes media bytes. The host configures
the per-media-part allowance at or above the largest per-part estimate of the packages it installs (1024 for today's
Gemini component); a component estimate above it goes to manual review, so a too-small allowance fails safe.

### 7.4 The host's generic checks

- **Out-of-bound checks** (against the host bound of §7.3, not the component's tightening):
  `Reported.input_tokens ≤ host bound` and `fallback_input_tokens ≤ host bound`; the settled amount ≤ the
  reservation (the existing settlement guard). A violation goes to manual review.
- **Internal-consistency checks**: if `per_input_tokens` is given, its length equals the input count and its sum
  equals `input_tokens`; `vector_count` equals the number of vectors the host extracted, which equals the input
  count; every vector is non-empty and all vectors in one request have the same length; when the request carries
  `dimensions` and the component declares the `dimensions` capability, the vector length equals it; if `index` is
  declared, its values are exactly `0..n`, each once.
- **`NorthIdentical` usage equality**: the northbound body the client receives carries the upstream's
  `usage.prompt_tokens`, while settlement uses the component's `Reported.input_tokens`. Since the body is by
  definition in the northbound shape, the host reads `usage.prompt_tokens` itself (as it does today,
  embeddings.rs:556-565), requires a non-negative integer, and requires it to equal `Reported.input_tokens`; a
  mismatch goes to manual review. The client therefore always sees the number it is billed for (the P24-E-F3 rule).
- **Undetectable zone** (the same formulation as the boundary record's §6.3): under-reporting, and deviation that
  falls within the upper bound, cannot be detected by the host. For Gemini this covers the whole charge, since the
  estimate is the component's; trust comes from pinned package digests, gate ② usage samples transcribed from the
  native arm, documentation-derived judges (Gemini's after DE3), and dual runs before cutover.

### 7.5 Settlement and labeling

| Component gives | Host settles | `tokens_estimated` | `quantity_estimated` | Northbound `usage.prompt_tokens` |
|---|---|---|---|---|
| `Reported(n)` | n | 0 | 0 | n (for `NorthIdentical`, the upstream's value, which §7.4 requires to equal n) |
| `NotReported` + fallback f | f | 1 | 1 | f |
| `NotReported`, no fallback | Not settled; 502, `delivery_unknown` | — | — | — |
| Parse failure / consistency check failure | Same as above | — | — | — |

Both labeling columns reuse the host's existing semantics: `tokens_estimated` means the token count was computed by
the gateway, and `quantity_estimated` means the billed quantity (and therefore the amount) is an estimate (the
`tokens_estimated` / `quantity_estimated` paragraphs of server:gateway/CLAUDE.md;
server:crates/gateway-provider-protocol/src/usage_types.rs:131-147). Gemini today is `EstimatedTokens`, with both
columns at 1 (the P24-E-F2 fix, embeddings.rs:1208-1216); this is unchanged before and after migration. The
northbound `usage` always carries the number used for settlement (the P24-E-F3 fix, embeddings.rs:1183-1188, is
covered by generic rendering; `NorthIdentical` by the equality check of §7.4).

## 8. `dimensions`, `encoding_format` and batching

- **`dimensions`**: the IR carries the requested value; the component places it per dialect (§6). When the model
  does not support it, the component returns a capability error; the host no longer judges by model.
- **`encoding_format`**: the IR carries the format the northbound caller wants. The host renders northbound with
  south's `render_vectors_v1`: base64 → float emits each f32 as its shortest round-trip decimal; float → base64
  rounds each number to the nearest f32 and encodes little-endian (what OpenAI's base64 form is defined as). For
  `NorthIdentical`, when every vector already has the requested encoding, the body is returned unchanged; otherwise
  the host renders a generic OpenAI embeddings response (fields beyond `object`, `data`, `model` and `usage` are then
  not preserved).
- **Batching**: `input_shape` preserves whether the northbound input was a single input or an array (§3 table); the
  northbound `data` is always an array. Input-count limits: 2048 at the contract level; at the dialect level the
  component refuses at build time (1 for Vertex). The host does no fan-out — one admission maps to one send, and the
  funds state machine is unchanged (the rule of host plan 58 P2).

## 9. What the host's generic executor is expected to do

1. Select the package by model-row routing (pinned by digest), and fetch the non-secret configuration and
   credential slots; when a slot is `minted`, first mint per the recipe and obtain the exported attributes
   (boundary record §3); a failure is a pre-admission error.
2. Parse the northbound request into `EmbeddingsRequestV1` per the §3 table, handing media inputs and oversized text
   as blobs; any refusal is a 400 before admission.
3. Call `build-embeddings-request`; capability error → 400, zero upstream calls.
4. `ProviderConfig::authorize` (kernel:provider.rs:361) and descriptor auth admission (boundary record §4.2);
   replace reference nodes with the blobs (`reference_integrity`); inject operator extra fields outside the immutable
   paths; seal the outbound descriptor.
5. Compute the reservation bound per §7.3, admit, and write the dispatch marker.
6. Send through south's buffered binary call (body limit 64 MiB, south-contracts/src/lib.rs:131).
7. Non-2xx → `map-provider-error` → northbound error; `unknown` → `delivery_unknown`; `rejected` → released after the
   dual run, `delivery_unknown` during it (§4).
8. 2xx → extract vectors per the locator, detect their encoding and erase them → `parse-embeddings-response` → the
   §7.4 checks.
9. Render the northbound response per `encoding_format` (for `NorthIdentical` with no conversion, the upstream bytes
   unchanged) and reserve the memory for it. Any failure here is `delivery_unknown`, not settled.
10. Settle and label per §7.5, then deliver the rendered body. Rendering comes before settlement so that the host
    never charges for a response it then fails to produce — today's order as well (embeddings.rs:1178-1226).

What no longer exists in the host: per-type arms, the per-type auth choices of §1 and §6 (auth arm, companion headers,
user-agent, secret field), the media constants, the `gemini-embedding-001` check, the per-type immutable tables, and
the leaf crate's Gemini embeddings translation.

## 10. Conformance (`south.embeddings-component.v1`)

**Fixture families**: `embeddings.request`, `embeddings.response`, `embeddings.error`.

**Rows required by name** (missing any one fails `Coverage`):

| Row | Assertion |
|---|---|
| `request.single-text` / `request.batch-text` | Descriptor and prepared value (including locator, fallback estimate, `max_input_tokens` and immutable paths) match exactly |
| `request.dimensions` | The dimensions land in the dialect's location |
| `request.refused-capability` | Input the dialect does not support (token ids, media or too many inputs) → capability error |
| `request.media` (components declaring `media`) | The media input appears in the body only as a reference node to its blob |
| `request.extra-fields` | Unmodelled fields are forwarded, ignored or refused as the component documents |
| `response.usage` | Usage sample: the `Reported` numbers, and `per_input_tokens` (when the dialect reports per input) |
| `response.missing-usage` | Dialect without a fallback → error; dialect with a fallback → `NotReported` |
| `response.count-mismatch` | The skeleton's count differs from the request's input count → error |
| `error.rejected-credential` | 401 / 403 are `rejected` and not retryable (reusing the provider world's check) |
| `error.server` | A 5xx is `unknown` |

**Checks**: `FixtureMatch`, `Determinism`, `UnknownFieldTolerance`, `EndpointConfinement`,
`AuthErrorsAreNotRetriable` (reusing the existing items of south-component-conformance/src/report.rs:19-64), plus:

- `DescriptorAuthWithinManifest` (boundary record §4.4);
- `ReferenceIntegrity` (image record §12.1): every reference node points at a blob of the request view, and no output
  string exceeds the media fallback threshold;
- `UsageNeverDefaulted`: response fixtures carry `usage_pointer`; the suite deletes that location and requires the
  component to report an error or `NotReported`, never a `Reported` zero;
- `LocatorResolves`: the prepared locator, applied to the paired response fixture, resolves exactly as many vectors
  as the request has inputs;
- determinism of the fallback estimate (the same request yields the same value twice).

**Host obligations** (gate ③): northbound parsing, vector extraction and erasure, encoding detection and rendering
equal south's golden vectors (in practice, the host calls south's pure functions); the reservation bound follows
§7.3, and a fixture whose component declares `max_input_tokens` above the host bound is reserved at the host bound;
the `NorthIdentical` equality check; rendering before settlement.

**Documentation-derived judges**: as in the provider world (boundary record §6.2 item 5), each of the three
embeddings packages south publishes has a usage judge whose expectations are derived from provider documentation,
written into the release discipline; Gemini's judge is added after DE3 is measured.

## 11. Migration and dual runs

| Step | Side | Content | Acceptance |
|---|---|---|---|
| E0 | Host | Done (2026-09-30): the P24-E-F1, E-F2, E-F3 and E-F4 fixes; empty input refused before admission (#60); the nvidia default is now expressed through request extras | — |
| E1 | South | This record's contract, WIT, runtime world and suite; the pure functions of §3 and §5; three reference implementations and packages (suggested: `embeddings-openai-compatible` carrying the families `openai-compatible` and `azure-openai-v1`, `embeddings-gemini`, `embeddings-vertex`) | The three packages pass the suite; a south minor release, listed in the release index |
| E2 | Host | §9's generic executor; model rows routed to components; dual-run reconciliation | See below |
| E3 | Host | Delete the three native arms and the embeddings part of the leaf crate | The J1 count falls; installing or removing an embeddings package needs zero host changes |

E3's "zero host changes" holds for the GLM, GLM Coding and Copilot families only once the boundary record's §3
recipes (phase B4) and §10 instance declarations (phase B7a) have landed; until then those families stay on their
native paths and remain J1 items.

**Dual-run reconciliation** (the same request goes through the native arm and the component arm separately, once
under each billing form), compared item by item:

1. Upstream request: method, URL, non-auth headers, body (JSON equality), including requests that carry unmodelled
   fields such as `input_type`.
2. Northbound response: bytes for `NorthIdentical`; otherwise vector values, order, count and `usage`.
3. Reservation, settled amount, `tokens_estimated`, `quantity_estimated`.
4. Handling of the failure fixtures: 2xx with missing usage (OpenAI-compatible, Vertex), missing vectors, count
   mismatch, empty vectors, 401, 429, 5xx.

**Intentional differences** (on record, not counted as reconciliation failures):

- When a Gemini batch response lacks `embeddings` or has an empty vector, the native arm fills in an empty array and
  returns 200 as usual (server:crates/gateway-provider-protocol/src/translate_gemini.rs:148, :153, :159); the
  component arm treats it as a protocol error per §7.4 and goes to `delivery_unknown`.
- Reservation: the §7.3 bound differs from the OpenAI-compatible arm's (today the whole northbound body's bytes,
  embeddings.rs:381-385): it counts only the inputs, counts token ids exactly and gives media a fixed allowance. For
  Gemini (tightened to the estimate) and Vertex (text bytes + 64) it is unchanged. It affects only frozen funds and
  admission when the balance is insufficient, not the settled amount.
- More than 2048 inputs on an OpenAI-compatible upstream: the native arm forwards them; the component arm refuses
  them before admission (the contract limit).
- `NorthIdentical` responses that need an encoding conversion are re-rendered (§8) and lose upstream fields beyond
  the OpenAI shape; the native arm returns them unchanged and unconverted.

Not a dual-run difference: during the dual run both arms park every non-2xx after dispatch as `delivery_unknown`
(§4); a `rejected` classification starts releasing the reservation only after cutover (boundary record §6.4), when the
native arm is gone. The classification itself is still compared in the dual run, through the `error.*` fixtures.

Token-id and empty inputs sent to Gemini are no longer a difference: the native arm now refuses them before admission
(P24-E-F4 and #60; embeddings.rs:948-959, translate_gemini.rs:46-52), as the component does at build time.

**Dependency**: the Vertex package depends on the credential recipes of the boundary record's §3 (service account →
bearer, with `project_id` exported); until recipes land, the host can use its existing service-account minting as a
stopgap, but that is a J1 red item and must be cleared before E3.

## 12. Versioning

- New world: the tuple's WIT package, world name and suite name are new by construction
  (2026-08-27-manifest-schema-beyond-one-world.md:160-162), so the existing 13 packages are unaffected.
- `compatibility.json`: `contracts` gains `embeddings: 1`, and `conformance` gains the suite id;
  `host_capabilities` records adoption per host.
- The world uses `contracts.media` v1 for blob references and the `as_is` transform (§3), and adds nothing to it; it
  therefore ships in or after the image world's first minor.
- Host link layer: to install packages for this world, the host must link a south version that knows it — a
  one-time upgrade for a new northbound surface (P21 §1.4); after that, adding or removing embeddings providers
  touches only the package layer.
- When this lands together with the compatibility range of the boundary record's §8, embeddings packages declare
  `contracts: {"media": 1, "embeddings": 1}`.

## 13. Rejected alternatives

- **Adding embeddings functions to the provider world and raising it to v3** (rejected by DE2): it would ripple
  through the existing four chat packages, while embeddings and chat share no request or response shape.
- **Defining embeddings types in the kernel IR** (rejected by DE1): it would have to travel the whole "community
  protocol → kernel mirror → south" chain, while the chat IR does not need embeddings.
- **The component reporting a reservation figure the host uses as is**: a reservation is a funds decision that
  freezes funds; the component may only tighten the host's bound (§7.3).
- **The reservation bound from the outbound body's bytes** (this record's first draft): the component would set its
  own ceiling, and media inputs would reserve by base64 length, orders of magnitude above any estimate (§7.3).
- **Media bytes inside the request type** (this record's first draft): they would cross the sandbox twice and fail
  above the runtime payload limit (§3).
- **A request type with only the modelled fields** (this record's first draft): client fields the OpenAI-compatible
  arm forwards today would be dropped silently (§3).
- **Declaring the vector encoding in the locator** (this record's first draft): fails after dispatch on upstreams
  that ignore `encoding_format` (§5).
- **The host keeping Gemini's estimate formula while the component reports only `NotReported`**: the media constants
  are Gemini's metering approximation; leaving them in the host is provider knowledge, and J1 would not reach zero.
- **The host fanning multiple inputs out to a dialect that accepts only one**: one admission would become several
  sends, and the host has no funds state for partial success today (host plan 58 P2 already declined this).

## 14. Open questions

- **E-Q1 (S)** Vectors extracted by the host per declaration, with the component seeing only the erased skeleton
  (recommended), or the component parsing the whole response (simpler to implement, but needs limits on batching or
  a relaxed payload limit and time limit for this world)? Suggest first measuring the timing of both approaches on a
  2048 × 3072 batch.
- **E-Q2 (L)** DE3: if Gemini native responses carry token counts, switch to reporting `Reported`; is the fallback
  estimate then kept as a backstop for missing reports, or removed so that a missing report goes to
  `delivery_unknown`?
  **Ruled (lv, 2026-09-30): removed.** If DE3 measures that token counts are reported, a missing report goes to
  `delivery_unknown`; if DE3 measures that they are not, the estimate stays and is labeled as an estimate.
- **E-Q3 (L)** Is it acceptable for two conventions to coexist — the estimate given by the component at build time
  (this record's approach) and, per the boundary record's Q12, a generic host estimate on the chat side?
  **Ruled (lv, 2026-09-30): acceptable**; see the boundary record §16 Q12.
- **E-Q4 (S)** Package granularity: three packages (recommended), or one package with three families.
- **E-Q5 (S, community host)** ARCHITECTURE.md:114-115 requires a metering vocabulary to have "a second consumer in
  sight": does the community host have, or will it have, an embeddings surface? If not, this contract's usage
  vocabulary needs an explicit exemption from the maintainers.
- **E-Q6 (L)** `per_input_tokens` has no host consumer today (the host uses only the sum). Keep it optional
  (recommended; zero cost, and it gives Vertex's consistency check something to hold on to), or leave it unmodeled
  in v1?
  **Ruled (lv, 2026-09-30): keep it optional.**
- **E-Q7 (S)** `NorthIdentical` makes the host return the upstream bytes unchanged, which also passes the upstream's
  `model` field through unchanged (as the host does today); whether the host should instead uniformly rewrite it to
  the northbound model name must be settled consistently with the host's northbound conventions.
- **E-Q8 (S)** The embeddings world depends on `contracts.media` v1 for blob references (§3), coupling its release to
  the image world's first minor. Accept the coupling (recommended; the alternative is a second copy of the same
  grammar), or give embeddings its own reference grammar?
- **E-Q9 (S)** South supplies the northbound parser, vector extraction and erasure, encoding detection and rendering
  as pure functions with golden vectors in `south-contracts` (§3, §5, §8), which both hosts call. Recommended; the
  alternative is a host suite that only tests each host's own implementation.
- **E-Q10 (L)** The bounds on unmodelled northbound fields (32 keys, 16 KiB) and the rule that a component may
  refuse such a field only with a fixture: acceptable?
  **Ruled (lv, 2026-10-01): accepted as proposed** — 32 keys, 16 KiB, and a component may refuse an unmodelled field
  only with a fixture (§3).
- **E-Q11 (L)** GitHub Copilot is not served on `/v1/embeddings` today (§1). After migration it could be, once its
  family declares its recipe and headers: serve it, or keep refusing?
  **Ruled (lv, 2026-10-01): the host has no special case for Copilot; whether Copilot rows are served on
  `/v1/embeddings` follows from whether the Copilot component declares embeddings support** (§6).

## Revision note (2026-10-01)

- Header: host baseline moved to `a82c852b` and every host citation updated to it; predecessors add the image record.
- §1: the provider-knowledge table lists the per-type auth arm, companion headers, user-agent and secret-field choice;
  Copilot's state today is stated; the removed `nvidia-nim` row-name default is no longer listed as present.
- §2: D3 covers bytes in both directions; D4 uses `NotReported`; D5 makes the bound the host's.
- §3: media inputs and oversized text go through the shared media vocabulary as blobs and reference nodes; unmodelled
  northbound fields are handed through as `extra`; a normative northbound parsing table; `NotReported`;
  `max_input_tokens`; the locator loses its encoding.
- §4: `map-provider-error` classifies non-2xx as `rejected` or `unknown`; media support per model comes from dialect
  words or the catalog.
- §5: vector encoding detected per vector; extraction and rendering supplied by south as pure functions.
- §6: rows for unmodelled fields and Vertex rounding; a table of the families beyond the three dialects (GLM, GLM
  Coding, Copilot, Azure) and what each depends on.
- §7.3: the bound is computed from the northbound request per the boundary record §6.3, excludes media bytes, and a
  component may only tighten it; Gemini's reservation is unchanged.
- §7.4: checks are against the host bound; a `NorthIdentical` usage equality check.
- §8, §9: rendering per `render_vectors_v1`; rendering moved before settlement; `rejected` handling.
- §10: rows for media, extra fields and 5xx; `ReferenceIntegrity`; host obligations.
- §11: E0 lists E-F4 and #60; the token-id difference is gone (the host fixed it); new intentional differences; E3
  depends on boundary phases B4 and B7a for the GLM, GLM Coding and Copilot families.
- §12: `contracts: {"media": 1, "embeddings": 1}`.
- §13: five first-draft choices recorded as rejected.
- §14: E-Q8 to E-Q11 added.
- Round 2 (2026-10-01), §7.3: the record states its instance of the boundary record §6.3 bound (inputs only, media
  bytes not counted, 64 per input); the reservation is the minimum, and every check uses the host bound only.
- Round 2, §4 / §11: `rejected` cited to the boundary record §6.4; releasing it is no longer listed as a dual-run
  difference, since during the dual run both arms park every non-2xx.
- Rulings of 2026-10-01:
  - E-Q10 ruled by lv: accepted as proposed (32 keys, 16 KiB, refusal of an unmodelled field only with a fixture).
  - E-Q11 ruled by lv: no host special case for Copilot; serving Copilot rows on `/v1/embeddings` follows from the
    Copilot component's embeddings declaration. §6 states it in place of the open product decision.
