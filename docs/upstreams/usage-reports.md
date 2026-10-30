# How upstreams report token usage

Cross-vendor facts about where and how upstreams report token usage, for the packages that parse it into
the canonical IR. Moved from the token-station-server host documentation on 2026-10-30. Each package's
design record is authoritative for what that package parses; the
[explicit cache reads record](../design/2026-10-17-explicit-cache-reads.md) covers the explicit / implicit
cache split.

## 1. Two conventions

- **Disjoint split (Anthropic Messages, Claude Code).** Input is reported in three non-overlapping fields;
  the total the Anthropic console shows is their sum:
  `input_tokens` (new uncached tokens), `cache_creation_input_tokens` (written into the cache, about 1.25×
  input; split further into `cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens`,
  1.25× and 2×), `cache_read_input_tokens` (served from the cache, about 0.1× input).
- **Cached subset (OpenAI-shaped wires).** The input count is the total; cached tokens are a subset in a
  sibling field and never add to it:

  | Field | Where |
  |---|---|
  | `prompt_tokens_details.cached_tokens` | OpenAI Chat, GLM, Bailian, Kimi |
  | `input_tokens_details.cached_tokens` | OpenAI Responses, Codex, xAI |
  | top-level `prompt_cache_hit_tokens` (with `prompt_cache_miss_tokens`) | DeepSeek; older Kimi |
  | `usageMetadata.cachedContentTokenCount` | Gemini native |

- **Kimi / Moonshot** (probed 2026-06-13, `kimi-k2.6` on `api.moonshot.cn`, the same 4K prefix twice): the
  first call reports no cache fields; the cache hit reports both `prompt_tokens_details.cached_tokens` and a
  redundant top-level `cached_tokens`, with `prompt_tokens` still the total. The final streaming chunk
  carries the same fields without `stream_options.include_usage`. `prompt_cache_hit_tokens` did not appear.

## 2. Where usage appears

- **Anthropic Messages, non-streaming:** `usage` on the response object (also `service_tier`).
- **Anthropic Messages, streaming:** `message_start.message.usage` carries the input buckets with
  `output_tokens: 0`; the final `message_delta.usage` carries the output count and, on some compatible
  upstreams, the input buckets again (cumulative for the message). Some compatible upstreams report
  `input_tokens: 0` on `message_start` and the real count only on the final frame.
- **OpenAI Chat, non-streaming:** `usage` on the root object (`prompt_tokens`, `completion_tokens`,
  `prompt_tokens_details.cached_tokens`, `completion_tokens_details.reasoning_tokens`, the latter a subset
  of `completion_tokens`).
- **OpenAI Chat, streaming:** most OpenAI-compatible upstreams send usage only when
  `stream_options.include_usage` is set, as a final chunk with empty `choices` before `data: [DONE]`.
- **OpenAI Responses, non-streaming:** `usage` on the response (`input_tokens`,
  `input_tokens_details.cached_tokens`, `output_tokens` including reasoning, `total_tokens`).
- **OpenAI Responses, streaming:** `response.completed` carries the final usage at `response.usage`. On
  Codex streams there is no top-level `usage` at all.
- **Missing `[DONE]`.** AWS `bedrock-mantle.*.api.aws/v1/chat/completions` was recorded as closing the
  stream cleanly without `data: [DONE]`; no capture confirms it either way (see the
  [Bedrock Mantle record](../design/2026-10-15-bedrock-mantle.md)).

## 3. Caching behaviour worth knowing

- **Claude Code CLI with an API key** marks system prompts, tool definitions and trailing turns with
  `cache_control`. Caching engages only above the model's minimum (1024 tokens for Sonnet / Opus, 2048 for
  Haiku), so short chats may not cache at all.
- **TTL.** The default ephemeral cache lasts 5 minutes; the 1-hour TTL is opted into per block (or with the
  `prompt-caching-scope-2026-01-05` beta). Gaps longer than the TTL show `cache_creation > 0,
  cache_read = 0`.
- **Anthropic console.** The Prompt Caching page lags the Usage page by about 30–60 minutes.
- **Codex cache metadata** outside the usage object — `response.prompt_cache_key` (a UUID) and
  `response.prompt_cache_retention` (typically `"24h"`) — are not token counts; the cached subset is still
  `response.usage.input_tokens_details.cached_tokens`.
