# The Claude Code subscription families in `provider-anthropic`

Status: accepted under lv's 2026-10-15 directive ("claude-code and Mantle too, by the existing code": migrate by the
host's native code, speed first; where native behaviour is doubtful, reproduce it and register the doubt as
**unverified**; do not wait for real-account captures, do not query production). Every open question below (§11) carries
a recommended default, and the implementation (§10) implements the defaults.

Date: 2026-10-15

Baseline: south `origin/main` = `6f4a555` (after #232, which gave every package its patch for the unreleased 0.66.0
cycle and moved `south-component-conformance` to 0.65.0). Host references carry a `server:` prefix and refer to
token-station-server `origin/dev-v2` at `dcfaf26d`; `server:…/` abbreviates `server:gateway/src/modules/inference/engine/`.
Statements marked **unverified** are conclusions from reading code, not from running anything against the upstream or
capturing traffic.

Predecessors: [`2026-08-20-controlled-user-agent.md`](2026-08-20-controlled-user-agent.md) (names `claude-code`'s
login-token user-agent), [`2026-09-30-host-zero-vendor-boundary.md`](2026-09-30-host-zero-vendor-boundary.md) (recipes
§3, §13.3, §13.5; Q10 headers in the component; §13.8 D6), [`2026-10-12-responses-r3-codex-and-request-fields.md`](2026-10-12-responses-r3-codex-and-request-fields.md)
(the closest precedent: a subscription OAuth family added to an existing provider package), the Kiro and Copilot
recipes (`provider-kiro`, `provider-openai-compatible`), and [`2026-10-12-kc1-canonical-ir-4-south.md`](2026-10-12-kc1-canonical-ir-4-south.md)
(canonical IR 4, which the host plan row T2 named as the Messages-surface prerequisite; shipped in 0.57.0).

## 1. What this delivers

| Item | Where | Change |
|---|---|---|
| `claude-code` family | `provider-anthropic` 1.1.10 → **1.2.0** | The login-token form: minted access token in `x-api-key`, the CLI's beta set and headers, the identity system prompt, the CLI user-agent (§3) |
| `claude-code-setup-token` family | same | The setup-token form: Bearer, the OAuth beta flag (§3) |
| Recipe `claude_code` | the package's `credentials` section, scoped to `claude-code` | The native refresh (§4) |
| Fixture pack | `crates/south-component-conformance/fixtures-claude-code/` | Judged together with `fixtures-anthropic/` (§8) |

## 2. Native behaviour (server `origin/dev-v2` `dcfaf26d`)

The host has one provider type, `claude_code`, and decides the request shape **per credential**: a credential with a
refresh token is the login-token form, one without is the setup-token form (`server:…/claude_code.rs:22-26, 45-49`;
`south_switch.rs:374-378` uses the same test, an empty string counting as present).

