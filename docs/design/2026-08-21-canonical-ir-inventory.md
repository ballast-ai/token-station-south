# Canonical IR Inventory and Contract Freeze (S0)

Status: D1–D5 ruled 2026-08-21 — D1 (evidence extraction into the component,
funds discipline stays host; both field-level freezes included) and D2 (chunk
entry point takes raw bounded bytes) ruled explicitly, both as recommended;
D3–D5 follow as recommended (D3 is D1's rejected alternative; D4/D5 are
mechanical discipline). S0 of the target-architecture transformation plan (doc
repo, `2026-08-20-target-architecture-transformation-plan.md`). One document,
no code; overrunning the timebox promotes the inventoried minimum instead of
waiting for completeness.

Sources audited, at fixed revisions:

- IR authority: `token-station` `crates/protocol` at **v1.2.3**, distributed as
  **kernel v0.2.0** (`token-station-kernel`, byte-identical mirror; `canonical_ir: 1`
  anchor). Verified byte-identical to the community integration branch at audit time.
- Component ABI donor: `token-station` `crates/plugin-api/wit/adapter.wit`
  (`token-station:adapter@1.0.0`, worlds `agent-adapter-v1` / `provider-adapter-v1`).
- Enterprise consumer: `token-station-server` `crates/gateway-provider-protocol`
  (the 9,239-line translation leaf) at dev-v2, pinned to kernel v0.1.0.

## 1. IR ownership: already one authority — freeze it, don't create it

The audit's first finding is a non-event, and that is the point: **there is no
third IR**. The enterprise server's `/v1/messages` translation family already
normalizes into `token_station_protocol::{ChatRequest, ChatResponse, StreamEvent…}`
via the kernel mirror; the community host's plugin boundary already names the same
types in `adapter.wit`'s doc comments. Invariant 6 of the plan is therefore an
existing fact to freeze, not a migration to run:

- **Definition authority**: `token-station` `crates/protocol` (types, `Extensions`
  compatibility rule, parse→serialize fixtures).
- **Distribution channel**: the kernel mirror, at tag-synced revisions
  (S0 baseline = **kernel v0.2.0 = token-station v1.2.3**). It never defines.
- **South production runtime**: never depends on either Rust crate. The WIT/ABI
  carries bounded JSON plus a `schema_id`; only conformance gate ② (component
  behavior) takes typed decode through a fixed kernel revision.

What the server does NOT route through the IR today, and why it matters for S5:
only `translate_ir.rs` / `translate_ir_sse.rs` (the `/v1/messages`-to-OpenAI
family) speak IR; the anthropic-direct, responses, gemini, bedrock and kiro
translators are wire↔wire. Those families meet the IR for the first time when
their component lands, so their S5 batches inherit an extra obligation: the
shadow comparison must pin today's wire↔wire output, not an imagined IR path.

## 2. The component-boundary IR subset, per type

The southbound world consumes exactly these types (everything reachable from the
`provider-adapter` interface). Fields are listed exhaustively; a field not listed
does not cross the boundary. `Extensions` = `BTreeMap<String, serde_json::Value>`
(ordered, unknown-key-preserving) and appears wherever noted.

**Inbound to the component** (host → component):

