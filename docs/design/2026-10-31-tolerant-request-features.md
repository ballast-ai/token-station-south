# Request features for routing: one tolerant north-codec entry

Status: **accepted** 2026-11-02 (lv: FQ-1 to FQ-4 in §6, with the review changes of pull request #319 applied here).
Requested by lv for the server's architecture boundary plan, Task 6. No code yet.

Date: 2026-10-31; amended 2026-11-02 after review.

Baseline: south `origin/main` = `92124cfc`; the review read `3bbcb8ae`. Host references carry a `server:` prefix and
refer to token-station-server `dev-v2` = `23150901e`; the review read `0fb9589a5`. Statements marked **unverified**
come from reading code, not from running anything.

## 1. Problem

Before a text request is admitted, the server's automatic routing reads features of the northbound body: the
prompt text for a token estimate, the number of tools, the response format, whether it streams. Its rules match on
them, and the kernel's `RequestFeatures::extract` consumes them from a canonical `ChatRequest`.

The server builds that `ChatRequest` with a wire parser of its own,
`server:gateway/src/modules/routing/domain.rs` `canonical_chat_for_routing`, which carries north-protocol vocabulary
the host should not own: role names, Anthropic's top-level `system`, the Responses `input` and `instructions`, the
Responses tool kinds. The server's boundary plan (Task 6) wants to delete it and read the features through the north
codec instead. It cannot use the codec's strict entries as they are:

- `chat_request_from_openai_chat`, `chat_request_from_anthropic_messages` and `chat_request_from_responses` refuse
  bodies that the host's routing parser still reads. An unknown role is one (`request.rs` refuses `messages[].role`
  outside the five Chat roles); a content part the IR does not model and a tool without `parameters` are likely
  others (**unverified**; the corpus in §4 settles which). Routing runs before admission; a body routing cannot read must make routing **abstain**, never turn
  a forwardable request into a new 400. Admission keeps its own strict checks.
- A failed strict parse gives no features at all, so the router would lose the token estimate and the tool count on
  exactly the bodies where it has nothing else.

Not every refused body is forwarded. The server's Chat path always runs the strict entry and answers a refusal with a
400 (`server:…/engine/text_admission/chat.rs`, `canonical_to_ir`), and Responses does the same on the request side
(`server:…/text_admission/sender/responses.rs`, `responses_request_ir`, ahead of the response-delivery choice; only
the response is passed through). Two kinds of body are really forwarded after a strict refusal:

- bodies on Messages request passthrough rows, which exist to carry blocks and top-level keys the IR does not model
  (`server:…/engine/text_admission.rs`, `messages_passthrough_needs`);
- any body the WASM chain repairs after routing (all three handlers call `decide` before `apply_wasm_chain`).

So the tolerant branch is essential for Messages. For Chat and Responses it serves only bodies a WASM chain repairs.
It covers all three kinds for uniformity and for parity with today's parser; if the Chat or Responses reader grows
awkward, abstaining there is a legitimate simplification.