| Behaviour | Native | Source |
|---|---|---|
| URL | `{endpoint}/v1/messages`, endpoint default `https://api.anthropic.com` | `text_admission.rs:778-781`; `server:gateway/src/infra/config/providers.rs:748` |
| Surfaces | Chat, Messages and Responses north surfaces, all translated to the Messages upstream with the `anthropic` translation | `text_admission/chat.rs:699`, `text_admission/messages.rs:257`, `text_admission/sender/responses.rs:531-540`, `south_component.rs:652-659` |
| Common headers | `content-type: application/json`; `accept: text/event-stream` (stream) or `application/json`; `anthropic-version: 2023-06-01` | `claude_code.rs:180-192` |
| Setup-token form | `Authorization: Bearer <oauth_token>`; `anthropic-beta: oauth-2025-04-20`; no user-agent; no body change | `claude_code.rs:36-38, 160-167, 206-213`; `text_admission/sender/messages.rs:271-279` |
| Login-token form | `x-api-key: <access token>`; `anthropic-beta: claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,prompt-caching-scope-2026-01-05`; `user-agent: claude-cli/2.1.114 (external, cli)`; `x-app: cli`; `anthropic-dangerous-direct-browser-access: true` | `claude_code.rs:193-205`; `upstream.rs:363, 369` |
| Identity | Login form only: `"You are Claude Code, Anthropic's official CLI for Claude."` as the first system block, idempotent | `claude_code.rs:51-104`; `upstream.rs:376-377`; applied after sealing, `text_admission.rs:316-331` |
| Client headers (Messages surface) | A client `anthropic-version` replaces the default; a client `anthropic-beta` is appended after the required set unless already in it | `text_admission/sender/messages.rs:323-345, 387-419` |
| Header extras | Operator header extras are refused for `claude_code` rows; body extras apply before the identity is injected | `request_extras.rs:1044-1057`; `text_admission.rs:316-331` |
| Refresh | `POST https://console.anthropic.com/v1/oauth/token`, JSON `{grant_type: refresh_token, refresh_token, client_id: 9d1c250a-e61b-44d9-88ed-5944d1962f5e}`; `expires_in` seconds (stored in milliseconds); rotates; 300 s margin; a fresh stored token is used without refresh | `token_refresh.rs:724-855`, margin `:52` |
| Refresh failures | Every non-2xx is a `BadRequest` for the request; 400 and 401 also latch the credential (`reauth_required`) until an operator edit; transport failure 503 | `token_refresh.rs:259-300, 834-842, 657-666` |
| Import | `~/.claude/.credentials.json`: `claudeAiOauth.accessToken`, `.refreshToken`, `.expiresAt` (epoch ms) | `server:gateway/src/modules/admin_ui/handler/credentials/mod.rs:218-232` |
| Save rule | kind `oauth`, `oauth_token` required, refresh token optional | `credentials/mod.rs:362-366, 1226-1235` |
| Subscription | `is_coding_plan` (429 subscription-quota alert) | `providers.rs:627-637` |
| Health | Token probe through `provider_pool::apply_auth`, which always presents the setup shape | `providers.rs:364-375`; `provider_pool.rs:522-529`; `upstream.rs:266-277` |
| BYOK | A minting provider for BYOK | `server:gateway/src/modules/byok/service.rs:299-309` |
| Pricing | Token-priced like `anthropic` | `text_admission.rs:1655-1675` |
| Output cap | `max_tokens` | `text_admission.rs:1480-1488` |
| Reasoning replay | Same leg as `anthropic` (`claude-signed-thinking`) | `text_admission/reasoning_replay.rs:151` |

## 3. Family mapping

**Decision (CC-Q1): two families, one per token form.** The native arm switches the request shape on the presence of a
refresh token. Three shapes in south could express that; only one uses forms south already ships without changing
`south-provider-api`:

- **One family with a selector** (boundary record §13.8 D6, which sketched exactly this: a selector over a `login` and a
  `setup` recipe, exporting `selected_recipe` so the component branches). The setup form presents the operator's token
  as is. A complete recipe needs one to four steps (`south-provider-api` `credentials.rs`, `check_recipe`), and a
  field candidate in `present` is reached only after the steps run, so the setup recipe would have to be a dead
  refresh step ending in `without_refresh_material: use_stored`. The host's `use_stored` presents the stored *minted*
  value (server P21 plan Q-K), which an operator's edit clears and a hand-entered setup token never fills. Allowing a
  zero-step recipe whose `present` is fields only would fix it, but that is a `south-provider-api` change (a crate
  bump and a lockfile wave) and a host interpreter change. And the per-family `user_agent` would put the CLI
  user-agent on setup-token requests too.
- **Two families** (chosen). `claude-code` is the login-token form: the `credentials` section applies to it alone, with
  the refresh recipe minting `provider_api_key`, and the manifest's `user_agent` declares the CLI value for it alone.
  `claude-code-setup-token` keeps the static slot, so the operator's setup token is the key, as the native arm uses
  `oauth_token` directly. Every native header, the user-agent included, is reproduced exactly per form.
- **A new package.** Rejected: the Codex precedent adds a subscription family to the package that speaks its wire
  (R3 record §1); Claude Code speaks the Messages wire byte for byte apart from headers, auth and the identity block.

The cost of two families is the difference D1 (§6): the form is chosen per provider row, not per credential. **Cutover consequence:** native mixes both forms in one row, so every native `claude_code` row must be split by form before it is routed to the component (host / ops prerequisite H2, §7).

| Family | Auth | Headers beyond `content-type` and `anthropic-version` | Body |
|---|---|---|---|
| `anthropic` | `x-api-key` (unchanged) | none (unchanged) | the Messages body |
| `claude-code` | `x-api-key` with the minted access token | `accept`, `anthropic-beta` (CLI set), `x-app: cli`, `anthropic-dangerous-direct-browser-access: true`; user-agent from the manifest | the Messages body, identity first in `system` |
| `claude-code-setup-token` | `Authorization: Bearer` with the operator's token | `accept`, `anthropic-beta: oauth-2025-04-20` | the Messages body |

