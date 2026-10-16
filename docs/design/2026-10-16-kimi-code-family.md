# The Kimi Code family in `provider-openai-compatible`

Status: accepted. lv ruled on 2026-10-16 (relayed by the host session) that `provider-openai-compatible` gains a
`kimi-code` family, following the `glm-coding` precedent
([`2026-10-15-openai-compatible-t9.md`](2026-10-15-openai-compatible-t9.md)), migrated by the native code under lv's
2026-10-15 directive (reproduce native; register doubts as **unverified**; no real-key captures, no production queries).
The open questions (§7) carry defaults, and the implementation implements them.

Date: 2026-10-16

Baseline: south `origin/main` = `cf7c51e` (v0.69.0 plus records). Host references carry a `server:` prefix and refer to
token-station-server `origin/dev-v2` at `0f9eef6d`; `server:…/` abbreviates `server:gateway/src/modules/inference/engine/`.
Statements marked **unverified** are conclusions from reading code, not from running anything against an upstream or
capturing traffic.

## 1. Why a family

The host's native-arm deletion plan maps `kimi_code` onto the existing `openai-compatible` family by configuration
alone (plan row T8; T9 record, preamble). That keeps the wire but loses one host behaviour: the 429 subscription-quota
alert. Native raises `coding_plan_quota_exhausted` on a 429 when the row's type is a coding plan
(`server:gateway/src/infra/config/providers.rs:592-634`, `is_coding_plan`, which lists `KimiCode`) or when the row's
component family is declared in `subscription_quota_families` (`server:…/text_admission/sender.rs:497-514`). An
`openai-compatible` row is neither, so after migration a Kimi Code seat past its window quota would 429 silently. That
is the same shape as the earlier `kimi_code` incident, where a missing parse arm typed the rows as `openai` and the
alert stopped (`server:…/config_seed/tests.rs:6-12`). The quota declaration is per family, so a family is needed;
configuration of `openai-compatible` cannot say it. The host needs no code: after the release it re-pins, operators
migrate the rows, and the `KimiCode` variant is deleted.

## 2. Native behaviour (server `origin/dev-v2` `0f9eef6d`)

Every non-test hit of `KimiCode` / `kimi_code` under `server:gateway/src` is accounted for below.

| Behaviour | Native | Source |
|---|---|---|
| Provider type | `ProviderType::KimiCode`, serialized `kimi_code`; "Same wire format as the metered `kimi`/OpenAI provider", a long-lived subscription token, no OAuth refresh; wire shape `OpenaiChat` | `providers.rs:96-106, 474, 561`; parse / print `server:…/config_seed/mod.rs:516, 734` |
| Endpoint | The type's admin-form preset is `https://api.moonshot.cn` (an origin, a prefill the operator may edit); `supports_responses` off and `accepts_max_completion_tokens` on, as the closure `d` defaults | `providers.rs:699-706, 746` |
| Chat URL | `build_upstream_url` has no `KimiCode` arm, so the default arm `{endpoint}{api_path}` applies, the endpoint's trailing slash trimmed: `{endpoint}/v1/chat/completions` | `server:…/upstream.rs:425-430, 592`; the Chat target arm `server:…/text_admission.rs:961-976`; test `upstream/tests.rs:581-596` |
| Auth | `Authorization: Bearer <api_key>` (`require_api_key`: the `api_key` field only), no user-agent impersonation, no refresh | `upstream.rs:162-171`; test `upstream/tests.rs:555-579`; south path `server:…/south_adapter.rs:165-177` (`Bearer`), `:306-332` (no companion headers), `:279-288` (no controlled user-agent) |
| Chat body | A row without `south_dialect` sends the client's Chat body with `model` replaced, `max_completion_tokens` rewritten to `max_tokens` and, when streaming, `stream_options.include_usage` | `server:…/text_admission/chat.rs:736-796`; `providers.rs:967-992` (`_ => true`) |
| Cap key | `max_tokens`, unless the row's `max_tokens_field` column is set, which decides alone | `providers.rs:967-992` |
| Messages | Not passed through: the client's Messages body is decoded to the IR and rendered by the `openai-compatible` translator (no `thinking` from the reasoning mode), then the host body policy (cap key, `include_usage`, `reasoning_content` stripped unless the model's `upstream_requires_reasoning_content`) | `server:…/text_admission/messages.rs:372-459` |
| Responses | The preset leaves `supports_responses` off, so a Responses request skips the native Responses branch and is served through the IR and the Chat target, as for `glm_coding` (T9 record §2.1). A row with `supports_responses` on, or a model with `upstream_requires_responses`, takes the native Responses branch, which has no `KimiCode` URL arm and is a 400 | `server:…/text_admission/sender/responses.rs:556-566, 763-766`; `text_admission.rs:977-989, 1044-1048` |
| Reasoning replay | Classified as `ChatCompatible` unless the row declares the `anthropic` dialect, exactly as `GlmCoding` | `server:…/text_admission/reasoning_replay.rs:150-185` |
| Model suffixes | No `-thinking` or `-high` mapping for `KimiCode` (only `Glm` / `GlmCoding` / Bailian, and the OpenAI types) | `server:…/body_patch.rs:517-561` |
| Output-cap validation, pricing | OpenAI cap rules; token-priced (`BoundedToken`); the type comment says the plan's models are priced at `$0`, which is catalog data | `text_admission.rs:1597-1605, 1722-1738`; `providers.rs:103-104` |
| Subscription | `is_coding_plan`: a 429 raises `coding_plan_quota_exhausted` | `providers.rs:592-634`; `sender.rs:497-514` |
| Images | No image surface (`None` in the image-observation table) | `server:gateway/src/modules/inference/handler/images/observe.rs:107-117` |
| Admin type list | Listed among the coding-plan types of the creation form | `providers.rs:668-674` |

