# North codec: closing the remaining northbound render gaps

Status: proposed — drafted for review by the host team (token-station-server P21), not accepted

Date: 2026-09-30

Predecessors: `2026-09-28-responses-north-codec.md` (the Responses mapping; "R2: bounded Claude reasoning replay
carrier"; its line 26 leaves the server's Native upstream error frames in the host shell),
`2026-09-28-responses-north-codec-validation.md` (the fixture and façade discipline extended here),
`2026-09-30-host-zero-vendor-boundary.md` (§6 usage, §11 follow-on components, §13 phasing), `ARCHITECTURE.md`
("Host-owned concerns"). Sibling, drafted in parallel: `2026-09-30-openai-responses-upstream-component.md` — it owns
the southbound half of §5 and produces the input of §6; this record refers to it by name and does not design it.

Origin: token-station-server plan P21 (`docs/product-review-v2/plans/2026-09-29-P21-*.md`) — §3.2 (the
"north codec" row), Appendix B.2 (the paragraph after the table) and Appendix B.4 ("no design and no plan"); P15
in the same directory (`2026-09-28-P15-Responses*.md`) with annexes A1–A3.

Baseline: south `origin/main` = v0.42.0 (`3135e36`); host `4d5bb4e5` (the docs-only commit `c4bd45e5` on top of it
was read for P21 §1.3 and §8.5). `codec:` abbreviates `crates/south-north-codec/src/`; `server:…/` abbreviates
`server:gateway/src/modules/inference/engine/text_admission/`; `leaf:` abbreviates
`server:crates/gateway-provider-protocol/src/`. Host plans are cited by section, not by line.

## 1. Problem

`south-north-codec` exists so that two hosts render the same client-visible bytes from the same IR (codec:lib.rs:3-7).
The host's audit lists five places where token-station-server still renders, classifies or re-reads northbound
wire itself (P21 Appendix B.2). This record says where each one goes.

**These are northbound concerns, and the host's zero-vendor rule does not require moving them.** DP0 forbids
provider logic in the host; northbound protocols are the host's product surface and may stay in the host or sink
into `south-north-codec` as P15 did (P21 §1.1; B.4 repeats it for these five items). The reason to move them is
the other goal: one renderer, so client-visible behaviour does not drift between the closed host and the community
host. None of the five unlocks a provider or gates a boundary phase. This record is therefore lower priority than
every phase of the boundary record (B1–B7, its §13) and must not block any of them.

| | Gap | Host today | Codec today |
|---|---|---|---|
| G1 | Chat chunk identity | Stamps `id` / `model` / `created` on every rendered chunk (server:…/sender.rs:1648-1683) | Chunks carry `object`, `choices`, `usage` only (codec:sse.rs:111-158) |
| G2 | Replay carrier classification | `ReplayRequirement::classify(body)` (server:…/reasoning_replay.rs:69-110) | Encodes, decodes and materialises the carrier (codec:responses/replay.rs:87-180; request.rs:447-518, 622-632); does not classify |
| G3 | Failure events on the Responses surface | Three hand-written paths (server:…/sender/responses.rs:2026-2130) | `StreamEvent::Error` → `response.failed` (codec:responses/stream.rs:634-638) |
| G4 | Single-burst Chat SSE | `render_single_burst_chat_sse` (server:…/sender.rs:1943-1993) | Streaming and non-streaming renderers; no burst |
| G5 | Counting forwarded text | `forwarded_delta_chars` (server:…/sender/checkpoint.rs:110-200) | Nothing |

Reading the code changed the list in three ways:

- **G1's second starting point is dead code.** `leaf:translate_responses_sse.rs` (250 lines; `chunk_id` state
  :20-32; `created` read from the clock on every chunk, :78) has no production caller at this baseline: a
  repository-wide search finds only its golden tests (`tests/golden/translate_responses_sse.rs` in the same
  crate). P21 Appendix B.2 already lists it under D1. It is a deletion, not a migration.