Surfaces: the component sees only the IR, so every north surface the host serves for a component row (Chat, Messages,
Responses through the north codecs) reaches the Messages upstream, as native does. No `north_passthrough` is declared,
as for `anthropic`. The response, stream, error and usage functions take no configuration and are the `anthropic`
family's unchanged.

The identity is injected after the body is built, with the native function's rules transcribed: absent, the identity
is the only block; a string becomes the identity block followed by the string as a text block; a block array (the body
builder emits one when a system part carries a prompt-cache marker) gains the identity as its first block; a string
that starts with the identity, or an array whose first block's text does, is left alone.

## 4. The credential recipe

### 4.1 Shape

```text
providers:            ["anthropic", "claude-code", "claude-code-setup-token"]
auth_arms:            ["bearer", "header_secret"]
endpoint:             all three https://api.anthropic.com
user_agent:           claude-code: "claude-cli/2.1.114 (external, cli)"
subscription_quota_families: ["claude-code", "claude-code-setup-token"]
immutable_body_paths: claude-code: ["system"]                         (CC-Q4)
credentials:          families ["claude-code"]
  fields:   refresh_token (secret, required)
  import:   refresh_token <- claude-code-credentials-json /claudeAiOauth/refreshToken
  seed:     provider_api_key <- /claudeAiOauth/accessToken, expires_at epoch_millis /claudeAiOauth/expiresAt
  slots:    provider_api_key minted by "claude_code"
  recipe claude_code:
    step refresh: oauth2_token, json, https://console.anthropic.com/v1/oauth/token, requires refresh_token,
                  params client_id (const 9d1c250a-e61b-44d9-88ed-5944d1962f5e), grant_type (const refresh_token),
                  refresh_token (field);
                  on_status 400 reauth_required, 401 reauth_required, 4xx transient (5xx: default transient);
                  extract access_token /access_token, refresh_token /refresh_token (optional),
                          expires_at relative_seconds /expires_in
    present refresh.access_token; rotates; write_back refresh_token <- refresh.refresh_token;
    without_refresh_material fail; default_seconds 60
```

### 4.2 Choices against native

- **`on_status`** reproduces native latching: only 400 and 401 latch the credential (`reauth_required`); every other
  4xx and every 5xx fails that attempt and is tried again on the next request (`transient`). This is Kiro's table.
- **`default_seconds` 60** (CC-Q6): native stores no expiry when `expires_in` is absent and so refreshes on the next
  request; the recipe has no "no expiry" form (a clock that yields none is `transient` without a default), and 60 s is
  the shortest the host clamp allows.
- **No refresh margin is declared** (the Codex precedent, R3 §9.2 item 6): native refreshes 300 s early; gate ① would
  need `min_ttl_seconds` above 300 to declare that. The host default (30 s) applies.
- **The seed** takes the access token and its millisecond expiry from the imported file, so an imported login is used
  without an exchange until it is due, as native stores it.
- **`refresh_token` required**: the section applies to the login-token form only, whose defining field it is. Native
  requires `oauth_token` instead; here the access token arrives only through the seed or the refresh.
- The native test seam `extras.tokenUrl` (`token_refresh.rs:716-722`) is not reproduced: recipes have none by design.

## 5. Decisions taken where native has no counterpart

- **D1. The family names.** `claude-code` for the login-token form (the name the controlled user-agent record already
  used for it) and `claude-code-setup-token` for the setup-token form.
- **D2. `accept` per mode** is sent by both Claude Code families and not by `anthropic`, as native does.
- **D3. `immutable_body_paths`** `system` for `claude-code` only (CC-Q4).
- **D4. `metadata.user_id`.** The package declares `host_values: end_user_hash` for every family, so a host that sends
  the hash gets `metadata.user_id` on Claude Code requests too, as on `anthropic` (KC1 record §5). Native sends the
  client's own `metadata` on the Messages surface pass-through. **Unverified** whether Anthropic's subscription
  validator inspects `metadata.user_id`.

## 6. Differences from native

