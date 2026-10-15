# Frozen samples: AWS Bedrock Mantle (`bedrock-mantle`)

Gate ② samples for the `bedrock-mantle` family of `provider-openai-responses` 1.4.0, judged by
`south.provider-component.v1` (design record `docs/design/2026-10-15-bedrock-mantle.md` §6). The family is
`openai-responses` on the endpoint template `https://bedrock-mantle.{region}.api.aws`, so its response, stream and usage
rules are the family-neutral ones of `fixtures-openai-responses/`.

**No row is a capture of Mantle's traffic.** Under lv's 2026-10-15 directive the rows transcribe the host's native Mantle
arm (server `origin/dev-v2`) and the documented OpenAI shapes it parses; the record's §8 lists what is unverified.

**This directory is judged together with `fixtures-openai-responses/` and `fixtures-openai-codex/`**: every gate ② run
of the package (native, sandboxed, and the release report) loads the three through
`tests/support/openai_responses_packs.rs`. Every row is named `mantle-*`, so no name repeats across them. Every request
row is for `bedrock-mantle`, declares the `region` config value the host passes, and has the base URL the host fills
from it (`bedrock_mantle_conformance_v1` checks both).

Each row's source:

- **Transcribed from the host's native tests.** `request.mantle-host-pinned` and `request.mantle-host-pinned-stream`:
  `responses_request(false)` / `(true)` (`gateway/tests/responses_providers.rs`), the northbound name replaced by the
  test's upstream id `openai.gpt-oss-120b-1:0` as the host does before building; their IR is re-derived from the north
  codec. `response.mantle-host-pinned`: `responses_native_json`. `stream.mantle-host-pinned`: `responses_native_sse`,
  split one frame per chunk (data-only frames, text deltas with no `output_item.added`).
  `request.mantle-region-us-west-2`: the region the host's URL unit test pins for the Responses surface
  (`engine/bedrock.rs`).
- **Native's `[DONE]` rule on the Responses surface.** `stream.mantle-trailing-done-refused`: the host-pinned stream
  plus `data: [DONE]`. Native parks it as invalid Responses SSE JSON; the component refuses it the same way.
- **What native forwards verbatim.** `request.mantle-request-fields` (`user`, `metadata`, `prompt_cache_key`,
  `truncation`) and `request.mantle-tools-and-reasoning-effort` (a function tool and `reasoning.effort` `high`, the
  value the host's `-high` suffix injects for Mantle and its KNOWLEDGE names for gpt-oss).
- **The family's own rules.** `request.mantle-stateful-fields-never-sent` (`store`, `previous_response_id` and `include`
  smuggled into `extensions` change nothing; the host refuses `previous_response_id` before the component) and
  `request.mantle-reasoning-replay-undeclared` (OpenAI encrypted reasoning refused for a model that does not declare
  `reasoning_replay.openai.v1`, record BM-Q4; native forwards it).
- **gpt-oss reasoning in the documented OpenAI shape.** `response.mantle-reasoning-text-and-cache` (a `reasoning` item
  with `reasoning_text` content, `cached_tokens` and `reasoning_tokens`) and `stream.mantle-reasoning-text`
  (`response.reasoning_text.*` events, data-only frames). These give the pack its cache and reasoning buckets for the
  usage judge; they are the shape the host's native parser reads, not a Mantle capture.
- **Usage sidecars** on the two response rows and the two stream rows that report usage, for `UsageNeverDefaulted`.

The chat surface on a Mantle row served by `openai-compatible` (record §4) is pinned in `fixtures/`:
`provider.request.bedrock-mantle-chat-stream` (the origin as the row endpoint gives the native chat URL) and
`provider.stream.bedrock-mantle-chat-without-done` (the host's `openai_chat_sse` without its final `[DONE]`; record §5,
difference D2).

Expectations were derived from the reference's rules, then each was read against the record and the host behaviour
above.
