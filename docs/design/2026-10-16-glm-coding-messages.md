# GLM Coding Plan Messages clients on a `glm-coding` row (host Q-T9)

Status: ruled. Corrected on 2026-10-16 from the host's read-back (§4, R5, R6, R11, S6). lv ruled §6 on 2026-10-16 as recommended (relayed by the host session): Q-GM1 = O1, Q-GM2 = (a),
Q-GM3 = register the remainder. The §4 fix is a separate pull request (#250); A+ follows in `provider-openai-compatible`.

Date: 2026-10-16

Baseline: south `origin/main` = `c8127a3`. Host references carry a `server:` prefix and refer to token-station-server
`origin/dev-v2` at `06dbbf80`; `server:…/` abbreviates `server:gateway/src/modules/inference/engine/`. Statements marked
**unverified** are conclusions from reading code, not from running anything against Z.ai or capturing traffic.

Predecessor: [`2026-10-15-openai-compatible-t9.md`](2026-10-15-openai-compatible-t9.md) (the `glm-coding` family; this
record expands its D1, T9-Q5 and the deferred option A+ of §10).

## 1. Why this record exists

lv ruled Q-T9 on 2026-10-15: GLM Coding Plan clients of `/v1/messages` stay on the native arm until the dual run accepts
the IR path. The host's T9 dual run (`server:gateway/tests/component_t9_dual_run.rs`,
`glm_messages_native_passthrough_and_component_ir_path_are_recorded`) cannot accept or refuse that path by itself: the
two arms send different wires to different Z.ai endpoints, so the case only records that both answer and both settle.
Until Q-T9 is decided, the native `glm_coding` arm cannot be deleted and the provider type cannot retire (host S7). The
host asked south (2026-10-16) to write down the difference surface and the options as questions for lv.

## 2. The two paths

**Native** (`server:…/text_admission/messages.rs:350-358`, `server:…/text_admission.rs:945-948`,
`server:…/upstream.rs:440-445`, `server:…/upstream.rs:153-160`, `server:…/text_admission/sender/messages.rs:387-419` and
`:1116-1118`): the client's Messages body, with only `model` replaced, is posted to
`{endpoint}/api/anthropic/v1/messages` with `Authorization: Bearer <api_key>` and `User-Agent: opencode/1.15.6`. The
client's `anthropic-version` and `anthropic-beta` are forwarded when sent; nothing else is added. The response body,
and every stream frame except a duplicated `message_stop`, reach the client as Z.ai sent them. Usage is read from the
Anthropic Messages wire.

**Component** (`glm-coding` family of `provider-openai-compatible` 2.8.0): the north codec parses the Messages body into
the canonical IR (`crates/south-north-codec/src/anthropic.rs`), the package builds a Chat Completions body for
`{endpoint}/api/coding/paas/v4/chat/completions` (`crates/south-component-conformance/src/reference.rs`), parses the
Chat response into the IR, and the host renders a Messages response or stream through the north codec
(`crates/south-north-codec/src/anthropic_sse.rs` for streams). Same Bearer and user-agent. The client's
`anthropic-*` headers are not forwarded (component rows never forward them, `server:…/text_admission/sender/messages.rs:560-563`).

## 3. Difference surface (component against native)

Native delivers whatever Z.ai's Anthropic-compatible endpoint does with each field; what that is, is **unverified**
for every row below. "Dropped" means silently: no error reaches the client.

### 3.1 Request

