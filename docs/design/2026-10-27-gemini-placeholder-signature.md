# Gemini placeholder tool-call signatures on replay

Status: accepted 2026-10-27 (lv). Implemented on branch `gemini-placeholder-signature`. Not released.

Date: 2026-10-27

Source: `token-station-server` backlog item 03 #209 (plan two of three), found by the server's deployed end-to-end
journey J25 (tool-call round trip over the Chat, Responses and Messages surfaces) against a real Gemini 3 upstream.

Supersedes one sentence of [Gemini 3 tool-call signatures](2026-10-26-gemini-tool-call-signatures.md): §3.1's closing
paragraph, "A call that arrived without a signature stays without one: nothing is synthesized". Everything else in that
record stands.

## 1. Why

Gemini 3 rejects a replayed `functionCall` part that has no `thoughtSignature` (HTTP 400, "Function call is missing a
thought_signature in functionCall parts … position 2"). The 2026-10-26 change carries the signature in the IR and on
the Chat Completions north wire, so a Chat client that sends `tool_calls[i].extra_content.google.thought_signature`
back works.

The Messages and Responses north surfaces carry no signature (2026-10-26 decision 3). A client on either surface, for
example Claude Code on Messages or a Codex-style client on Responses, therefore replays every tool call unsigned and
the second turn fails on Gemini 3. The same happens to any Chat client that drops `extra_content`, and to a
conversation that began on another model.

## 2. Evidence

Measured on 2026-10-27 through the server's local deployment, over the **Chat** surface, **non-streaming**, against the
Gemini API (`gemini-3.8-flash`) and Vertex (`vertex_ai/gemini-2.5-flash`). One run per case. The replayed turn is the
first turn's tool call with its signature removed or replaced.

| Model | Replayed `functionCall` signature | Result |
|---|---|---|
| `gemini-3.8-flash` | removed | 400, "missing a thought_signature … position 2" |
| `gemini-3.8-flash` | `skip_thought_signature_validator` | 200, the answer used the tool result |
| `gemini-3.8-flash` | `context_engineering_is_the_way_to_go` | 200, the answer used the tool result |
| `gemini-3.8-flash` | `abc` (arbitrary string) | 400 |
| `vertex_ai/gemini-2.5-flash` | removed | 200 |
| `vertex_ai/gemini-2.5-flash` | `skip_thought_signature_validator` | 200 |

Two parallel calls in one step (`gemini-3.8-flash`; the upstream signed only the first):

| Replayed signatures | Result |
|---|---|
| none on either call | 400 |
| placeholder on the first call only | 200 |
| placeholder on both calls | 200 |
| the upstream's own signatures, unchanged | 200 |

So the upstream validates the shape of a signature, accepts Google's documented placeholder values, and does not need
one on every call of a parallel batch.

## 3. Decisions (lv, 2026-10-27)

1. When an assistant turn is replayed and **none** of its tool calls carries a signature, the translator writes the
   placeholder `skip_thought_signature_validator` on the **first** `functionCall` part of that turn. A turn in which any
   call already carries a signature is left exactly as it is, and no further call is given one.
2. The rule is the same for every model. Gemini 2.5 accepts the placeholder (§2), so there is no model-name check to
   keep current.
3. Messages and Responses stay as they are: they still carry no signature. This record is what makes them usable on
   Gemini 3.

## 4. What changes

`gemini_text.rs` (shared by `provider-gemini` and `provider-vertex`), request direction only.

| Case in a replayed assistant turn | Before | After |
|---|---|---|
| A call carries a signature | Written back on its own `functionCall` part | Unchanged |
| No call carries a signature | No `thoughtSignature` anywhere | `thoughtSignature: "skip_thought_signature_validator"` on the first `functionCall` part only |
| Several calls, only some signed | Signed ones keep theirs | Unchanged: nothing is added |
| A call carries an empty signature | Refused, 400 `capability` | Unchanged: refused, no placeholder |
| Several assistant turns (sequential steps) | Each turn replays its own signatures | Each turn is judged on its own: an unsigned step gets its own placeholder, a signed step is untouched |

Not changed: response parsing (both modes), the stream parser, the north codec, the IR, the kernel pin.

## 5. Refusals and degradations

| Case | Handling |
|---|---|
| A call arrives without a signature | A placeholder is sent in its place. This **bypasses the upstream's signature check**; it is the documented way to replay a call the model did not sign in this conversation (a history that came from another model, or from a client that cannot carry the field). |
| Model quality | Google's error text warns that a missing signature may degrade the model's performance. Whether the placeholder degrades it in the same way has **not been measured**. |
| An empty signature | Still refused (`capability`, 400): an empty string is an explicit invalid value, not an absent one. |
| A client sends a real but wrong signature | Replayed as is; the upstream decides (an arbitrary string is refused, §2). |
| A turn that is not an assistant turn | Untouched. |

## 6. Compatibility and identity

`gemini_text.rs` lives in `south-component-conformance`, one of the three crates every component links (boundary record
§13.12, §16 Q47). A compatible change to it is a patch: `south-component-conformance` 0.76.0 → 0.76.1. It changes every
`component.wasm`, so every package takes a new version:
- `provider-gemini` 1.6.0 → **1.7.0** and `provider-vertex` 1.3.0 → **1.4.0** (behaviour changed);
- every other package takes one patch with unchanged behaviour.

`south_runtime`, the kernel tuple and the workspace release version do not move: no manifest field or runtime meaning
is introduced. The release commit, the tag and the host re-pin are separate steps.

## 7. Verification

- Fixture packs. `fixtures-gemini/`: the three existing request cases whose assistant turn is unsigned now expect the
  placeholder (`a-tool-result-after-plain-text-is-not-merged`, `cache-markers-are-dropped-and-top-k-is-sent`,
  `tool-round-trip-keys-the-result-by-name`); new cases cover parallel unsigned calls (placeholder on the first only)
  and a signed step followed by an unsigned step (the second step gets its own). `fixtures-vertex/`: the two existing
  unsigned cases are updated and the parallel unsigned case is added. The existing cases for a real signature
  (`parallel-tool-results-share-one-user-turn`, `sequential-tool-calls-replay-their-own-signatures`) and for an empty
  signature (refused) must not change. Gate ② runs the reference and the sandboxed package over the same packs.
- Order: the fixtures are changed first and the conformance tests fail, then the implementation turns them green.
- Identity: `gemini_placeholder_retires_every_published_077_package_identity` pins the round.

## 8. Not verified

- Streaming and parallel calls were measured only through the Chat surface, non-streaming, on one Gemini 3 model and
  one Gemini 2.5 model. No Messages or Responses client was run against the new behaviour.
- The effect of a placeholder on answer quality (§5).
- Whether Google keeps accepting the placeholder values; they come from Google's documentation and the 2026-10-27
  measurement, not from a contract.
- Real-upstream verification of this change after release is a server-side step.
