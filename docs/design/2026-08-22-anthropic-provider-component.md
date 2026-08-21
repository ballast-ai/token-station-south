# The official Anthropic Messages provider component

**Date**: 2026-08-22
**Status**: proposed → landing with this change
**Related**: `2026-08-21-component-conformance.md` (gates ① and ②),
`2026-08-21-provider-api-promotion.md` (the v2 world),
`2026-08-21-openai-compat-host-parity.md` (the first component's parity slice)

---

## 1. Why this component, and why now

`provider-openai-compatible` is the only component south ships. It covers two
dialects (`openai-compatible`, `azure-openai-v1`) because they differ by an
auth arm and a base path, not by a wire shape.

Anthropic Messages is a genuinely different wire shape — a separate `system`
field, typed content blocks, `tool_use`/`tool_result` blocks, `thinking`
blocks with replay signatures, and an SSE dialect with block-scoped deltas.
It needs its own component.

It is also **the only remaining dialect whose component can ship today**. The
enterprise host surveyed the three candidates and found:

| Dialect | URL expressible? | Auth expressible? |
|---|---|---|
| **Anthropic** | ✅ `ProviderApi::Messages` resolves to `{origin}/v1/messages` | ✅ `x-api-key` is a sanctioned credential header → `HeaderSecret` |
| Gemini | ❌ model in the path, `:method` suffix, and a query parameter | ✅ |
| Bedrock | ❌ model in the path | ❌ SigV4 has no arm |

Gemini and Bedrock both wait on the request-finalizer slice
(`2026-08-20-host-signed-request-finalizer.md`). Anthropic does not.

## 2. What the component covers

The three southbound directions, plus the two the trait requires:

| Family | Covered |
|---|---|
| `request` | IR `ChatRequest` → Messages request body, `x-api-key` auth, `anthropic-version` header |
| `response` | Messages response → IR `ChatResponse`, including thinking blocks **with their signatures** and both cache buckets |
| `stream` | Messages SSE → IR `StreamEvent`s, including `signature_delta` and per-frame usage |
| `error` | Anthropic error envelope → the stable error catalog |
| `capabilities` | The operator's declared catalog (no network, so no upstream catalog to query) |

## 3. Decisions

### D1 — Thinking blocks are carried with their signatures

Anthropic's extended thinking requires a later turn to replay a thinking block
together with its `signature`; the upstream rejects a replay whose signature is
missing or altered. The IR models this exactly (`ContentPart::Thinking.signature`,
`StreamEvent::ThinkingSignatureDelta`), so the component carries it in both
directions and never rewrites it.

`redacted_thinking` is opaque to everyone but the upstream and exists only to be
replayed; it round-trips as `ContentPart::RedactedThinking`.

### D2 — Usage is reported per frame, never folded

A stream carries usage twice: input-side counts in `message_start`, the final
output count in `message_delta`. The IR contract is explicit that a stream may
carry several `Usage` events and that **the consumer** folds them with
`Usage::absorb`. The component therefore emits what each frame said and folds
nothing.

This is not a style preference. A component that folds reports a stale running
total when a later frame legitimately reports zero for a bucket, which is
exactly what the enterprise host's ledger caught when its reference did fold.

### D3 — The terminal sequence is `Finish` → `Usage` → `Done`

A `message_delta` records the stop reason; the terminal triple is emitted when
that frame carries usage, or at EOF otherwise. A stream that never reported a
stop reason gets **no synthetic terminal** — inventing one would render "the
upstream never finished" as "finished normally".

Repeated `message_delta` frames overwrite the recorded reason rather than
producing several `Finish` events.

### D4 — Unknown enum values survive verbatim

Unknown `stop_reason` values become `FinishReason::Other`, not a bucketed
`Stop`. Anthropic ships reasons this crate does not model (`refusal`,
`pause_turn`, `model_context_window_exceeded`); collapsing them would tell the
caller the model finished normally when it did not.

### D5 — `anthropic-version` is a component-chosen constant

The dialect requires the header on every request. It is a wire-protocol
constant of the dialect, not an operator setting, so the component sets it and
the value is frozen by a fixture. The host never supplies it.

### D6 — The `system` field takes text only

Messages models `system` as a string. Thinking, images, and unknown parts have
no place there; the component takes the text parts and drops the rest rather
than refusing the request, because a system prompt that is partly untranslatable
is still worth sending.

Empty text contributes nothing and is skipped, so a caller never gets a blank
line it did not write.

## 4. Fixture pack

The pack lives beside the existing one (`fixtures-anthropic/`) rather than
replacing it: two components, two frozen packs. Gate ② runs the same suite over
each.

Cases are written **from the intent above**, not recorded from the
implementation's behaviour — a fixture recorded from an implementation freezes
whatever that implementation happened to do, including its bugs.

## 5. What this change does not do

- **No host adoption.** Both hosts keep their current adapters; adopting the
  component is each host's own slice behind its own switch.
- **No shared-code extraction.** The two references duplicate a few small
  helpers. Extracting them before a third dialect exists would be guessing at
  the shape of the abstraction.