| Type | Fields (all serde-default-tolerant) | Open-JSON positions |
|---|---|---|
| `ChatRequest` | `model: String`, `messages: Vec<Message>`, `tools: Vec<ToolDef>`, `response_format: Option<ResponseFormat>`, `tool_choice: Option<ToolChoice>`, `sampling: Sampling`, `stream: bool`, `extensions` | `ToolDef.parameters: Value`; `ResponseFormat::JsonSchema{json_schema: Value}`; `ToolChoice::Other(Value)`; `extensions` |
| `Message` | `role: Role{system,user,assistant,tool}`, `content: Option<Content{Text(String) \| Parts(Vec<ContentPart>)}>`, `tool_calls: Vec<ToolCall>`, `tool_call_id: Option<String>`, `name: Option<String>`, `extensions` | `ContentPart::Unknown(Value)` (verbatim survival); `extensions` |
| `ContentPart` | `Text{text}`, `ImageUrl{image_url:{url, detail?}}`, `Thinking{thinking, signature?}`, `RedactedThinking{data}`, `Unknown(Value)` | `Unknown` |
| `ToolCall` | `id`, `name`, `arguments: String` (exact model bytes; never parsed at this layer) | — |
| `Sampling` | `temperature?`, `top_p?`, `max_output_tokens?`, `stop: Vec<String>` | — |
| `ProviderConfig` (policy-fenced subset, §6) | `provider: String`, `base_url: ProviderEndpoint`, `auth: Option<SecretRef>`, `models: Vec<ModelCapability>`, `extensions` | `extensions` |
| `StreamChunk` | `data` — see D2: the v1 `String` cannot carry eventstream bytes | — |
| `HttpResponseParts` | `status: u16`, `headers: BTreeMap<String,String>`, `body: String`, `extensions` | `extensions` |

**Outbound from the component** (component → host):

