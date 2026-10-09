# Frozen samples: Bedrock InvokeModel, Anthropic (`/model/{model}/invoke`)

Gate ② samples for `provider-anthropic-bedrock-invoke` (family `anthropic-bedrock-invoke`), judged by
`south.provider-component.v1` (`docs/design/2026-10-08-bedrock-invoke-anthropic-component.md` §9.2). No row is a capture
of real Bedrock traffic: the owner ruled (I-Q5) that one capture of a real InvokeModel stream, a throttled stream and a
model error happens before the host's cutover, not before this release. Each row's source:

- **Request rows derived from `fixtures-anthropic/`**, with exactly the three edits the host's native arm makes
  (`augment_anthropic_body_for_bedrock`, token-station-server `gateway/src/modules/inference/engine/bedrock.rs`): the
  body loses `model` and `stream` and gains `anthropic_version: "bedrock-2023-05-31"`; the URL is
  `https://bedrock-runtime.us-east-1.amazonaws.com/model/{model}/invoke` (or `/invoke-with-response-stream`, AWS
  `InvokeModelWithResponseStream`) with the model as one encoded path segment; the headers are the native arm's
  (`content-type`, `accept`, `x-amzn-bedrock-accept`); no auth (the `host_signed` arm). Model ids `claude-x` become
  `anthropic.claude-x-v1:0`. `chat` and `stream-uses-the-stream-operation` (from `chat`),
  `default-max-tokens-when-the-caller-sets-none` (from `max-tokens-defaults`), `model-id-with-a-slash-stays-one-segment`
  (from `chat`, with the inference-profile ARN of the Converse packs, SF26), the four `dialect-*` rows and
  `tool-choice-none-withholds-the-declarations`. The test
  `anthropic_bedrock_invoke_conformance_v1::the_derived_request_rows_are_the_messages_rows_with_the_three_edits` pins
  the derivation, so the two packs cannot drift.
- **Request rows from the Converse pack's IR inputs**: `reasoning-replay-multiple-blocks` and
  `parallel-tool-results-are-consecutive-user-turns` take the Converse rows' `chat_request` unchanged; the expected
  Messages bodies are what `provider-anthropic` builds for that IR (one `user` turn per tool result, which Anthropic's
  Messages documentation combines into one turn), checked by hand against the Messages API reference.
- **Response rows**: Anthropic Messages response bodies in the shape of Anthropic's Messages API reference and of the
  host's native-arm test `aws_claude_invoke_nonstream_is_sigv4_signed_and_bills_all_four_anthropic_buckets`
  (`gateway/tests/bedrock_surface.rs`, whose four-bucket usage `usage` reuses). `cached-usage` carries the
  `cache_creation` 5-minute / 1-hour split of Anthropic's prompt-caching documentation. `unknown-stop-reason-survives`
  uses `model_context_window_exceeded`, a Bedrock-documented stop reason this dialect does not model.
- **Stream rows**: AWS's `InvokeModelWithResponseStream` documentation (`PayloadPart`: each `chunk` event carries
  `{"bytes": <base64 of one Anthropic stream event>}`) and Anthropic's streaming documentation for the decoded events,
  written in the canonical re-encoding the host feeds (`south_contracts::reencode_eventstream_v1`: `event: chunk`,
  compact JSON). The event sequence of `text` is the host's native-arm test
  `aws_claude_invoke_stream_decodes_the_eventstream_and_bills_like_nonstream`; `chunk-with-invalid-base64-is-refused`
  reuses its `chunk-bad-base64` payload `%%%` (`gateway/src/modules/inference/engine/bedrock_durable_stream.rs`).
  **Inferred, not captured:** the random-length padding member `p` beside `bytes` (`padding-member-is-ignored`), the
  `amazon-bedrock-invocationMetrics` object on `message_stop` (ignored, I-Q11), the exception spelling
  `throttlingException`, and the in-band Anthropic `error` event on InvokeModel (`in-band-error-ends-the-stream`).
  `terminal-delta-carries-the-real-input` is the host's 03 #86 shape (input 0 on `message_start`, the real input on
  the terminal `message_delta`); `a-shrinking-cumulative-count-is-refused` is the half of that rule the I-Q7 ruling
  added.
- **Error rows**: Bedrock's documented exception names (`ThrottlingException`, `UnrecognizedClientException`,
  `ValidationException`), named in the `x-amzn-errortype` header (qualified, as Bedrock sends it) or the body's
  `__type`; the validation message is the one the host's native arm records for a body that carries `stream`.

Expectations were produced by the reference, then each was read against these sources; the derived request rows are
also recomputed independently by the test named above. The usage judge
(`tests/usage_ir_contract_v1.rs`) recomputes every row's prompt total from the wire by Anthropic's documented formula,
not from the reference.
