# North codec: closing the remaining northbound render gaps

Status: proposed — drafted for review by the host team (token-station-server P21), not accepted

Date: 2026-09-30

Revised: 2026-10-01 after independent review (see the revision note at the end).

Predecessors: `2026-09-28-responses-north-codec.md` (the Responses mapping; "R2: bounded Claude reasoning replay
carrier"; its line 26 leaves the server's Native upstream error frames in the host shell),
`2026-09-28-responses-north-codec-validation.md` (the fixture and façade discipline extended here),
`2026-09-30-host-zero-vendor-boundary.md` (§6 usage, §11 follow-on components, §13 phasing), `ARCHITECTURE.md`
("Host-owned concerns"). Siblings: `2026-09-30-openai-responses-upstream-component.md` — it owns the southbound half
of §5 and produces the input of §6; `2026-09-30-kiro-provider-component.md` — its estimate rulings change the weight
of §7. This record refers to both by name and does not design them.

Origin: token-station-server plan P21 (`docs/product-review-v2/plans/2026-09-29-P21-*.md`) — §3.2 (the
"north codec" row), Appendix B.2 (the paragraph after the table) and Appendix B.4 ("no design and no plan"); P15
in the same directory (`2026-09-28-P15-Responses*.md`) with annexes A1–A3.

Baseline: south `origin/main` code = v0.42.0 (`3135e36`); host `a82c852b`. Between the first draft's host baseline
(`4d5bb4e5`) and `a82c852b` nothing under `server:…/` (below) changed; `leaf:translate_responses.rs` gained nine
lines at :292 (the fix that keeps array-form system messages, host issue #61), and its citations below are updated.
`codec:` abbreviates `crates/south-north-codec/src/`; `server:…/` abbreviates
`server:gateway/src/modules/inference/engine/text_admission/`; `leaf:` abbreviates
`server:crates/gateway-provider-protocol/src/`. Host plans are cited by section, not by line.

## 1. Problem

`south-north-codec` exists so that two hosts render the same client-visible documents from the same IR
(codec:lib.rs:3-7). The host's audit lists five places where token-station-server still renders, classifies or
re-reads northbound wire itself (P21 Appendix B.2). This record says where each one goes.

**These are northbound concerns, and the host's zero-vendor rule does not require moving them.** DP0 forbids
provider logic in the host; northbound protocols are the host's product surface and may stay in the host or sink
into `south-north-codec` as P15 did (P21 §1.1; B.4 repeats it for these five items). The reason to move them is
the other goal: one renderer, so client-visible behaviour does not drift between the closed host and the community
host. None of the five unlocks a provider or gates a boundary phase, with one exception noted in §5: the owner's
rulings on the Responses surface make G3's stateless failure event a prerequisite of the Responses record's
pass-through delivery. Otherwise this record is lower priority than every phase of the boundary record (B1–B7b, its
§13) and must not block any of them.

| | Gap | Host today | Codec today |
|---|---|---|---|
| G1 | Chat chunk identity | Stamps `id` / `model` / `created` on every rendered chunk (server:…/sender.rs:1648-1683), at seven sites | Chunks carry `object`, `choices`, `usage` only (codec:sse.rs:111-158) |
| G2 | Replay carrier classification | `ReplayRequirement::classify(body)` (server:…/reasoning_replay.rs:69-110) | Encodes, decodes and materialises the carrier (codec:responses/replay.rs:87-180; request.rs:447-518, 622-632); does not classify |
| G3 | Failure events on the Responses surface | Three hand-written paths (server:…/sender/responses.rs:2026-2130) | `StreamEvent::Error` → `response.failed` (codec:responses/stream.rs:634-638) |
| G4 | Single-burst Chat SSE | `render_single_burst_chat_sse` (server:…/sender.rs:1943-1993) | Streaming and non-streaming renderers; no burst |
| G5 | Counting forwarded text | `forwarded_delta_chars` (server:…/sender/checkpoint.rs:110-200) | Nothing |

Reading the code changed the list in three ways:

- **G1's second starting point is dead code.** `leaf:translate_responses_sse.rs` (250 lines; `chunk_id` state
  :20-32; `created` read from the clock on every chunk, :78) has no production caller at this baseline: a
  repository-wide search finds only its golden tests (`tests/golden/translate_responses_sse.rs` in the same
  crate). P21 Appendix B.2 already lists it under D1. It is a deletion, not a migration.
- **G3 is adoption plus a host refactor.** The codec already renders a failure terminal and already has the "only
  an explicit Error may follow a mapping failure" state (codec:responses/stream.rs:539-557, 570-574). The host does
  not route its own failures through either, and the place where it recovers failures today sits outside the render
  state, so adopting the codec needs the restructuring described in §5.3.
- **Equal bytes are not a property of the codec alone, and this record does not require them.** The codec returns
  `serde_json::Value`. The host builds with `serde_json/preserve_order` (server:gateway/Cargo.toml:52), and Cargo's
  feature unification gives the codec that feature inside the host, so the codec's objects serialize there in
  insertion order; built on its own in south (`Cargo.toml:19`, no `preserve_order`), the same objects serialize with
  sorted keys. SSE framing (`data: `, blank lines, keepalive comments) is written by each host. §8 therefore defines
  equality as equal JSON values over a canonical frame sequence (D7).

## 2. Decisions

- **D1 Scope.** Additive codec functions only; no IR change, no component tuple change, no new world.
- **D2 (G1) Identity is a host-supplied context.** A new wrapper, `OpenAiChatStream`, renders complete chunks from
  `ChatStreamContext { id, model, created }`, classifies each chunk (`Delta`, `Terminal`, `Usage`) by the event that
  produced it, folds usage into the held frames, and puts `role` on the first delta of each choice of every Chat
  stream it renders, not only the burst (N-Q2 and N-Q4 ruled for the wrapper; N-Q4's extension to every Chat stream
  ruled 2026-10-01).
  `openai_chat_frames` and `OpenAiChatSseState` are untouched.
- **D3 (G2) Classification moves; the leg filter does not.** `classify_reasoning_replay` reads **every** item list
  a leg may receive — `input`, and `messages` when the options admit it — not only the list the parser reads. Which
  provider leg may receive which class stays in the host.
- **D4 (G3) No new failure shape, and one failure shape on every Responses stream.** Where a render state exists the
  host feeds it `StreamEvent::Error`; a relayed stream gets a stateless form that renders the same `response.failed`
  from the relayed stream's identity. Both are required now: the owner ruled N-Q3 (gateway-originated failures, every
  provider's stream) and the Responses record's R-Q1 (pass-through streams), and today's Native pass-through leg
  already relays with no render state. Relayed **upstream** failure frames are replaced too, by the same
  fixed-message event (N-Q3's extension, ruled 2026-10-01, §5.2). Recognising an upstream failure frame is southbound
  (Responses record).
- **D5 (G4) A burst is the streaming renderer applied to an expanded single-choice response**, not a second
  renderer. More than one choice is `Unrenderable`. The host renders before it settles.
- **D6 (G5) The codec says where forwarded text lives; the host estimates and settles.** Specified here;
  implemented only once a second consumer is in sight (ARCHITECTURE.md:114-115; N-Q9 as ruled). The count is not
  the settlement meter for `usage_evidence: absent` families: the Kiro record's K-Q19 (ruled 2026-10-01) counts
  output as produced, over the IR events, so the forwarded count stays checkpoint and manual-review evidence only
  (§7); N-Q10 asks the community host.
- **D7 Equality rule.** Two renderings are equal when their canonical frame sequences are equal and each JSON
  payload is equal as a JSON value (§8). This record does not require equal bytes across hosts; N-Q5 must be ruled
  before G1 ships.

## 3. G1 — Chat chunk identity

**Today.**

- The codec's state doc says identity is absent on purpose: "this wire's chunks carry neither" the routed model
  nor a message id (codec:sse.rs:32-35). An `Error` event renders nothing on this wire and ends the stream (:105).
  The non-streaming renderer does take host facts — `ResponseContext { created, fallback_id }`
  (codec:response.rs:18-28) — and applies the `chatcmpl-` prefix without stacking it (:117-120).
- The host adds the three fields after rendering, on the ground that OpenAI clients read them on every chunk
  (server:…/sender.rs:1648-1652). `ChatChunkStamp::new` mints `chatcmpl-<uuid>`, reads the clock once per stream
  and takes the model from its caller (:1660-1667); `apply` inserts each key only when absent (:1669-1682). It is
  applied at **seven** sites: Bedrock Invoke (:1603-1608), Bedrock Converse (:1638-1643), three inline in the Chat
  stream loop (:4415, :4510, :4578), `render_ir_chat_frames` (:4701), which serves Gemini (:4445) and Kiro (:4746),
  and the Kiro end-of-stream tail, which renders what `parser.finish()` returns (:4961). Every construction site
  passes the upstream model id (:1558 and :1569 via :2450; :4251; :4839).
- Five of those sites decide which chunk is terminal by testing `choices[0].finish_reason` for non-null (:4416,
  :4511, :4579, :4702-4706, :4962-4966), hold that chunk until settlement, then replace its `usage` wholesale with
  `exact_chat_usage_json` (:4641-4646; :2113-2129): three keys plus `prompt_tokens_details.cached_tokens`, and an
  error when the total overflows (:2115).
- Community host: south's docs do not describe its Chat stream chunks. Not established (N-Q8).

**Proposal.**

```rust
pub struct ChatStreamContext { pub id: String, pub model: String, pub created: i64 }
pub enum OpenAiChatFrameKind { Delta, Terminal, Usage }
pub struct OpenAiChatFrame { pub kind: OpenAiChatFrameKind, pub data: Value }
pub struct OpenAiChatStream { /* ChatStreamContext + OpenAiChatSseState + roles already sent */ }
impl OpenAiChatStream {
    pub fn new(context: ChatStreamContext) -> Self;
    pub fn frames(&mut self, events: &[StreamEvent]) -> Result<Vec<OpenAiChatFrame>, CodecError>;
}
/// The one rendering of this wire's `usage` object. `OutOfRange` when the total overflows.
pub fn openai_chat_usage(usage: &Usage) -> Result<Value, CodecError>;
```

- **Identity.** Every frame carries `id`, `object`, `created`, `model`, then `choices` (and `usage`), inserted in
  that order. `id` follows `prefixed_id` (codec:response.rs:117-120), so streaming and non-streaming agree.
- **Kinds.** `Terminal` is the chunk rendered from a `Finish` event — whether it was flushed by a later event, by
  `Done`, or merged with a `Usage` — **regardless of whether its `finish_reason` is null**: the codec renders
  `Finish { finish_reason: None }` as a chunk with a null reason (codec:sse.rs:91-93, 140-142), and a probe on
  non-null reasons would let that chunk through unheld. `Usage` is the trailing `choices: []` chunk of the fold rule
  below. Every other chunk is `Delta`. The kinds replace the host's five `finish_reason` probes; the host holds
  `Terminal` and `Usage` frames until settlement.
- **Fold (N-Q2, ruled for the closed host).** A `Usage` event never produces a chunk of its own while the stream can
  still finish: it is folded into the wrapper's usage (`Usage::absorb`, as today). The folded usage is written on the
  `Terminal` frame when that frame is emitted. If `Done` arrives and no `Finish` was ever seen, the wrapper emits one
  `Usage` frame — `choices: []`, the `stream_options.include_usage` shape — carrying the folded usage, provided any
  usage was folded; otherwise nothing. An `Error` still renders nothing and ends the stream (codec:sse.rs:105).
- **Role (N-Q4, ruled for the closed host).** The first `Delta` frame of each choice index carries
  `delta.role: "assistant"`. The ruling covers the wrapper, and its extension of 2026-10-01 applies it to every Chat
  stream rendered through the wrapper, not only the burst (N-Q4). The wrapper has no flag to turn it off.
- **Overflow.** `openai_chat_usage` and `frames` report a total that overflows `u64` as `OutOfRange` at
  `usage.total_tokens`, matching the host's refusal (server:…/sender.rs:2115); the existing `usage_chunk` adds
  without a check (codec:sse.rs:126) and is left as it is (D2).
- `openai_chat_usage` exposes what is written twice inside the codec (codec:response.rs:93-104; sse.rs:123-135) and
  a third time in the host. The host converts its settled evidence to an IR `Usage` and calls it; the conversion
  (which folds the host's two cache buckets into `cache_read_tokens`) stays in the host.

**Stays in the host.** Minting the id; reading the clock; choosing which model string clients see (N-Q1, ruled);
holding the `Terminal` / `Usage` frames until settlement; the settled numbers; the `[DONE]` sentinel
(codec:sse.rs:57-59).

**Client-visible changes, on every Chat stream the wrapper renders** (listed again in §9):

- `delta.role: "assistant"` on the first delta of each choice, on every Chat stream (N-Q4 and its extension, ruled
  2026-10-01). No streamed chunk carries it today (codec:sse.rs:68-90, 144-149).
- `completion_tokens_details.reasoning_tokens` on the terminal chunk whenever the settled usage has a non-zero
  reasoning count: the codec writes it (codec:sse.rs:133-135) and the host's wholesale replacement drops it today
  (server:…/sender.rs:2113-2129). On the Responses surface the host already keeps the detail buckets
  (…/sender/responses.rs:2000-2013), so this aligns the two surfaces.
- No `choices: []` chunk before the terminal chunk (fold). Today one can be sent before any content — observed while
  reading, not run: the Anthropic reference parser emits a `Usage` event on `message_start`
  (crates/south-component-conformance/src/reference_anthropic.rs:537-543), the codec renders every `Usage` event as
  a chunk (codec:sse.rs:95-103, 111-115; pinned by the test starting at tests/openai_chat.rs:397), and the host's
  probe does not match an empty `choices`, so the chunk is forwarded at once (server:…/sender.rs:4416-4431),
  whether or not the client sent `stream_options.include_usage`.
- Key order: identity fields first. Under the host's `preserve_order` build the old stamp appended them after
  `choices`; the new frames are JSON-equal but not byte-equal to the old ones (§8).

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
  (server:…/sender/responses.rs:1830), the parser reads top-level `messages` in preference to `input` and then
  ignores `input` entirely (codec:responses/request.rs:27-31); it dispatches a `type = "reasoning"` item under
  `messages` like any other (:198-213). `classify` reads `input` only. Two consequences (code reading, not run;
  `seal_target` and the component-side checks were not traced):
  - **Suspected defect today:** a carrier placed under `messages` is classified `None`, and `leg_accepts(None, …)`
    admits every leg (server:…/reasoning_replay.rs:205-206).
  - **Why "read what the parser reads" is not enough:** a body with both lists has its `input` ignored by the parser
    but forwarded unchanged by a leg that sends the client's body (the Native pass-through leg; Responses record
    §1). A classifier that followed the parser would miss a carrier under `input` in such a body and could let it
    reach that leg. Today's `classify` catches that case; the first draft of this record would have lost it.
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

/// Scans `input[]` and, when `options.allow_messages`, also `messages[]`: the union of what any leg may receive.
pub fn classify_reasoning_replay(body: &Value, options: &ResponsesRequestOptions)
    -> Result<ReasoningReplayClass, ReasoningReplayError>;
```

- Both lists are scanned whenever they are arrays; the class is the union over both, and a carrier in one list with
  an unmarked opaque in the other is `MixedCarriers`. `field` names the list and index
  (`messages[3].encrypted_content`).
- Every refusal maps to the existing stable code `reasoning_replay_invalid` (codec:lib.rs:93). The function names a
  class; it does not interpret an unmarked opaque, which R2 already rules out (same record, :62-63).
- Recommended in the same release: `chat_request_from_responses` applies the same two refusals (unknown version,
  mixing) on the list it reads, so a host that parses without classifying cannot carry an unknown version as an
  opaque — a change to an existing function (§10; N-Q6).

**Stays in the host.** `ReplayLeg`, `leg_accepts`, `declares_claude_replay` and the reserved capability vocabulary
(server:…/reasoning_replay.rs:119-217), which need the provider type and the routing chain; the HTTP mapping; the
pre-send re-check; keeping carrier content out of logs and metrics.

## 5. G3 — Failure events on the Responses surface

### 5.1 Today: three client-visible shapes

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

### 5.2 Rulings that apply, and the split

The owner ruled on 2026-09-30 (N-Q3 below): a gateway-originated failure on the Responses surface is
`response.failed`, on every provider's stream, with a fixed public message. The owner also ruled the Responses
record's R-Q1 for the host side: a family may declare pass-through delivery on this surface (`north_passthrough`,
that record's §8.2; called "north-identical" in its first draft), so some Responses streams relay upstream bytes and
have no render state. Today's Native pass-through leg is such a stream already.

**Relayed upstream failure frames are covered by N-Q3 as well** (the owner's extension of 2026-10-01). The ruling
of 2026-09-30 covered failures the gateway originates; the extension covers the upstream's own failure frames on
relayed streams: a relayed frame whose `parse-stream-chunk` call returns `Error` is not forwarded, and the host sends
its own fixed-message `response.failed` in its place. Every Responses stream — rendered or relayed, any provider —
ends a failure the same way, and the upstream's own message text never reaches the client on this path; the internal
reason stays in the host's logs. `responses_error_event` serves both halves.

| Concern | Direction | Owner |
|---|---|---|
| Recognising an upstream `response.failed` / `error` / typeless error object and turning it into an `ErrorEnvelope` (`responses_stream_error_message`, :2026-2057; the match in `responses_stream_error_frame`, :2062-2084) | Southbound | The OpenAI Responses upstream component (Responses record §6.4) |
| Rendering the client-facing failure event, on rendered and relayed streams | Northbound | `south-north-codec` — `responses_frames` today, plus `responses_error_event` below |
| Deciding that a failure happened, the funds transition, the fixed public message and code, whether to recover at all | — | Host |

### 5.3 Proposal

**Rendered streams.** The host renders its own failures by passing one `StreamEvent::Error { error }` to
`responses_frames`, including after a codec mapping failure (the codec's awaiting-error state admits exactly that).
`error.message` is the fixed public message; `error.code` is the host's classification. This supersedes line 26 of
the Responses record for the wrap and synthesize cases.

**Relayed streams.** Required now (D4):

```rust
/// Identity of a relayed Responses stream, read by the host from the `response` object of the relayed
/// `response.created` event.
pub struct RelayedResponseIdentity { pub response_id: String, pub model: String, pub created_at: i64 }

/// One `response.failed` event for a relayed stream with no render state. `sequence_number` is one past the last
/// relayed event's. The payload is built by the same code as the render state's failure frame, so the two cannot
/// drift.
pub fn responses_error_event(error: &ErrorEnvelope, relayed: &RelayedResponseIdentity, sequence_number: u64)
    -> ResponsesFrame;
```

- The host reads `response.id`, `response.model`, `response.created_at` and each event's `sequence_number` from the
  frames it relays. That is reading the northbound protocol the host serves, not provider knowledge; the frames are
  split by `decode_sse_v1`, the pure SSE decoder south places in `south-contracts` beside the eventstream deframer
  (boundary record §5.2). The host splits northbound frames only to find the terminal frame and, here, to read this
  identity; it never picks a decoder to parse a provider's stream for the component (boundary record §15).
- If the failure comes **before** any upstream frame was relayed, there is no relayed identity: the host builds a
  `ResponsesSseState` from its own context (minted id, routed model, its clock) and feeds `StreamEvent::Error`; the
  codec emits `response.created` and `response.failed`, as on a rendered stream.
- The first draft took only an optional sequence number. That cannot produce a `response.failed`: the event's
  `response` object carries `id`, `created_at` and `model` (codec:responses/stream.rs:634), and a relayed stream's
  numbering is the upstream's.

**What the host must change.** Adopting this is not a call-site swap:

- Today the failure event is produced by `recover_responses_stream_errors`, which wraps the byte stream **after**
  the output meter and the keepalive layer (server:…/sender/responses.rs:1807-1812, 2104-2130). It sees only an
  `std::io::Error` and a Codex flag; the render state lives inside the generator (:1575 onward) and is gone by then.
- So failure rendering moves **inside the generator**, at every exit that parks the stream today — the
  `park_stream` / `mark_stream_delivery_unknown` calls at :1601, :1613, :1626, :1633, :1653, :1660, :1672, :1678,
  :1693, :1702, :1710, :1719, :1726, :1741, :1751 and :1764. Each exit does, in this order: the funds transition
  (`delivery_unknown`), then exactly one failure frame (from the render state, or from `responses_error_event` on a
  relayed stream), then the end of the stream. The client never sees a failure event before the ledger records it.
- `recover_responses_stream_errors` and the `is_codex` switch (:1528-1529, :1666, :1812) retire; an error raised
  after the generator (transport write, keepalive) cannot produce a frame and ends the connection as today.
- **Host test.** For each exit above, drive the stream with a fake upstream (relayed and rendered variants) to that
  exit and assert: the durable row is in `delivery_unknown` before the failure frame is observed; the client stream
  ends with exactly one `response.failed`; it carries the fixed message and a `sequence_number` one past the last
  frame; nothing follows it. The record's fixture pack (§8) supplies the expected payloads.

## 6. G4 — Single-burst Chat SSE

**Today.** A Chat request routed to a Responses-only upstream is always sent non-streaming
(server:…/chat.rs:303-314). If the client asked to stream, the host translates the upstream body to a Chat envelope
with the leaf's wire-to-wire `codex_responses_to_openai_chat` (leaf:translate_responses.rs:624-728) and calls
`render_single_burst_chat_sse` — its only caller (server:…/sender.rs:3779-3786). That function emits two chunks and
`[DONE]`: the first chunk's `delta` is the whole `message` object of `choices[0]` — any other choice is silently not
rendered (:1961); the second carries `finish_reason` (default `"stop"`) and `usage` from `exact_chat_usage_json`
(the T-3 fix). Missing `id` / `created` / `model` default to `"chatcmpl-burst"` / `0` / `""` (:1943-1993). The host
renders before it settles: the rendered bytes are built at :3779-3800 and the settlement is finalized at :3870.
Community host: no evidence in south's docs.

Code reading, not run: the tool calls in that `delta` are in the non-streaming shape — `id`, `type`, `function`, no
`index` (leaf:translate_responses.rs:662-669) — whereas the codec's streaming renderer always writes `index`
(codec:sse.rs:85). When the upstream body has no `id`, the leaf's fallback `"chatcmpl-codex"` is prefixed again,
giving `"chatcmpl-chatcmpl-codex"` (leaf:translate_responses.rs:702, 713).

**Proposal.**

```rust
pub fn openai_chat_burst(response: &ChatResponse, context: &ChatStreamContext)
    -> Result<Vec<OpenAiChatFrame>, CodecError>;
```

Defined as `OpenAiChatStream::new(context).frames(expand(response))`, where `expand` is the canonical event
sequence of a complete single-choice response: thinking as `ThinkingDelta`, text as `Delta`, each tool call as one
`ToolCallDelta` carrying id, name and the full arguments, then `Finish`, one `Usage`, `Done`.

- **Exactly one choice.** A response with more than one choice is `Unrenderable` at `choices`. The IR stream cannot
  express a second choice's tool calls or finish: `ToolCallDelta.index` is the tool-call ordinal and `Finish` has no
  choice index (kernel:stream.rs:33-43, 85-90), and the codec renders both on choice 0 (codec:sse.rs:89, 119, 156).
  Rendering them anyway would put the second choice's calls on the first — a plausible answer that is not what the
  model produced, which the codec refuses elsewhere (codec:lib.rs:77-85). D1 excludes an IR change. Today's input
  path only ever produces one choice, so the refusal is not reachable before the Responses component lands.
- Tool-call arguments that are not valid JSON are refused, as `openai_chat_response` does (codec:response.rs:69-77).
- `response.usage` is rendered as given. The host writes its settled evidence into the IR value first; that evidence
  is parsed from the upstream body before settlement, so it is available at render time.
- **Render before settle.** The host keeps today's order: render the burst, and only if that succeeds finalize the
  settlement and send. An `Unrenderable` burst is handled as the non-streaming Chat path handles an unrenderable 2xx
  today (502, server:…/sender.rs:2089-2104), before finalize — never a settled request with an error body.
- **Input until the Responses component lands.** The host can reach IR through the leaf's existing Chat-envelope
  parser (`openai_chat_response_to_ir`, leaf:translate_ir.rs:205), so G4 does not wait for the sibling. That input
  keeps the leaf's losses: one choice, no reasoning, text dropped when tool calls are present, `created` from the
  clock (replaced by the context), and the doubled id prefix above (leaf:translate_responses.rs:624-728). The fixture
  row `thinking-and-text` therefore exercises the codec, not anything the host can feed, until the component's IR
  replaces this path.

**Stays in the host.** Deciding that a burst is needed; the render-then-settle order; `[DONE]`.

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
(server:gateway/src/modules/inference/engine/token_counter/estimate.rs:19-21). Today it feeds a mid-stream snapshot
that is evidence for manual review and never settles by itself (checkpoint.rs:17-20).

**This count is not billing evidence.** Under the boundary record's Q12 ruling and the Kiro record's K-Q1 and K-Q2
rulings, a family that declares `usage_evidence: absent` is settled from the host's generic estimator, and the host
enforces the authorized cap on its own output meter. The Kiro record's K-Q19 (ruled 2026-10-01; its Appendix B.1
item 2) fixes what that estimator counts: output as produced — the IR events the component emitted — not the frames
forwarded to the client. The settlement meter is therefore a new host count over IR events, with one rule for
streaming and non-streaming answers, and this meter stays checkpoint and manual-review evidence only. The invariant
below still ties the two for a stream the client read to the end (the forwarded count of the rendered frames equals
the count of the text-bearing events the wire renders), but a difference in which fields this table counts changes
only the snapshot, not the settled amount.

**Proposal.**

```rust
pub enum NorthWire { OpenAiChat, AnthropicMessages, OpenAiResponses }
pub struct ForwardedText { pub chars: usize, pub reasoning: bool }
/// Total: an unrecognised payload counts zero. `data` is one SSE `data:` payload, already parsed.
pub fn forwarded_text(wire: NorthWire, data: &Value) -> ForwardedText;
/// The same rule over a complete non-streaming response body of that wire.
pub fn forwarded_text_of_response(wire: NorthWire, body: &Value) -> ForwardedText;
```

The host knows the surface at every attachment site, so the wire is passed instead of sniffed. The reason to
co-locate is an invariant nothing enforces today: for every response the codec renders, streaming or not, the count
equals the characters of the text-bearing input events **that the wire renders** — events a wire drops are excluded
(`RedactedThinking` and signatures on Chat, codec:sse.rs:72-74), and the non-streaming count of a response equals the
streaming count of its expanded events (§6). Writing that test forces two decisions the host's table leaves open
(code reading): tool names are counted on Chat and Anthropic but not on Responses, where the name travels in
`response.output_item.added` (codec:responses/stream.rs:805-811), and a local-shell call is emitted without any
`.delta` event (:497-501, 828-833). Since `absent` families settle from the produced IR events (K-Q19, ruled
2026-10-01), those two decisions affect checkpoint evidence, not billing; they still belong in the fixture review, so
the two hosts agree, not in whichever host implements the table first.

**Stays in the host.** SSE line splitting; the meter and its lock; the `ceil(chars / 4)` estimate; the snapshot,
the lease and everything about settlement; cap enforcement; the Gemini-native arm — the codec has no Gemini
northbound wire (codec:lib.rs:42-52) and this record proposes none.

**Scheduling.** The owner's ruling on N-Q9 stands: G5 stays in the host until a second consumer appears. With K-Q19
ruled for "produced", the count has no billing role that could make one; a second consumer is a community host that
keeps the same count as checkpoint or manual-review evidence (the Kiro record's K-Q15 says it has its own
implementation of that provider). N-Q10 asks the community host.

## 8. Conformance and acceptance

**Fixtures.** A pack under `crates/south-north-codec/fixtures/render-gaps/`, one JSON file per case holding
`input`, `context` and `expected`. The codec's existing tests keep their inline fixtures (validation record, :7);
files are needed here because a second repository must read them at the pinned tag.

| Family | Rows required by name | Assertion |
|---|---|---|
| `chat.stream` | `identity-on-every-chunk`, `role-on-first-delta`, `terminal-kind`, `terminal-with-null-reason`, `usage-folded-into-terminal`, `usage-only` (Done without Finish), `error-renders-nothing`, `usage-overflow` | Frames and kinds equal; every chunk carries the context's three fields |
| `chat.burst` | `text`, `thinking-and-text`, `tool-calls`, `refuses-multi-choice`, `invalid-arguments` | Equal to the stream rendering of the expanded response; tool calls carry `index`; the two refusals are `Unrenderable` |
| `responses.replay-classify` | `none`, `native-opaque`, `claude-carrier`, one row per refusal, `carrier-under-messages`, `carrier-in-input-beside-messages`, `mixed-across-lists` | Class or refusal reason and field; error text contains no carrier bytes |
| `responses.failure` | `before-created`, `mid-stream`, `after-mapping-error`, `relayed-after-created`, `relayed-before-any-frame`, one row per `ErrorCode` | Frames equal; exactly one terminal; the relayed rows carry the relayed identity and the next sequence number |
| `forwarded-text` | one row per frame type each renderer can emit, one non-streaming body per wire, plus `unknown-shape` | Counts equal; the render/count invariant of §7 |

**Equality rule (D7).** A rendering is compared as its **canonical frame sequence**:

1. Split the SSE text into events at blank lines.
2. Drop comment lines and events that contain only comments (the host's keepalive, `: keepalive`,
   server:gateway/src/modules/inference/engine/streaming/keepalive.rs:30).
3. Keep `data: [DONE]` as a sentinel element, compared by presence and position only.
4. Map every other event to the pair (its `event:` name, or none; its `data:` payload parsed as JSON).

Two renderings are equal when their canonical sequences have the same length and order, each `event:` name is equal,
and each payload is equal as a JSON value — object key order ignored. For the codec alone the sequence is its frame
list (Chat frames have no `event:` name; the sentinel is the host's). `expected` is stored with sorted keys, which is
what south's own build produces. Properties, at the codec's fixed 32 samples: batching independence for
`OpenAiChatStream`; `openai_chat_burst` equal to the expanded stream; the §7 invariant.

**What this does not promise.** Byte-equal output across the two hosts: key order depends on each host's
`serde_json` features, and framing bytes are each host's. Whether south should own key order (typed serializers, or
`preserve_order` in the codec) is N-Q5, an open south question. It must be ruled **before G1 ships**, because G1
changes the server's key order anyway (§3) and should change it once. This record recommends JSON-value equality.

**What a host must run.** A parity test that loads the pack from the pinned south checkout, drives the host's own
seam — for the server, `leaf:north_codec.rs` plus the sender's frame encoder and keepalive layer — and compares under
the canonical sequence; it catches a host that still patches frames after the codec. The G3 host test of §5.3 runs
beside it. Adoption is recorded in `compatibility.json` `host_capabilities`, e.g. a `north_render` entry with a case
count beside the existing `provider_stream` entries.

## 9. Migration and dual runs

| Order | Gap | Why here | South | Host |
|---|---|---|---|---|
| 1 | G2 | Smallest; no rendered bytes change; the community half of P15 R3 is unwritten, so this prevents a second copy; closes the field mismatch of §4 | `classify_reasoning_replay`, exported marker | Replace `classify`; keep the leg filter |
| 2 | G1 | Every Chat stream path; prerequisite of G4. Needs N-Q5 ruled first | `OpenAiChatStream`, `openai_chat_usage` | Replace the stamp, its seven application sites and the five terminal probes |
| 3 | G4 | One call site; reuses G1 | `openai_chat_burst` | Replace the burst renderer; keep render-then-settle |
| 4 | G3 | **Mandatory**: the owner's N-Q3 and R-Q1 rulings need one failure shape for gateway-originated failures on every Responses stream, relayed ones included; a prerequisite of the Responses record's pass-through cutover | `responses_error_event`, `RelayedResponseIdentity` | The generator refactor and host test of §5.3; delete the hand encoders and the recovery wrapper |
| 5 | G5 | Waits for N-Q9 / N-Q10 (a second consumer); stays in the host until then. Not the settlement meter for `absent` families — the Kiro record's K-Q19 (ruled 2026-10-01) counts output as produced IR events — so it remains checkpoint and manual-review evidence only | `forwarded_text`, `forwarded_text_of_response` | Replace three of the four arms |

**Golden comparison against the host's current output.** Before each switch the host captures the current
function's output on the fixture inputs as golden files, with the id and clock injected (`ChatChunkStamp::new` reads
both internally, server:…/sender.rs:1661-1667, so it needs a test constructor). After the switch the same inputs go
through the codec path, and every difference must be one of the **intentional differences** below:

- G1: `role` on the first delta of each choice, on every stream (N-Q4 and its extension, ruled 2026-10-01);
  `completion_tokens_details.reasoning_tokens` on the terminal chunk when the settled usage carries
  reasoning; no `choices: []` chunk before the terminal one (N-Q2); a `Finish` with no reason is held as the
  terminal; identity fields first, so under `preserve_order` the output is JSON-equal, not byte-equal, to the old one.
- G2: a carrier under top-level `messages` is classified instead of passing as `None`; a carrier in `input` beside
  `messages` stays classified, as today.
- G3: every provider's stream ends a gateway-originated failure with `response.failed` carrying `sequence_number`,
  the codec's `error.type` / `error.code` and the fixed public message — instead of `event: error` with
  `upstream_stream_error` (Codex) or a bare transport error (every other provider). By N-Q3's extension (ruled
  2026-10-01), the same replaces the relayed upstream frame and its `upstream_error` re-encoding.
- G4: tool calls carry `index`; content kinds arrive as separate deltas instead of one `message` object; `role` on
  the first delta (N-Q4); a response with more than one choice is refused instead of silently truncated to the first.
- G5: none on the three wires unless the fixture review decides the two asymmetries of §7.

**Host code that retires** (measured at `a82c852b`):

| File | Retires | Lines |
|---|---|---|
| server:…/sender.rs | `ChatChunkStamp` (:1648-1683); `render_single_burst_chat_sse` (:1943-1993); the JSON shape in `exact_chat_usage_json` (:2108-2129 — the conversion from the host's evidence to IR `Usage` stays); seven stamp applications and five terminal probes | 36 + 51 + 22 + about 30 |
| server:…/reasoning_replay.rs | marker constants (:24-28), `ReplayInvalid` (:41-66), `classify` (:69-110) | 73 of 217 production lines |
| server:…/sender/responses.rs | `encode_responses_error_event` (:2086-2099), `responses_stream_error_frame` (:2059-2084), `recover_responses_stream_errors` (:2104-2130) and the `is_codex` switch | about 70; `responses_stream_error_message` (:2026-2057, 32 lines) goes to the Responses record. The generator gains the failure path of §5.3 |
| server:…/sender/checkpoint.rs | `forwarded_delta_chars` (:110-200) except its Gemini arm (:149-173) | 66 of 91 |
| leaf:translate_responses_sse.rs | The whole file — dead today, deletable without this record | 250 |

About 330 lines of production code besides the dead file. The case for this record is one renderer, not the count.

## 10. Versioning

- The codec has no version of its own: `version.workspace = true` and `publish = false`
  (crates/south-north-codec/Cargo.toml), currently 0.42.0. Hosts pin south by git tag (server:Cargo.toml:117). No
  `compatibility.json` contract number covers the codec and no component tuple changes.
- Everything proposed is a new function or type. No existing signature, state struct or output changes, so taking
  the release is not a breaking change for the community host. The one exception is optional: tightening
  `chat_request_from_responses` (§4) would refuse an unknown `tsr.` version that is carried as an opaque today. It
  ships only with the community host's agreement (N-Q6).
- Each step is a south minor. Raising the south pin counts as modifying the host (ruled 2026-09-30, P21 §1.3).
  That is acceptable outside DP0, but it argues for few releases: G2 alone; then G1, G4 and G3's
  `responses_error_event` together, after N-Q5; G5 only if N-Q10 finds a second consumer.
- The Responses record depends on two further codec changes (its NC-1 and NC-2, about OpenAI reasoning items).
  They are specified there, not here.

## 11. Rejected alternatives

- **Leave all five in the host.** Legitimate under DP0, and the default for G5 (D6). Rejected for G2 because a
  second copy is about to be written; for G1 and G4 because a wire the codec renders incompletely makes every host
  patch frames after it, which is where drift starts; for G3 because the owner's rulings require one failure shape on
  relayed and rendered streams, which only a shared builder keeps equal.
- **Add `id` / `model` / `created` to `OpenAiChatSseState`.** Its fields are public and it derives `Default`
  (codec:sse.rs:36-53): a new field breaks any consumer that builds it with a struct literal, and an identity-less
  default is what `AnthropicSseState` deliberately refuses to have (codec:anthropic_sse.rs:13-18).
- **The codec mints the id, reads the clock, or decides the replay leg.** The first two are against the crate's
  rule (codec:lib.rs:22-27); the third needs the provider type and the routing chain, which R2 left to the host.
- **Port the host's burst function as a second renderer.** "Burst equals the expanded stream" would be a test to
  maintain rather than a definition, and the missing `index` would be ported with it.
- **Render only the first choice of a multi-choice burst**, as the host does today. It hands the client a
  plausible answer with choices missing.
- **Classify only the list the parser reads** (this record's first draft). A leg that forwards the client's body
  sees both lists.
- **A stateless failure event with only a sequence number** (this record's first draft). It cannot build the
  `response` object a `response.failed` event carries.
- **Count text inside the render states instead of re-reading forwarded frames.** It would miss frames the codec
  did not render and count frames rendered but never forwarded (server:…/sender/checkpoint.rs:8-10).

## 12. Open questions

S = south maintainers, L = lv, C = community host.

On 2026-09-30 lv ruled on the L-tagged questions as recommended; the rulings are recorded under each question. On
2026-10-01 lv extended the rulings under N-Q3 (relayed upstream failure frames) and N-Q4 (every Chat stream). The
halves tagged S or C remain open.

- **N-Q1 (L)** Which `model` string do Chat chunks carry: the upstream model id (today, at every site) or the name
  the client requested? The codec renders what it is given. Recommended: no change in this migration.
  **Ruled (lv, 2026-09-30): no change in this migration.**
- **N-Q2 (L, C)** Should `OpenAiChatStream` fold early `Usage` events instead of emitting an empty-`choices` chunk
  mid-stream (§3)? Recommended: yes — fold until the terminal chunk; honouring `stream_options.include_usage` is a
  later product decision. The old function keeps its behaviour.
  **Ruled for the closed host (lv, 2026-09-30): fold.**
  Note (2026-10-01): §3 now states where folded usage goes when no `Usage` follows the `Finish`, and when a stream
  with no `Finish` still gets one usage frame. This specifies the ruling; it does not extend it.
- **N-Q3 (L, S)** Gateway-originated failures on the Responses surface: (a) `response.failed`, the codec's shape,
  or a top-level `error` event — and if the latter, nested as the host writes it today or flat (the public API
  reference was not checked for this record); (b) on every provider's stream, or only Codex as today; (c) a fixed
  public message, or the internal reason as today. Recommended: `response.failed`, every provider, fixed message.
  **Ruled for the closed host (lv, 2026-09-30): `response.failed`, on every provider's stream, with a fixed public
  message.** This also removes a per-provider branch the host has today (recovery only for one provider).
  Note (2026-10-01): review proposed extending the ruling to the upstream's own failure frames on relayed streams
  (§5.2; the Responses record's revision made the same proposal); that is ruled below. The 2026-09-30 part already
  required `responses_error_event`.
  **Ruled for the closed host (lv, 2026-10-01): relayed upstream failure frames are covered by N-Q3** — such a
  frame is replaced by the host's fixed-message failure event, not forwarded (§5.2; the Responses record's R-Q1).
- **N-Q4 (L, C)** Should the first delta of each choice carry `role: "assistant"`? The burst does today, streamed
  chunks do not, and G4 needs one rule. Recommended: yes, in the new wrapper only.
  **Ruled for the closed host (lv, 2026-09-30): yes, in the new wrapper only.**
  Note (2026-10-01): G1 moves every Chat stream onto the wrapper, so the ruling as worded puts the role on every Chat
  stream, not only the burst. Review recommended accepting that (one rule for all Chat streams); that is ruled
  below, and the context-flag alternative is dropped (§3, §9).
  **Ruled for the closed host (lv, 2026-10-01): `role: "assistant"` on the first delta of each choice applies to
  every Chat stream rendered through the new wrapper**, not only the burst.
- **N-Q5 (S)** Is JSON-value equality over the canonical frame sequence (D7, §8) enough, or should the codec own key
  order — typed serializers for the new frames, or enabling `preserve_order` itself? Recommended: JSON-value
  equality. **Must be ruled before G1 ships** (§9 order 2).
- **N-Q6 (S, C)** Tighten `chat_request_from_responses` to refuse unknown `tsr.` versions and mixed carriers (§4)?
  Will the community half of the P15 R3 carrier work take `classify_reasoning_replay` from the codec?
- **N-Q7 (S, with the Responses record)** Is the Responses record's pass-through delivery (`north_passthrough`, its
  §8.2) accepted? **The host side is ruled** (lv, 2026-09-30, that record's R-Q1: pass-through by declaration); the
  south maintainers' half remains open. `responses_error_event` is required either way, because today's Native
  pass-through leg already relays upstream bytes with no render state (§5.2).
- **N-Q8 (C)** Does the community host put identity fields on Chat chunks today, and does it enable `preserve_order`?
- **N-Q9 (C, L)** Does the community host have, or plan, a consumer for `forwarded_text`? Recommended: G5 stays in
  the host until one appears.
  **Ruled for the closed host (lv, 2026-09-30): G5 stays in the host until a second consumer appears.**
  Note (2026-10-01): ruling kept. The count does not become billing evidence for `absent` families: the Kiro
  record's K-Q19 (ruled 2026-10-01) counts output as produced, over the IR events, so the forwarded count stays
  checkpoint and manual-review evidence only and G5 stays in the host (§7); N-Q10 turns the second-consumer question
  into a concrete one.
- **N-Q10 (C, S)** Does the community host keep a count of forwarded text as checkpoint or manual-review evidence
  (for example for a `usage_evidence: absent` family such as Kiro, per the Kiro record's K-Q15)? If so, that is the
  second consumer N-Q9 waits for. Recommended: then ship G5, including the non-streaming counterpart. It is not a
  billing question: the Kiro record's K-Q19 (ruled 2026-10-01) settles `absent` families from the IR events produced,
  not from forwarded text.

## Revision note (2026-10-01)

- Header: host baseline moved to `a82c852b`; the leaf's `translate_responses.rs` citations shifted by nine lines.
- §1, §2: equality restated as JSON-value over a canonical frame sequence (D7); G3 described as adoption plus a
  host refactor; G3 is no longer optional; G5's possible billing role noted.
- §3 G1: seventh stamping site (Kiro end-of-stream tail, sender.rs:4961) and fifth probe added; `Terminal` defined
  by the producing event, null reason included; fold rule and the `Usage` frame specified; overflow is an error;
  the two client-visible changes (role on every stream, as proposed; reasoning detail in the terminal usage) stated.
- §4 G2: classification scans the union of `input` and `messages`; the raw-forwarding case explained; new refusal
  across lists.
- §5 G3: owner rulings (N-Q3, the Responses record's R-Q1) applied; replacing relayed upstream failure frames with
  the host's fixed-message event proposed; `responses_error_event` takes the relayed identity and the next sequence
  number and is required; the generator refactor, the park exits and the host test specified.
- §6 G4: more than one choice is `Unrenderable` (multi-choice row dropped); render-then-settle kept and stated; the
  interim input's losses listed.
- §7 G5: the count's possible billing role for `absent` families noted (Kiro K-Q1, K-Q2); non-streaming counterpart
  and invariant exclusions added.
- §8: fixture rows revised; canonical frame sequence defined; N-Q5 must be ruled before G1.
- §9: G3 step mandatory; intentional differences and retired code updated (seven sites, five probes, recovery
  wrapper).
- §10, §11: release grouping updated; three rejected alternatives added.
- §12: N-Q2, N-Q3, N-Q4, N-Q7, N-Q9 annotated (rulings kept); N-Q10 added.
- Consistency round, §10: the Responses record's codec dependencies are NC-1 and NC-2 only; NC-3 is withdrawn there.
- Consistency round, §5.3: `decode_sse_v1` cited to boundary record §5.2 (its home is `south-contracts`); the host's
  frame splitting on a pass-through stream bounded as boundary record §15 states it.
- Consistency round, §2 D6, §7, §9, §12 N-Q9 and N-Q10: whether the forwarded count becomes billing evidence depends
  on the Kiro record's K-Q19 (produced or delivered output), not yet ruled; the citation is the Kiro record's
  Appendix B.1 item 2.
- Consistency round, §2 D2 and D4, §3, §5.2, §9, §12: extending N-Q3 to relayed upstream failure frames and N-Q4 to
  every Chat stream are presented as proposals awaiting the owner; the text added to those rulings, and to N-Q2 and
  N-Q9, moved out of the "Ruled" sentences into dated notes.
- Rulings of 2026-10-01:
  - N-Q3 extension ruled by lv: relayed upstream failure frames are covered by N-Q3 and replaced by the host's
    fixed-message failure event (the same ruling as the Responses record's R-Q1 extension). §2 D4, §5.2, §9 and the
    N-Q3 note state it.
  - N-Q4 extension ruled by lv: `role: "assistant"` on the first delta of each choice applies to every Chat stream
    rendered through the new wrapper, not only the burst; the context-flag alternative is dropped. §2 D2, §3, §9 and
    the N-Q4 note state it.
  - Kiro K-Q19 ruled by lv (output counted as produced IR events): the forwarded-text count is not the settlement
    meter for `usage_evidence: absent` families; it stays checkpoint and manual-review evidence only, and G5 stays in
    the host (N-Q9). §2 D6, §7 (billing paragraph, the fixture-review sentence, scheduling), the §9 G5 row, §10's
    release grouping, the N-Q9 note and N-Q10 state it; "not yet ruled" is dropped.