The remaining hits are tests (`upstream/tests.rs`, `south_switch.rs`, `config_seed/tests.rs`, `byok/service/tests.rs`,
`south_adapter/tests.rs`, `sender/tests.rs`, `infra/config/tests*.rs`) and one comment in
`catalog/repo/providers/providers_models_db_tests.rs`.

## 3. Mapping

**Decision (KC-Q1, ruled): a new family `kimi-code` of `provider-openai-compatible`.** The wire is the package's
`openai-compatible` Chat Completions wire byte for byte; what the family adds is native's URL composition and the
subscription flag.

- **URL.** `{base_url}/v1/chat/completions`, the base's trailing slash trimmed (`reference::kimi_code::CHAT_PATH`),
  native's default arm. On an origin this equals what `openai-compatible` resolves (the kernel's `resolve` adds `/v1`
  to an origin-only base); on an endpoint with a path it does not (`resolve` keeps a custom root and appends
  `chat/completions` without `/v1`), and only the fixed path matches native (D1, KC-Q2).
- **Auth.** `Auth::bearer` on the static slot `provider_api_key`, as `glm-coding`.
- **Headers.** `content-type` only. No `user_agent` declaration: native impersonates none.
- **Subscription.** `subscription_quota_families` gains `kimi-code` (native `is_coding_plan`).
- **Client fields.** `kimi-code` joins `client_passthrough::FAMILIES`: native sends a `kimi_code` row's client body
  (`chat.rs:736-796`), so a client's `thinking` / `enable_thinking` is forwarded verbatim when sent (T9-Q9 (a)).
- **Reasoning mode.** Not rendered: `render_glm_thinking` (A+) stays `glm-coding`'s, since native's Messages path for
  `kimi_code` renders through the `openai-compatible` translator, which writes no `thinking`.
- **Cap key.** `max_tokens`, the package's default for every non-Azure family.
- **No endpoint template** (KC-Q3), no config keys, no credentials section, no request facts. The manifest's `endpoint`
  field is a fixed template, not a form prefill; declaring `https://api.moonshot.cn` would refuse every edited endpoint.
  The native preset is kept as `reference::kimi_code::NATIVE_PRESET_ENDPOINT` for the tests only.

## 4. Differences from native

