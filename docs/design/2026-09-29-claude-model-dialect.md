# Claude model dialect: sampling, forced tool choice and reasoning effort

Status: released in v0.41.0 (#120); `provider-anthropic` 1.0.7, `provider-bedrock-converse` 1.0.4.
Origin: token-station-server plan P19 (decisions DP1–DP5 taken 2026-09-29).

## Problem

The Anthropic Messages and Bedrock Converse components sent every Claude model
the same request shape. Claude generations now disagree, and every disagreement
is a hard 400. Measured against the Messages API on 2026-09-29:

| Model | `temperature` | `tool_choice: any` | adaptive + `effort` | `budget_tokens` | `effort` alone |
|---|---|---|---|---|---|
| `claude-sonnet-4-6` | 200 | — | 200 (`xhigh` rejected) | — | 200 |
| `claude-opus-4-8` | 400 | — | 200 | — | 200 |
| `claude-opus-5` | 400 (`top_p` too) | 200 | 200 | 400 | — |
| `claude-fable-5-1` | 400 | 400 | 200 | — | — |
| `claude-opus-5-5` | 400 | 400 | — | — | — |
| `claude-haiku-4-5-20251001` | — | — | — | 200 | 400 |

With thinking on, `temperature` must be 1 or unset and `top_p` at least 0.95 or
unset, and the budget form rejects a forced `tool_choice` (all measured).

The components also ignored `extensions.reasoning_effort`, which the shared
north codec fills from Responses `reasoning.effort` and Chat `reasoning_effort`,
so a caller's reasoning effort never reached a Claude target.

## Decision

The component cannot tell dialects apart from the model name: hosts route
aliases, Bedrock IDs and private deployments. The host declares the dialect per
model in `supported_parameters` (the channel `reasoning_replay.claude.v1` already
uses), and `anthropic_dialect` reads it. Declaring nothing keeps the request
shape byte-for-byte as before.

| Word | Effect |
|---|---|
| `anthropic.sampling.none` | `temperature` and `top_p` are dropped, not sent. |
| `anthropic.tool_choice.auto_only` | A forced tool choice (`required` or a named tool) is refused with a 400 capability error. |
| `anthropic.thinking.adaptive` | A caller's effort becomes `thinking: {type: adaptive}` + `output_config.effort`. |
| `anthropic.thinking.budget` | A caller's effort becomes `thinking: {type: enabled, budget_tokens}`. |
| `anthropic.effort.xhigh` | `xhigh` is sent as `xhigh`; otherwise as `high`. |

The two thinking words are exclusive; declaring both is refused.

Effort mapping (the caller vocabulary is OpenAI's):

| Caller | Adaptive | Budget |
|---|---|---|
| absent | nothing sent | nothing sent |
| `none`, `minimal` | `low` (never `disabled`: Fable 5.1 and Opus 5.5 reject it) | nothing sent |
| `low`, `medium`, `high` | same level | 1024, 4096, 16384 tokens, only when below `max_tokens` |
| `xhigh` | `xhigh` if declared, else `high` | as `high` |
| anything else | 400 capability error | 400 capability error |

When thinking is sent, `temperature` and `top_p` are dropped. The budget form is
skipped when the request forces a tool: the forced choice is a guarantee the
caller asked for, the effort only a preference. Converse carries the same
thinking fields in `additionalModelRequestFields`, and has no `maxTokens`
default of its own, so its budget form needs the caller's limit.

Dropping follows the kernel `Sampling` contract (an adapter drops what its
provider does not support rather than approximating it). A forced tool choice
is refused rather than downgraded to `auto`, because `auto` lets the model
answer without the tool the caller required.

## Evidence

- `anthropic_dialect` unit tests: mapping, budget limits, exclusivity.
- `provider.request.dialect-*` fixtures (four Anthropic, three Converse), run by
  gate two and by the sandbox parity tests against the Wasm builds.
- `tests/anthropic_dialect_v1.rs`: the refusals through both components' real
  `build_http_request`, and a forced tool still reaching an undeclared model.
- Bedrock is not measured (no AWS credentials when this was written); the
  Converse fields follow Bedrock's documented passthrough and need a live check
  before a host declares these words on Bedrock models.