- **G3 is mostly adoption.** The codec already renders a failure terminal and already has the "only an explicit
  Error may follow a mapping failure" state (codec:responses/stream.rs:539-557, 570-574). The host does not route
  its own failures through either (§5).
- **Byte equality is not a property of the codec alone.** The codec returns `serde_json::Value`. The host builds
  with `serde_json/preserve_order` (server:gateway/Cargo.toml:52), so its objects serialize in insertion order;
  south's workspace does not enable it (Cargo.toml:19), so the codec's own tests see sorted keys. South's docs do
  not record what the community host does. §8 defines the equality rule accordingly.

## 2. Decisions

- **D1 Scope.** Additive codec functions only; no IR change, no component tuple change, no new world.
- **D2 (G1) Identity is a host-supplied context.** A new wrapper, `OpenAiChatStream`, renders complete chunks from
  `ChatStreamContext { id, model, created }`; `openai_chat_frames` and `OpenAiChatSseState` are untouched.
- **D3 (G2) Classification moves; the leg filter does not.** `classify_reasoning_replay` reads the same item list
  the request parser reads. Which provider leg may receive which class stays in the host.
- **D4 (G3) No new failure shape.** Where a render state exists the host feeds it `StreamEvent::Error`; a relayed
  stream gets a stateless form. Recognising an upstream failure frame is southbound (sibling record).
- **D5 (G4) A burst is the streaming renderer applied to an expanded response**, not a second renderer.
- **D6 (G5) The codec says where forwarded text lives; the host estimates and settles.** Specified here;
  implemented only once a second consumer is in sight (ARCHITECTURE.md:114-115).
- **D7 Equality rule.** SSE framing is compared byte for byte; each JSON payload is compared as a JSON value (§8).

## 3. G1 — Chat chunk identity

**Today.**

- The codec's state doc says identity is absent on purpose: "this wire's chunks carry neither" the routed model
  nor a message id (codec:sse.rs:32-35). An `Error` event renders nothing on this wire and ends the stream (:105).
  The non-streaming renderer does take host facts — `ResponseContext { created, fallback_id }`
  (codec:response.rs:18-28) — and applies the `chatcmpl-` prefix without stacking it (:117-120).
- The host adds the three fields after rendering, on the ground that OpenAI clients read them on every chunk
  (server:…/sender.rs:1648-1652). `ChatChunkStamp::new` mints `chatcmpl-<uuid>`, reads the clock once per stream
  and takes the model from its caller (:1660-1667); `apply` inserts each key only when absent (:1669-1682). It is
  applied at six sites: Bedrock Invoke (:1603-1608), Bedrock Converse (:1638-1643), three inline in the Chat stream
  loop (:4415, :4510, :4578) and `render_ir_chat_frames` (:4701), which serves Gemini (:4445) and Kiro (:4746).
  Every construction site passes the upstream model id (:1558 and :1569 via :2450; :4251; :4839).
- Four of those sites decide which chunk is terminal by testing `choices[0].finish_reason` for non-null (:4416,
  :4511, :4579, :4702-4706), hold that chunk until settlement, then replace its `usage` wholesale with
  `exact_chat_usage_json` (:4641-4646; :2113-2129): three keys plus `prompt_tokens_details.cached_tokens`.
- Community host: south's docs do not describe its Chat stream chunks. Not established (N-Q8).

**Proposal.**

```rust
pub struct ChatStreamContext { pub id: String, pub model: String, pub created: i64 }
pub enum OpenAiChatFrameKind { Delta, Usage, Terminal }   // Terminal = carries a non-null finish_reason
pub struct OpenAiChatFrame { pub kind: OpenAiChatFrameKind, pub data: Value }
pub struct OpenAiChatStream { /* ChatStreamContext + OpenAiChatSseState */ }
impl OpenAiChatStream {
    pub fn new(context: ChatStreamContext) -> Self;
    pub fn frames(&mut self, events: &[StreamEvent]) -> Vec<OpenAiChatFrame>;
}
pub fn openai_chat_usage(usage: &Usage) -> Value;         // the one rendering of this wire's `usage` object
```

