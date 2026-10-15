# The GLM Coding Plan and dated Azure OpenAI families in `provider-openai-compatible` (host plan row T9)

Status: accepted under lv's 2026-10-15 directive (migrate by the host's native code, speed first; where native
behaviour is doubtful, reproduce it and register the doubt as **unverified**; do not wait for real-key captures, do
not query production). Every open question below (§9) carries a recommended default, and the implementation (§8)
implements the defaults.

Date: 2026-10-15

Baseline: south `origin/main` = `732cf80` (after #236; #232 already gave every package its patch for the unreleased
0.66.0 cycle and moved `south-component-conformance` to 0.65.0). Host references carry a `server:` prefix and refer to
token-station-server `origin/dev-v2` at `0f6dd6d4`; `server:…/` abbreviates `server:gateway/src/modules/inference/engine/`.
Statements marked **unverified** are conclusions from reading code, not from running anything against an upstream or
capturing traffic.

Predecessors: [`2026-08-20-controlled-user-agent.md`](2026-08-20-controlled-user-agent.md) (the controlled user-agent
slot, which named the GLM Coding Plan as one of its two first users), [`2026-08-18-controlled-query-support.md`](2026-08-18-controlled-query-support.md)
(`api-version` as a sanctioned query parameter, with Azure OpenAI as its first user),
[`2026-09-30-host-zero-vendor-boundary.md`](2026-09-30-host-zero-vendor-boundary.md) (per-family `user_agent`, config
keys with a `default`, the `api_version_date` syntax, §13.5 D7 for Copilot), and
[`2026-10-15-claude-code-family.md`](2026-10-15-claude-code-family.md) (the closest precedent: subscription families
added to an existing package inside the open cycle).

The host's native-arm deletion plan (`server:docs/product-review-v2/plans/`, dated 2026-10-12, §1.1) lists the row as
"T9 | glm_coding UA, azure dated form | no matching family | class 3 (needs a south package)". Row T8 already maps
plain `glm`, `kimi_code`, `deepinfra`, the Bailian and BytePlus text rows and Azure's v1 surface onto the existing
families by configuration alone.

## 1. What this delivers