| # | Client field | Component path | Note |
|---|---|---|---|
| R1 | `thinking` `{type, budget_tokens}` | Parsed into `reasoning.mode` / `reasoning.budget_tokens` (`anthropic.rs:190-226`); `glm-coding` renders neither, and the codec consumed the key, so 2.8.0's forwarding of `extensions["thinking"]` (`reference.rs:116-147`, `forward_client_fields`) never sees it. **Dropped**: GLM runs its default thinking mode. A `type` outside the codec's set is a 400 | Option A+ (§5) |
| R2 | `cache_control` markers (system, content, tools, tool results, top level) | Validated (an invalid marker is a 400) and typed in the IR, then **dropped**: Chat has no marker | Whether Z.ai's Anthropic endpoint honours them is unverified; the Chat endpoint's own caching still reports `cached_tokens` (R-9) |
| R3 | `system` block array | Joined into one string | Text kept |
| R4 | `temperature`, `top_p`, `max_tokens`, `stop_sequences` | Kept (`stop_sequences` → `stop`) | — |
| R5 | `top_k`, `metadata`, `service_tier`, unknown top-level keys (`service_tier`: the server host's admission answers 400 before either arm, `authorize.rs:414-417`) | **Dropped** (`top_k` is typed but not rendered; `metadata` is kept out of the IR; the rest go to `extensions`, which `glm-coding` forwards only for `thinking` / `enable_thinking`) | A key the IR reserves (`reasoning_effort`, `parallel_tool_calls`, …) sent on Messages is a 400 |
| R6 | `tool_choice` | `auto` → `auto`, `any` → `required`, `tool` → named function; `disable_parallel_tool_use` → `parallel_tool_calls` (unreachable on the server host: its admission accepts `auto` / `any` only with no other key, so a choice carrying `disable_parallel_tool_use` is a 400 before either arm). **`none` is lost**: the codec maps it to no choice (`anthropic.rs:572-586`), so the tools are sent and the model may call them | A south defect, not a GLM difference: §4 |
| R7 | Image blocks | base64 → data URL, url → url, as `image_url` | — |
| R8 | `document` blocks (PDF) | **400** (the Chat body has no carrier) | Native passes them to Z.ai; whether Z.ai accepts them is unverified |
| R9 | `tool_result` | String or text array flattened to a string; **non-text inner blocks and `is_error` dropped** | — |
| R10 | History `thinking` blocks | Kept in the IR with their signature; on the wire they become `reasoning_content` only if the model row declares `upstream_requires_reasoning_content` (otherwise dropped); the signature is always lost; `redacted_thinking` dropped | GLM's preserved-thinking behaviour on Chat needs the model flag; unverified |
| R11 | Server tools (`web_search` and the like) | Become client function tools: name kept, `type` lost | **Corrected (host read-back, 2026-10-16):** the server host's admission refuses typed server tools with 400 before either arm (`validate_anthropic_tools`), so native does not forward them and the difference is unreachable there |
| R12 | `anthropic-version`, `anthropic-beta` headers | Not sent | Same as every component row; `anthropic-beta` not forwarded is the claude-code precedent |

### 3.2 Response

| # | Item | Component path |
|---|---|---|
| S1 | GLM `reasoning_content` | A `thinking` block (stream: `thinking_delta`s) with **no signature** and no `signature_delta` |
| S2 | Usage | `input_tokens` = prompt minus cached; `cache_read_input_tokens` = `cached_tokens` when above 0; no `cache_creation_input_tokens` (GLM reports none); a missing or inconsistent `total_tokens` is a 502 (settlement evidence) |
| S3 | `stop_reason` | `stop` → `end_turn`, `length` → `max_tokens`, `tool_calls` → `tool_use`, `content_filter` → `content_filter`; any other GLM value (`sensitive`, `network_error`) travels verbatim, by design (`anthropic.rs:364-378`), so a Messages client can see a value outside Anthropic's vocabulary |
| S4 | `id`, `model` | Non-streaming: Z.ai's Chat `id` with a `msg_` prefix and Z.ai's `model`. Streaming: a host-synthesized id and the routed model |
| S5 | Stream shape | Synthesized frames; `message_start` carries input usage 0 (GLM reports usage at the end, the full usage is in `message_delta`); no `ping`; blocks ordered thinking, text, tools; an upstream error mid-stream becomes one generic `error` frame |
| S6 | Errors | Rendered by the host in its own error shape. **Narrowed (host read-back, 2026-10-16):** native wraps Z.ai's upstream errors in the host's error shape too, so neither arm delivers Z.ai's Anthropic error body; what differs is only what the host's shape carries from each upstream body |

Money: both arms settle from the upstream's own token report, so the amounts follow whatever each endpoint reports for
the same prompt. Whether Z.ai's two endpoints count the same prompt identically is **unverified**; on the subscription
the upstream cost is flat either way.

## 4. A defect found on the way: `tool_choice: {"type": "none"}`

`tool_choice_from_wire` in `crates/south-north-codec/src/anthropic.rs:572-586` has arms for `auto`, `any` and `tool`
and returns no choice for everything else, `none` included. The IR has `ToolChoice::None`, and the OpenAI Chat codec
fills it (`crates/south-north-codec/src/request.rs:225`). So a Messages client that forbids tool calls for a turn gets
its tools sent with no choice, which every upstream reads as "the model decides". This affects every component row a
Messages client reaches, not only `glm-coding`: `provider-anthropic` withholds both the tools and the choice when it
sees `ToolChoice::None` (`reference_anthropic.rs:461-468`, `tools_of`), but that branch never fires because the codec never
produces it.

**Correction (2026-10-16, host read-back).** The first version said native passes the client's choice through. On
the server host it never gets that far: text billing admission (`server:gateway/src/modules/inference/engine/token_counter/authorize.rs`
`validate_anthropic_tool_choice`, `:634-651`) accepts a Messages `tool_choice` only as `auto` or `any` with no other
key, or one named tool, and answers 400 for `none` before either arm runs. So the defect was unreachable there, and
the host was refusing a valid Anthropic request. lv ruled on 2026-10-16 that the host admits `none`; the codec fix
(#250) is what makes that admission correct. The defect stays real for any host that admits `none`.

It is a correctness defect, so it is fixed without a ruling, in a separate code change: the codec maps `none` to
`ToolChoice::None`, with codec and per-dialect tests. It changes `south-north-codec`, so hosts pick it up at the next
repin.

## 5. Options

| Option | What | Cost | Left over |
|---|---|---|---|
| **O1. Accept the IR path, with A+** | `glm-coding` renders `reasoning.mode` as GLM `thinking.type` (A+, §6 Q-GM2), plus the §4 fix. Messages clients move with the rest of the row | A minor of `provider-openai-compatible` and fixture rows; no contract or host change; the host turns its Q-T9 case into registered differences and drops preflight T9G4's hold | R2, R5, R8–R12 and S1–S6 as registered differences |
| O2. An Anthropic-wire family | A new `provider-anthropic` family (for example `glm-coding-anthropic`): `{endpoint}/api/anthropic/v1/messages`, Bearer `api_key`, user-agent `opencode/1.15.6`, subscription-quota family. Needs component code: `Family::of` admits three names today, Bearer is tied to the setup-token arm, and the path needs a family constant | A minor of `provider-anthropic`, conformance rows, a host repin. **One row serves one family**: Chat and Responses clients of such a row would go through the IR to the Anthropic wire, which is D1 in reverse, unless operators split the models across two rows | Thinking, cache markers, `top_k` and document blocks kept; still lost: `metadata.user_id` (becomes `end_user_hash`), `is_error`, history signatures (unless the model declares Claude replay), `service_tier` and unknown keys; `temperature` / `top_p` / `top_k` dropped when thinking is on; adds `anthropic-version: 2023-06-01`; the response is still rendered by the codec unless the family also declares `north_passthrough = messages` (allowed by gate ① today) and the host implements Messages delivery |
| O3. Messages pass-through | The host sends the client's own Messages body, as native does, and delivers Z.ai's bytes | A contract change (gate ① admits request pass-through only for a protocol the north codec does not speak, `manifest.rs:1553-1557`), a new conformance suite, a generic host executor for Messages request and response pass-through, plus a family that speaks the Anthropic wire (O2's work) | Closest to native; largest |
| O4. Keep native | GLM Messages stays on the native arm | None now | `glm_coding` cannot retire; S7 keeps the variant |

O1 follows the precedents lv set for the same shape of choice: Bedrock Mantle (Q-MT1, one row serves three surfaces on
the family's wire) and Vertex text (Q-VX1, Messages and Responses switch to the component's wire), with the wire
switch registered as a difference. O2 remains the fallback if the post-cutover checks show a loss that matters
(T9-Q5).

## 6. Questions for lv (ruled 2026-10-16, all as recommended)

- **Q-GM1. Which option.** Recommendation: **O1**. The losses that matter most to a coding client are the thinking
  switch (fixed by A+) and `tool_choice: none` (fixed by §4 for every row); the rest are fields GLM's Chat endpoint has
  no carrier for, and whether Z.ai's Anthropic endpoint does anything with them is unverified. Real-key checks after
  cutover (lv, 2026-10-15) cover the doubt; O2 is the fallback.
- **Q-GM2. A+ and `adaptive`.** With O1, `glm-coding` writes `thinking: {"type": "enabled" | "disabled"}` from
  `reasoning.mode`; `budget_tokens` has no GLM carrier and is dropped. `adaptive` has no GLM spelling:
  (a) **omit `thinking`**, so the upstream default decides (recommended: closest to "let the model decide");
  (b) send `enabled`; (c) refuse with 400. A Chat client's own `thinking` extension still wins (2.8.0); a Messages
  client never has one, because the codec consumes the key. With (a), a `-thinking` alias row (Q-SFX `keep_existing`
  extra) then injects `enabled` for an `adaptive` client, where native passes `adaptive` to Z.ai; registered as a
  difference.
- **Q-GM3. Register the remainder.** Under O1, register R2, R5, R8–R12 and S1–S6 as intentional differences, as the
  Mantle and Vertex precedents did. Recommendation: yes. R8 (`document` → 400) is the one a client sees as an error;
  the alternative is a host-side refusal before admission, which changes nothing for the client.

## 7. Follow-up (O1 ruled)

South: `provider-openai-compatible` minor (A+ in `glm-coding` only, fixtures for `enabled`, `disabled`, `adaptive` and
absent, plus a Messages-shaped fixture), and the §4 codec fix in the same cycle. Host (not south's work): repin; the T9
dual-run case becomes a registered-difference case for R and S above; preflight T9G4 stops holding Messages models on
native; the T9 cutover handbook drops the Q-T9 hold.
