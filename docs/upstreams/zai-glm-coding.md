# Z.AI GLM Coding Plan

Upstream facts for the `glm-coding` family of
[`provider-openai-compatible`](../../components/provider-openai-compatible/manifest.json) (text) and of
[`image-openai-compatible`](../../components/image-openai-compatible/manifest.json) (images). Moved from the
token-station-server host documentation (`gateway/README.md` and its archived native-arm knowledge base)
on 2026-11-04. Request shape, the declared user agent and the subscription-quota declaration are in the
[T9 record](../design/2026-10-15-openai-compatible-t9.md) and the
[controlled user-agent record](../design/2026-08-20-controlled-user-agent.md).

- **What it is.** A flat-rate monthly or quarterly subscription authenticated with a Bearer API key from the
  Z.AI dashboard (no OAuth, no refresh). It covers `glm-5.2`, `glm-5.1`, `glm-5-turbo`, `glm-4.7` and
  `glm-4.5-air` (checked 2026-06-13).
- **Endpoints.** OpenAI-compatible at `https://api.z.ai/api/coding/paas/v4/...`; an Anthropic-compatible
  endpoint at `https://api.z.ai/api/anthropic/v1/messages` also exists (the `glm-coding` family serves Chat
  Completions only; see the T9 record, D1).
- **Only listed client tools are allowed.** The plan's terms forbid SDK-based access, and Z.AI detects
  unsupported tools by the `User-Agent` header. Sending `opencode/<version>` identifies the caller as
  OpenCode, one of the listed tools. Z.AI may revoke a subscription if it detects the impersonation or
  changes the policy; three policy violations can lead to a permanent ban. Known reports:
  [badlogic/pi-mono#4187](https://github.com/badlogic/pi-mono/issues/4187) (flagged for generic OpenAI SDK
  headers such as `user-agent: OpenAI/JS 6.26.0` and `x-stainless-*`) and
  [cline/cline#6761](https://github.com/cline/cline/issues/6761). Refresh the pinned OpenCode version when
  the live OpenCode release drifts more than about six months from it.
- **Quota.** Each seat has a 5-hour and a weekly quota; an exhausted seat answers 429.
- **Prices.** The plan is billed by quota upstream, not per token. Z.AI publishes per-token pay-as-you-go
  prices at <https://docs.z.ai/guides/overview/pricing> for most of the covered models (checked 2026-06-13).
  GLM-5.2 had no pay-as-you-go card on that date: the Chinese Coding Plan docs describe quota multipliers
  instead (3x during 14:00-18:00 UTC+8, 2x off-peak, with a temporary 1x off-peak benefit through
  September), which are not a per-token rate. GLM-5.2 lists a 1,000,000-token context window and 131,072
  max output tokens; Claude Code examples select the 1M variant as `glm-5.2[1m]`.