| Item | Where | Change |
|---|---|---|
| `glm-coding` family | `provider-openai-compatible` 2.6.10 → **2.7.0** | Z.ai GLM Coding Plan Chat Completions at `{endpoint}/api/coding/paas/v4/chat/completions`, `bearer`, the `OpenCode` user-agent declared, a subscription-quota family (§3.1) |
| `azure-openai-dated` family | same | Azure OpenAI's deployment-addressed surface, `{endpoint}/openai/deployments/{deployment}/chat/completions?api-version={version}`, `header_secret` `api-key`, the version from the config key `api_version` (default `2024-10-21`) (§3.2) |
| Fixtures | `crates/south-component-conformance/fixtures/` (the package's one pack) | Six request rows (§7) |
| Tests | `tests/openai_compatible_t9_v1.rs` and the package's existing tests | Native values and native URL cases pinned (§7) |

## 2. Native behaviour (server `origin/dev-v2` `0f6dd6d4`)

### 2.1 `glm_coding`

| Behaviour | Native | Source |
|---|---|---|
| Provider type | `ProviderType::GlmCoding`, serialized `glm_coding`; wire shape `OpenaiChat` | `server:gateway/src/infra/config/providers.rs:77-78, 481, 562` |
| Endpoint default | `https://api.z.ai` (an origin; the operator may edit it) | `providers.rs:749` |
| Chat URL | Every path other than `/v1/messages`: the canonical path with `/v1` stripped, under `{endpoint}/api/coding/paas/v4`; a trailing slash on the endpoint is trimmed. Chat Completions: `{endpoint}/api/coding/paas/v4/chat/completions` | `server:…/upstream.rs:425-449`; tests `:1331-1357` |
| Messages URL | `/v1/messages` goes to Z.ai's Anthropic-compatible endpoint `{endpoint}/api/anthropic/v1/messages`, the client's Messages body passed through with `model` replaced | `upstream.rs:440-445`, test `:1319-1328`; `text_admission.rs:880-883`; `text_admission/messages.rs:257-266` |
| Surfaces | Chat (`text_admission.rs:896-910`) and Messages (above); Responses is refused (not in the Responses arm, `text_admission.rs:911-923`) | as cited |
| Auth | `Authorization: Bearer <api_key>` (the `api_key` field only; plain `glm` also accepts `oauth_token`) | `upstream.rs:153-161`; `south_adapter.rs:224-227, 422` |
| User-agent | `User-Agent: opencode/1.15.6` (`GLM_CODING_IMPERSONATION_UA`, "OpenCode, one of Z.ai's whitelisted Coding Plan tools"; without it Z.ai's anti-SDK detection may revoke the subscription) | `upstream.rs:380-389`; on the south path through the controlled user-agent slot, `south_adapter.rs:279-292`; test `upstream.rs:1211-1234` |
| Other headers | None: no companion constant headers | `south_adapter.rs:326-331` |
| Chat body | The client's Chat body with `model` replaced by the upstream model, `max_completion_tokens` rewritten to `max_tokens` (unconditional for this type) | `text_admission/chat.rs:616-650`; `providers.rs:856-880` |
| `-thinking` suffix | A model alias with the `-thinking` suffix gets `thinking: {"type": "enabled"}` injected into the canonical body unless the client set `thinking` | `server:…/body_patch.rs:504-560`; called from `server:gateway/src/modules/inference/handler/chat.rs:207-212` and the Messages and Responses handlers |
| Subscription | `is_coding_plan`: a 429 raises the subscription-quota alert | `providers.rs:627-637` |
| Pricing, output cap | Token-priced; the OpenAI cap rules | `text_admission.rs:1536-1545, 1659-1676` |

### 2.2 `azure_openai` (dated form)

| Behaviour | Native | Source |
|---|---|---|
| Provider type | `ProviderType::AzureOpenai`, serialized `azure_openai`; preset endpoint `https://<your-resource>.openai.azure.com`, extra field `api_version` defaulting to `2024-10-21` | `providers.rs:209-219, 764-768` |
| Version selection | `model.azure_api_version` > `provider.api_version` > `AZURE_OPENAI_DEFAULT_API_VERSION` (`2024-10-21`); the literal `v1` (any case) selects the unversioned surface | `upstream.rs:5-10, 551-580`; `providers.rs:979-986`; `server:gateway/src/infra/config/models.rs:326` |
| Dated URL | `{endpoint}/openai/deployments/{upstream_model}{path}?api-version={version}`, the canonical path with `/v1` stripped; the endpoint used verbatim (`<res>.openai.azure.com` or `<res>.services.ai.azure.com`), a trailing slash trimmed; neither the deployment nor the version is escaped | `upstream.rs:551-580`; tests `:1421-1520`; `server:…/text_admission/sender.rs:7966-7996` |
| v1 URL | `{endpoint}/openai/v1{path}`, deployment in the body's `model` (already served by `azure-openai-v1`, row T8) | as above |
| Surfaces | Chat (`TextTransport::Azure`), Responses when the provider supports it (`/openai/deployments/{d}/responses?api-version=`), embeddings (`/embeddings`) | `text_admission.rs:884-895`; tests `upstream.rs:1444-1462` |
| Auth | `api-key: <api_key>`, no Bearer | `upstream.rs:190-196`; test `:1044-1066` |
| Other headers | None | `south_adapter.rs:358-360` |
| Chat body | The client's Chat body with `model` replaced by the deployment name, `max_completion_tokens` rewritten to `max_tokens` unless the row sets `accepts_max_completion_tokens` | `text_admission/chat.rs:616-650`; `providers.rs:863-867` |

## 3. Family mapping

### 3.1 `glm-coding`

**Decision (T9-Q1): a new family of `provider-openai-compatible`.** The Chat leg is the OpenAI Chat Completions wire
byte for byte; what differs from `openai-compatible` is a fixed path below an origin endpoint, the declared
user-agent and the subscription flag. A user-agent is declared per family, so a family is needed: configuration of
`openai-compatible` cannot send it. A new package is rejected for the same reason as for Claude Code: the package
that speaks the wire takes the family (Copilot, `gemini-openai-compatible`).

- **URL.** `{base_url}/api/coding/paas/v4/chat/completions`, with a trailing slash on the base trimmed, as native.
  The operator's `base_url` is an origin, exactly the native row's `endpoint`, so a row moves without rewriting it
  (T9-Q2).
- **Auth.** `Auth::bearer` on the static slot `provider_api_key`.
- **Headers.** `content-type` only; the manifest declares `user_agent` `opencode/1.15.6` for this family alone, and
  the host sends it through the controlled slot, as it does today on its south path (`south_adapter.rs:279-292`).
- **Subscription.** `subscription_quota_families` gains `glm-coding` (native `is_coding_plan`).
- **Body, responses, stream, errors.** The package's shared translation, unchanged.
- **No endpoint template** (T9-Q2), no config keys, no credentials section, no request facts (the top-level fields).

Plain `glm` has no family and needs none: row T8 serves it as `openai-compatible` with the base
`https://api.z.ai/api/paas/v4` (native appends the stripped path to an endpoint that already carries `/api/paas/v4`,
`upstream.rs:436-439`). `glm_coding` differs from it in the fixed Coding Plan path below an origin, the user-agent, the
subscription flag, the credential field (`api_key` only) and the Messages leg (§5 D1).

### 3.2 `azure-openai-dated`

**Decision (T9-Q3): a new family, not a config value of `azure-openai-v1`.** The two surfaces differ in route (the
deployment in the path, the version in the query) and in what the operator configures, while body, auth and readers
are the same. A family keeps `azure-openai-v1`'s published behaviour and its rows untouched; a config value on
`azure-openai-v1` would give the v1 family a second URL shape and a key that is meaningless on its own surface.

- **URL.** `{base_url}/openai/deployments/{deployment}/chat/completions?api-version={version}`, `base_url` the
  operator's resource endpoint verbatim (trailing slash trimmed), `deployment` the request's model (the host's upstream
  model, as native), `version` the config value.
- **Config key.** `api_version`, syntax `api_version_date` (`YYYY-MM-DD` with an optional `-preview`), optional, with
  the manifest `default` `2024-10-21`. The component falls back to the same constant when the key is absent, because a
  host passes a key's default into `fill_endpoint` only and this family has no endpoint template; a test pins the two
  together.
- **Query.** `api-version` is one of the contract's sanctioned query parameters (`QueryParameterV1::ApiVersion`), so the
  manifest declares no `query_parameters`, as `provider-gemini` declares none for `alt`.
- **Auth.** `Auth::header("api-key")` on the static slot, admitted as `HeaderSecret(ApiKey)`, as `azure-openai-v1`.
- **Headers.** `content-type` only. No user-agent, no subscription flag.
- **No endpoint template** (the resource host is per operator, as for `azure-openai-v1`), no credentials section, no
  request facts: the body carries `model` (the deployment) at the top level, as native's body does, so the top-level
  locations hold (T9-Q6).

## 4. Decisions taken where native has no counterpart

- **D1. Family names** `glm-coding` and `azure-openai-dated` (T9-Q4).
- **D2. The default lives in two places**, the manifest `default` (for the operator form and a host's validation) and
  the component's constant (for a host that omits the key). Both are native's `2024-10-21`; a test pins them equal.
- **D3. A version that is not dated is refused** with `internal` 500 before any request is built, so nothing but a
  dated value reaches the query even from a host that skipped the manifest syntax check. `v1` belongs to
  `azure-openai-v1`.
- **D4. An empty model is refused** with `capability` 400: native would send `/openai/deployments//chat/completions`,
  which the kernel's endpoint check refuses anyway (an empty segment).
- **D5. The deployment is one path segment**, escaped with the crate's `url_segment` encoder (as Gemini's model is).
  Azure deployment names are letters, digits, `-`, `_` and `.`, which the encoder leaves unchanged.

## 5. Differences from native

| # | Difference | Status |
|---|---|---|
| D1 | `glm-coding` serves Chat Completions only. On a component row the host translates a Messages request through the IR to this Chat endpoint; native passes the client's Messages body to Z.ai's Anthropic-compatible endpoint `{endpoint}/api/anthropic/v1/messages`. What the Chat translation loses on that surface (thinking blocks, cache markers, Anthropic-only fields) is lost for `glm-coding` Messages clients. | **Unverified** impact; T9-Q5 (an Anthropic-wire family later if the dual run shows a loss that matters) |
| D2 | The client's Chat body is rebuilt from the IR, so fields the IR does not carry are not sent. Native forwards the client's body; for Z.ai that includes `thinking`, `do_sample`, `request_id`, `tool_stream` and similar GLM-only fields. Shared with every `openai-compatible` row (plain `glm` on T8 has the same loss). | **Unverified** impact; shared with T8 |
| D3 | The `-thinking` suffix: native injects `thinking: {"type": "enabled"}` by provider type (`Glm`, `GlmCoding`) into the client body. A component row is `ProviderType::Component`, so the host does not inject it, and the component has no IR field it could render it from (the IR's reasoning mode is rendered as `reasoning_effort` when the model declares it). | Registered; host prerequisite H4 (shared with Bedrock Mantle D6) |
| D4 | `max_completion_tokens`: the component always writes the IR cap as `max_tokens`. Native rewrites to `max_tokens` too, except on an `azure_openai` row with `accepts_max_completion_tokens` set, which passes `max_completion_tokens` through. Shared with `azure-openai-v1`. | Registered |
| D5 | The per-model version override (`ModelConfig.azure_api_version`) has no counterpart: the version is a row value. A model whose override differs from its row's version needs a row of its own. | Host prerequisite H3 |
| D6 | A version that is not `YYYY-MM-DD[-preview]` (other than `v1`): native sends it as is; the family refuses it (§4 D3) and a host's validation refuses to save it. **Unverified** whether any live row carries such a value. | Registered |
| D7 | A deployment name with a character outside RFC 3986 `pchar`: native sends it raw, the family escapes it (§4 D5). Unreachable for valid Azure deployment names. | Registered |
| D8 | Dated Responses (`/openai/deployments/{d}/responses?api-version=`) and dated embeddings are not covered: `provider-openai-responses` and `embeddings-openai-compatible` have no dated family. A dated `azure_openai` row whose models serve Responses or embeddings keeps those surfaces on the native arm, or moves to the v1 surface. | Out of T9's scope; T9-Q7 |
| D9 | `glm-coding` reads the credential's `api_key` slot only, as native (plain `glm` also took `oauth_token`). | Identical |
| D10 | Operator header extras, `x-request-id` and `traceparent` forwarding: whatever the host does for component rows applies. | Host-generic |

## 6. Host prerequisites for cutover

- **H1. Families and pins.** The host re-pins to the release that ships `provider-openai-compatible` 2.7.0 and records
  `south_runtime` 0.60.0 or newer for it (unchanged: the package already declared `subscription_quota_families`).
- **H2. Row migration.** A `glm_coding` row becomes a `component` row with dialect `glm-coding` and the same endpoint
  (an origin) and `api_key` credentials. A dated `azure_openai` row (effective version not `v1`) becomes a
  `component` row with dialect `azure-openai-dated`, the same endpoint and `api_key` credentials, and the config
  value `api_version` = the row's `api_version` (omitted when unset, so the default applies). A row whose effective
  version is `v1` goes to `azure-openai-v1` with the base `{endpoint}/openai/v1` (row T8).
- **H3. Per-model versions.** Before migrating an `azure_openai` row, every model whose `azure_api_version` differs
  from the row's effective version moves to a row of its own (a `v1` override to an `azure-openai-v1` row), since the
  version is now per row (D5). A preflight should list such models.
- **H4. A generic host model-suffix mechanism for component rows.** Native maps model-alias suffixes to request
  fields by provider type (`body_patch.rs:504-560`); a component row is `ProviderType::Component`, so it gets none of
  them (D3). The host needs one generic mechanism that applies a suffix to component rows by declaration rather than by
  provider type, or accepts the loss. Bedrock Mantle needs the same mechanism for its `-high` suffix
  ([`2026-10-15-bedrock-mantle.md`](2026-10-15-bedrock-mantle.md) §7 D6, §9 H5), so one host change serves both rows.
  Until it lands, a `glm_coding` model reached through a `-thinking` alias stays on the native arm.
- **H5. The query on component rows.** The host's component transport passes the descriptor's `api-version` query
  through as a sanctioned parameter, as it does for `provider-gemini`'s `alt=sse`. **Unverified by a run** for this
  family; the host's dual run is where it is first exercised.
- **H6. The user-agent on component rows.** The host fills the controlled slot from the manifest's declared value per
  family (as for `github-copilot` and `claude-code`). Z.ai may revoke a Coding Plan subscription that is used without
  a whitelisted user-agent, so the dual run must assert `user-agent: opencode/1.15.6` on the wire before cutover.
- **H7. Messages traffic on `glm-coding`.** Because of D1, a host that wants native's Messages behaviour keeps Messages
  clients of `glm_coding` models on the native arm until the dual run accepts the translated path or T9-Q5 lands.
- **H8. The dual run.** A replay-style dual run of each native arm against its family with the differences of §5
  registered, as the R2 and I3 precedents do; lv's 2026-10-15 ruling makes a passing dual run sufficient for cutover.

## 7. Fixtures and tests

Six request rows join the package's pack `fixtures/`, transcribed from native's URL tests and auth arms:
`glm-coding` (non-streaming, cap and temperature), `glm-coding-stream` (trailing slash on the endpoint),
`azure-openai-dated` (`2025-04-01-preview` on the classic host), `azure-openai-dated-default-version` (no value, the
`services.ai.azure.com` host, streaming), and the refusals `azure-openai-dated-refuses-an-undated-version` (`v1`) and
`azure-openai-dated-refuses-a-request-without-a-model`. Gate ② judges them natively and in the sandbox
(`sandbox_parity_v1`), so the release's gate ② report covers both families.

`openai_compatible_t9_v1` pins: the Coding Plan path, Bearer and no other header; the declared user-agent against
native's literal; the subscription set; that `glm-coding` builds the same descriptor as `openai-compatible` at the
Coding Plan root; native's six dated-URL cases (both hosts, explicit and default versions, the per-provider preview,
a trailing slash); `api-key` admission; that `azure-openai-dated` differs from `azure-openai-v1` only in the URL; the
manifest default against native's literal; the refusals of §4 D3 to D5; the escaped deployment segment; and that an
undeclared family (`glm_coding`, `azure_openai`, `glm`, near misses) is refused.

`undeclared_values_v1`'s reader that acts on any `declared` key now reads every value rather than the first, because
`azure-openai-dated`'s own `api_version` sorts before the injected key and would otherwise hide the read.

## 8. Implementation and versioning (Q47)

`reference.rs` gains the modules `glm_coding` and `azure_openai_dated` (the native constants), the dated URL builder,
and the two families in the URL and auth dispatch; an undeclared family is still refused. The component crate is
unchanged apart from its version: it wraps the same reference.

No Q47 round: #232 already moved `south-component-conformance` to 0.65.0 and gave every package its patch for the
unreleased 0.66.0 cycle; this change rides on both. Only the changed package moves:

| Item | Before | After |
|---|---|---|
| `provider-openai-compatible` | 2.6.10 (unreleased patch) | **2.7.0** (new families) |
| its `south_runtime` | 0.60.0 | 0.60.0: every field it uses (`user_agent`, a config key with a `default`, `api_version_date`, `subscription_quota_families`) is parsed by 0.60.0 |
| `south-component-conformance`, `south-provider-api`, `south-contracts` | 0.65.0, 0.58.0, 0.56.0 | unchanged |
| every other package | — | unchanged |

`shipped_packages_v1` records the move (the provider-vertex retirement row now ends at 2.7.0).

## 9. Open questions (defaults implemented)

- **T9-Q1** `glm-coding` as a family of `provider-openai-compatible` (§3.1). **Default: yes.**
- **T9-Q2** `glm-coding`'s `base_url` is the native origin endpoint with the Coding Plan path appended, and no
  endpoint template is declared, so an operator-edited native endpoint (Z.ai's other hosts) keeps working.
  **Default: yes.** The alternative, a fixed endpoint `https://api.z.ai`, would refuse every other host.
- **T9-Q3** The dated form as a new family rather than a config value of `azure-openai-v1` (§3.2). **Default: new family.**
- **T9-Q4** Family names `glm-coding` and `azure-openai-dated`. **Default: as named.**
- **T9-Q5** Native's Messages leg (Z.ai's Anthropic-compatible endpoint) as a `provider-anthropic` family. **Default:
  not now**; Messages goes through the IR to the Chat endpoint (D1), and a later family follows only if the dual run
  shows a loss that matters. It would move `provider-anthropic` again in this cycle.
- **T9-Q6** No request facts for `azure-openai-dated`: the model is checked at the body's top-level `model`, not in the
  URL. **Default: yes**, since native's body carries the deployment too.
- **T9-Q7** Dated Responses and embeddings families (D8). **Default: not in T9**; a host that needs them before the v1
  surface is adopted asks for them as a separate row.
- **T9-Q8** Keep native's user-agent `opencode/1.15.6` and default version `2024-10-21` as they are, to be bumped
  together with the host's constants. **Default: native values.**

## Revision note

- 2026-10-15: first draft, accepted under lv's directive of the same day; implemented with the defaults.
