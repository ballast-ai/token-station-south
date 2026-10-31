# Request features for routing: one tolerant north-codec entry

Status: **proposed** 2026-10-31, awaiting review (FQ-1 to FQ-4 below). Requested by lv for the server's architecture
boundary plan, Task 6.

Date: 2026-10-31

Baseline: south `origin/main` = `92124cfc`. Host references carry a `server:` prefix and refer to
token-station-server `dev-v2` = `23150901e`. Statements marked **unverified** come from reading code, not from running
anything.

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

The parser never fails; its failure mode is "one feature fewer". The token estimate under-counts a dropped content
part, which does not change which route is correct.

## 3. Proposal

A new module `south_north_codec::features`, host-only like the rest of the crate (no component links it; it is a dev
dependency of the conformance crate, so no package identity moves):

```rust
pub enum NorthRequestKindV1 { OpenAiChat, AnthropicMessages, OpenAiResponses }

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
   result and `tolerant_because` is `None`: routing then sees the same IR the dispatch path builds from the same body.
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
- malformed but forwardable bodies (unknown role, an unmodelled content part, a tool without `parameters`, an
  unknown Responses item type, `response_format` of the wrong shape) give `Some(code)` and the features §2 says;
- a property test over arbitrary JSON values for each kind: the entry returns and never panics.

## 5. Host adoption (server Task 6, for reference)

After the release the server pins it, adds a parity test that runs its corpus through both its parser and
`request_for_features_v1` and compares the features, runs both in production with a fixed mismatch reason metric,
switches routing to the codec entry with the same fail-open semantics, and deletes `canonical_chat_for_routing`.
Differences the parity test finds are either fixed in this entry or registered by the host.

## 6. Open questions

| # | Question | Recommendation |
|---|---|---|
| FQ-1 | Strict first, or tolerant extraction only | Strict first: on a valid body routing reads exactly the dispatch IR, and the tolerant rules only matter where strict refuses |
| FQ-2 | Count the Responses-only tool kinds as the host does today | Yes; it matches what the codec sends upstream (lv 2026-10-22) |
| FQ-3 | Should the tolerant result say which parts it dropped | No; the refusal's stable code is enough for the host's mismatch metric, and a per-part report would invite logging content |
| FQ-4 | Include the Gemini `generateContent` body | No; routing does not read it today. A later record can add a fourth kind |