| # | Difference | Status |
|---|---|---|
| D1 | URL. Identical for every endpoint a native row can hold, origin or path. Edge: the kernel normalizes a base whose path ends in `/chat/completions`, `/responses`, `/messages` or `/models` by dropping that suffix, and refuses ambiguous paths (dot segments, repeated separators); native would append to such an endpoint verbatim. A native row with such an endpoint already sends a doubled path. | Registered; unreachable for a working row |
| D2 | The client's Chat body is rebuilt from the IR, so client fields the IR does not carry are not sent, except `thinking` and `enable_thinking` (forwarded). Shared with every `openai-compatible` row (T9 D2). | **Unverified** impact |
| D3 | Host body policy on Messages and on Responses through Chat: native applies `align_host_body_policy` (cap key, `include_usage`, `reasoning_content` stripped unless the model's `upstream_requires_reasoning_content`); a component row gets no such policy. The package writes `max_tokens`, writes `include_usage` itself when streaming, and writes `reasoning_content` only when the model declares the parameter (`supported_parameters`). Host-generic, shared with `glm-coding` and every T8 row. | **Unverified** impact; host-generic |
| D4 | Responses through Chat: T9 D11 applies (an assistant turn whose IR carries reasoning plus text: native collapses the text segments to a string, the component sends an array). A row with `supports_responses` on, or a model with `upstream_requires_responses`, is a 400 natively; a component row serves it through Chat. | Registered |
| D5 | The cap key: `max_tokens`; a native row whose `max_tokens_field` column says otherwise sends that key natively. Shared with T9 D4; the host preflight lists such rows. | Registered |
| D6 | User-agent: neither side declares one; whatever the host's HTTP client sends by default goes on both. | Host-generic |
| D7 | Credential: the `api_key` field only, as native's `require_api_key`. | Identical |
| D8 | Model suffixes: native has none for `kimi_code`, so there is nothing to reproduce. | Identical |
| D9 | Operator header extras, `x-request-id`, `traceparent`: whatever the host does for component rows. | Host-generic |

## 5. Host prerequisites for cutover

- **H1. Re-pin** to the release that ships `provider-openai-compatible` 2.10.0; its `south_runtime` stays 0.60.0
  (`subscription_quota_families` needs nothing newer).
- **H2. Row migration.** A `kimi_code` row becomes a `component` row with dialect `kimi-code`, the **same** endpoint
  (do not append `/v1`: the family appends it) and the `api_key` credential. A row already migrated to
  `openai-compatible` under T8 moves to `kimi-code`: an origin base is kept; a base that ends in `/v1` (T8's way of
  reaching native's URL on an endpoint with a path) drops that `/v1`, or the URL doubles it. The preflight should check
  both and list rows whose `max_tokens_field` column is set (D5) or whose `supports_responses` / model
  `upstream_requires_responses` is on (D4).
- **H3. The alert.** The dual run asserts that a 429 on a `kimi-code` row raises `coding_plan_quota_exhausted` with
  `family = kimi-code`; this is the behaviour the family exists for.
- **H4. Variant deletion.** After the rows are migrated, `ProviderType::KimiCode` is deleted (S7); the vendor-identifier
  vocabulary is frozen, so the deletion does not blind the J1 gate.

## 6. Fixtures and tests

Five request rows join the package's pack (`crates/south-component-conformance/fixtures/`): `kimi-code` (the native
preset origin, cap and temperature), `kimi-code-stream` (trailing slash, streaming), `kimi-code-endpoint-with-a-path`
(`https://api.kimi.com/coding`; which host live rows name is **unverified**, and either works since the endpoint is
taken as is), `kimi-code-client-thinking` (forwarded) and `kimi-code-ignores-reasoning-mode` (no `thinking` from
`reasoning.mode`). Gate ② judges them natively and in the sandbox (`sandbox_parity_v1`).

`tests/kimi_code_v1.rs` pins native's URL cases (origin, trailing slash, a path) against native's literals; agreement
with `openai-compatible` on an origin only, and that a T8 row reaches the same descriptor with the base `{endpoint}/v1`;
Bearer and no other header, admitted from the static slot; the subscription set, no user-agent, no endpoint template,
no config keys, no recipe, top-level request facts; the client field passthrough; no `thinking` from the reasoning
mode on Chat or from a Messages body; `max_tokens`; refusal of near-miss family names (`kimi_code` among them); and
that gate ② runs every row. The package's existing family lists (`component_conformance_v1`,
`gemini_openai_compatible_v1`, `request_facts_v1`, `sandbox_parity_v1`, `openai_compatible_t9_v1`) gain the family.

## 7. Open questions (defaults implemented)

- **KC-Q1** A family of `provider-openai-compatible`, named `kimi-code`. **Ruled** (lv, 2026-10-16).
- **KC-Q2** The URL as native's fixed path below the operator's endpoint rather than the kernel's `resolve`.
  **Default: fixed path**, because it equals native on every endpoint, while `resolve` equals it on an origin only.
- **KC-Q3** No endpoint template and no default endpoint in the manifest. **Default: none**, as T9-Q2 for
  `glm-coding`: native's preset is a form prefill, and a template would refuse edited endpoints.

## 8. Versioning (Q47)

The change touches `south-component-conformance`, which every guest links, after release 0.69.0, so it is a Q47 round
against v0.69.0 and opens the 0.70.0 cycle:

| Item | Before | After |
|---|---|---|
| `south-component-conformance` | 0.68.0 | **0.69.0** |
| `provider-openai-compatible` | 2.9.0 | **2.10.0** (a new family) |
| its `south_runtime` | 0.60.0 | 0.60.0 |
| every other package | as published with 0.69.0 | one patch, unchanged behaviour, `south_runtime` and contracts unchanged |
| `south-contracts`, `south-provider-api` | 0.56.0, 0.59.0 | unchanged |

`shipped_packages_v1::kimi_code_retires_every_published_069_package_identity` pins all 40 identities; the 0.68.0
round's table now asserts only that its identities retired.