- Every frame carries `id`, `object`, `created`, `model`, then `choices` (and `usage`), inserted in that order.
  `id` follows `prefixed_id` (codec:response.rs:117-120), so streaming and non-streaming agree.
- `kind` replaces the host's `finish_reason` probes. The rule is the codec's: it already decides when
  `finish_reason` and `usage` share a chunk (codec:sse.rs:12-25, 111-122).
- `openai_chat_usage` exposes what is written twice inside the codec (codec:response.rs:93-104; sse.rs:123-135)
  and a third time in the host. The host converts its settled evidence to an IR `Usage` and calls it.

**Stays in the host.** Minting the id; reading the clock; choosing which model string clients see; holding the
terminal frame until settlement; the settled numbers; the `[DONE]` sentinel (codec:sse.rs:57-59).

**Observed while reading — code reading, not run.**

- *Interim usage chunk.* The Anthropic reference parser emits a `Usage` event on `message_start`
  (crates/south-component-conformance/src/reference_anthropic.rs:537-543). The codec renders every `Usage` event as
  a chunk; with no pending finish that chunk has `"choices": []` (codec:sse.rs:95-103, 111-115; pinned by
  tests/openai_chat.rs:397). The host's probe does not match an empty `choices`, so the chunk is forwarded at once
  (server:…/sender.rs:4416-4431). Inferred: a Chat client of an Anthropic-dialect upstream receives an
  empty-`choices` usage chunk before any content, whether or not it sent `stream_options.include_usage` (N-Q2).
- *Reasoning detail.* The wholesale replacement drops the codec's `completion_tokens_details.reasoning_tokens`
  from the Chat terminal chunk (codec:sse.rs:133-135 against server:…/sender.rs:2113-2129); on the Responses
  surface the host overwrites only the three totals and keeps the detail buckets (…/sender/responses.rs:2000-2013).
- *No `role`.* No streamed chunk carries `delta.role` (codec:sse.rs:68-90, 144-149); the burst does (§6).

## 4. G2 — Replay carrier classification

**Today.**

- `ReplayRequirement::classify` walks `body.input[]`, looks only at `type = "reasoning"` items, and returns `None`,
  `NativeOpaque` or `ClaudeCarrier`. It refuses an unknown `tsr.` version, a carrier item without a string `id`, a
  non-string `encrypted_content`, a carrier the codec cannot decode, and a carrier mixed with an unmarked opaque
  (server:…/reasoning_replay.rs:69-110); the five reasons are for logs only (:41-66), and the host re-declares the
  markers `"tsr."` and `"tsr.c1."` (:24-28) while the codec's are private (codec:responses/replay.rs:8;
  request.rs:623). It runs before routing, on the rewritten body and on the original one
  (server:…/sender/responses.rs:139-145, 375-380).
- The codec's request parser overlaps but applies a different rule: it decodes only `tsr.c1.` values and requires
  an `id` (codec:responses/request.rs:622-632). Any other value — an unknown `tsr.` version included — is kept as
  the extension `responses_reasoning_encrypted_content` (:612-621), and mixing is not checked.
- The two read different fields. With `allow_messages`, which the server enables
  (server:…/sender/responses.rs:1830), the parser reads top-level `messages` in preference to `input`
  (codec:responses/request.rs:27-31) and dispatches a `type = "reasoning"` item there like any other (:198-213).
  `classify` reads `input` only. **Suspected defect (code reading, not run; `seal_target` and the component-side
  checks were not traced):** a carrier placed under `messages` is classified `None`, and `leg_accepts(None, …)`
  admits every leg (server:…/reasoning_replay.rs:205-206).
