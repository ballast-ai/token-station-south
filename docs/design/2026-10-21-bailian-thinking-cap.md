# Bailian thinking cap: a reasoning cap in `request_facts` and the `bailian-openai-compatible` family

Status: proposed 2026-10-21 (requested by lv; the open points in §8 await lv's review). Implemented on branch
`feature/bailian-thinking-cap`.

Date: 2026-10-21

Baseline: south `origin/main` = `0f1ebf8f` (release 0.75.0). Host references carry a `server:` prefix and refer to
token-station-server `dev-v2`; `server:…/` abbreviates `server:gateway/src/modules/inference/engine/`. Statements
marked **unverified** come from reading code or documentation, not from running anything.

## 1. Problem

Alibaba Bailian's (DashScope's) OpenAI-compatible Chat endpoint (`{root}/compatible-mode/v1/chat/completions`) serves
Qwen thinking models (`qwen3.7-plus`, `qwen3.8-max`, `qwen3.8-flash`, `qwen3.8-omni-flash`) that think by default. On
these models `max_tokens` limits only the final answer; the reasoning is not counted against it, while
`completion_tokens` (what the host bills) includes the reasoning.

The host seals the authorised output cap N into the request (`server:…/text_admission/component_facts.rs`
`check_output_cap`), and its settlement guard UV4 parks any settlement whose billed output exceeds N. UV4's premise is
that every vendor's cap includes reasoning. On these models it does not: a request capped at N can bill far more than
N, and every such settlement is parked for review. Today Bailian rows are `openai-compatible` rows and hit this
directly.

## 2. Measured facts (real upstream, 2026-10-21)

`thinking_budget` (an integer body field) is a hard cap on `reasoning_tokens` on all four models. With thinking on,
an answer cut by `max_tokens` comes back as `max_tokens + 3` tokens (`finish_reason: length`); an answer that ends
naturally after thinking can be `max_tokens + 2`.

| Model | `thinking_budget` | `max_tokens` | reasoning | answer | finish |
|---|---|---|---|---|---|
| `qwen3.7-plus` | 20 | 50 | 20 | 53 | `length` |
| `qwen3.7-plus` | 1000 | 40 | 748 | 42 | (thinking ended before the budget) |
| `qwen3.8-flash` | 30 | 40 | 30 | 3 | `stop` |
| `qwen3.8-max` | 30 | 40 | 30 | 43 | `length` |
| `qwen3.8-omni-flash` | 30 | 40 | 30 | 43 | `length` |
| (all four) | 50 | — | exactly 50, three times | — | — |

Budgets of 20 and 30 were also exact. So the total output is at most `thinking_budget + max_tokens + 3`.

## 3. Why a package-only split fails the host's seal

The obvious fix is for the component to split N: `thinking_budget = B`, `max_tokens = N - B - 3`. But the host's
seal check requires exactly one of the family's declared `request_facts.output_cap` pointers to hold the cap, with a
value **equal** to N; anything else is refusal R16 (503). A split request therefore fails the seal on every host. The
package cannot fix this alone: the manifest has to say where the second cap lives and how much the upstream may
overrun, and the host's seal has to read that.

## 4. Manifest: `RequestFactsV1` gains `reasoning_cap` and `cap_overhead`

```json
"request_facts": {
  "bailian-openai-compatible": {
    "output_cap": ["/max_tokens"],
    "reasoning_cap": ["/thinking_budget"],
    "cap_overhead": 3,
    "model": {"body": "/model"},
    "stream": {"body": "/stream"}
  }
}
```

- `reasoning_cap: Vec<String>`: JSON Pointers where the family writes a separate reasoning (thinking) cap. Default
  empty, omitted when empty.
- `cap_overhead: u32`: tokens the upstream may emit beyond the two caps. Default 0, omitted when 0.

**Semantics.** When a sealed request carries a value at a declared `reasoning_cap` pointer, the request's total
output bound is `answer cap + reasoning cap + cap_overhead`, and that sum must equal the authorised cap N: exactly one
`output_cap` pointer holds the answer cap a ≥ 1, exactly one `reasoning_cap` pointer holds the reasoning cap b ≥ 1,
and `a + b + cap_overhead == N`. When no `reasoning_cap` pointer holds a value, the existing rule holds unchanged: one
`output_cap` pointer holds exactly N. A reasoning cap without an authorised cap is never written.

