# The embeddings contract and the `embeddings-adapter-v1` world

Status: proposed — drafted for review by the host team (token-station-server P21), not accepted

Date: 2026-09-30

Predecessors: `2026-09-20-task-adapter-v2-candidate.md` (the precedent of a south-local contract, with conformance
owning the IR-bearing prepared type and the single JSON codec), `2026-09-19-runtime-second-world.md` (one runtime
hosting several worlds), `2026-09-27-task-contract-v6-facts.md` (request estimates and reported usage modeled
separately; `immutable_body_paths`), `2026-09-30-host-zero-vendor-boundary.md` (this record depends on its §3
credential recipes, §4 descriptor auth admission, §6 usage discipline and §8 compatibility range).

Origin: token-station-server `docs/product-review-v2/plans/2026-09-30-P24-embeddings与同步音乐迁组件.md`, items E1,
DE1 (the contract lives south-local, not in the kernel IR) and DE2 (a new world rather than raising the provider
world to v3) — both approved as recommended on 2026-09-30, as relayed by the host team; DE3 (whether Gemini
responses carry token counts) awaits measurement. The umbrella plan is P21 in the same directory (DP0, DP1).

Baseline: south `origin/main` = v0.42.0 (`3135e36`); kernel `f585bc83` (protocol 0.4.0). Host line numbers refer
to token-station-server `8b2a1976` and carry a `server:` prefix; kernel line numbers carry `kernel:` and refer to
`crates/protocol/src/`.

## 1. Problem

The host's `/v1/embeddings` splits into three execution arms by `ProviderType`
(server:gateway/src/modules/inference/handler/embeddings.rs:172-205), and each carries provider knowledge:

| Arm | Provider knowledge in the host |
|---|---|
| OpenAI-compatible | Adds `input_type` for `nvidia-nim` by row name (:213-225); the immutable-field table is chosen by type (:56-78); URL differences for Azure and Copilot are handled by `build_upstream_url` |
| Gemini native | A single input goes to `:embedContent`, an array to `:batchEmbedContents` (:958-971); request / response translation lives in the leaf crate (server:crates/gateway-provider-protocol/src/translate_gemini.rs:64, 119); estimates by "1 token per 4 bytes + media constants 258 / 512 / 1024" and settles by that estimate (embeddings.rs:45-53, 942-956) |
| Vertex `:predict` | Accepts only a single input (:637-686); reads `predictions[0].embeddings.statistics.token_count`, and goes to `delivery_unknown` when it cannot (:688-720); mints a Bearer from the service account on each call |
| Shared entry point | Multimedia input is allowed only for Gemini, and `gemini-embedding-001` is hard-coded as text-only (:151-169) |

The provider world cannot hold this: its capability vocabulary is closed to chat / stream / tool_call /
json_schema (crates/south-provider-api/src/manifest.rs:69-74), and a test specifically pins that declaring
`embeddings` is refused (crates/south-provider-api/tests/provider_api_v2.rs:222-231); the kernel's `ProviderApi`
has only four kinds and no IR types for embeddings (kernel:provider.rs:137-143); and south's earliest record lists an
embedding IR as a non-goal (2026-08-16-minimal-provider-call.md:38).

## 2. Decisions

- **D1 The contract is south-local (DE1).** The IR-independent request, estimate, usage and vector-locator types go
  in `south-contracts`; the IR-bearing `PreparedEmbeddingsV1` and the single JSON codec go in
  `south-component-conformance` — the same division as task-v2 (ARCHITECTURE.md:86-90;
  2026-09-20-task-adapter-v2-candidate.md:27). The kernel IR does not change; descriptor, response, configuration and
  error reuse the kernel's `HttpRequestDescriptor`, `HttpResponseParts`, `ProviderConfig` and `ErrorEnvelope` (in
  JSON form).
- **D2 A separate WIT package and world (DE2).** Package `token-station:embeddings-adapter@1.0.0`, world
  `embeddings-adapter-v1`. The reason for a separate package is the same as for the task world: a package's version
  is shared by every world in it, so putting this in `token-station:adapter` would let chat-side changes force
  version signals on the embeddings side (manifest.rs:19-25).
- **D3 Vectors do not cross the component boundary.** The component declares, in the prepared value, where the
  vectors sit in the upstream response; the host extracts them per that declaration; the component sees only the
  response skeleton with the vectors erased (§5).