| # | Difference | Status |
|---|---|---|
| D1 | The token form is chosen per provider row (family), not per credential. A native row holding both kinds of credential must be split into two rows before routing. | Justified (§3); host / ops prerequisite H2 (§7) |
| D2 | A client's `anthropic-beta` and `anthropic-version` (Messages surface) are not sent: the component does not see client headers. Native appends the client's beta flags after the required set (e.g. `context-1m-*`, `fine-grained-tool-streaming-*`). The `anthropic` family has the same loss today. | **Unverified** impact on Claude Code clients; registered |
| D3 | The identity is injected into the body the component builds from the IR, not into the client's own Messages body. A client's `system` that the north codec does not carry verbatim (non-text system blocks) differs exactly as it does for `anthropic`. | Shared with `anthropic` |
| D4 | Operator body extras cannot set or remove `system` on `claude-code` (immutable). Native applied body extras before the identity, so an extra could set `system` and the identity was still prepended. | Justified (CC-Q4): keeps the identity first |
| D5 | Operator header extras: native refuses them for `claude_code`; a component row has no declaration for that, so whatever the host does for component rows applies. | **Unverified**; host prerequisite H5 |
| D6 | A refresh response without `expires_in`: native refreshes again on the next request; the recipe caches the token 60 s. | Registered (CC-Q6) |
| D7 | Refresh margin 30 s (host default) instead of native 300 s. | Registered (Codex precedent) |
| D8 | A refresh response without `refresh_token` keeps the stored one (no-wipe). Native passes `None` into its CAS write; whether that clears the column, and so flips the credential to the setup form, depends on `write_refreshed_cas`. | **Unverified** native defect; the recipe's behaviour is the safe one |
| D9 | A 5xx or a non-latching 4xx from the token endpoint: native returns 400 to the client; the recipe is `transient`, which the host maps to its own failure class. Latching is identical. | Registered (as Codex RQ3-Q9, Kiro) |
| D10 | A hand-entered login credential with an access token and an expiry: native uses the access token until it is due; the recipe has no manual seed, so the first request refreshes. Imports are identical. | Registered |
| D11 | The setup-token family keeps a static slot: whatever the host does for static slots applies (no refresh, no latch on a 401 from the upstream). Native also never refreshes a setup token. | Identical in effect |
| D12 | `metadata.user_id` from the host hash (§5 D4). | **Unverified** whether the validator cares |
| D13 | No `x-request-id` or `traceparent` from the component; native forwarded the client's. Host-generic. | Host's concern |

## 7. Host prerequisites for cutover

- **H1. A minted slot presented through `x-api-key`: met on the host, read not run.** Every shipped minted slot
  (Codex, Copilot, Kiro, Vertex) is presented as Bearer; `claude-code` is the first to present one through
  `x-api-key`. `admit_descriptor_auth` admits `Auth::Header("x-api-key")` on a minted slot as
  `HeaderSecret(XApiKey)`. On the host the presentation and the value are resolved independently. For a `component`
  row, `prepare_dynamic_south_auth` (`server:…/text_admission/sender.rs:957, 1018`; it is called from the Chat,
  Messages, Responses and `/v1beta` senders, `sender.rs:4054`, `sender/messages.rs:569, 633`,
  `sender/responses.rs:1229`, `sender/gemini/component.rs:305`) calls `prepare_component_south_auth`
  (`sender.rs:1087-1172`). That function takes the auth kind from the sealed admission through
  `component_auth_kind` (`sender.rs:1130-1142`; `text_admission/component_facts.rs:329-334` maps `HeaderSecret(h)` to
  `SouthAuthKind::Header(h)`). It takes the value from `recipe_executor.resolve` whenever the slot is minted, whatever
  the kind (`sender.rs:1119-1123, 1143-1161`), and hands both to `PreparedSouthAuth::new(kind, value, …)`
  (`sender.rs:1164`). The host's manifest admission has no rule tying a minted slot to Bearer (`component_admission.rs:164-170`
  constrains only the `oauth` arm). So a minted access token reaches the wire in `x-api-key`. **Unverified by a run**:
  no host test presents a minted slot through a header. The host's dual run (H8) is where this is first exercised, and
  the host should add one test for it before cutover.