**Gate ① (`validate_request_facts`).** Every `reasoning_cap` pointer is a distinct member pointer; at most
`MAX_OUTPUT_CAP_LOCATIONS` (4) of them; none also an `output_cap` pointer; a non-empty `reasoning_cap` needs a non-empty
`output_cap` (a split with no answer cap is meaningless); `cap_overhead` is allowed only with a non-empty
`reasoning_cap` and is at most `MAX_CAP_OVERHEAD` (64).

**Gate ② (`RequestFactsHonoured`).** The suite checks the split form above on every built request, and its cap
mutation (build again with another IR cap) removes both pointer sets before comparing the two bodies. The mutation
builds with N − 1, so a fixture at the smallest split cap (N = 5) would rebuild at 4 and be refused; that edge is
pinned by a behaviour test instead of a fixture.

**Gate ② (`ImmutablePathsHonoured`).** That check requires each declared immutable path to be present on every
request of the family or on none. A path that is also one of the family's `reasoning_cap` locations is exempt: a
reasoning cap is present exactly when the request splits, and `RequestFactsHonoured` pins its value on every
request. Declaring the path immutable still tells a host that request extras may not touch it.

**Compatibility.** `RequestFactsV1` is `deny_unknown_fields`, so a runtime before the release that ships the fields
refuses a manifest that uses them. No contract number moves (manifest fields only, as with `model_parameters` in
0.74.0). Existing manifests serialize byte-identically (both fields are omitted at their defaults).

## 5. The `bailian-openai-compatible` family in `provider-openai-compatible`

The wire is the `openai-compatible` Chat Completions wire. Rules that differ:

- **URL.** The row's endpoint is the API root (`https://dashscope.aliyuncs.com/compatible-mode/v1`, or the
  international root); the component posts to `{endpoint}/chat/completions` (a trailing slash on the endpoint is
  trimmed). On an endpoint with a path this is the URL an `openai-compatible` row resolves from the same endpoint, so
  an existing Bailian row moves by changing its family alone. No endpoint template: the operator picks the region.
- **Auth.** `Authorization: Bearer`, the static `provider_api_key` slot.
- **Usage.** Parsed as on every family of the package, Bailian's explicit cache reads included
  (`2026-10-17-explicit-cache-reads.md`); `completion_tokens_details.reasoning_tokens` is the reasoning subset.
- **Client fields.** `enable_thinking` and `thinking` are forwarded as on `openai-compatible`
  (`client_passthrough::FAMILIES`). The client's `thinking_budget` is consumed by the split below; it is never
  forwarded verbatim.
- **Thinking state.** The client's own `enable_thinking` boolean (forwarded verbatim) decides when it sent one;
  otherwise thinking is on unless the IR reasoning mode is `disabled`. The client's switch wins over the IR mode so
  that the split always follows what the upstream receives: a client `enable_thinking: true` beside an IR `disabled`
  is split, never sent unbounded. A `null` or non-boolean `enable_thinking` counts as not sent.
- **The split.** When thinking is on, the IR carries a cap N, **and** the model declares the parameter word
  `thinking_budget` in `ProviderConfig.models[].supported_parameters`:
  - B is the client's `thinking_budget` if it sent one, else the IR's `reasoning.budget_tokens`, else
    `floor((N - 3) / 2)`; B is clamped to `[1, N - 4]`;
  - the body carries `thinking_budget = B` and `max_tokens = N - B - 3`;
  - N < 5 leaves no room for both caps: capability error 400 before dispatch;
  - a client `thinking_budget` that is not a non-negative integer (other than `null`, which counts as absent) is a
    capability error 400. It is checked only when the split consumes it; otherwise it is dropped unread, as today.
- **Off, uncapped, or undeclared.** Otherwise the body carries `max_tokens = N` (when N is set) and no
  `thinking_budget`, as `openai-compatible` does today.
- **IR `disabled` (BT-Q1).** When the IR reasoning mode is `disabled`, the model declares `thinking_budget` and the
  client sent no boolean `enable_thinking`, the body carries `enable_thinking: false`. These models think by default, so
  without it a Messages client's `thinking: {"type": "disabled"}` would let the upstream think with no budget and
  overrun N.
- **Declarations.** `request_facts` as in §4; `immutable_body_paths: ["thinking_budget"]`, so operator request extras
  cannot set or rewrite the reasoning cap. `enable_thinking` stays mutable. `model_parameters`: the three
  `openai-compatible` words (`anthropic.tool_choice.auto_only`, `reasoning_content`, `reasoning_effort`) plus
  `thinking_budget`. Not a subscription-quota family (pay as you go).

