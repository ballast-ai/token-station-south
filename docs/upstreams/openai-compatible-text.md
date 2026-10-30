# OpenAI-compatible text upstreams

Upstream facts for text upstreams served by the `openai-compatible` family of
[`provider-openai-compatible`](../../components/provider-openai-compatible/manifest.json). Moved from the
token-station-server host documentation on 2026-10-30. Each entry gives the API root a provider row
uses as its endpoint.

## Xiaomi MiMo

- API root `https://api.xiaomimimo.com/v1`, Bearer auth (the host `api.platform.xiaomimimo.com` named in
  some early material is wrong). The Token Plan subscription endpoints
  (`token-plan-{ams,cn}.xiaomimimo.com/v1`) are a separate tier.
- Chat models: `mimo-v2.5-pro` (reasoning flagship), `mimo-v2-pro`, `mimo-v2-omni` (text, vision and
  audio input), `mimo-v2-flash`.

## DeepSeek

- API root `https://api.deepseek.com/v1`, Bearer auth; the bare host answers both `/chat/completions` and
  `/v1/chat/completions`.
- The output cap is `max_tokens` only; `max_completion_tokens` is not documented and is refused.
- Cache hits are reported as top-level `usage.prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`
  (the shape Kimi also uses), not as `prompt_tokens_details.cached_tokens`.
- Models (checked 2026-05-28): `deepseek-v4-flash` and `deepseek-v4-pro`, both 1M context and 384K
  maximum output. Thinking is a request parameter (`thinking` or `reasoning_effort`), not a separate
  model. The older `deepseek-chat` / `deepseek-reasoner` ids were scheduled for deprecation on
  2026-07-24 and map to v4-flash modes.
- Prices per million tokens (input / cache hit / output; `api-docs.deepseek.com/quick_start/pricing`,
  checked 2026-05-28): v4-flash $0.14 / $0.0028 / $0.28; v4-pro $0.435 / $0.003625 / $0.87. The v4-pro
  figures were shown as a 75% launch discount that the page said would become the permanent price after
  2026-05-31 15:59 UTC.

## Meituan LongCat

- API root `https://api.longcat.chat/openai/v1` (note the `/openai` prefix), Bearer auth. An
  Anthropic-shape surface also exists at `/anthropic/v1/messages`.
- The output cap is `max_tokens`; `max_completion_tokens` is not documented.
- Models (checked 2026-06-02): one live id, `LongCat-2.0-Preview`, 1M context and 128K maximum output;
  the earlier `LongCat-Flash-*` ids are no longer documented.
- Pricing: the platform was in public beta with no paid quota ("does not support paid quota
  purchases"). Free quota is 5M tokens per day per account (up to 120M after submitting feedback),
  resetting at midnight Beijing time with no carry-over; exhausting it answers 429. No cache or
  reasoning usage fields are documented.