- Relation to R2: R2 fixed the carrier format and left target mismatch and HTTP status mapping to the host
  (`2026-09-28-responses-north-codec.md`:61-66). The unknown-version and mixing rules come from P15 A2 §4, not R2.
- Community host: P15 A2 §7 step 5 has both hosts implement carrier parsing and candidate filtering; P15 A3 records
  the community half as outstanding. One copy exists today; a second is about to be written.

**Proposal.**

```rust
pub const REASONING_REPLAY_MARKER_PREFIX: &str = "tsr.";
pub enum ReasoningReplayClass { None, NativeOpaque, ClaudeCarrier }
pub enum ReasoningReplayRefusal { UnknownCarrierVersion, MixedCarriers, Carrier, CarrierWithoutId, NotAString }
pub struct ReasoningReplayError { pub reason: ReasoningReplayRefusal, pub field: String }  // path, never content

/// Reads the item list `chat_request_from_responses` would read under the same options.
pub fn classify_reasoning_replay(body: &Value, options: &ResponsesRequestOptions)
    -> Result<ReasoningReplayClass, ReasoningReplayError>;
```

Every refusal maps to the existing stable code `reasoning_replay_invalid` (codec:lib.rs:93). The function names a
class; it does not interpret an unmarked opaque, which R2 already rules out (same record, :62-63). Recommended in
the same release: `chat_request_from_responses` applies the same two refusals, so a host that parses without
classifying cannot carry an unknown version as an opaque — a change to an existing function (§10; N-Q6).

**Stays in the host.** `ReplayLeg`, `leg_accepts`, `declares_claude_replay` and the reserved capability vocabulary
(server:…/reasoning_replay.rs:119-217), which need the provider type and the routing chain; the HTTP mapping; the
pre-send re-check; keeping carrier content out of logs and metrics.

## 5. G3 — Failure events on the Responses surface

**Today: three client-visible shapes.**

1. *Codec-rendered.* A `StreamEvent::Error` becomes `response.failed` with `sequence_number`, preceded by
   `response.created` if nothing was sent yet; `error.type` is always `"server_error"` and `error.code` comes from a
   closed table (codec:responses/stream.rs:578, 634-638, 838-853). The host forwards it as an ordinary frame: its
   terminal test matches only `response.completed` / `.incomplete` (server:…/sender/responses.rs:1993-1998).
2. *Upstream frame relayed, Codex only.* On the Native pass-through path, when the provider type is Codex
   (:1528-1529, :1666), an in-band `response.failed` or `error` frame is forwarded byte for byte; a frame of any
   other type that carries a top-level `error` is re-encoded as `event: error` with
   `{"type":"error","error":{"type","message"}}` and no `sequence_number` (:2062-2099).
3. *Host-originated, Codex only.* An I/O or validation error after HTTP 200 becomes the same `error` event with type
   `upstream_stream_error` (:2104-2130, applied at :1812); other providers' streams end with a transport error.
   The message is the host's internal reason string, which for a read failure contains the provider row name
   (:1613-1615, :2147-2156) — code reading, not run.

When the codec itself refuses to render, it waits for an explicit `Error` (codec:responses/stream.rs:548-555); the
host parks the stream and sends none (server:…/sender/responses.rs:1630-1636). Community host: at the pinned commit
it renders `failed` directly from an IR error (P15 A1 row E08), and the codec's code table follows that wire
(Responses record, :26). South's docs record no top-level `error` event there.

**Split.**

| Concern | Direction | Owner |
|---|---|---|
| Recognising an upstream `response.failed` / `error` / typeless error object and turning it into an `ErrorEnvelope` (`responses_stream_error_message`, :2026-2057; the match in `responses_stream_error_frame`, :2062-2084) | Southbound | The OpenAI Responses upstream component — sibling record |
| Rendering the client-facing failure event | Northbound | `south-north-codec` — already has it |
| Deciding that a failure happened, the funds transition, which message text a client may see, whether to recover at all | — | Host |