| Type | Fields | Open-JSON positions |
|---|---|---|
| `HttpRequestDescriptor` | `method: HttpMethod`, `url: String`, `headers: SafeHeaders` (credential names refused on construction AND deserialization), `body: Option<Value>`, `auth: Option<Auth{Bearer\|Header\|OAuth}>`, `extensions` | `body`; `extensions` |
| `ChatResponse` | `id`, `model`, `choices: Vec<Choice{index, message, finish_reason: Option<FinishReason incl. Other(String)>, stop_sequence?}>`, `usage: Usage`, `extensions` | `extensions` |
| `Usage` | `input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `cache_write_5m_tokens`, `cache_write_1h_tokens`, `reasoning_tokens` (all `u64`, zero-default) | — |
| `StreamEvent` (internally tagged, closed) | `Delta{index, content}`, `ToolCallDelta{index, id?, name?, arguments_delta}`, `Usage{usage}`, `ThinkingDelta`, `ThinkingSignatureDelta`, `Finish{finish_reason?, stop_sequence?}`, `Done{finish_reason?, stop_sequence?}`, `Error{error}` | — |
| `ErrorEnvelope` | `code: ErrorCode` (closed 13-variant catalog with `is_retriable_elsewhere` kept beside it), `http_status: u16`, `message`, `provider_message?`, `retry_after_ms?`, `extensions` | `extensions` |
| `ModelCapability` | `model`, `tool/vision/json_schema: bool`, `*_state: Option<CapabilityState>`, `context_window`, `max_output_tokens`, `supported_parameters`, `extensions` | `extensions` |

The open-JSON positions are the standing reason WIT holds an envelope rather
than records (the `adapter.wit` header ruling, reaffirmed): every one of them is
an open value that WIT cannot express and that mirroring would fork.

**Northbound is out of scope.** `AgentRequestEnvelope`, `HeaderDigest`,
`Principal`, `AgentHint` serve the community host's agent half, which survives
unchanged; the server's client-facing surfaces are host product code (§5).

## 3. The ABI envelope

Every boundary call carries `json = string` payloads named by canonical type
(the `adapter.wit` convention), with one addition and one exception:

- **Addition — `schema_id`**: the manifest and every runtime handshake carry a
  `schema_id` naming the IR revision the component was built against (format:
  `token-station-protocol@<crate-version>/<kernel-tag>`, e.g.
  `token-station-protocol@0.3.0/v0.2.0`). Payloads themselves stay unstamped —
  per-call stamping would tax every chunk; admission is where mismatch dies
  (§4).
- **Exception — stream chunks are bytes** (D2): the chunk entry point takes raw
  bounded bytes, not JSON. Everything else remains JSON.

## 4. The compatibility tuple

Frozen as the admission handshake. All seven live in the component manifest;
the runtime refuses any mismatch at load time — refusal, never silent
degradation, and never a partial acceptance:

| # | Field | S0 baseline | Owner of the value |
|---|---|---|---|
| 1 | IR schema id (`schema_id` above) | `token-station-protocol@<v>/v0.2.0` | community `crates/protocol` |
| 2 | kernel distribution version + revision | `0.2.0` @ its `source_commit` | kernel manifest |
| 3 | WIT package version | `token-station:adapter@x.y.z` (S1 sets it; donor is `@1.0.0`) | south `provider-api` |
| 4 | WIT world name (= manifest `api_version`) | `provider-adapter-v1` lineage; S1 names the south world | south `provider-api` |
| 5 | south runtime version | south workspace semver (0.8.0+) | south |
| 6 | conformance suite id + version | gate ② suite id, versioned | south conformance |
| 7 | component version | component's own semver | component |

## 5. Request/response state machines

Owner column is the load-bearing one: `component` (sandboxed translation),
`south` (orchestration/transport contracts), `host` (policy, credentials,
funds). An arrow whose error column names an `ErrorEnvelope` code is a
component-reported failure; south/host arrows fail with south's frozen codes.

### 5.1 Buffered path

| # | Arrow | Input | Output | Errors | Owner |
|---|---|---|---|---|---|
| B1 | Admit component | manifest + compatibility tuple | admitted instance | tuple mismatch → load refusal (gate ①/④) | south runtime |
| B2 | Fence config | host provider record | `ProviderConfig` (fenced subset, §6) | — (construction is host code) | host |
| B3 | `build-http-request` | `ChatRequest` + `ProviderConfig` (JSON) | `HttpRequestDescriptor` (JSON) | `ErrorEnvelope` (e.g. `capability`, `invalid_request`) | component |
| B4 | Authorize descriptor | descriptor + config | authorized descriptor | `DescriptorError` (URL outside endpoint / credential slot mismatch) → host error, request never leaves | host (check defined in protocol) |
| B5 | Resolve + prepare | descriptor auth (slot name), host credential store | prepared request (credential injected; minting front-loaded for OAuth-shaped auth) | credential resolution failure; host bound violations | host + south (`PreparedSecretResolverV1`, prelude) |
| B6 | Transport execute | prepared request, deadline, cancellation | `BufferedHttpResponseV1` → projected `HttpResponseParts` | south frozen codes (`CANCELLED`, `DEADLINE_EXCEEDED`, 9 transport codes) | south |
| B7 | `parse-response` | `HttpResponseParts` (JSON) | `ChatResponse` (JSON, incl. exact `Usage`) | `ErrorEnvelope` (`provider_protocol_error` for 2xx garbage) | component |
| B8 | `map-provider-error` | non-2xx `HttpResponseParts` | `ErrorEnvelope` on the closed catalog | (total function; unmappable → `internal`) | component |
| B9 | Settle | `ChatResponse.usage` / `ErrorEnvelope` | billing/quota/receipt facts | evidence rejection → manual review, never invented usage | host |

### 5.2 Streaming path

| # | Arrow | Input | Output | Errors | Owner |
|---|---|---|---|---|---|
| S1–S5 | = B1–B5 with the streaming transport | | | | as above |
| S6 | Transport open | prepared request | headers-ready stream; non-2xx collapses to bounded `Rejected` | south frozen codes; `Rejected` carries head+body to host classification (B8's mapping applies to the rejected body) | south |
| S7 | Pull chunk | live stream | bounded chunk **bytes** | `STREAM_*` codes (read/idle/deadline/cancel); terminal per south contract | south |
| S8 | `parse-stream-chunk` | chunk bytes (D2) | `list<StreamEvent>` (JSON; zero or more — fragments must not be buffered whole) | `ErrorEnvelope` → host stops the exchange | component |
| S9 | Event fold | `StreamEvent`s | client rendering (host surface) + usage evidence accumulation (D3) | `StreamEvent::Error` → stop; evidence violations → settle refusal | host |
| S10 | Terminal classification | last event + transport terminal | `StreamOutcome{Complete\|FailedAfterPartial\|FailedBeforeOutput\|ClientCancelled}` | only `Complete` may settle as success | host |
| S11 | Settle | folded `Usage` + outcome | funds facts | as B9 | host |

Cancellation and deadline pre-empt every arrow from B5/S5 onward (south's
biased-select contract); a cancellation observed before B6/S6 is a zero-fact
fallback, after it a delivery-unknown — that split is host funds policy and
stays out of components.

## 6. `ProviderConfig` policy fence

The component sees the four-field subset in §2 and nothing else. Explicitly
fenced host-side (the enterprise config record is the concrete threat model:
it carries all of these today): pricing and rate cards, markup, billing form,
quotas and windows, retry/fallback budgets, timeouts, proxy/egress policy,
routing weights and channels, `upstream_requires_responses`-style surface
flags, modality lists, credential values and refresh state, scope/kill
switches. Rationale: each is either funds policy, routing policy, or a
credential — the three things invariant 4 keeps sovereign. A component that
needs one of these has mis-located a decision; the answer is a new declared
field on the IR (plan-visible), never a smuggled `extensions` key (§8, D5).

## 7. Auth mapping across the estate

| IR `Auth` (descriptor) | south arm | Who does the work |
|---|---|---|
| `Bearer{secret}` | `Bearer` | host resolves slot → south assembles `Authorization: Bearer` |
| `Header{name, secret}` (name ∈ credential-header catalog) | `HeaderSecret(SecretHeaderV1)` (name ∈ frozen sanctioned enum) | as above, verbatim header |
| `OAuth{secret, scopes}` | `Bearer` after **host-side minting**, front-loaded before the funds marker (prelude `PreparedSecretResolverV1` pattern) | host mints; component never sees the exchange |
| — (inexpressible in v1: SigV4) | `HostSigned` + `RequestFinalizer` (its own south slice, plan decision 8) | host signs the finalized bytes; component declares `emits` |

Two catalogs must not drift: protocol's `CREDENTIAL_HEADERS` (redaction/refusal
list) and south's `SecretHeaderV1` + `RESERVED_HEADERS` (sanctioned wire arms).
Today sanctioned ⊂ credential-catalog holds. Gate ② carries a fixture asserting
the inclusion at the pinned revisions, so a new sanctioned header cannot land
without the redaction side knowing it (D4).

The `host.sign` WIT import (HMAC over adapter-chosen bytes, never into
`authorization`) is orthogonal to the Finalizer and survives for body/plain-
header signatures; components must not use it to imitate SigV4.

## 8. Difficulty rulings

### D1 — usage evidence: extraction is component translation; discipline is host funds policy

The server's exact-evidence layer (`usage_evidence.rs`, 1,913 lines) is
per-dialect state machinery: field-complete, terminal-explicit, arithmetically
self-consistent, duplicate-rejecting, order-checking — five wires, five state
machines. The IR's `Usage::absorb` (last-nonzero-wins) is deliberately weaker.
Ruling (recommended): **per-dialect evidence extraction moves into the
component; the funds discipline stays host.** Concretely: a component must emit
`StreamEvent::Usage` exactly as often as the provider reports (twice for
Anthropic), each report already normalized to `Usage` fields; gate ② gains
adversarial per-dialect fixtures (duplicate, out-of-order, zero-erasure,
missing-terminal) so "exact" is machine-judged; the host folds with `absorb`
and applies its own settle refusal (`StreamOutcome` ≠ `Complete` never
settles as success). The server's state machines retire per-dialect as each
S5 batch lands, not before.

Two field-level deltas found by the audit, resolved as follows:

- **Cache conventions.** Server `TokenUsage` distinguishes OpenAI's
  subset-cached (`cached_input_tokens ⊂ input_tokens`) from Anthropic's
  disjoint buckets; IR `Usage` has one `cache_read_tokens` with no stated
  convention. Freeze the IR convention as **provider-native and named by the
  dialect**: for OpenAI-shape dialects `cache_read_tokens` is the subset count
  (input_tokens unchanged, total), for Anthropic-shape dialects the disjoint
  count (input_tokens = uncached). The component's dialect is in the manifest,
  so the host's pricing knows which convention it is folding — this matches
  what both hosts already do and avoids a lossy renormalization at the
  boundary. The convention table becomes part of gate ② fixtures.
- **The thinking marker.** Server billing selects mode-dependent output rates
  from `reasoning_tokens > 0` **or** the bare presence of reasoning content
  with no token count. `Usage.reasoning_tokens` alone loses the second signal.
  Components must emit `ThinkingDelta` events when reasoning content streams
  (they already must, for rendering); the host derives the marker from
  `reasoning_tokens > 0 ∨ thinking-deltas seen`. No IR change; the derivation
  rule is frozen here and pinned by a gate ② fixture.

### D2 — stream chunks cross the boundary as bytes, not JSON strings

`StreamChunk.data: String` cannot carry Bedrock eventstream frames (binary,
non-UTF-8), and base64-in-JSON taxes every SSE chunk ~33% for the sake of the
one binary dialect. Ruling (recommended): the south component ABI's chunk entry
point takes **raw bounded bytes** (`list<u8>` at the WIT level; south's
existing `StreamChunkV1` bound applies) and returns the usual JSON
`list<StreamEvent>`. The community `StreamChunk` JSON type remains for the old
plugin ABI during its S4 transition window and retires with it. This is the
S1-visible consequence of the plan's capability-matrix row "eventstream decode
goes in the component".

### D3 — evidence provenance is not added to `StreamEvent`

Considered and rejected: stamping usage events with wire provenance (which
upstream frame produced them) so the host could re-run server-style order
checks. That would leak dialect back across the boundary the component exists
to seal. The order/duplication checks belong with the dialect knowledge —
inside the component, enforced by gate ② (D1). `StreamEvent` stays as-is.

### D4 — catalog-inclusion fixture

The sanctioned-header ⊆ credential-header check of §7, at pinned revisions, as
a standing gate ② fixture. Cheap, and it turns a silent catalog drift into a
red gate.

### D5 — `extensions` at the component boundary are data, not contract

Components must round-trip `extensions` verbatim (the IR already promises
this) and must not *behave* on an extensions key: a key that changes component
behavior is an undeclared contract and fails review. Promotion path: extensions
key → typed IR field (community protocol release) → kernel sync → schema_id
bump. Gate ② includes an unknown-field round-trip fixture per payload type.

## 9. provider-api promotion criteria (the S1 gate)

`south-provider-api` leaves placeholder status when all of:

1. WIT world for the south component boundary compiles, named per tuple #4,
   embedding the §3 envelope (JSON payloads + bytes chunk entry point).
2. Manifest schema validates, carrying the full seven-field tuple of §4 and
   the declared dialect + auth arm(s) + `emits` (for `HostSigned`, with its slice).
3. The reference component (`provider-openai-compatible` donor) signs the
   contract: builds against the WIT, passes manifest admission (gate ①).
4. south's `compatibility.json` `provider_api.wit_version` goes non-null, and
   the boundary-purity gates extend to the new crate (no `token-station-*`
   dependencies in the production runtime path — plan risk table's S3 gate,
   armed early).

Auth arms stay declarative slot references; `HostSigned`/Finalizer and any
companion-header arm remain their own slices with their own records.

## 10. What S0 deliberately leaves open

- Per-dialect chunk grammars (SSE line discipline, eventstream framing) are
  gate ② fixture content, authored per S5 batch — not frozen here.
- The performance benchmark threshold (plan decision 7) is due before the
  first S5 batch, not at S0.
- Northbound (agent) ABI evolution: untouched by this plan.
