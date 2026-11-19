# Tool-call arguments cut off by the output cap

Status: accepted (lv chose option A on 2026-11-19). Not released. The release bump and the host re-pin are separate
steps.

Date: 2026-11-19

Records: [OpenAI Responses upstream component](2026-09-30-openai-responses-upstream-component.md) §5 (R-Q2),
[boundary record](2026-09-30-host-zero-vendor-boundary.md) §13.12 (Q47).

## 1. What happened

A production host answered six Chat requests for an Azure-served OpenAI model with `502`, reason `TranslationFailed`.
The upstream had answered `2xx`; the host's Chat render failed with:

```text
choices[0].message.tool_calls[0].arguments:
tool call "final_result" produced arguments that are not valid JSON
```

`final_result` is the structured-output tool of a common agent framework, which sends no output cap, so the host
applied its default cap. Each request ran 70 to 80 seconds before failing, and the failures came in bursts of three
within three minutes, as one request retried. The record's reading: the model spent most of the cap reasoning, the
cap stopped it inside the call's JSON, and two rules of this repository turned a truncated answer into an
unexplained gateway error. The host has the upstream bodies as review evidence; confirming the reading against them
is the host's step (§6).

1. **`provider-openai-responses` dropped the reason.** An `incomplete` response with `max_output_tokens` settles with
   finish reason `length` (R-Q2), but `response_of` replaced every finish reason with `tool_calls` whenever the output
   held a function call. The stream terminal reuses `response_of`, so a stream lost the reason the same way.
2. **The Chat codec refused the call.** `openai_chat_response` refuses arguments that are not valid JSON, so that a
   client never receives garbage it cannot attribute. With the reason gone, it could not tell a cut-off call from a
   corrupted one.

The client received neither the partial call nor the reason, and could not recover by raising its cap. The host also
parked each request for manual review instead of settling the tokens the upstream produced.

## 2. Decisions

- **D1. `length` relaxes the well-formedness check.** `openai_chat_response` renders a tool call whose arguments are not
  valid JSON when, and only when, the choice's finish reason is `length`. The arguments are the exact bytes the model
  produced, unchanged. OpenAI's Chat Completions API does not validate arguments either: its reference tells clients
  to validate them before calling the function, and developers report cut-off arguments delivered under
  `finish_reason: "length"` (checked 2026-11-19; no captured traffic in this repository). The reason is what
  attributes the bytes to the model, which is what the check protects. Every other finish reason keeps the refusal.
- **D2. An `incomplete` Responses answer keeps its reason.** In `response_of`, a function call turns `stop` into
  `tool_calls`; `length` and `content_filter` stand. The stream terminal follows, since it reuses `response_of`.
- **D3. No new error.** Option B, a dedicated error for a cut-off call, was rejected: no status code fits a cap the host
  may have chosen, the upstream has already produced and billed the tokens, and a stream has already delivered the
  partial arguments by the time the terminal arrives.

## 3. Not changed

- **The Messages codec** (`anthropic.rs`) still refuses arguments that are not valid JSON. Its `tool_use.input` is a
  JSON object, so partial arguments have no faithful rendering there. A Messages client of a Responses row gets the
  refusal, now under the right finish reason. A separate record decides that surface if it matters.
- **`content_filter` with a cut-off call** keeps its finish reason (D2) but is still refused by the Chat codec: only
  `length` relaxes the check.
- **Other upstream parsers.** The OpenAI-compatible parser already keeps `length`. Gemini replaces every reason with
  `tool_calls` when a call is present, but its calls carry structured arguments and cannot be cut off mid-JSON. Kiro's
  wire has no terminal event and no cap signal. None of them is changed.
- **Streaming Chat** never checked arguments; it now receives `length` instead of `tool_calls` from Responses rows.

## 4. Tests, written first and failing on the baseline

- `south-north-codec` `openai_chat::tool_arguments_cut_off_by_the_output_cap_are_delivered_under_length`: partial
  arguments under `length` render byte for byte; under `tool_calls` and `stop` they are still refused. On the
  baseline it failed with the production error text.
- `fixtures-openai-responses` rows `response.incomplete-mid-tool-call` and `stream.incomplete-mid-tool-call`, listed in
  `the_shipped_pack_carries_every_decided_row`. On the baseline gate ② produced `tool_calls` for both. The usage judge
  reads the new rows from the wire like every other row.

## 5. Package identities (Q47)

`south-component-conformance` changes, so it takes a version (0.81.0 → 0.81.1, a compatible change) and every package
takes a new version (boundary record §13.12). `provider-openai-responses` takes a minor version (1.6.5 → 1.7.0)
because its answers change; every other package takes the Q47 patch. No package relies on anything a runtime it
already declares cannot read, so every `south_runtime` declaration stays. No contract number or kernel tuple moves.

## 6. Hosts

- The Chat codec runs in the host. A host gets D1 by re-pinning to the release that carries it, and D2 with the new
  `provider-openai-responses` package from that release's index.
- Once both are in, a cut-off call settles at the upstream's reported usage like any other `length` answer, instead
  of being parked for review.
- Before the re-pin, the host should read the review evidence of the six requests: an `incomplete` status with
  `max_output_tokens`, or output tokens equal to the applied cap, confirms §1. A complete answer with invalid
  arguments would point elsewhere.
- Telling the client which cap applied when the host chose it is the host's decision (its `x-output-cap` header
  today reports only a client cap the host lowered).