**Proposal.** No new shape where a render state exists: the host renders its own failures by passing one
`StreamEvent::Error { error }` to `responses_frames`, including after a codec mapping failure. This supersedes
line 26 of the Responses record for the wrap and synthesize cases; a frame relayed verbatim stays the host's. The
sibling record recommends keeping a relayed mode with no render state (its §8.2, north-identical delivery). If
that is accepted, a failure the host originates on such a stream needs a stateless form:

```rust
/// For a relayed stream with no render state. The shape is decided under N-Q3.
pub fn responses_error_event(error: &ErrorEnvelope, sequence_number: Option<u64>) -> ResponsesFrame;
```

## 6. G4 — Single-burst Chat SSE

**Today.** A Chat request routed to a Responses-only upstream is always sent non-streaming
(server:…/chat.rs:303-314). If the client asked to stream, the host translates the upstream body to a Chat envelope
with the leaf's wire-to-wire `codex_responses_to_openai_chat` (leaf:translate_responses.rs:615-719) and calls
`render_single_burst_chat_sse` — its only caller (server:…/sender.rs:3779-3786). That function emits two chunks and
`[DONE]`: the first chunk's `delta` is the whole `message` object of `choices[0]`; the second carries
`finish_reason` (default `"stop"`) and `usage` from `exact_chat_usage_json` (the T-3 fix). Missing `id` / `created`
/ `model` default to `"chatcmpl-burst"` / `0` / `""` (:1943-1993). Community host: no evidence in south's docs.

Code reading, not run: the tool calls in that `delta` are in the non-streaming shape — `id`, `type`, `function`,
no `index` (leaf:translate_responses.rs:653-660) — whereas the codec's streaming renderer always writes `index`
(codec:sse.rs:85).

**Proposal.**

```rust
pub fn openai_chat_burst(response: &ChatResponse, context: &ChatStreamContext)
    -> Result<Vec<OpenAiChatFrame>, CodecError>;
```

Defined as `OpenAiChatStream::new(context).frames(expand(response))`, where `expand` is the canonical event
sequence of a complete response: per choice, thinking as `ThinkingDelta`, text as `Delta`, each tool call as one
`ToolCallDelta` carrying id, name and the full arguments, then `Finish`, one `Usage`, `Done`. It refuses tool-call
arguments that are not valid JSON, as `openai_chat_response` does (codec:response.rs:69-77). `response.usage` is
rendered as given; a host that settles from its own evidence writes those numbers into the IR value first. The
input is IR: the sibling record's component produces it, and until then the host can reach IR through the leaf's
existing Chat-envelope parser (`openai_chat_response_to_ir`), so G4 does not wait for the sibling.

**Stays in the host.** Deciding that a burst is needed; settlement before the first byte; `[DONE]`.

## 7. G5 — Counting forwarded text

**Today.** A meter wraps the outermost response stream, so it counts frames actually handed to the client, including
frames the codec never rendered (server:…/sender/checkpoint.rs:8-10, 94-107; attached at sender.rs:2581, 4676,
5045, sender/messages.rs:1519, sender/responses.rs:1809, sender/gemini.rs:496). `observe_frame` splits SSE lines and
skips `[DONE]`, comments and non-JSON (:53-92); `forwarded_delta_chars` tells four client shapes apart (:110-200):

| Wire | Counted (Unicode scalar values) | Reasoning flag |
|---|---|---|
| OpenAI Chat | `choices[].delta`: `content`, `refusal`, `reasoning_content`, `reasoning`, `tool_calls[].function.name` / `.arguments` | reasoning text non-empty |
| Anthropic Messages | `content_block_delta`: `text`, `partial_json`, `thinking`; `content_block_start`: `name`, `text` | `thinking` non-empty |
| OpenAI Responses | the string `delta` of any event whose type ends in `.delta` and does not contain `audio` | type contains `reasoning` |
| Gemini native | `candidates[].content.parts[]`: `text`, `functionCall.name`, serialized `args` | part has `thought: true` |

