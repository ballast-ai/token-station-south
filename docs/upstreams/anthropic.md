# Anthropic

Upstream facts for [`provider-anthropic`](../../components/provider-anthropic/manifest.json) and
[`provider-anthropic-bedrock-invoke`](../../components/provider-anthropic-bedrock-invoke/manifest.json).
Moved from the token-station-server host documentation on 2026-10-30. Request and response shapes are in
the [Anthropic provider component](../design/2026-08-22-anthropic-provider-component.md) and
[Claude Code family](../design/2026-10-15-claude-code-family.md) records.

- **The long-context premium is retired.** Older Sonnet models on the 1M-context beta once billed prompts
  above 200K tokens at about 2× ($6 / $22.50 against $3 / $15). The current pricing page
  (`platform.claude.com/docs/en/about-claude/pricing`, mirrored on `anthropic.com/pricing`, checked
  2026-07-02) has no 200K tier at all and states that Claude Fable 5, Opus 4.8 / 4.7 / 4.6, Sonnet 5 and
  Sonnet 4.6 include the full 1M window at standard pricing.
- **Reading the Sonnet price row.** Input $3, 5-minute cache write $3.75 (1.25×), 1-hour cache write $6
  (2×), cache read $0.30 (0.1×), output $15 per million tokens. The $6 column is the 1-hour cache write,
  not a context premium.
- **Sonnet 5** lists $3 / $15; the $2 / $10 shown on the marketing page was an introductory rate through
  2026-08-31.
