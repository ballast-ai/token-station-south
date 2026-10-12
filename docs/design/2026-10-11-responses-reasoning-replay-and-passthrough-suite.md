# OpenAI reasoning replay through the IR (NC-1) and the pass-through delivery suite

Status: accepted (lv, 2026-10-12: NP-Q1 to NP-Q13 ruled as recommended, §8). The two changes it designs were approved
in principle by lv on 2026-10-11. Implemented on branch `responses-nc1-passthrough` (§9), merged as #183; released in 0.55.0
(`docs/design/2026-10-12-release-0.55.0.md`).

Date: 2026-10-11

Baseline: south `origin/main` = `85b94f1` (release 0.54.0). South line numbers refer to that commit. Host references
carry a `server:` prefix and refer to token-station-server `origin/dev-v2` as read on 2026-10-11 (`server:…/`
abbreviates `server:gateway/src/modules/inference/engine/`). Statements marked **unverified** are conclusions from
reading code or published documentation, not from running anything or capturing traffic.

Parent record: [`2026-09-30-openai-responses-upstream-component.md`](2026-09-30-openai-responses-upstream-component.md)
(accepted; below "the record"). This document designs two items that record left as follow-ups:

- **NC-1** (record §4.5, §9): the north codec keeps every OpenAI reasoning input item, in order and with its position,
  and `provider-openai-responses` replays the items for a model that declares `reasoning_replay.openai.v1`, adding
  `include: ["reasoning.encrypted_content"]` for it. Ruled in principle by R-Q8 (2026-10-10): "v1 does NC-1 only and
  relies on pass-through delivery for the response half; no kernel change".
- **The gate ③ suite `south.north-passthrough-delivery.v1`** (record §8.2, §12.4): what a host must prove before it
  forwards upstream bytes to a Responses client under `north_passthrough`. The record names the suite and lists its
  assertions; nothing of it exists in south yet.

Other predecessors: [`2026-09-28-responses-north-codec.md`](2026-09-28-responses-north-codec.md) (its "R2" section:
the bounded Claude replay carrier, whose markers this design reuses),
[`2026-09-30-north-codec-render-gaps.md`](2026-09-30-north-codec-render-gaps.md) (proposed, not accepted; its G3
`responses_error_event` is the failure frame the suite expects), [`2026-08-21-canonical-ir-inventory.md`](2026-08-21-canonical-ir-inventory.md)
(D5: `extensions` are data), [`2026-09-30-host-zero-vendor-boundary.md`](2026-09-30-host-zero-vendor-boundary.md)
(§13.12 / Q47 crate versions, §13.13 `south-host-grammars`).

**Why a new record rather than a section of the parent.** The parent is accepted and its §17 records an implemented
step; this change re-opens a D5 question (§8 NP-Q1), changes three packages' behaviour beyond the parent's scope
(§2.6), and adds a host suite in another crate. Every earlier change of comparable weight got its own dated record
(the north codec's validation record, the render-gaps record, the kernel re-pin). The parent gains only a dated
pointer in its revision note and under §9 NC-1, the way it recorded the 2026-10-10 amendments.

Host context (read-only): the host's R2 plan (`server:docs/product-review-v2/plans/2026-10-11-P21-B6-4-R2-*.md`, the
Responses host wiring step; its §5) lists two client-visible losses that keep OpenAI rows on the native leg: (1) the component sends no `include`, so the
upstream returns no `encrypted_content`; (2) a request replaying native-era opaque reasoning items is refused on a
component row with `reasoning_replay_target_mismatch`, because `leg_accepts` admits an unmarked OpenAI opaque value
only on the native leg (`server:…/text_admission/reasoning_replay.rs`, `leg_accepts`). §2 removes the cause of both;
the host changes that complete the removal are in §2.7 and §3.6.

## 1. What is wrong today

### 1.1 The codec keeps one reasoning item per assistant turn, and no position

`chat_request_from_responses` builds one IR `Message` per input item and then merges adjacent assistant messages
(crates/south-north-codec/src/responses/request.rs:44-56). A `reasoning` item becomes an assistant message whose
content is one `Thinking { signature: None }` part per summary or content part, plus two single-valued extensions,
`responses_reasoning_id` and `responses_reasoning_encrypted_content` (request.rs:578-621). `merge_extensions`
(request.rs:429-445) appends only `responses_reasoning_replay_items`, the Claude carrier list; every other key is
overwritten by the later item. So a turn of the form

```
reasoning(rs_1) · function_call(c1) · reasoning(rs_2) · message("done")
```

reaches the IR as one assistant message with the text, one tool call, both items' summary text as `Thinking` parts,
and the `id` and `encrypted_content` of `rs_2` only. The order of `rs_1` relative to `c1` is gone as well: tool calls
live in `Message.tool_calls`, outside the content, and adjacent assistant texts are joined with `\n` into one text
(request.rs:401-427).

The Claude carrier (`tsr.c1.`) does not have this problem because its layout travels inside the carrier
(`TextRef` / `ToolCallRef` blocks, materialised by `materialize_replay_message`, request.rs:447-518). An OpenAI
`encrypted_content` is opaque to south: it carries no layout of its own.

### 1.2 The component cannot replay, and never asks for encrypted reasoning

`provider-openai-responses` 1.0.0 drops `Thinking` parts from assistant turns (`assistant_text`,
crates/south-component-conformance/src/reference_openai_responses.rs:169-192), reads none of the reasoning
extensions, and never sends `include` (`body_of`, :305-377). Its conformance test pins that the two single-valued keys
do not change the body (crates/south-component-conformance/tests/openai_responses_conformance_v1.rs:250-277).