The count becomes `output_tokens` by `ceil(chars / 4)`
(server:gateway/src/modules/inference/engine/token_counter/estimate.rs:19-21) in a snapshot that is evidence for
manual review and never settles by itself (checkpoint.rs:17-20).

**Proposal.**

```rust
pub enum NorthWire { OpenAiChat, AnthropicMessages, OpenAiResponses }
pub struct ForwardedText { pub chars: usize, pub reasoning: bool }
/// Total: an unrecognised payload counts zero. `data` is one SSE `data:` payload, already parsed.
pub fn forwarded_text(wire: NorthWire, data: &Value) -> ForwardedText;
```

The host knows the surface at every attachment site, so the wire is passed instead of sniffed. The reason to
co-locate is an invariant nothing enforces today: for every stream the codec renders, the sum of `forwarded_text`
over its frames equals the characters of the text-bearing input events. Writing that test forces two decisions
the host's table leaves open (code reading): tool names are counted on Chat and Anthropic but not on Responses,
where the name travels in `response.output_item.added` (codec:responses/stream.rs:805-811), and a local-shell call
is emitted without any `.delta` event (:497-501, 828-833).

**Stays in the host.** SSE line splitting; the meter and its lock; the `ceil(chars / 4)` estimate; the snapshot,
the lease and everything about settlement; the Gemini-native arm — the codec has no Gemini northbound wire
(codec:lib.rs:42-52) and this record proposes none. Per D6 this section is specified, not scheduled: a character
count feeding a billing estimate is metering-adjacent (N-Q9).

## 8. Conformance and acceptance

**Fixtures.** A pack under `crates/south-north-codec/fixtures/render-gaps/`, one JSON file per case holding
`input`, `context` and `expected`. The codec's existing tests keep their inline fixtures (validation record, :7);
files are needed here because a second repository must read them at the pinned tag.

| Family | Rows required by name | Assertion |
|---|---|---|
| `chat.stream` | `identity-on-every-chunk`, `terminal-kind`, `usage-only`, `error-renders-nothing` | Frames equal; every chunk carries the context's three fields |
| `chat.burst` | `text`, `thinking-and-text`, `tool-calls`, `multi-choice`, `invalid-arguments` | Equal to the stream rendering of the expanded response; tool calls carry `index` |
| `responses.replay-classify` | `none`, `native-opaque`, `claude-carrier`, one row per refusal, `carrier-under-messages` | Class or refusal reason; error text contains no carrier bytes |
| `responses.failure` | `before-created`, `mid-stream`, `after-mapping-error`, one row per `ErrorCode` | Frames equal; exactly one terminal |
| `forwarded-text` | one row per frame type each renderer can emit, plus `unknown-shape` | Counts equal; the render/count invariant of §7 |

**Equality rule (D7).** Two renderings are equal when their SSE frame sequences have the same length and order,
each frame's `event:` line is byte-equal, and each `data:` payload is equal as a JSON value — object key order
ignored. `expected` is stored with sorted keys, which is what south's build produces; a host that wants a byte
comparison canonicalises its own output the same way first. Properties, at the codec's fixed 32 samples: batching
independence for `OpenAiChatStream`; `openai_chat_burst` equal to the expanded stream; the §7 invariant.

**What a host must run.** A parity test that loads the pack from the pinned south checkout and drives the host's
own seam — for the server, `leaf:north_codec.rs` plus the sender's frame encoder — under the equality rule; it
catches a host that still patches frames after the codec. Adoption is recorded in `compatibility.json`
`host_capabilities`, e.g. a `north_render` entry with a case count beside the existing `provider_stream` entries.

## 9. Migration and dual runs