Worked example, N = 73 with no budget from the client: B = 35, `max_tokens` = 35; the measured worst case is
35 + 35 + 3 = 73 = N. With the measured row b20/mt50: N = 73 would produce exactly that request from a client
`thinking_budget` of 20, and the observed output (20 + 53 = 73) meets N exactly.

## 6. The host change (server repo, not part of this change)

After re-pinning to the release that ships these fields:

1. **Seal (`check_output_cap`).** When the family declares `reasoning_cap` and the sealed body holds a value at one of
   its pointers: exactly one `output_cap` pointer holds a ≥ 1, exactly one `reasoning_cap` pointer holds b ≥ 1, and
   `a + b + cap_overhead == N`, else R16. Otherwise the existing rule.
2. **Request extras.** `thinking_budget` is declared immutable; the host's existing immutable-path skip covers it.
3. **UV4 and the reservation are unchanged.** The reservation stays N; UV4 still parks billed output above N. The
   split makes the upstream's measured worst case equal N.
4. A host that re-pins without changing the seal refuses split requests with R16 (fail closed, never a funds leak).
   Operators should declare the `thinking_budget` word on a model only after the host's seal change is deployed.

## 7. Versions and tests

- `provider-openai-compatible` 2.12.1 → **2.13.0** (a new family), `compatibility.south_runtime` 0.74.0 → **0.76.0**
  (it declares `reasoning_cap` / `cap_overhead`, which a 0.75.0 runtime's gate ① refuses). As in the 0.74.0 cycle,
  the feature commits keep 0.74.0 so the branch stays consistent with workspace 0.75.0; the release 0.76.0 commit
  raises it.
- **Q47 is deferred to the integrator.** This change touches `south-provider-api` and `south-component-conformance`,
  which every component links. Their version bumps and every other package's Q47 patch are left to the combined
  release bump, because other workstreams change the same crates in parallel. Until then
  `scripts/check_crate_versions.py`, the digest-stability check and the `shipped_packages_v1` version rows fail on
  this branch by design. The integration commit made the bump: `south-provider-api` 0.62.0 → 0.63.0,
  `south-component-conformance` 0.74.0 → 0.75.0, every other package the Q47 patch
  (`shipped_packages_v1::bailian_retires_every_published_075_package_identity`).
- Gate ② for every component changes in two places (§4): the split form in `RequestFactsHonoured` and the
  reasoning-cap exemption in `ImmutablePathsHonoured`. No shipped package other than this one declares either field,
  so no other package's gate ② result changes.
- Tests: `south-provider-api` `tests/request_facts_reasoning_cap_v1.rs` (round-trip, defaults omitted, every gate ①
  refusal); `south-component-conformance` `tests/request_facts_v1.rs` (the split passes gate ②; a wrong sum, two
  reasoning values or a reasoning value without a declaration fail it); `tests/bailian_openai_compatible_v1.rs` (the
  family's rules, pinned to the measured facts); fixtures `provider.request.bailian-openai-compatible-*` and
  `provider.response.bailian-thinking-usage`; sandbox parity runs the whole pack in the built component.

## 8. Open points

- **BT-Q1** IR `disabled` writes `enable_thinking: false` on a model declaring `thinking_budget` (§5). The request
  only said "off writes `max_tokens` only"; this adds one field because the alternative overruns N. Recommended: keep.
- **BT-Q2** The default split is half of `N - 3`. A smaller default reasoning share gives longer answers; a larger one
  gives the model more room to think. **Unverified** which serves clients better.
- **BT-Q3** Operator request extras may still set `enable_thinking: true` on a request whose client turned thinking
  off. The component then writes no `thinking_budget`, so the upstream thinks without a budget and can overrun N
  (UV4 parks it). Making `enable_thinking` immutable on this family closes the hole but removes an operator switch.
- **BT-Q4** The overhead of 3 is measured on non-streaming requests on four models. **Unverified** for streaming and
  for other Qwen models; a model whose overhead is larger would be parked by UV4, not under-billed.
- **BT-Q5** A model that does not think by default but declares `thinking_budget` gets a split anyway: its answer is
  capped at `N - B - 3` though it never thinks. Safe, but the answer is shorter than it could be; operators should
  declare the word only on thinking-by-default models.