The upstream returns `encrypted_content` on a stateless request only when the request asks for it with
`include: ["reasoning.encrypted_content"]` (OpenAI's published semantics; **unverified** by capture). Without it a
client has nothing to replay on the next turn, whichever delivery path the host uses.

### 1.3 Nothing proves a host's pass-through

`north_passthrough: {"openai-responses": "responses"}` is declared and admitted by gate ① since 0.54.0. The record
makes the gate ③ suite a hard precondition of any host forwarding upstream bytes (§12.4); south has no such suite, so
the host renders every Responses answer through the codec (R2 plan §1, its out-of-scope table). Under re-rendering, `encrypted_content`
is lost in streams (no IR stream event carries it; R-Q8 left a kernel event for later) and in non-streaming answers
(NC-2 not done). So NC-1 alone fixes the request half; the response half needs pass-through, which needs the suite.

## 2. NC-1: OpenAI reasoning replay through the IR

### 2.1 Representation — no kernel change

R-Q8 rules out a kernel change. The only channel left is `Message.extensions`, and the IR already carries reasoning
replay there: the Claude carrier's two markers `reasoning_replay_protocol_family` and `reasoning_replay_block_layout`
(written by the codec, request.rs:516, 628-631; stream.rs:208-214; read by `reasoning_replay::validated_layout`,
crates/south-component-conformance/src/reasoning_replay.rs:13-80, and by the Anthropic, Bedrock Invoke and Converse
references). NC-1 reuses those two markers with a second family value and adds one key for the opaque items:

| Key (on an assistant `Message`) | Value |
|---|---|
| `reasoning_replay_protocol_family` | `"openai-encrypted-reasoning"` (new value; the existing value is `"claude-signed-thinking"`) |
| `responses_opaque_reasoning_items` (new key) | The run's replayable reasoning items, in input order. Each is the closed projection `{type: "reasoning", id, summary, encrypted_content}` plus `content` when the client sent a `content` array; every other key of the client's item is dropped (§2.2) |
| `reasoning_replay_block_layout` | The run's order: one entry per item, per text part and per tool call — `{"kind": "reasoning_item", "index": k}` (new kind; `k` indexes the items list), `{"kind": "content", "ordinal": n}` (an IR content part, as today), `{"kind": "tool_call", "call_id": …}` (as today) |

Validation rules for the `openai-encrypted-reasoning` family (a sibling of `validated_layout`, in the same module):

- the message is an assistant message; all three keys are present, or none;
- every item is referenced exactly once, every `Text` content part exactly once, every tool call exactly once;
- `content` entries may reference `Text` parts only. `Thinking { signature: None }` parts may be present and are not
  referenced: they are the summary text of the items, kept for targets and hosts that present reasoning text, and
  already contained in the items themselves. `RedactedThinking`, signed `Thinking`, `ImageUrl` and `Unknown` parts make
  the layout invalid;
- bounds mirror the Claude carrier's (`replay.rs:12-15`): at most 128 layout entries, at most 1 MiB per
  `encrypted_content`, at most 4 MiB of `encrypted_content` per message (NP-Q11).

Why the markers are reused rather than new keys: every reference that already reads them fails closed on an unknown
family. `validated_layout` returns `Err` for any family other than `claude-signed-thinking` (reasoning_replay.rs:19-21),
and the Anthropic, Invoke and Converse references turn that into a 400 capability error ("invalid reasoning replay
markers or layout", reference_anthropic.rs:214-215, 922-923). A message carrying OpenAI opaque reasoning therefore
cannot be silently relabelled as Claude material — the relabelling P15 A1 I13 and A2 §4 rule out — without any change
to those references. The two references that compare the family for equality only (OpenAI-compatible, reference.rs:747-754;
Gemini, reference_gemini.rs:587-592) would drop the items silently; §2.6 closes that.

**D5.** S0 D5 says a component must not behave on an `extensions` key; its 2026-10-08 amendment opens typed channels
(`ProviderConfig.declared`, `ChatRequest.host_values`) and keeps `extensions` as data. Neither typed channel fits:
`host_values` holds host-minted values of at most 4096 printable ASCII bytes, per request, from a closed vocabulary
(boundary record §13.8), while these are client-supplied, per-message, ordered and up to megabytes. The Claude replay
markers are the standing exception this design extends: components already behave on
`reasoning_replay_protocol_family` and `reasoning_replay_block_layout` (they gate and shape the Anthropic body). NC-1
adds one key and one family value under the same exception, gated on a declared word exactly as the Claude carrier is.
R-Q15 ruled "no new key" for step R1; NC-1 is the step R-Q8 ruled for afterwards, and it cannot be done without a new
key or a kernel change. NP-Q1 asks lv to confirm the exception; promotion to a typed IR field joins R-Q15's kernel-chain
change.

### 2.2 The codec change

`reasoning_input` (request.rs:578-635) classifies a reasoning item:

| Item | Today | NC-1 |
|---|---|---|
| `encrypted_content` starts with `tsr.` | Claude carrier path (`tsr.c1.` only) | unchanged |
| `encrypted_content` a non-empty string without the marker | two single-valued keys, overwritten by later items | **replayable**: projected into a one-element items list, with a one-entry layout `[{"kind": "reasoning_item", "index": 0}]` and the family marker; `id` must be a non-empty string (as for a carrier item, request.rs:626), `summary` must be an array (absent is read as `[]`) |
| no `encrypted_content`, `null`, or empty | two single-valued keys (the `id` only) | not replayable: its summary text stays as `Thinking` parts, as today; nothing else is kept (NP-Q2) |

Every per-item assistant message that the merge will combine carries a one-run layout of its own: a text message
`[{"kind": "content", "ordinal": 0}]`, a function call `[{"kind": "tool_call", "call_id": …}]`. Merging a run
concatenates layouts with offsets (item indexes by the items already in the run, content ordinals by the parts already
in the content) — so the merged layout states the input order by construction, whatever the run's shape.

- **Only runs that contain a replayable item change.** For such a run the merge keeps each assistant text as its own
  `Text` part instead of joining texts with `\n`, so the component can replay two message items as two. A run without
  a replayable item is merged exactly as today, and its layout bookkeeping is dropped; every existing codec fixture's
  IR stays byte-identical, which the codec's validation suite checks.
- **Mixed families refuse.** A request carrying both a `tsr.` carrier and an unmarked opaque value is refused with the
  codec's existing `reasoning_replay_invalid`, the same verdict the host already gives (`ReplayInvalid::MixedCarriers`,
  `server:…/text_admission/reasoning_replay.rs`). South never interprets one family as the other.
- **The two single-valued keys are no longer written** (NP-Q5). Nothing in south reads them; the host only removes them
  before sending to targets that cannot replay (`server:…/text_admission/sender/responses.rs`, around :2205-2211).
- **Constant.** The codec exports `OPENAI_REASONING_REPLAY_CAPABILITY = "reasoning_replay.openai.v1"` beside the
  existing `CLAUDE_REASONING_REPLAY_CAPABILITY` (replay.rs:16), so the host takes the word from south rather than
  copying it (the host's known-word list already imports the Claude constant from the codec).

The codec does not interpret `encrypted_content`, does not decode it, and does not log or echo it in errors (error
paths name the field only, as today).

### 2.3 The word `reasoning_replay.openai.v1`

| Question | Answer |
|---|---|
| Where it is declared | In the model's `supported_parameters` (`ProviderConfig.models[].supported_parameters`), set by the operator per model — the channel `reasoning_replay.claude.v1` and the `anthropic.*` words use. |
| Who publishes the spelling | South: the codec constant above (for hosts), and `responses_vocabulary` in the conformance crate (for components), with the R-Q13 agreement test asserting the two spellings are equal. |
| Who checks it, in south | The `provider-openai-responses` component, at build time (§2.4). |
| Who checks it, in the host | The host's target filter (`leg_accepts`): a request carrying unmarked OpenAI opaque reasoning may go to a component leg only when the target model declares the word (§2.7). The host's reserved-namespace check must learn the word, or a model declaring it fails to save (`server:gateway/src/infra/config/capabilities.rs`, `KNOWN_REPLAY_PARAMETERS`). |
| What it means | "This target is an OpenAI Responses reasoning model whose upstream accepts its own encrypted reasoning back on a stateless request, and returns it when asked." It says nothing about which provider; a third-party Responses upstream gets it only with its own captured fixture pack (R-Q16). |

### 2.4 The component change (`provider-openai-responses` 1.1.0)

In `build_http_request`, after the existing Claude-carrier refusal (reference_openai_responses.rs:1199-1216):

1. If any message carries the `openai-encrypted-reasoning` family and the target model does not declare
   `reasoning_replay.openai.v1`: capability error, 400, zero upstream calls ("reasoning replay requires the target model
   capability reasoning_replay.openai.v1"), mirroring the Claude refusal (reference_anthropic.rs:314-317).
2. If a message's markers fail validation (§2.1): capability error, as the Anthropic reference does for its layout.
3. An assistant message with valid markers is emitted **in layout order**: a `reasoning_item` entry becomes the stored
   projection verbatim (`type`, `id`, `summary`, `encrypted_content`, and `content` if present); a `content` entry
   becomes `{role: "assistant", content: [{type: "output_text", text}]}`; a `tool_call` entry becomes the
   `function_call` item of record §4.2. Unreferenced `Thinking` parts are not sent (record §4.5, unchanged).
   An assistant message without markers is emitted exactly as in 1.0.0.
4. If the target model declares the word, the body gains `include: ["reasoning.encrypted_content"]` — on every request
   to that model, with or without replay items in it, because the first turn is the one whose answer must carry the
   material (NP-Q3). A model that does not declare the word gets no `include`, as in 1.0.0.

What stays refused or unchanged:

- Claude carriers (record §4.5 row 1), `previous_response_id` and `store: true` (R-Q4: the body is still stateless,
  `store: false`), `input_file` and `file_id` (D10).
- Plain `Thinking` text without an item (record §4.5 row 2): still not sent.
- Reasoning items without `encrypted_content`: not sent (they cannot be resolved on a stateless request).
- The response side is unchanged: `encrypted_content` on output reasoning items is already tolerated and not carried
  into the IR (record §5, §6.1; output items are read by `output_item`, reference_openai_responses.rs:522-574, which
  ignores keys it does not use). Under re-rendering the client still does not receive it (record §8.3); under
  pass-through it receives the upstream's bytes.

**The input bound (record §7.4).** Replayed `encrypted_content` comes from the northbound request, so the request's
bytes still bound what is sent. Whether the upstream's input token count for replayed reasoning stays below the host's
bound computed from those bytes is **unverified**: if the ciphertext is compressed plaintext, a token count above the
ciphertext's length in bytes is conceivable, and the host's generic input check would then park honest responses. The
test plan (§7) requires a captured multi-turn replay exchange measuring it before any row declares the word.

### 2.5 Codex (`openai-codex`, step R3)

The Codex family is not built (R3); NC-1 applies to it unchanged when it is. Codex clients rely on replay — the
reason R-Q14 keeps Codex rows on the native leg "until NC-1". Read with §1.3, NC-1 is necessary but not sufficient:
Codex only streams (record §4.3), and a re-rendered stream cannot carry `encrypted_content` (record §8.3), so a Codex
client on a component row gets its reasoning material back only through pass-through delivery. The precondition for
cutting Codex rows over is therefore: NC-1 released, the host's pass-through verified under the suite of §3, and R3
itself (recipe executor, the R-Q5 measurement). The same holds for OpenAI rows whose clients replay reasoning (the
host's R2 plan §5 loss 1); OpenAI rows whose clients do not replay are unaffected by either.

Codex models are reasoning models, so their rows would declare the word as operator data; no Codex-specific rule is
added. The family's `immutable_body_paths` (record §3.4) do not include `include` (NP-Q6).

### 2.6 The other references

| Reference (package) | Today, on a message with the new family | Proposed |
|---|---|---|
| Anthropic, Bedrock Invoke, Converse, Converse-bearer | 400, "invalid reasoning replay markers or layout" (`validated_layout` rejects the family) | unchanged; a fixture row pins it |
| OpenAI-compatible (`provider-openai-compatible`) | items dropped silently (equality check on the Claude family only) | 400, "the OpenAI-compatible dialect cannot consume OpenAI reasoning replay" (NP-Q4) |
| Gemini (`provider-gemini`) | items dropped silently | 400, likewise (NP-Q4) |
| Embeddings, image, task packages | not provider-world chat; never see a `ChatRequest` | unchanged |

The host's target filter already keeps such requests away from these legs; the refusals are defence in depth, the
same symmetry the Claude carrier has ("OpenAI-compatible and Gemini refuse explicitly", north codec record, R2).

### 2.7 What the host does (high level, for its own plan)

1. Re-pin to the release carrying NC-1; add `reasoning_replay.openai.v1` to the reserved-namespace known words, from
   the codec constant.
2. `leg_accepts`: `NativeOpaque` is also accepted by `ReplayLeg::Component` when the target model declares the word —
   by declaration, never by provider type (DP0).
3. Where the host strips reasoning for targets that cannot replay, strip the three NC-1 keys instead of the two
   retired single-valued keys.
4. Operators declare the word on reasoning models of `openai-responses` rows after the capture of §7 item 6.

## 3. The gate ③ suite `south.north-passthrough-delivery.v1`

### 3.1 What it proves

Record §8.2 lets a host forward the upstream's own bytes to a Responses client when the family declares
`north_passthrough: "responses"`, the inbound surface is Responses and the request mapped losslessly. The suite proves,
from the outside, the host's wiring around that decision — the same role `south.eventstream-framing.v1` plays for
`stream_framing` (crates/south-provider-conformance/src/eventstream_framing.rs:1-17). It does not test a component;
gate ② and the component's own tests do that (record §17.1 "SSE").

It governs exactly the manifests that declare `north_passthrough` for some family. At 0.54.0 that is one:
`provider-openai-responses` (`{"openai-responses": "responses"}`); `openai-codex` joins in R3. A host that never
passes bytes through needs no claim, and renders always (record §8.2, "Two hosts").

### 3.2 Where it lives, and how it avoids the IR

`scripts/check-boundaries.sh` allows the kernel IR and the north codec only in `south-component-conformance` and
`south-north-codec` (`KERNEL_IR_CONSUMERS`, `CODEC_PACKAGE`; the codec edge is checked for every dependency kind), and
forbids any component-linked crate from depending on `south-host-grammars`. The suite needs the SSE decoder's frame
positions and must not need the IR. So:

- **Home:** `south-provider-conformance` (host-side, workspace-versioned; its other host suites live there), gaining a
  normal dependency on `south-host-grammars` (allowed: that crate has no dependencies and this one is not
  component-linked). No package changes because of the suite. NP-Q7.
- **No IR in the suite.** The suite scripts the component's answer per frame in its own vocabulary,
  `FrameVerdictV1`: `Events` (events, no terminal), `Done` (the terminal triple), `Error { code }`, `Trap` (the call
  fails), `Lag` (the double holds this frame and emits its events with the next call — the only observable form of a
  component that keeps a partial frame, §3.4). The host's harness maps each verdict to its own IR-level double of the
  component and runs its real pass-through executor around it.
- **Reference executor:** `south-testkit` gains a reference pass-through executor driven by the same verdicts, and the
  suite's self-test runs it plus one mutant per assertion (forwards before the call returns, releases the terminal
  before settlement, relays the failure frame, …), each of which must fail — the pattern of the other gate ③ suites.

### 3.3 The harness

```text
trait NorthPassthroughHarnessV1 {
    // Which delivery the host chooses for a setup, before dispatch.
    fn delivery(&self, setup: &PassthroughSetupV1) -> DeliveryModeV1;          // PassThrough | Rendered | RefusedBeforeDispatch
    // A streaming exchange: upstream chunks in, a timeline out.
    fn stream(&self, setup: &PassthroughSetupV1, chunks: &[&[u8]],
              script: &[FrameVerdictV1], settlement: SettlementScriptV1) -> PassthroughTimelineV1;
    // A non-streaming exchange.
    fn buffered(&self, setup: &PassthroughSetupV1, status: u16, body: &[u8],
                verdict: BufferedVerdictV1, settlement: SettlementScriptV1) -> PassthroughTimelineV1;
}
```

(A sketch for review; names and shapes are settled with the implementation.)

- `PassthroughSetupV1`: inbound surface (`chat_completions` / `responses` / `messages`), the family's
  `north_passthrough` value or none, the client's tool list (for the lossless test), and the host's fixed public
  failure message, which the harness declares so the suite can recognise it.
- `SettlementScriptV1`: whether persisting the settlement succeeds.
- `PassthroughTimelineV1`: the ordered steps the host took — `Fed(bytes)` (one `parse-stream-chunk` call),
  `Returned(i)`, `Forwarded(bytes)`, `SettlementPersisted`, `MarkedDeliveryUnknown`, `SettledAsFailure`, `Ended`. The
  order is the evidence: "forwarded only after its call returned", "terminal only after settlement" and "funds
  transition before the failure frame" are all statements about this order.

How a host proves conformance: implement the harness around the code between its transport and the client, run
`run_north_passthrough_conformance_v1`, and report the case count; south records
`host_capabilities.<host>.north_passthrough_delivery = {"status": "verified", "cases": N}` in `compatibility.json` at
the next release, as for the other suites (new key, `not_verified` for both hosts until then). The record's rule that a
host does not pass bytes through before this is `verified` for it (record §12.4) is unchanged.

### 3.4 Cases

Upstream bodies are suite constants, plus a copy of three stream rows of the R1 fixture pack whose equality with the
pack is pinned by a test that reads both files.

| Id | Drives | Asserts |
|---|---|---|
| `stream.forwarded-unchanged` | A complete stream with LF, CR and CRLF line ends, a BOM, comment lines and a multi-line `data`; whole, at every two-chunk split, and one byte per chunk | `Fed` slices are exactly the upstream bytes cut at `SseDecoderV1::position`; the concatenated `Forwarded` bytes equal the upstream bytes (modulo NP-Q9); identical outcome for every split |
| `stream.forward-after-return` | Same | Each frame's `Forwarded` follows its own `Returned`, never precedes it |
| `stream.terminal-withheld` | Same | The terminal frame's bytes appear only after `SettlementPersisted`; everything before it is forwarded before settlement |
| `stream.incomplete-terminal` | A stream ending in `response.incomplete` (`max_output_tokens`) | As above: `response.incomplete` is a terminal type |
| `stream.terminal-type-without-done` | Script returns `Events` for the `response.completed` frame | Nothing more forwarded; `MarkedDeliveryUnknown`, then exactly one failure frame (§3.5), then `Ended` |
| `stream.done-without-terminal-type` | Script returns `Done` for a `response.output_text.delta` frame | Same as above |
| `stream.lagging-component` | Script `Lag` on one frame mid-stream | The terminal frame's call does not return `Done`; parks as above. This is how a component that holds a partial frame fails closed at the host: the host cannot observe the component's buffer, so the property itself is pinned on the component side (record §17.1), and the two-signal terminal rule catches its consequence |
| `stream.upstream-failure-replaced` | An upstream `response.failed` frame carrying a marker string in its message; script `Error { rate_limit }` | The failure frame is not forwarded; the marker never appears in any `Forwarded` bytes; one host failure frame follows with `sequence_number` one past the last forwarded frame and the relayed `response.id` / `model` / `created_at`; `SettledAsFailure` |
| `stream.error-event-replaced` | An upstream `type: "error"` frame without `sequence_number` | Same |
| `stream.component-error` | Script `Error` on an ordinary frame | That frame is not forwarded; failure frame; parked or failed per the host's generic path, before the frame |
| `stream.component-trap` | Script `Trap` | Same as `stream.component-error` |
| `stream.failure-before-first-frame` | Script `Error` on the first frame | No upstream byte forwarded; the client sees the host's own `response.created` and `response.failed` (render-gaps record §5.3) |
| `stream.truncated` | The upstream ends after a non-terminal frame | No terminal released; failure frame; `MarkedDeliveryUnknown` |
| `stream.settlement-fails` | A clean stream; settlement persistence fails | The terminal frame is never forwarded; failure frame |
| `stream.terminal-at-end-of-input` | The terminal frame is not followed by a blank line | Per NP-Q10 |
| `request.lossy-tool-map-renders` | Setup with a namespaced client tool | `delivery` is `Rendered` or `RefusedBeforeDispatch`, never `PassThrough` |
| `request.undeclared-renders` | Family without `north_passthrough` | `Rendered` |
| `request.other-surface-renders` | Declared family, inbound Chat and Messages surfaces | `Rendered` |
| `buffered.forwarded-after-parse` | 2xx body, verdict `Ok` | Body forwarded byte for byte, after the parse returned and after `SettlementPersisted` (NP-Q12) |
| `buffered.parse-failure-not-relayed` | 2xx body with a marker, verdict `Error` | Marker never forwarded; the host's error |
| `buffered.non-2xx-not-relayed` | 429 body with a marker | Marker never forwarded; the host's error from `map-provider-error` |

Not covered, by design: condition 3 of record §8.2 (a host that needs the codec's render state renders, or runs the
codec beside the relay) depends on host features the suite cannot see; it stays a host-side obligation.

### 3.5 The failure frame

The suite checks the host's failure frame structurally: one SSE event of type `response.failed`, valid JSON, the fixed
message the harness declared, `sequence_number` one past the last forwarded frame's, the relayed identity, nothing
after it, and no upstream text anywhere in the forwarded bytes. It does not compare bytes with the codec: the suite
crate may not depend on the codec (§3.2), and payload equality with `responses_error_event` is the render-gaps record's
own host test (its §5.3). That function does not exist yet — the render-gaps record is proposed, not accepted — so
NP-Q8 asks to accept its G3 codec half and ship it with this change, since a host cannot adopt the suite without it.

### 3.6 Stream framing

The host cuts the upstream bytes with `SseDecoderV1` (`south-host-grammars`, 0.53.0; record §6 and §8.2 as amended
2026-10-10) and hands the component one whole frame per call; the component splits with its own
`sse_split`, whose agreement with the decoder (golden vectors, property test, "one event per cut frame") is pinned on
the component side (record §17.1). Two framing details the record does not settle:

- **Bytes that dispatch nothing** (comments, stray blank lines) between frames belong to the next frame's span, so they
  are forwarded with it. Host-originated keep-alive comments are another matter: NP-Q9.
- **A terminal frame ended by end of input**: `SseDecoderV1` dispatches it (its documented departure from the
  standard), but a standard client discards an event no blank line ended. NP-Q10.

What the host must do to adopt the suite, in short: implement the pass-through executor generically (keyed on
`north_passthrough[family] == inbound surface` and the lossless test, never on provider identity), cut frames with
`SseDecoderV1::position`, feed one frame per call, forward after a clean return, withhold the terminal frame on the
two signals until settlement, replace failures with its fixed-message `response.failed` after the funds transition,
implement the harness, and pass the suite under both billing forms.

## 4. What does not change

- No WIT change, no new world, no manifest field, no contract number (`compatibility.json` `schema_version` stays 7),
  no kernel pin change.
- Gate ① and gate ② semantics are unchanged; new fixture rows use existing forms (the request-refusal form of 0.54.0).
- No package's `south_runtime` moves: nothing a package relies on is new to the runtime (the word is data in
  `supported_parameters`).
- Record §8.3 stays true for re-rendered answers: no `encrypted_content`, upstream ids, annotations or `refusal` parts.

## 5. Versioning and release (Q47)

| Item | Change | Why |
|---|---|---|
| `south-component-conformance` | 0.52.0 → **0.53.0** (minor) | New public constants in `responses_vocabulary`, the family validator, changed reference behaviour (§2.4, §2.6), new fixtures |
| `south-provider-api` | unchanged | No manifest change |
| `south-contracts` | unchanged by this change | (the co-traveller below changes it) |
| Every package (20 at 0.54.0) | version bump, since the conformance crate enters every `component.wasm`; the digest-stability check fails a package that keeps its version | Q47 |
| `provider-openai-responses` | 1.0.0 → **1.1.0** | New behaviour (§2.4) |
| `provider-openai-compatible`, `provider-gemini` | **minor** | New refusal (§2.6), if NP-Q4 is ruled as recommended; patch otherwise |
| All other packages | patch | Unchanged behaviour |
| `south-north-codec` | workspace version | NC-1, the new constant, and (NP-Q8) `responses_error_event`; host-visible IR change for replay runs |
| `south-provider-conformance`, `south-testkit` | workspace version | The suite and its reference executor; no package moves |
| `compatibility.json` | `host_capabilities.*.north_passthrough_delivery: not_verified` | New suite |

Release: proposed to ride **0.55.0** together with the pending branch `image-azure-edit-immutable` (image-azure 1.0.2,
which changes `south-contracts` and `south-component-conformance`). Co-traveller only: within one release each changed
crate takes one version bump and each package one new version, whichever branch lands first; the release commit
reconciles the lockfiles (**unverified**: whether that branch already bumps crate versions — its diff at the time of
writing touches no `Cargo.toml`). `scripts/check_crate_versions.py --unreleased` and `scripts/check-declared-runtime.sh
--build` run before the release as usual.

**Breaking for hosts on re-pin** (to be restated in the release record): the codec no longer writes
`responses_reasoning_id` / `responses_reasoning_encrypted_content` and writes the NC-1 keys instead for replayable
runs; such runs keep assistant texts as separate parts. A host that re-pins without §2.7 still refuses opaque replay on
component legs (its filter is unchanged), so nothing new reaches a component until the host opts in.

## 6. Rejected alternatives

- **A family-tagged opaque reasoning event in the kernel stream contract** (R-Q8's other branch): ruled out for v1.
- **`ChatRequest.host_values` as the channel**: host-minted, per request, ≤ 4096 printable ASCII bytes (§2.1).
- **Folding positions into the existing `responses_reasoning_replay_items` key**: that key means "Claude carrier items"
  to the codec's renderer (`output.rs:38-40`, 101-122 decodes each as `tsr.c1.`); giving it a second meaning would make
  the renderer and the target filter guess.
- **A south-minted carrier around the OpenAI value** (`tsr.o1.…` with the layout inside, like `tsr.c1.`): it would hand
  clients a rewritten `encrypted_content`, which a client can then only replay through Token Station, and it needs
  pass-through to rewrite upstream bytes — exactly what pass-through must not do.
- **Sending `include` only when the client asked for it**: the codec drops `include` (record §4.6); carrying it would
  be one more `extensions` key, for a choice the model's declaration already makes (NP-Q3).
- **Putting the suite in `south-component-conformance` or the codec**: the first is component-linked and may not take
  `south-host-grammars`; the second is a pure mapping crate. A new IR-consuming crate would need a change to the
  boundary allowlist for no gain over abstract verdicts (§3.2).

## 7. Test plan (no code in this change)

Each item starts with a failing test, per CONTRIBUTING.

**North codec (`south-north-codec`)**

1. Red first: two replayable items in one run both survive, in order — fails today on the overwrite (request.rs:612-621).
2. Interleavings: `reasoning·function_call·reasoning·message`, `message·reasoning·message`, a run that is only a
   reasoning item, two runs separated by a tool output; the layout states the input order; property test over random
   interleavings (32 cases, as the codec's other property tests).
3. Every existing codec fixture's IR is byte-identical (runs without replayable items untouched).
4. Refusals: carrier plus opaque in one request; opaque item without `id`; non-array `summary`; each bound of §2.1;
   errors never echo the value.
5. Agreement test (R-Q13): codec `OPENAI_REASONING_REPLAY_CAPABILITY` equals the conformance vocabulary's spelling;
   the codec writes exactly the keys `responses_vocabulary` names.

**Component (gate ② rows in `fixtures-openai-responses/`)**

6. `request.reasoning-replay`, `request.reasoning-replay-interleaved`, `request.reasoning-replay-multi-turn`: items
   verbatim (closed projection) and in layout order; `include` present.
7. `request.reasoning-replay-include-only`: declared model, no items → `include` present, body otherwise as 1.0.0.
8. `request.refused-reasoning-replay-undeclared`, `request.refused-reasoning-replay-invalid-layout`: capability errors.
9. Every existing request row unchanged (models without the word): proven by the pack itself.
10. `response.reasoning-encrypted-content`, `stream.reasoning-encrypted-content`: output items carrying
    `encrypted_content` parse to the same IR as without it.
11. Round-trip judge (record §9): north parse ∘ south build reproduces the client's interleaved items (projection) in
    order.
12. `openai_responses_conformance_v1::only_the_three_precedent_extension_keys_change_the_body` is rewritten: the two
    retired keys still change nothing; the NC-1 keys change the body only with the word.
13. Sandbox parity on the frozen pack, as for every package.
14. Fixture rows `request.refused-openai-reasoning-replay` for the OpenAI-compatible and Gemini packs (NP-Q4), and one
    each for Anthropic and Converse pinning the existing refusal.
15. **Capture** (R-Q16 discipline): a recorded OpenAI multi-turn exchange with `include`, replaying the returned items
    statelessly, archived with the pack; it pins the upstream's acceptance of the projection and measures replayed
    input tokens against the request's bytes (§2.4). Until it exists, no row declares the word.

**Suite (`south-provider-conformance`, `south-testkit`)**

16. The cases of §3.4 against the testkit reference executor: all pass.
17. One mutant executor per assertion, each failing exactly the cases that name it.
18. Fixture-copy test: the suite's three Responses streams equal the R1 pack's rows.
19. `host_capability_evidence_freshness_v1` learns the new key.

Per the host's rules, nothing here may run longer than the local short-test window; the every-byte split cases use
streams small enough for that.

## 8. Open questions for lv

- **NP-Q1** (D5) Confirm that NC-1 extends the Claude replay exception: one new key
  (`responses_opaque_reasoning_items`) and one new value of `reasoning_replay_protocol_family`, which the Responses
  component behaves on, gated by `reasoning_replay.openai.v1`. **Recommended: yes.** R-Q8 rules out the kernel, and
  `host_values` does not fit; promotion to a typed field joins R-Q15's kernel-chain change.
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q2** Only items with a non-empty, unmarked `encrypted_content` are replayable; items without it stay summary
  text and are not sent. **Recommended: yes** — on a stateless request the upstream cannot resolve an item by `id`
  alone (**unverified** by capture; item 15 checks it).
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q3** `include: ["reasoning.encrypted_content"]` is sent on every request to a model declaring the word, not only
  when the client asked. **Recommended: yes** — the codec does not carry `include`, and the word already says the
  model is a replay target.
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q4** The OpenAI-compatible and Gemini references refuse a message carrying the OpenAI family (400) instead of
  dropping the items. **Recommended: yes** (symmetric with the Claude carrier; costs a minor bump of two packages).
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q5** The codec stops writing `responses_reasoning_id` / `responses_reasoning_encrypted_content`.
  **Recommended: yes** — nothing in south reads them, the host only strips them, and keeping them would keep the
  overwrite defect visible as data.
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q6** Declare `include` in `immutable_body_paths` so operator extras cannot add or strip it. **Recommended: not
  now** — the gate ② check `ImmutablePathsHonoured` requires a path to be in one state across all of a family's rows,
  and `include` depends on the model's word; revisit with a per-declaration form of the check if extras misuse it.
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q7** The suite lives in `south-provider-conformance`, scripts the component with abstract verdicts, and takes a
  normal dependency on `south-host-grammars`; the reference executor lives in `south-testkit`. **Recommended: yes**
  (no boundary-allowlist change, no package bump).
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q8** Accept the G3 codec half of the render-gaps record (`RelayedResponseIdentity`, `responses_error_event`) and
  ship it with this change. **Recommended: yes** — the suite's failure cases and any host adoption need the one
  failure shape N-Q3 ruled; the rest of that record stays as it is.
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q9** Host keep-alive comments under pass-through: allowed only at frame boundaries, ignored by the suite's
  byte comparison, or forbidden. **Recommended: allowed at frame boundaries only** — SSE clients ignore comments, and
  a reasoning model can be silent long enough for intermediaries to drop an idle connection (**unverified** whether
  the host's keep-alive layer runs on this path today).
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q10** A terminal frame the upstream ended without a blank line: the host appends the minimal terminator
  (`\n`, or `\n\n` when the frame's last line has no line end) when it releases it, or parks. **Recommended: append**,
  pinned by `stream.terminal-at-end-of-input` — the evidence is complete and settled, and appending framing changes no
  event; parking would put a complete answer into manual review for a framing slip.
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q11** Bounds for the OpenAI family: 128 layout entries, 1 MiB per `encrypted_content`, 4 MiB per message, the
  Claude carrier's numbers. **Recommended: yes**; a run is bounded by tool outputs, so long agent sessions do not
  accumulate into one message (**unverified** against real Codex item sizes; item 15 records them).
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q12** Non-streaming pass-through also withholds the body until the settlement is persisted (the record states
  only "after `parse-response` succeeded"). **Recommended: yes** — the same settlement-before-visibility guarantee as
  streams.
  **Ruled (lv, 2026-10-12): as recommended.**
- **NP-Q13** Ship NC-1 and the suite together in 0.55.0 with image-azure 1.0.2. **Recommended: yes** — the suite alone
  moves no package and could ship earlier, but neither removes a client-visible loss without the other (§1.3, §2.5).
  **Ruled (lv, 2026-10-12): as recommended.**

## 9. Implementation (2026-10-12)

Implemented on branch `responses-nc1-passthrough` after the rulings of §8; not released. Commits, in order: the
rulings; the north codec (NC-1 and the G3 half); the conformance crate (the 1.1.0 reference, the NP-Q4 refusals,
fixtures, Q47 bumps); the gate ③ suite with its `south-testkit` reference executor and `compatibility.json`; these
notes and the README / ARCHITECTURE paragraphs.

### 9.1 What landed

- **North codec.** `chat_request_from_responses` merges an assistant run only once the run is complete
  (`flush_assistant_run`): a run holding a replayable item becomes one message with the three NC-1 keys of §2.1; any
  other run merges exactly as before. `reasoning_input` keeps an unmarked, non-empty `encrypted_content` as the closed
  projection (`type`, `id`, `summary` — `[]` when absent — `encrypted_content`, and `content` when it is an array),
  refuses an unknown `tsr.` version and an opaque item without a non-empty `id`, and no longer writes the two
  single-valued keys (NP-Q5). A request carrying both families, and the NP-Q11 bounds, are `reasoning_replay_invalid`.
  New public items: `OPENAI_REASONING_REPLAY_CAPABILITY`, `OPENAI_REASONING_REPLAY_FAMILY`,
  `OPENAI_REASONING_ITEMS_EXTENSION`, and (NP-Q8) `RelayedResponseIdentity` and `responses_error_event`, built by the
  same function as the render state's failure frame.
- **Conformance crate (0.53.0).** `reasoning_replay::openai_layout` validates an OpenAI-family turn; the Responses
  reference (1.1.0) replays it for a declaring model, refuses it otherwise, and sends `include` to every declaring model;
  the OpenAI-compatible and Gemini references refuse the family (NP-Q4); `responses_vocabulary::replay` names every
  spelling. Fixtures: `request.reasoning-replay`, `-interleaved`, `-multi-turn`, `-include-only` (codec-derived, from
  `replay_client_bodies()`), `request.refused-reasoning-replay-undeclared`, `-invalid-layout`,
  `response.reasoning-encrypted-content`, `stream.reasoning-encrypted-content`, and
  `request.refused-openai-reasoning-replay` in the OpenAI-compatible, Gemini, Anthropic, Converse and Converse-bearer
  packs. Two existing tests learn that a refusal row is possible in a pack: the bearer-pack mirror test compares a
  refusal byte for byte (it carries no auth), and `undeclared_values_v1` no longer expects a refusal row to catch a
  component that reads a value after building.
- **Suite.** `south.north-passthrough-delivery.v1`, 22 cases (§3.4's 21 plus
  `DeliveryPassesThroughWhenEveryConditionHolds`, the positive delivery case the mutants need), in
  `south-provider-conformance`; the reference executor and `NorthPassthroughFaultV1` (eighteen mistakes) in
  `south-testkit`. `compatibility.json` gains `north_passthrough_delivery_suite(_id)` and
  `host_capabilities.*.north_passthrough_delivery: not_verified`.

### 9.2 Decisions taken where the record is silent

1. **The lagging component** (§3.4 `stream.lagging-component`) is scripted as every frame answering `Events` and
   `finish` answering `Done`: a component that held the terminal frame back emits its `Done` only at end of input, with
   no frame of its own. The suite therefore needs no `Lag` verdict; the four frame verdicts are `Events`, `Done`,
   `Error`, `Trap`, and `finish` answers `Clean`, `Done` or `Truncated`.
2. **The NP-Q10 terminator** keeps the frame's own line ending: nothing when the frame already ends with a blank line,
   `\n` after an LF- or CRLF-terminated last line, `\r` after a CR-terminated one (appending `\n` there would turn the
   CR into a CRLF and still leave no blank line), and `\n\n` when the last line has no ending
   (`north_passthrough_terminator_v1`).
3. **Comment lines a host adds** (NP-Q9) are tolerated only at frame boundaries: before each expected frame the suite
   skips `:` lines (each optionally followed by one blank line) the host wrote, and nothing else.
4. **Funds on a failure.** The suite accepts either `MarkedDeliveryUnknown` or `SettledAsFailure` as the funds
   transition, once, before the failure event; it requires no `SettlementPersisted` on a failure.
5. **A failure before any relayed frame** must show the host's own `response.created` and `response.failed`, with the
   same id and consecutive sequence numbers, the latter carrying the fixed message (render-gaps record §5.3).
6. **Non-streaming refusals** must forward something (the host's own error) and never the upstream body or its text.
7. **Bounds** count `encrypted_content` bytes only, as the Claude carrier counts its opaque fields.
8. **Two other references stay as they were**: Anthropic, Invoke and Converse already refuse the family through
   `validated_layout`; a fixture row in the Anthropic and Converse packs pins it.

### 9.3 Failing-first evidence

- `nc1_keeps_every_opaque_reasoning_item_of_a_run_in_order` failed before the codec change; the run kept only
  `{"responses_reasoning_encrypted_content":"opaque-two","responses_reasoning_id":"rs_2"}`.
- With the 1.1.0 replay and `include` disabled, `gate_two_passes_over_the_shipped_pack`,
  `every_request_is_stateless_whatever_the_ir_carries` and `the_replay_keys_change_the_body_only_for_a_declaring_model`
  fail; without the NP-Q4 checks, the `refused-openai-reasoning-replay` rows of the OpenAI-compatible and Gemini packs
  fail (the items were silently dropped).
- `responses_error_event` did not exist before its test (a compile failure).
- The suite's self-test: each of the eighteen mistakes fails exactly the cases pinned for it.

### 9.4 Versions (Q47)

| Item | Before | After |
|---|---|---|
| `south-component-conformance` | 0.52.0 | 0.53.0 |
| `south-contracts`, `south-provider-api` | 0.51.0, 0.52.0 | unchanged |
| `provider-openai-responses` | 1.0.0 | 1.1.0 |
| `provider-openai-compatible` | 2.4.6 | 2.5.0 |
| `provider-gemini` | 1.1.14 | 1.2.0 |
| `provider-anthropic` | 1.0.18 | 1.0.19 |
| `provider-anthropic-bedrock-invoke` | 1.0.2 | 1.0.3 |
| `provider-bedrock-converse` | 1.0.15 | 1.0.16 |
| `provider-bedrock-converse-bearer` | 1.0.7 | 1.0.8 |
| `task-kling` | 1.0.14 | 1.0.15 |
| `task-kling-v2` | 0.32.12 | 0.32.13 |
| `task-minimax-v2`, `task-bailian-v2` | 0.31.11 | 0.31.12 |
| `task-xai-v2`, `task-veo-v2`, `task-wan-image-v2`, `task-gmi-image-v2` | 0.35.11 | 0.35.12 |
| `task-byteplus-v2` | 0.36.11 | 0.36.12 |
| `embeddings-openai-compatible` | 1.0.5 | 1.0.6 |
| `embeddings-gemini` | 1.1.2 | 1.1.3 |
| `embeddings-vertex` | 1.0.4 | 1.0.5 |
| `image-azure` | 1.0.1 | 1.0.2 (the co-traveller `image-azure-edit-immutable` reuses this version) |

No package's `south_runtime` moves (`shipped_packages_v1::nc1_retires_every_published_054_package_identity`). The
workspace version is not touched; this is not a release.

## Revision note

- 2026-10-11: first draft (proposed).
- 2026-10-12: lv ruled NP-Q1 to NP-Q13 as recommended; the status is accepted and each question records the ruling.
- 2026-10-12: implemented (§9, new); the status line notes it.
- 2026-10-12: released in 0.55.0 with `image-azure` 1.0.2 (#184), which also moved `south-contracts` to 0.52.0; the
  status line says so.