| Order | Gap | Why here | South | Host |
|---|---|---|---|---|
| 1 | G2 | Smallest; no rendered bytes change; the community half of P15 R3 is unwritten, so this prevents a second copy; closes the field mismatch of §4 | `classify_reasoning_replay`, exported marker | Replace `classify`; keep the leg filter |
| 2 | G1 | Every Chat stream path; prerequisite of G4 | `OpenAiChatStream`, `openai_chat_usage` | Replace the stamp, its six application sites and the terminal probes |
| 3 | G4 | One call site; reuses G1 | `openai_chat_burst` | Replace the burst renderer |
| 4 | G3 | Waits for the sibling record (N-Q7) and N-Q3 | Nothing, or `responses_error_event` | Feed `StreamEvent::Error`; delete the hand encoders |
| 5 | G5 | Waits for N-Q9 | `forwarded_text` | Replace three of the four arms |

**Golden comparison against the host's current output.** Before each switch the host captures the current
function's output on the fixture inputs as golden files, with the id and clock injected (`ChatChunkStamp::new` reads
both internally, server:…/sender.rs:1661-1667, so it needs a test constructor). After the switch the same inputs go
through the codec path, and every difference must be one of the **intentional differences** below:

- G1: the three fields are inserted ahead of `choices` instead of being appended by the stamp, so under the host's
  `preserve_order` build the output is JSON-equal, not byte-equal, to the old one (inferred). If N-Q2 is accepted,
  no empty-`choices` usage chunk before the terminal one.
- G2: a carrier under top-level `messages` is classified instead of passing as `None`.
- G3: `error.type` and `error.code` come from the codec's table instead of `upstream_error` /
  `upstream_stream_error` or the upstream's own string; the event is `response.failed` with a `sequence_number`.
- G4: tool calls carry `index`; content kinds arrive as separate deltas instead of one `message` object; every
  choice is rendered; `role` per N-Q4.
- G5: none on the three wires unless the fixture review decides the two asymmetries of §7.

**Host code that retires** (measured at `4d5bb4e5`):

