# Gemini 3 tool-call signatures and parallel tool results

Status: accepted 2026-10-26 (lv). Implemented on branch `gemini3-tool-signatures`.

Date: 2026-10-26

Source: the `token-station-server` repository's 2026-10-26 plan for the Gemini 3 tool round trip (in its
`docs/plans/`), which moves into South the fix the server's production deploy branch `aws-deploy-v3` made in its host
(PR #175). The server rule is that vendor logic lives only in South, so the host-side fix is not carried forward.

## 1. Why

Gemini 3 attaches an opaque `thoughtSignature` to each `functionCall` part of a model turn. On the next turn the
upstream requires that signature back, unchanged, on the same `functionCall` part; without it the request fails with
400. The canonical IR's `ToolCall` and `StreamEvent::ToolCallDelta` had no place for it, so `gemini_text.rs` dropped
the signature on the way in and could not send it back on the way out. Thinking-text signatures were already carried
(`ContentPart::Thinking.signature`, `RedactedThinking`); call signatures were not.

A second defect sat next to it: the results of one parallel tool-call batch each became their own `user` turn, while
Gemini expects them in a single `user` turn.

## 2. Decisions (lv, 2026-10-26)

1. The IR carries the signature in a dedicated field: `ToolCall.signature` and `StreamEvent::ToolCallDelta.signature`,
   both `Option<String>`, omitted when `None`. Kernel protocol 0.7.0 (`canonical_ir` 5, `stream` 3).
2. The Chat Completions north wire carries it at `tool_calls[i].extra_content.google.thought_signature`, the position
   Google's OpenAI-compatibility documentation uses (and the one the `aws-deploy-v3` fix and its clients already use).
3. Only the Chat north surface carries it. Messages and Responses ignore the field for now.
4. No temporary host guard in the server.

## 3. What changes

### 3.1 `gemini_text.rs` (shared by `provider-gemini` and `provider-vertex`)

| Direction | Before | After |
|---|---|---|
| Response, non-streaming (`parse_response`) | A `functionCall` part's `thoughtSignature` is dropped | Copied into that call's `ToolCall.signature` |
| Response, streaming (`events_of`) | Same | Copied into that call's `ToolCallDelta.signature` |
| Request (`conversation_of`), assistant tool call | `functionCall` part without a signature | When the IR call has a signature, it is written back on the same `functionCall` part (`{"functionCall": …, "thoughtSignature": …}`), never as a separate part |
| Request, consecutive tool results | One `user` turn per result | A result is appended to the previous turn when that turn is a `user` turn made only of `functionResponse` parts; otherwise it opens a new turn |

A call that arrived without a signature stays without one: nothing is synthesized, and no call borrows another's (in a
parallel batch only the first call is signed). The merge rule leaves a tool result that follows any other content —
text, an image, a signature part — in a turn of its own.

### 3.2 `south-north-codec` (Chat Completions)

- Rendering, non-streaming (`openai_chat_response`) and streaming (`openai_chat_frames`): a call with a signature gets
  `"extra_content": {"google": {"thought_signature": <signature>}}`; a call without one gets no `extra_content`.
- Decoding (`chat_request_from_openai_chat`): an assistant `tool_calls[j].extra_content.google.thought_signature`
  becomes that call's `ToolCall.signature`.
- New `CodecError::ToolCallSignatureInvalid { field }` (stable code `tool_call_signature_invalid`), added rather than
  repurposing an existing variant.

The codec is linked by hosts, not by guests, so it does not change any package's identity by itself.

## 4. Refusals and degradations

| Case | Handling | Where |
|---|---|---|
| The upstream sends a `thoughtSignature` on a `functionCall` part that is not a non-empty string | `provider_protocol_error` 502: "the upstream sent a functionCall whose thoughtSignature is not a non-empty string" | Response parsing, both modes |
| A client sends back an assistant tool call with an empty IR signature | `capability` 400: "an assistant tool call carries an empty signature" | Request translation |
| A Chat client sends `extra_content.google.thought_signature` that is not a non-empty string | `CodecError::ToolCallSignatureInvalid`, field `messages[i].tool_calls[j].extra_content.google.thought_signature` | North decoding |
| A model other than Gemini 3 receives a signed history | The signature is replayed as is; the upstream decides | Upstream |
| Messages / Responses north surfaces | The signature is not rendered and not read (decision 3) | North codec |

## 5. Compatibility and identity

The kernel pin moves to protocol 0.7.0. Every shipped package declares the kernel it is built against
(`compatibility.ir_schema_id`, `kernel_revision`), so every package takes a new identity in the release, as with the
protocol 0.6.0 move. Only `provider-gemini` and `provider-vertex` change behavior.

## 6. Verification

- Fixture packs: `fixtures-gemini/` gains parallel replay with result merge, sequential multi-step replay, no merge
  across plain text, an empty client signature (refused), a parallel response where only the first call is signed, a
  non-string upstream signature (refused), and a streamed signature on its delta. `fixtures-vertex/` gains the
  parallel replay and the parallel response. Gate ② runs the reference and the sandboxed package over the same packs.
- `south-north-codec`: rendering (both modes), decoding, and refusal of empty and non-string signatures.
- Mutation: with signature capture disabled, the response and stream fixtures fail.

## 7. Not verified

- Whether Gemini 3 ever streams a signature on a part separate from its `functionCall`. A developer-forum report says
  the signature can arrive before the call's `tool_calls` entry in OpenAI-compatible streaming; that is the
  OpenAI-compatible endpoint, not `streamGenerateContent`. No capture of a real Gemini 3 / Vertex parallel tool-call
  stream has been taken yet; one should be taken before relying on the streaming path in production.
- Real-upstream verification (Gemini 3 with parallel tool calls) is not part of this change.