- **D4 Usage**: the component reports the numbers the upstream reported, or declares "the upstream did not report"
  and supplies an **estimated fallback** when building the request; the host settles from this and labels
  estimates truthfully (§7).
- **D5 The reservation upper bound is computed by the host with a provider-agnostic formula** (§7.3); the component
  does not supply a reservation figure.

## 3. Contract types (`south-contracts`, IR-independent)

Sketch (names are a draft):

```rust
pub const EMBEDDINGS_CONTRACT_VERSION: u16 = 1;
pub const MAX_EMBEDDING_INPUTS: usize = 2048;          // initial value taken from OpenAI's published limit
pub const MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES: usize = 8 * 1024;

pub struct EmbeddingsRequestV1 {
    model: String,                    // the upstream model selected by routing
    inputs: Vec<EmbeddingInputV1>,    // 1..=MAX_EMBEDDING_INPUTS
    input_shape: InputShapeV1,        // Single | Array: whether the northbound caller sent a string or an array
    dimensions: Option<u32>,          // > 0
    encoding_format: EncodingV1,      // Float | Base64: the format the northbound caller wants
    user: Option<String>,
}

pub enum EmbeddingInputV1 {
    Text(String),
    TokenIds(Vec<u32>),
    Media { mime_type: String, data_base64: String },  // the host parses data: URIs per RFC 2397
}

pub struct EmbeddingsEstimateV1 {
    fallback_input_tokens: Option<u64>,   // null = cannot be settled if the upstream does not report
}

pub enum VectorLocatorV1 {
    NorthIdentical,                                   // the upstream body is itself an OpenAI embeddings response
    Array { array: JsonPointer, vector: JsonPointer, index: Option<JsonPointer>, encoding: VectorEncodingV1 },
    Single { vector: JsonPointer, encoding: VectorEncodingV1 },
}
pub enum VectorEncodingV1 { Float, Base64F32Le }

pub enum UsageSourceV1 { Reported, Absent }

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
```

On the `conformance` side:

```rust
pub struct PreparedEmbeddingsV1 {
    descriptor: HttpRequestDescriptor,         // kernel IR
    estimate: EmbeddingsEstimateV1,
    vectors: VectorLocatorV1,
    immutable_body_paths: Option<Vec<String>>, // syntax and semantics as in task contract 6 (south-contracts/src/task_v2.rs:49-57)
    parse_context: serde_json::Value,          // handed back unchanged by the host to parse, never interpreted; bounded
}
```

`immutable_body_paths` reuses task contract 6's definition and validation function (`validate_immutable_body_paths`)
directly: `null` = the component takes no position, and the host must not inject extra fields; `[]` = the host may
inject anywhere; non-empty = these dotted paths, their ancestors and their descendants must not be rewritten. It
replaces the host's per-type `OPENAI_/GEMINI_/VERTEX_EMBEDDINGS_OWNED_FIELDS` (server:…/embeddings.rs:56-78). Array
indices are not part of the syntax, so per-item fields in Gemini's batch form remain out of reach — the same
limitation the host has today (same file, :60-61).

## 4. WIT and manifest