| File | Retires | Lines |
|---|---|---|
| server:…/sender.rs | `ChatChunkStamp` (:1648-1683); `render_single_burst_chat_sse` (:1943-1993); the JSON shape in `exact_chat_usage_json` (:2108-2129 — the conversion from the host's evidence to IR `Usage` stays); six stamp applications and four terminal probes | 36 + 51 + 22 + about 25 |
| server:…/reasoning_replay.rs | marker constants (:24-28), `ReplayInvalid` (:41-66), `classify` (:69-110) | 73 of 217 production lines |
| server:…/sender/responses.rs | `encode_responses_error_event` (:2086-2099) and `responses_stream_error_frame` (:2059-2084) | 40; `responses_stream_error_message` (:2026-2057, 32 lines) goes to the sibling record |
| server:…/sender/checkpoint.rs | `forwarded_delta_chars` (:110-200) except its Gemini arm (:149-173) | 66 of 91 |
| leaf:translate_responses_sse.rs | The whole file — dead today, deletable without this record | 250 |

About 310 lines of production code besides the dead file. The case for this record is one renderer, not the count.

## 10. Versioning

- The codec has no version of its own: `version.workspace = true` and `publish = false`
  (crates/south-north-codec/Cargo.toml), currently 0.42.0. Hosts pin south by git tag (server:Cargo.toml:117). No
  `compatibility.json` contract number covers the codec and no component tuple changes.
- Everything proposed is a new function or type. No existing signature, state struct or output changes, so taking
  the release is not a breaking change for the community host. The one exception is optional: tightening
  `chat_request_from_responses` (§4) would refuse an unknown `tsr.` version that is carried as an opaque today. It
  ships only with the community host's agreement (N-Q6).
- Each step is a south minor. Raising the south pin counts as modifying the host (ruled 2026-09-30, P21 §1.3).
  That is acceptable outside DP0, but it argues for two releases rather than five: G2 alone, then G1 with G4.
- The sibling record depends on two further codec changes (its NC-1 and NC-2, both about OpenAI reasoning items).
  They are specified there, not here.

## 11. Rejected alternatives

- **Leave all five in the host.** Legitimate under DP0, and the default for G5 (D6). Rejected for G2 because a
  second copy is about to be written, and for G1 and G4 because a wire the codec renders incompletely makes every
  host patch frames after it, which is where drift starts.
- **Add `id` / `model` / `created` to `OpenAiChatSseState`.** Its fields are public and it derives `Default`
  (codec:sse.rs:36-53): a new field breaks any consumer that builds it with a struct literal, and an identity-less
  default is what `AnthropicSseState` deliberately refuses to have (codec:anthropic_sse.rs:13-18).
- **The codec mints the id, reads the clock, or decides the replay leg.** The first two are against the crate's
  rule (codec:lib.rs:22-27); the third needs the provider type and the routing chain, which R2 left to the host.
- **Port the host's burst function as a second renderer.** "Burst equals the expanded stream" would be a test to
  maintain rather than a definition, and the missing `index` would be ported with it.
- **Count text inside the render states instead of re-reading forwarded frames.** It would miss frames the codec
  did not render and count frames rendered but never forwarded (server:…/sender/checkpoint.rs:8-10).

## 12. Open questions

S = south maintainers, L = lv, C = community host.

On 2026-09-30 lv ruled on the L-tagged questions as recommended; the rulings are recorded under each question. The
halves tagged S or C remain open.

- **N-Q1 (L)** Which `model` string do Chat chunks carry: the upstream model id (today, at every site) or the name
  the client requested? The codec renders what it is given. Recommended: no change in this migration.
  **Ruled (lv, 2026-09-30): no change in this migration.**
- **N-Q2 (L, C)** Should `OpenAiChatStream` fold early `Usage` events instead of emitting an empty-`choices` chunk
  mid-stream (§3)? Recommended: yes — fold until the terminal chunk; honouring `stream_options.include_usage` is a
  later product decision. The old function keeps its behaviour.
  **Ruled for the closed host (lv, 2026-09-30): fold.**
- **N-Q3 (L, S)** Gateway-originated failures on the Responses surface: (a) `response.failed`, the codec's shape,
  or a top-level `error` event — and if the latter, nested as the host writes it today or flat (the public API
  reference was not checked for this record); (b) on every provider's stream, or only Codex as today; (c) a fixed
  public message, or the internal reason as today. Recommended: `response.failed`, every provider, fixed message.
  **Ruled for the closed host (lv, 2026-09-30): `response.failed`, on every provider's stream, with a fixed public
  message.** This also removes a per-provider branch the host has today (recovery only for one provider).
- **N-Q4 (L, C)** Should the first delta of each choice carry `role: "assistant"`? The burst does today, streamed
  chunks do not, and G4 needs one rule. Recommended: yes, in the new wrapper only.
  **Ruled for the closed host (lv, 2026-09-30): yes, in the new wrapper only.**
- **N-Q5 (S)** Is JSON-value equality (D7) enough, or should the codec own key order — typed serializers for the
  new frames, or enabling `preserve_order` itself?
- **N-Q6 (S, C)** Tighten `chat_request_from_responses` to refuse unknown `tsr.` versions and mixed carriers (§4)?
  Will the community half of the P15 R3 carrier work take `classify_reasoning_replay` from the codec?
- **N-Q7 (S, with the sibling record)** Is the sibling's north-identical delivery (its §8.2) accepted? If not, every
  Responses stream has a render state and `responses_error_event` is dropped.
- **N-Q8 (C)** Does the community host put identity fields on Chat chunks today, and does it enable `preserve_order`?
- **N-Q9 (C, L)** Does the community host have, or plan, a consumer for `forwarded_text`? Recommended: G5 stays in
  the host until one appears.
  **Ruled for the closed host (lv, 2026-09-30): G5 stays in the host until a second consumer appears.**