- **H2. Row split before routing (consequence of CC-Q1).** Native mixes both token forms in one `claude_code` row and
  picks the shape per credential (`claude_code.rs:45-49`). Here the form is the row's family. Before any row is routed
  to the component, an operator (or the host's migration) must split every native row by form: credentials with a
  refresh token go to a `claude-code` row, credentials without one to a `claude-code-setup-token` row. A native row
  holding both kinds becomes two rows in one routing group. A credential placed in the wrong family fails: a setup
  token in `claude-code` is refused at save (no refresh token), and a login token in `claude-code-setup-token` is sent
  as a Bearer setup token without the CLI headers (on native evidence, Sonnet / Opus answer 429). The rule is native's
  `is_login_style`, and an empty refresh token counts as present (`south_switch.rs:374-378`). The operator form must
  apply the same rule when a credential is added later.
- **H3. Credential migration.** For `claude-code`: the stored `refresh_token` becomes the recipe field; the stored
  `oauth_token` and `expires_at` (milliseconds) become the stored minted value and its expiry (seconds), so a fresh
  native token is used without an exchange. For `claude-code-setup-token`: the stored `oauth_token` becomes the static
  key.
- **H4. Endpoint confirmation.** The operator confirms `https://console.anthropic.com/v1/oauth/token` for the package
  digest before the recipe runs (boundary record §16 Q18).
- **H5. Header extras.** The host keeps operator header extras off `claude-code` rows if it wants native's refusal
  (D5); south has no declaration for it, and adding one is a host-generic decision.
- **H6. Health.** Native probes the token via the setup shape; for `claude-code` the host's recipe-executor probe
  applies, for `claude-code-setup-token` a static-slot row has no token probe.
- **H7. Pins and re-pin.** The host re-pins to the release that ships 1.2.0, and records `south_runtime` 0.60.0 or
  newer for this package (it now declares `subscription_quota_families`).
- **H8. The dual run.** A replay-style dual run of the native arm against both families with the differences of §6
  registered, as the R2 / I3 precedents do; lv's 2026-10-15 ruling makes a passing dual run sufficient for cutover.

## 8. Fixtures and tests

`fixtures-claude-code/` (README there): eight request rows transcribed from the native tests (two setup-token, six
login-token, covering every arm of the identity injection, both `accept` values and the end-user hash) and ten
credential rows (rotation, rotation without a new token, the 60 s default, the 24 h clamp, the missing refresh token,
on-status 400 / 401 / 403 / 429 / 500). The directory is judged with `fixtures-anthropic/` as one pack
(`tests/support/anthropic_packs.rs`) by the native test, the sandbox parity test and therefore the release's gate ②
report, as the Codex pack is. `claude_code_conformance_v1` pins the manifest to the native values (families,
endpoints, user-agent, subscription flag, immutable path, the full credentials section), runs every status from 400 to
599 through the recipe, the exact header set and auth presentation per family (with descriptor auth admission against
the shipped manifest), the identity rules (the native unit tests transcribed), and that the families share one body
builder.

## 9. Versioning (Q47)

No Q47 round: #232 already moved `south-component-conformance` to 0.65.0 and gave every package its patch for the
unreleased 0.66.0 cycle; this change rides on both. Only the changed package moves:

| Item | Before | After |
|---|---|---|
| `provider-anthropic` | 1.1.10 (unreleased patch) | **1.2.0** (new families) |
| its `south_runtime` | 0.57.0 | **0.60.0**: `subscription_quota_families` is parsed from 0.60.0 |
| `south-component-conformance`, `south-provider-api`, `south-contracts` | 0.65.0, 0.58.0, 0.56.0 | unchanged |
| every other package | — | unchanged |

`shipped_packages_v1` records the move (`declared_runtime_now`, and the provider-vertex retirement row now ends at
1.2.0).

## 10. Implementation

`reference_anthropic.rs` gains `Family` (refusing an undeclared family as before), the module `claude_code` (the native
constants), and `inject_claude_code_identity`. The component crate is unchanged apart from its version: it wraps the
same reference.

## 11. Open questions (defaults implemented)

- **CC-Q1** Two families instead of one family with a selector (§3). **Default: two families.**
- **CC-Q2** Family names `claude-code` and `claude-code-setup-token`. **Default: as named.**
- **CC-Q3** Keep the CLI user-agent `claude-cli/2.1.114 (external, cli)` and the beta set as native has them, to be
  bumped together with the host's constants. **Default: native values.**
- **CC-Q4** `system` immutable for `claude-code` so operator extras cannot drop the identity (D4). **Default: yes.**
- **CC-Q5** `on_status` 400 / 401 `reauth_required`, other 4xx and 5xx `transient` (native latching). **Default: yes.**
- **CC-Q6** `default_seconds` 60 for a refresh response without `expires_in` (D6). **Default: yes.**
- **CC-Q7** No refresh margin (D7), as Codex. **Default: yes.**
- **CC-Q8** `metadata.user_id` sent from the host hash as for `anthropic` (D12). **Default: yes; unverified.**

## Revision note

- 2026-10-15: first draft, accepted under lv's directive of the same day; implemented with the defaults.
- 2026-10-15: H1 answered from the host code (met when read, not run); the row split before routing stated as a host / ops
  prerequisite (H2).
