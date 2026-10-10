# Frozen samples: OpenAI Responses (`/v1/responses`)

Gate ② samples for `provider-openai-responses` (family `openai-responses`), judged by `south.provider-component.v1`
(`docs/design/2026-09-30-openai-responses-upstream-component.md` §12.1, step R1). This is **OpenAI's own pack** (R-Q16):
no row is a capture of real traffic, and no row stands for another upstream. A third-party Responses upstream (Groq,
xAI, Bailian, Azure, Bedrock's OpenAI-shaped endpoint) is admitted only by a pack of its own captured traffic, added in a
later release (record §3.3). The Codex rows and the `credential.*` rows of §12.1 belong to step R3.

Each row's source:

- **Request rows derived from the north codec** (`text`, `stream-with-cap`, `instructions-and-history`,
  `tools-and-tool-choice`, `structured-output`, `json-object-output`, `reasoning-effort`): the `chat_request` is what
  `south-north-codec` parses from an OpenAI Responses client body (record §9: "a request fixture for this package is a
  north codec output"). The client bodies are in `tests/support/openai_responses_client_bodies.rs`, and
  `openai_responses_vocabulary_v1::the_codec_derived_request_rows_are_north_codec_outputs` re-derives each row, so the
  pack cannot drift from the codec.
- **Hand-written request rows**: IR the other northbound codecs produce (a Chat tool-result turn, image and audio parts,
  `stop`, a thinking part, the Anthropic named-tool `tool_choice`), the stateless rule (`stateful-fields-never-sent`:
  `store`, `previous_response_id` and `include` smuggled into `extensions` change nothing, D4 / R-Q4), and every
  capability refusal of §4 (`refused-*`: the Claude replay carrier, an unmodelled part, an unknown `tool_choice` object,
  `input_file`, `file_id`, a tool result without a call id). Expected bodies follow the §4.1 and §4.2 tables: leading
  system messages joined with `\n` into `instructions`, later ones as `system` items; message items without `type` or
  `id`; `store: false` always; `strict`, `parallel_tool_calls` and `reasoning.effort` only from the three `extensions`
  keys the OpenAI-compatible reference reads (R-Q15).
- **Response rows**: response objects in the shape of OpenAI's Responses API reference (the response object, its
  `output` items, `incomplete_details`, `usage` with `input_tokens_details.cached_tokens` and
  `output_tokens_details.reasoning_tokens`). `cache-write-tokens-are-a-subset` uses `input_tokens_details.cache_write_tokens`,
  which OpenAI's reference does not document: it is read because the host's native parser reads it (record §7.1), and is
  marked as host parity, not documentation. `zero-tool-usage-is-accepted` and `nonzero-tool-usage` use the host's
  `tool_usage` shapes (zero hosted-tool telemetry passes; any non-zero counter is refused, R-Q3).
  `incomplete-unknown-reason` uses `turn_limit`, a reason outside the ruled pair.
- **Stream rows**: `response.*` events in the shape of OpenAI's streaming reference, each with `sequence_number`, the
  SSE `event:` line equal to the payload's `type`. To keep gate ②'s every-byte split fast, the `response` object inside
  a lifecycle or terminal event carries only `id`, `object`, `created_at`, `status`, `error`, `incomplete_details`,
  `model`, `output` and `usage` — not the request echo OpenAI also sends, which the component ignores. The failure
  rows cover the three shapes of §6.4: `response.failed` (with and without usage), OpenAI's `error` event (`code` and
  `message` at the top level), the Codex-style `error` event that nests an `error` object and has no `sequence_number`,
  and a frame with no `type` and a top-level `error` object. One row per evidence rule of §6.2 (duplicate terminal,
  frame after the terminal, sequence regression, response id change, item identity, unknown event type, unknown item
  type, missing terminal), plus `event-name-disagrees-with-type`, `arguments-contradict-the-stream` and
  `crlf-comments-and-split-frames` (CRLF line ends, a comment keep-alive, frames split across chunks).
- **Error rows**: OpenAI's documented error body (`error.message`, `type`, `param`, `code`) for an invalid key, a
  rate limit with `retry-after`, and `context_length_exceeded`, mapped by the OpenAI-compatible reference's table
  unchanged (record §5).

Expectations were produced by the reference, then each was read against the record and these sources. The usage judge
(`tests/usage_ir_contract_v1.rs`) recomputes every row's prompt total from the wire by OpenAI's documented convention
(the cached part inside `input_tokens`, reasoning inside `output_tokens`, `total_tokens = input_tokens + output_tokens`),
not from the reference, and pins the §6.4 error-code table.