```wit
// Embeddings component ABI, version 1. All exports are pure translation; the host owns
// credentials, HTTP, vector extraction, pricing and settlement.
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

    // HttpResponseParts of a non-2xx -> ErrorEnvelope on the closed catalog.
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
"`gemini-embedding-001` accepts only text". If model-level facts need to be known to the host before routing, they
go through the model catalog of the boundary record's §7.5.

**Runtime.** Following task-v2: add one `bindgen!` module, one instance kind and three `call_*` functions
(crates/south-provider-runtime/src/bindings.rs:25-45; component.rs:760-770); the import scan refuses any `host`
namespace for this world (as loader.rs:216-224 does for task-v2).

## 5. Vector payloads: the host extracts per declaration

**Why the component does not parse the whole response.**

- The runtime's per-call boundary payload limit is 16 MiB with a 2-second wall clock
  (crates/south-provider-runtime/src/runtime.rs:30-32). A batch of 2048 inputs × 3072 dimensions easily exceeds
  several tens of MiB as float JSON; the host's Gemini arm has already switched to the 64 MiB binary transport
  because of size (server:…/embeddings.rs:1029-1033; south-contracts/src/lib.rs:121, 131). Passing such a body into
  and out of wasm twice would hit both the size limit and the time limit.
- The host's OpenAI-compatible arm today returns the upstream bytes to the client **unchanged**
  (server:…/embeddings.rs:630-635), including base64 vectors returned by the upstream. If the component
  re-serializes the floats, the values are equal but the text may differ, so dual-run reconciliation could compare
  only values, not bytes.

**Locator declaration** (`VectorLocatorV1`, given by `build-embeddings-request` for each request — the single and
batch shapes may differ):

| Form | Meaning | Used for |
|---|---|---|
| `NorthIdentical` | The upstream body is already a northbound OpenAI embeddings response; the host validates it and returns it unchanged | OpenAI-compatible |
| `Array { array, vector, index, encoding }` | `array` points at the vector array, `vector` is the pointer within an element, `index` is optional (when present, sort by it) | Gemini batch `/embeddings` + `/values`; Vertex `/predictions` + `/embeddings/values` |
| `Single { vector, encoding }` | A single vector | Gemini single `/embedding/values` |

`NorthIdentical` is not provider knowledge: the northbound protocol is the host's product surface (P21 §1.1), and
the host already knows the shape of an OpenAI embeddings response.

**Erasure protocol.** The host parses the 2xx body (bounded), extracts every vector per the locator, replaces each
vector position with `null`, and hands this skeleton together with `parse_context` to `parse-embeddings-response`.
The component sees structure, counts and usage fields, but no floats. The `vector_count` the component counts must
match the number of vectors the host extracted.

**Alternative (not recommended)**: the component parses the whole response and returns IR vectors, and the host
only renders. This is the simplest to implement, but it either limits batch size and dimensions so responses stay
within 16 MiB, or raises this world's payload limit and time limit; and it gives up byte-level pass-through. If the
south maintainers consider the erasure protocol too heavy, this can be chosen instead after measurement
(§14 E-Q1).

## 6. How the three dialects land

| | OpenAI-compatible (including `azure-openai-v1`) | Gemini native | Vertex `:predict` |
|---|---|---|---|
| URL | `base_url` + `/embeddings` (built by the component; the kernel's `ProviderApi` has no embeddings, so `resolve` cannot be used) | `…/v1beta/models/{model}:embedContent` or `:batchEmbedContents` (by `input_shape`) | `…/publishers/google/models/{model}:predict`; project and region come from non-secret configuration or minted exported attributes (boundary record §3.3, §7.3) |
| Auth | `bearer`; Azure uses `header_secret` `api-key` | `header_secret` `x-goog-api-key` | `bearer`, with the slot minted by the service-account recipe |
| Batch | Supported, up to 2048 inputs | Supported (`batchEmbedContents`) | Accepts only one; more than one is a capability error at build time (the same rule as host plan 58 P2) |
| `dimensions` | Forwarded unchanged | Per-item `outputDimensionality` | `parameters.outputDimensionality` |
| `encoding_format` | Can be forwarded; the locator's `encoding` becomes `Base64F32Le` or `Float` accordingly | The upstream returns floats only | The upstream returns floats only |
| Token-id input | Forwarded | Capability error | Capability error |
| Media input | Capability error | Per model: supported models convert it to `inline_data`; text-only models give a capability error | Capability error |
| Vector locator | `NorthIdentical` | `Single` / `Array` | `Array` |
| Usage source | `Reported`: `usage.prompt_tokens` | `Absent` (until DE3 is measured) | `Reported`: the sum of per-item `statistics.token_count`, with `per_input_tokens` given |
| Fallback estimate | None (cannot be settled if not reported) | Yes (§7.2) | None |
| Immutable paths | `["input", "model"]` | `["content", "requests", "model"]` | `["instances"]` |

The immutable paths are taken verbatim from the host's three tables today (server:…/embeddings.rs:62-64), so the
behavior of request extras (additional request configuration) is the same before and after migration.
`nvidia-nim`'s default `input_type` does not go into the component: P24 E0 already expresses it through the
operator's request extras; it is operator data, not dialect.

Vertex responses also carry `metadata.billableCharacterCount` (used by older models priced by character); the host
does not read it today, and v1 of this contract does not model it.

## 7. Usage facts and estimates

### 7.1 Semantics

- `Reported`: the upstream reported the input token count. `input_tokens` is required. When the upstream reports per
  input (Vertex), `per_input_tokens` is given as well.
- `Absent`: the upstream did not report this time. Whether the call can be settled depends on the
  `fallback_input_tokens` given at build time: if present, settle by it and label it as estimated; if not, it
  cannot be settled.
- The same discipline as the provider world (boundary record §6): **a 2xx that cannot be parsed must not be
  reported as zero**. OpenAI-compatible and Vertex must report an error when usage is missing (consistent with the
  host's rule after the P24-E-F1 fix: server:…/embeddings.rs:571-601); substituting `Reported { input_tokens: 0 }`
  is not allowed.

### 7.2 Gemini's fallback estimate

The fallback value is computed by the component when it builds the request, with the formula copied verbatim from
the host's current implementation so that dual runs agree: text by UTF-8 byte count, `(len + 3) / 4`; media by a
constant per MIME top-level type — audio 512, video 1024, everything else 258 (server:…/embeddings.rs:45-53,
942-956). These constants are Gemini's metering approximation and therefore dialect knowledge; once they are in
the component, the host no longer holds them.

If DE3 (whether Gemini native responses carry token counts) measures "yes", the component switches to reporting
`Reported`, and whether the fallback value is kept as a backstop for missing reports or removed is for lv to decide
(§14 E-Q2).

### 7.3 Reservation upper bound (host, provider-agnostic)

The host computes the input-token upper bound as the outbound descriptor body's byte count plus 64 per input: a
token occupies at least one byte, and the 64 is left for special tokens — this is the host Vertex arm's formula
(server:…/embeddings.rs:790-794) generalized to the whole body; the OpenAI-compatible arm today uses the body byte
count (same file, :242-244). The component supplies no reservation figure: a reservation only freezes funds, and
computing it with a generic formula means the component does not decide whether the user is let through. For the
differences from the native arms, see "Intentional differences" in §11.

### 7.4 The host's generic checks

- **Out-of-bound checks**: `Reported.input_tokens ≤ reservation upper bound` and
  `fallback_input_tokens ≤ reservation upper bound`; otherwise the call goes to manual review (reusing the existing
  settlement guard "amount ≤ reservation").
- **Internal-consistency checks**: if `per_input_tokens` is given, its length equals the input count and its sum
  equals `input_tokens`; `vector_count` equals the number of vectors the host extracted, which equals the input
  count; every vector is non-empty and all vectors in one request have the same length; when the request carries
  `dimensions` and the component declares the `dimensions` capability, the vector length equals it; if `index` is
  declared, its values are exactly `0..n`, each once.
- **Undetectable zone** (the same formulation as the boundary record's §6.3): under-reporting, and deviation that
  falls within the upper bound, cannot be detected by the host. Trust comes from pinned package digests, gate ②
  usage samples and documentation-derived judges, and dual runs before cutover.

### 7.5 Settlement and labeling

| Component gives | Host settles | `tokens_estimated` | `quantity_estimated` | Northbound `usage.prompt_tokens` |
|---|---|---|---|---|
| `Reported(n)` | n | 0 | 0 | n (for `NorthIdentical`, the upstream's original value) |
| `Absent` + fallback f | f | 1 | 1 | f |
| `Absent`, no fallback | Not settled; 502, `delivery_unknown` | — | — | — |
| Parse failure / consistency check failure | Same as above | — | — | — |

Both labeling columns reuse the host's existing semantics: `tokens_estimated` means the token count was computed by
the gateway, and `quantity_estimated` means the billed quantity (and therefore the amount) is an estimate (the
`tokens_estimated` / `quantity_estimated` paragraphs of server:gateway/CLAUDE.md;
server:crates/gateway-provider-protocol/src/usage_types.rs:131-147). Gemini today is `EstimatedTokens`, with both
columns at 1 (the P24-E-F2 fix); this is unchanged before and after migration. The northbound `usage` always carries
the number used for settlement (the P24-E-F3 fix is covered naturally by generic rendering).

## 8. `dimensions`, `encoding_format` and batching

- **`dimensions`**: the IR carries the requested value; the component places it per dialect (§6). When the model
  does not support it, the component returns a capability error; the host no longer judges by model.
- **`encoding_format`**: the IR carries the format the northbound caller wants. The host converts as needed when
  rendering northbound: float → base64 (little-endian f32) and base64 → float, both lossless for f32. For
  `NorthIdentical` where the upstream already returned the requested format, the body is returned unchanged;
  otherwise the host converts, then renders.
- **Batching**: `input_shape` preserves whether the northbound input was a string or an array; the northbound
  `data` is always an array. Input-count limits: 2048 at the contract level; at the dialect level the component
  refuses at build time (1 for Vertex). The host does no fan-out — one admission maps to one send, and the funds
  state machine is unchanged (the rule of host plan 58 P2).

## 9. What the host's generic executor is expected to do

1. Select the package by model-row routing (pinned by digest), and fetch the non-secret configuration and
   credential slots; when a slot is `minted`, first mint per the recipe and obtain the exported attributes
   (boundary record §3); a failure is a pre-admission error.
2. The host parses the northbound request into `EmbeddingsRequestV1` (data: URIs are parsed per RFC 2397 into
   `Media`).
3. Call `build-embeddings-request`; capability error → 400, zero upstream calls.
4. `ProviderConfig::authorize` (kernel:provider.rs:361) and descriptor auth admission (boundary record §4.2);
   inject operator extra fields outside the immutable paths; seal the outbound descriptor.
5. Compute the reservation upper bound per §7.3, admit, and write the dispatch marker.
6. Send through south's buffered binary call (body limit 64 MiB, south-contracts/src/lib.rs:131).
7. Non-2xx → `map-provider-error` → northbound error; funds are handled by the host's existing rules for failures
   after sending.
8. 2xx → extract vectors per the locator and erase them → `parse-embeddings-response` → the §7.4 checks.
9. Settle and label per §7.5.
10. Render the OpenAI embeddings response per the northbound `encoding_format`; for `NorthIdentical` with no
    conversion needed, return the upstream bytes unchanged.

What no longer exists in the host: per-type arms, the `nvidia-nim` row-name check, the media constants, the
`gemini-embedding-001` check, the per-type immutable tables, and the leaf crate's Gemini embeddings translation.

## 10. Conformance (`south.embeddings-component.v1`)

**Fixture families**: `embeddings.request`, `embeddings.response`, `embeddings.error`.

**Rows required by name** (missing any one fails `Coverage`):

| Row | Assertion |
|---|---|
| `request.single-text` / `request.batch-text` | Descriptor and prepared value (including locator, fallback estimate and immutable paths) match exactly |
| `request.dimensions` | The dimensions land in the dialect's location |
| `request.refused-capability` | Input the dialect does not support (token ids, media or too many inputs) → capability error |
| `response.usage` | Usage sample: the `Reported` numbers, and `per_input_tokens` (when the dialect reports per input) |
| `response.missing-usage` | Dialect without a fallback → error; dialect with a fallback → `Absent` |
| `response.count-mismatch` | The skeleton's count differs from the request's input count → error |
| `error.rejected-credential` | 401 / 403 are not retryable (reusing the provider world's check) |

**Checks**: `FixtureMatch`, `Determinism`, `UnknownFieldTolerance`, `EndpointConfinement`,
`AuthErrorsAreNotRetriable` (reusing the existing items of south-component-conformance/src/report.rs:19-64), plus:

- `DescriptorAuthWithinManifest` (boundary record §4.4);
- `UsageNeverDefaulted`: response fixtures carry `usage_pointer`; the suite deletes that location and requires the
  component to report an error or `Absent`, never a `Reported` zero;
- `LocatorResolves`: the prepared locator, applied to the paired response fixture, resolves exactly as many vectors
  as the request has inputs;
- determinism of the fallback estimate (the same request yields the same value twice).

**Documentation-derived judges**: as in the provider world (boundary record §6.2 item 5), each of the three
embeddings packages south publishes has a usage judge whose expectations are derived from provider documentation,
written into the release discipline; Gemini's judge is added after DE3 is measured.

## 11. Migration and dual runs

| Step | Side | Content | Acceptance |
|---|---|---|---|
| E0 | Host | Done (2026-09-30): the P24-E-F1, E-F2 and E-F3 fixes; the nvidia default is now expressed through request extras | — |
| E1 | South | This record's contract, WIT, runtime world and suite; three reference implementations and packages (suggested: `embeddings-openai-compatible` carrying the two families `openai-compatible` and `azure-openai-v1`, `embeddings-gemini`, `embeddings-vertex`) | The three packages pass the suite; a south minor release, listed in the release index |
| E2 | Host | §9's generic executor; model rows routed to components; dual-run reconciliation | See below |
| E3 | Host | Delete the three native arms and the embeddings part of the leaf crate | The J1 count falls; installing or removing an embeddings package needs zero host changes |

**Dual-run reconciliation** (the same request goes through the native arm and the component arm separately, once
under each billing form), compared item by item:

1. Upstream request: method, URL, non-auth headers, body (JSON equality).
2. Northbound response: bytes for `NorthIdentical`; otherwise vector values, order, count and `usage`.
3. Reservation, settled amount, `tokens_estimated`, `quantity_estimated`.
4. Handling of the failure fixtures: 2xx with missing usage (OpenAI-compatible, Vertex), missing vectors, count
   mismatch, empty vectors, 401, 429, 5xx.

**Intentional differences** (on record, not counted as reconciliation failures):

- When a Gemini batch response lacks `embeddings` or has an empty vector, the native arm fills in an empty array and
  returns 200 as usual (server:crates/gateway-provider-protocol/src/translate_gemini.rs:131, 142); the component arm
  treats it as a protocol error per §7.4 and goes to `delivery_unknown`.
- Non-string input sent to Gemini (token-id arrays): the native arm translates it into `{"text": ""}` and sends it,
  recording an estimate of 0 tokens (`embedding_input_to_gemini_part` in translate_gemini.rs; the `unwrap_or("")` at
  server:…/embeddings.rs:942-956), which amounts to answering with an empty-text vector billed at 0; the component
  arm gives a capability error at build time — 400, zero upstream calls. This is an existing host defect; following
  the practice of P21 §9, fix it in the native arm first, then dual-run.
- Reservation: the generic upper bound of §7.3 is slightly higher than native Gemini's (upper bound = the estimate)
  and Vertex's (input text bytes + 64); it affects only the frozen funds and admission when the balance is
  insufficient, not the settled amount.

**Dependency**: the Vertex package depends on the credential recipes of the boundary record's §3 (service account →
bearer, with `project_id` exported); until recipes land, the host can use its existing service-account minting as a
stopgap, but that is a J1 red item and must be cleared before E3.

## 12. Versioning

- New world: the tuple's WIT package, world name and suite name are new by construction
  (2026-08-27-manifest-schema-beyond-one-world.md:157-159), so the existing 13 packages are unaffected.
- `compatibility.json`: `contracts` gains `embeddings: 1`, and `conformance` gains the suite id;
  `host_capabilities` records adoption per host.
- Host link layer: to install packages for this world, the host must link a south version that knows it — a
  one-time upgrade for a new northbound surface (P21 §1.4); after that, adding or removing embeddings providers
  touches only the package layer.
- When this lands together with the compatibility range of the boundary record's §8, embeddings packages declare
  `contracts: {"embeddings": 1}`.

## 13. Rejected alternatives

- **Adding embeddings functions to the provider world and raising it to v3** (rejected by DE2): it would ripple
  through the existing four chat packages, while embeddings and chat share no request or response shape.
- **Defining embeddings types in the kernel IR** (rejected by DE1): it would have to travel the whole "community
  protocol → kernel mirror → south" chain, while the chat IR does not need embeddings.
- **The component reporting a reservation figure**: a reservation is a funds decision that freezes funds; computed
  with a generic formula, it leaves the component no say over admission (§7.3).
- **The host keeping Gemini's estimate formula while the component reports only `Absent`**: the media constants are
  Gemini's metering approximation; leaving them in the host is provider knowledge, and J1 would not reach zero.
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
- **E-Q3 (L)** Is it acceptable for two conventions to coexist — the estimate given by the component at build time
  (this record's approach) and, per the boundary record's Q12, a generic host estimate on the chat side?
- **E-Q4 (S)** Package granularity: three packages (recommended), or one package with three families.
- **E-Q5 (S, community host)** ARCHITECTURE.md:114-115 requires a metering vocabulary to have "a second consumer in
  sight": does the community host have, or will it have, an embeddings surface? If not, this contract's usage
  vocabulary needs an explicit exemption from the maintainers.
- **E-Q6 (L)** `per_input_tokens` has no host consumer today (the host uses only the sum). Keep it optional
  (recommended; zero cost, and it gives Vertex's consistency check something to hold on to), or leave it unmodeled
  in v1?
- **E-Q7 (S)** `NorthIdentical` makes the host return the upstream bytes unchanged, which also passes the upstream's
  `model` field through unchanged (as the host does today); whether the host should instead uniformly rewrite it to
  the northbound model name must be settled consistently with the host's northbound conventions.