Copying a tolerant parser into the server is ruled out (boundary plan, "do not copy South's implementation to get
around a missing interface"). This record adds the interface.

## 2. What the routing layer reads

Transcribed from `canonical_chat_for_routing` (**unverified** beyond reading it):

| Field | Chat | Messages | Responses |
|---|---|---|---|
| System text | `messages[]` with role `system` / `developer` | top-level `system`: string or text blocks | `instructions`, and `input[]` items with role `system` / `developer` |
| Turns | `messages[]`: role `assistant`, `tool`, anything else `user`; `content` read as IR `Content`, dropped when it does not parse | same as Chat | `input`: a string (one user turn) or items with `role` and text `content`; read only when no non-system turn came from `messages` |
| Tools | `tools[]`: the `{type, function: {...}}` wrapper or the flat IR shape; an entry without `name` is skipped | same | as Chat, plus `namespace` (each named child counts) and `tool_search` / `local_shell` (one tool each, no name; lv 2026-10-22) |
| Response format | `response_format` as IR `ResponseFormat`, dropped when it does not parse | same | same |
| Stream | `stream` (bool, default false) | same | same |

The parser never fails; its failure mode is "one feature fewer". A missing feature can still change the route, and it
goes missing on ordinary valid bodies, not only on malformed ones:

| Feature | What it drives | What today's parser does |
|---|---|---|
| `estimated_input_tokens` | The context-window check (router-core `route.rs`, `effective_window`) and the token thresholds of user rules (`server:gateway/src/modules/routing/rules.rs`) | Leaves `tool_calls` empty and adds no tokens for Anthropic `tool_use` / `tool_result` blocks, so a long agent session is under-counted and can go to a model whose window is too small |
| `has_images` | The vision filter (`route.rs`) | Stays false for Anthropic image blocks and Responses input images: neither is read as `ContentPart::ImageUrl` |
| `requested_max_output_tokens` | The complexity score, where `max_output_tokens_per_point` is above zero | Always `None`: the parser never sets `sampling` |
| `message_count` | The complexity score and user rules | Counts wire messages; the strict Anthropic entry restructures `tool_result` blocks, so the count can move |

Reading a valid body through the strict entry fixes these, and it therefore changes routing decisions on valid traffic
(§5, FQ-1). The tolerant branch keeps today's under-count; Messages passthrough bodies are where that applies, so the
estimate is not accurate there.

## 3. Proposal

A new module `south_north_codec::features`, host-only like the rest of the crate (no component links it; it is a dev
dependency of the conformance crate, so no package identity moves):

```rust
pub enum NorthRequestKindV1 {
    OpenAiChat,
    AnthropicMessages,
    /// The options the caller's dispatch path passes to `chat_request_from_responses`.
    OpenAiResponses(ResponsesRequestOptions),
}

pub struct FeatureRequestV1 {
    /// The request the features are read from. `model` is the one the caller passes.
    pub request: ChatRequest,
    /// `None` when the strict entry accepted the body; otherwise the strict refusal's stable code.
    pub tolerant_because: Option<&'static str>,
}

/// Never fails and never panics, whatever `body` is.
pub fn request_for_features_v1(kind: NorthRequestKindV1, model: &str, body: &Value) -> FeatureRequestV1;
```

1. **Strict first (FQ-1).** The entry runs the strict entry for `kind`. When it accepts the body, `request` is that
   result and `tolerant_because` is `None`. On a row that builds IR, with the same Responses options as dispatch (the
   server passes `allow_messages`, `allow_call_aliases` and `preserve_unknown_content`), routing then reads the IR the
   dispatch path builds from the same body. That does not hold on a Messages request passthrough row, which forwards
   the client body, or for a body the WASM chain rewrites after routing. The options are the caller's so the host's
   policy is not frozen inside South; with other options, bodies dispatch accepts would land on the tolerant branch and
   pollute the mismatch metric.
2. **Tolerant otherwise.** When the strict entry refuses, the entry reads the fields of §2 field by field, with the
   same fallbacks (unknown role is `user`, an unparseable `content` or `response_format` is dropped, a tool without a
   name is skipped), and sets `tolerant_because` to the refusal's `CodecError::stable_code()`. The host counts that
   code as the dual-run reason (Task 6 Step 2) without logging the prompt or the body.
3. **The Responses tool kinds keep today's counting (FQ-2):** `namespace` counts its named children,
   `tool_search` and `local_shell` count once. The codec converts each of them into one upstream tool
   (`responses/tools.rs`), so the count matches what is sent.
4. **Out of scope (FQ-4):** the Gemini `generateContent` body. The server's routing parser does not read `contents`
   today, and the `/v1beta` surface forwards bodies unchanged.

The strict entries are unchanged. `request_for_features_v1` is additive; the crate takes a minor at the next release.

## 4. Tests

In `south-north-codec`:

- a corpus per kind: every valid body gives `tolerant_because == None` and the strict entry's exact result;
- refused bodies that are really forwarded (§1), which the corpus centres on: Messages passthrough bodies with blocks
  and top-level keys the IR does not model, give `Some(code)` and the features §2 says;
- other malformed bodies a WASM chain could repair (unknown role, an unmodelled content part, a tool without
  `parameters`, an unknown Responses item type, `response_format` of the wrong shape), likewise;
- a property test over arbitrary JSON values for each kind: the entry returns and never panics.

## 5. Host adoption (server Task 6, for reference)

After the release the server pins it, adds a parity test that runs its corpus through both its parser and
`request_for_features_v1` and compares the features, runs both in production with a fixed mismatch reason metric,
switches routing to the codec entry with the same fail-open semantics, and deletes `canonical_chat_for_routing`.

The parity run will report real mismatches on valid bodies, because adopting strict first changes routing on valid
traffic (§2). Those are expected. On a valid body this entry returns the strict result by definition (§4) and cannot
fix anything, so the host registers those differences; only differences on the tolerant branch can be fixed in this
entry. The server compares both parsers on live traffic before it switches routing over.

## 6. Decisions

lv, 2026-11-02, as recommended; the review's changes are in the sections above.

| # | Question | Decision |
|---|---|---|
| FQ-1 | Strict first, or tolerant extraction only | Strict first, accepting that it changes routing on valid traffic (§2, §5). On a row that builds IR, routing then reads the dispatch IR, and the tolerant rules only matter where strict refuses |
| FQ-2 | Count the Responses-only tool kinds as the host does today | Yes; it matches what the codec sends upstream (lv 2026-10-22) |
| FQ-3 | Should the tolerant result say which parts it dropped | No; the refusal's stable code is enough for the host's mismatch metric, and a per-part report would invite logging content |
| FQ-4 | Include the Gemini `generateContent` body | No; routing does not read it today, and the routing decision is reached only from the Chat, Messages and Responses handlers. A later record can add a fourth kind |
