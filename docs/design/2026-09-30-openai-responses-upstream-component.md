# The OpenAI Responses upstream dialect component (`provider-openai-responses`)

Status: accepted (lv, 2026-10-10: the remaining S-tagged questions ruled, §16) — drafted by the host team
(token-station-server P21). Step R1 implemented 2026-10-10, not released (§17).

Date: 2026-09-30

Revised: 2026-10-01 after independent review (see the revision note at the end).

Predecessors: `2026-09-30-host-zero-vendor-boundary.md` (the umbrella; this record expands the first row of its §11
and depends on its §3 credential recipe, §4 descriptor auth admission, §5 `stream_framing`, §6 usage discipline, §7
`request_facts` and dialect words, §8 compatibility range, §10 instance declaration),
`2026-09-30-embeddings-contract.md` (structure of this record only; its `NorthIdentical` locator is a different
mechanism from this record's `north_passthrough`, §8),
`2026-09-28-responses-north-codec.md` and its validation record (the northbound Responses mapping in
`south-north-codec`), `2026-09-29-claude-model-dialect.md` (dialect words in `supported_parameters`),
`2026-08-21-canonical-ir-inventory.md` (S0 D1: usage extraction is component translation; D3: no provenance on
`StreamEvent`), `2026-08-23-renderer-refusal-for-unmappable-blocks.md` (a renderer refuses what it cannot spell).
Sibling, drafted in parallel: `2026-09-30-north-codec-render-gaps.md` — northbound only; it renders the client-facing
failure event (its G3) and the single-burst Chat stream (its G4), while this record owns recognising the upstream's
failure frames, all three shapes the host recognises today (§6.4), and produces the IR both consume. An upstream
failure frame is never relayed to the client as is, on any delivery path (§8.2).

Origin: token-station-server plan P21 (`docs/product-review-v2/plans/2026-09-29-P21-*.md`) (§2.5, §5 S3–S5,
Appendix B.2 and B.4, where this component is listed as scope only, with no design), and
plan P15 (`2026-09-28-P15-Responses*.md`) with annexes A1 (I12–I19, O03–O06) and A2 (§4 target matrix).
Owner rulings applied here, as relayed by the host team on 2026-09-30:
**DP7 / boundary Q10** — south takes in client-identification headers, the host keeps no special case; **boundary Q1**
— bumping the south or kernel pin counts as modifying the host; **boundary Q12 / embeddings E-Q3** — the two estimate
conventions may coexist (not raised again here; this dialect reports usage). Boundary Q13 (the reasoning-token
convention) is still open.

Baseline: south `origin/main` = v0.42.0 (`3135e36`); kernel `f585bc83` (protocol 0.4.0 / kernel v0.3.0); host
a82c852b. Host line numbers refer to token-station-server `a82c852b` and carry a `server:` prefix (the first draft
cited `4d5bb4e5`; of the files cited here only `translate_responses.rs` changed since, by the #61 fix, which moved its
lines after 298 down by nine); kernel line numbers carry `kernel:` and refer to `crates/protocol/src/` at `f585bc83`.
South line numbers refer to the baseline. `server:…/` abbreviates `server:gateway/src/modules/inference/engine/`;
`leaf:` abbreviates `server:crates/gateway-provider-protocol/src/`. Statements marked "inferred" are conclusions from
reading code, not from running it.

## 1. Problem

The OpenAI Responses API is one of the two text upstream dialects the host still translates and meters in its own code
(the other is Kiro; P21 §3.1, boundary §11). Four pieces of host code carry it:

| Piece | Where | What it knows |
|---|---|---|
| Native pass-through leg of `/v1/responses` | server:…/text_admission/sender/responses.rs:412-455, 1385, 1646-1698 | Which targets speak Responses (`supports_responses` on the provider row, `upstream_requires_responses` on the model row, or the types `OpenaiCodex` / `AwsBedrockOpenai`); forwards the client's body with only `model` and the cap rewritten; forwards upstream frames unchanged and withholds `response.completed` until settlement |
| Chat → Responses conversion for "Responses only" models | server:…/text_admission/chat.rs:501-510; leaf:translate_responses.rs:283-568, 624-728 | Converts a Chat Completions body to a Responses body, always calls the upstream non-streaming, converts the answer back, and replays it to a streaming client as one burst (server:…/text_admission/sender.rs:3779-3800) |
| Strict usage evidence for the Responses wire | leaf:usage_evidence.rs:543-955, 1959-2271 (plus :176-202, 1496-1513, 1531-1539, 1671-1675) | Terminal status, usage arithmetic, the closed list of `response.*` events, item and part identity, `tool_usage` |
| Codex backend specifics | leaf:translate_responses.rs:10-35; server:…/text_admission/sender/responses.rs:438-453, 660-674, 1666-1676, 2026-2130; server:…/token_refresh.rs:114-250; server:…/upstream.rs:260-271, 535-539; server:…/text_admission/sender.rs:806-830 | Request rules (no cap, no `temperature`, `store`, default `instructions`, forced `stream`), the URL suffix, in-band error frames under HTTP 200, the OAuth refresh, the account header |

The usage part measures **784 lines** at the baseline (734 excluding blank and comment lines) across the seven ranges
listed above; P21 Appendix B.2 estimates it at 699. South has no counterpart: none of the four reference
implementations parses the Responses wire, and `south-north-codec` maps it in the opposite direction only
(crates/south-north-codec/src/lib.rs:26-29: "Rendering IR into a *provider's* wire, and parsing a provider's response
back into IR, belong to the provider components").

Two consequences of the host owning this dialect are visible in code today:

- The three northbound surfaces treat a Responses upstream differently. `/v1/responses` passes bytes through;
  `/v1/chat/completions` reaches it only for `provider_type = openai` models flagged `upstream_requires_responses`,
  always non-streaming upstream (server:…/text_admission/chat.rs:303-313 seals with `streaming = false`);
  `/v1/messages` cannot reach it — the cap contract admits `max_completion_tokens` / `max_tokens` only when the model
  is **not** flagged (server:…/text_admission.rs:1157-1176), so the seal fails (inferred). Codex is refused on Chat
  and Messages outright (server:…/text_admission/chat.rs:623-635; server:…/text_admission/messages.rs:357-372).
- Whether a target "speaks Responses" is asked at several sites, by provider flag, model flag and provider type
  together (server:…/text_admission.rs:756, 788, 797-800, 959-961, 1147; server:…/execution_plan.rs:252-264;
  server:…/text_admission/sender/responses.rs:414-420;
  server:gateway/src/modules/admin_ui/handler/routing/canary.rs:225).

## 2. Decisions

- **D1 A new provider package, `provider-openai-responses`, in the existing world `provider-adapter-v2`.** No WIT
  change. It is not a family of `provider-openai-compatible`, and "Responses only" is not a dialect word (§3.2).
- **D2 Two families in v1**: `openai-responses` (OpenAI's public Responses API with a static key, and any other
  upstream admitted to it by its own captured-traffic fixture pack, §3.3) and `openai-codex` (the Codex subscription
  backend: minted bearer, account header, its own request rules). Both share one response and stream wire, which is
  what lets them share a package (§3.1).
- **D3 The component maps IR ⇄ Responses wire in both directions and is the only source of usage evidence** (§4–§7).
  Evidence rules the host enforces today are moved into the component unchanged in strength.
- **D4 Requests are always stateless**: the component always sends `store: false` and never `previous_response_id`.
  Whether a client's `previous_response_id` is resolved from history the host keeps itself, or refused, is the
  host's northbound decision, taken before the IR is built (§4.4). Nothing that asks for upstream state reaches the
  component.
- **D5 Usage is strict**: `usage_evidence: reported`, declared once for the package (umbrella R6); a terminal without
  exact usage is an error, never a zero; a non-zero `tool_usage` is an error because the IR has no bucket for it (§7).
- **D6 `response.incomplete` with usage settles as a success only for a closed set of reasons**: by the owner's
  ruling R-Q2, `max_output_tokens` gives finish reason `length`, and by its extension of 2026-10-01, `content_filter`
  gives finish reason `content_filter`. Every other reason is `provider_protocol_error` (§5, §6.3).
- **D7 On the Responses northbound surface the host may deliver the upstream's own bytes** ("pass-through"), per the
  owner's ruling R-Q1, when the family declares `north_passthrough` and the request mapped losslessly (§8.2). The
  component's IR events stay the only evidence; the terminal frame is withheld by northbound type and `Done`
  together; upstream failure frames are replaced by the host's fixed-message failure event (the owner's ruling of
  2026-10-01 extending N-Q3 to relayed failures, §8.2). Re-rendering through the north codec is the fallback and is
  what the Chat and Messages surfaces get by construction.
- **D8 Codex credentials use credential recipe v1** (umbrella §3.3), and the client-identification material the
  backend is sent today is designed in: the OAuth client id and scope in the recipe, the account header in the
  component (DP7, ruled 2026-09-30: south takes them in; the host keeps no special case) (§10). Under the umbrella's
  recipe trust model (§3.4), the exported account id comes from the stored non-secret field only; the id token in a
  refresh response is only a `must_equal_field` check that the account did not change. The clock is a fixed
  3000-second window, and the recipe declares `write_back` because Codex rotates (§10.2).
- **D9 This dialect needs no new instance in any compiled-in vocabulary** (secret header, query name, quota header).
  The one instance it could need later — a `user-agent` value — would use the umbrella's single user-agent proposal
  (§10.4, R-Q6).
- **D10 Inputs whose token count the host cannot bound are refused in v1**: `input_file` parts, and any `file_id`
  reference (§4.2). The prompt is then bounded by the northbound request, which §7.4's input check relies on.
- **D11 Operator extras may not rewrite what the family fixes**: each family declares `immutable_body_paths`
  (§3.4), replacing the host's Codex-only save-time check (§13.4).

## 3. Package, families and manifest

### 3.1 Why a separate package

`parse-response`, `parse-stream-chunk` and `map-provider-error` receive no `provider-config`
(crates/south-provider-api/wit/provider-adapter.wit:116, 125, 132); only `build-http-request` does (:108). Families in
one package can therefore differ in how a request is addressed and authenticated, but must share the response wire.
That is exactly the existing split: `openai-compatible` and `azure-openai-v1` differ only in the auth arm
(crates/south-component-conformance/src/reference.rs:640-648). Chat Completions and Responses do not share a response
wire, so a Responses family inside `provider-openai-compatible` would have to tell the two apart by inspecting each
body and each frame. A second cost: every change to the Responses mapping would bump the identity of the package that
every OpenAI-compatible provider pins.

### 3.2 "Responses only" is a family, not a dialect word

Umbrella §7.4 states the rule this section argues: a dialect word can change the request shape
**within** a package's wire (the six `anthropic.*` words do that); it cannot move a model onto another package's wire,
for the reason in §3.1. The host flag `upstream_requires_responses`
(server:gateway/src/infra/config/models.rs:243-253) is therefore replaced by **which family serves the model**:

- A provider row whose family is `openai-responses` sends every model on it through the Responses wire, on all three
  northbound surfaces, streaming included. No flag, no per-surface branch.
- A provider that serves some models through Chat Completions and others only through Responses needs either two
  provider rows with the same endpoint and credential (no new host mechanism; inferred), or a generic model-row family
  override in the host (the "model-row routing" of P21 DP3). Which one is a host question (R-Q12); neither needs
  anything from this package.

The provider-level flag `supports_responses` (server:gateway/src/infra/config/providers.rs:875-887) disappears the
same way: a provider row's family says which wire it speaks.

### 3.3 Families

| | `openai-responses` | `openai-codex` |
|---|---|---|
| Upstream | OpenAI's public Responses API; another upstream only after its own fixture pack passes (below) | The Codex subscription backend |
| URL | `base_url.resolve(ProviderApi::Responses)` (kernel:provider.rs:126-133, 138-161) | `{base_url}/codex/responses` — the URL the host builds today (server:…/upstream.rs:535-539) |
| Auth | `Auth::bearer`, slot `provider_api_key` (static) | `Auth::bearer`, slot `codex_access_token` (minted, §10.2) |
| Extra request header | none | `chatgpt-account-id`, when the credential exports an account id (§10.3) |
| Output cap on the wire | `max_output_tokens` | none (§4.3) |
| Non-streaming request | supported | refused at build time (§4.3) |
| `request_facts.stream` | `{"body": "/stream"}` | `{"body": "/stream"}` — the wire has a switch; the component always sets it (§4.3) |
| `immutable_body_paths` | `store` | `store`, `stream`, `instructions`, `temperature`, `max_output_tokens` |

Declared once for the package, not per family (umbrella R6: the response-side functions receive no configuration):
`usage_evidence: reported`, and `stream_framing` absent, which means `bytes`.

**Which upstreams the `openai-responses` family covers.** The family's usage mapping (§7) and its closed event list
(§6.2) are pinned against OpenAI's published semantics by the documentation-derived judge (§12.3). That judge vouches
for OpenAI only. The host's `supports_responses` flag is set today for other upstreams too — its own comment names
OpenAI, Groq, xAI, Bailian and Codex (server:gateway/src/infra/config/providers.rs:875-878), and an xAI model is the
example of a "Responses only" model (server:gateway/src/infra/config/models.rs:243-245;
leaf:translate_responses.rs:556-557). If such an upstream reported, say, `output_tokens` without reasoning and a
`total_tokens` that also leaves reasoning out, every check of §7 would pass and reasoning would go unbilled
(inferred; the host's native parser has the same exposure today, so this is not a regression). Rule for v1:

- A host routes an upstream other than OpenAI to `openai-responses` only after the package carries a fixture pack
  captured from that upstream's real traffic (request, non-streaming body, stream, usage with cache and reasoning
  where the upstream offers them) and the pack passes the same judges. The pack is part of the package release; the
  release index lists which upstreams have one.
- An upstream whose wire, event list or usage convention differs gets a family of its own in this package (a
  package-layer change), not a dialect word, for the reason in §3.1.

Which of the two routes is the default is R-Q16.

Upstreams the host's native leg also serves today, and how each is covered:

- **Azure OpenAI** (`api-key` header; server:…/upstream.rs:183-189, 606-633): one more family in this package,
  differing only in auth presentation, exactly as `azure-openai-v1` does in the compatible package. Not in v1; the
  host treats its Responses wire as identical (it uses the same strict parser), which a fixture pack must confirm.
- **Bedrock's OpenAI-shaped endpoint** (server:…/text_admission.rs:664-670): with an API key it is an ordinary
  `openai-responses` row whose base URL is the regional endpoint (inferred), subject to the fixture-pack rule above;
  with SigV4 it needs a sibling package, because `host_signed` admits no other arm in the same manifest
  (crates/south-provider-api/src/manifest.rs:383-385).
- **Groq, xAI, Bailian and other third-party Responses upstreams**: the fixture-pack rule above.

The host's native Responses code retires only when every upstream it serves in production is covered (§13.4).

### 3.4 Manifest sketch

`usage_evidence` (umbrella §6.2), `stream_framing` (§5.2), `request_facts` (§7.2, including `output_cap: []` for a
wire with no cap field) and `credentials` (§3.3) are the umbrella's fields, written with its draft names.
`usage_evidence` and `stream_framing` are package-level scalars (umbrella R6); `stream_framing` is omitted below
because its absence means `bytes`.

```json
{
  "name": "provider-openai-responses",
  "version": "1.0.0",
  "api_version": "provider-adapter-v2",
  "providers": ["openai-responses", "openai-codex"],
  "capabilities": ["chat", "json_schema", "stream", "tool_call"],
  "auth_arms": ["bearer"],
  "permissions": { "network": false, "filesystem": false,
                   "secrets": ["provider_api_key", "codex_access_token"] },
  "usage_evidence": "reported",
  "request_facts": {
    "openai-responses": { "output_cap": ["/max_output_tokens"], "model": { "body": "/model" },
                          "stream": { "body": "/stream" } },
    "openai-codex":     { "output_cap": [], "model": { "body": "/model" }, "stream": { "body": "/stream" } }
  },
  "immutable_body_paths": {
    "openai-responses": ["store"],
    "openai-codex":     ["store", "stream", "instructions", "temperature", "max_output_tokens"]
  },
  "north_passthrough": { "openai-responses": "responses", "openai-codex": "responses" },
  "credentials": { "schema": "south.credential-recipe.v1", "…": "see §10.2" },
  "conformance": { "required_suite": "south.provider-component.v1", "fixtures": "fixtures/" },
  "compatibility": { "…": "as the other provider packages; range fields per boundary §8" }
}
```

The Codex family's `output_cap: []` uses the umbrella's form for a wire with no cap field (§7.2), pending the
measurement of R-Q5 (§4.3). Two fields are **proposals of this record**, not in the umbrella:

- `immutable_body_paths` (per family): dotted body paths that operator request extras may neither set, remove nor
  rewrite, including their ancestors and descendants. It reuses the grammar and bounds the task world already has
  for the same purpose (`validate_immutable_body_paths`, crates/south-contracts/src/task_v2.rs:48-69, contract 6),
  declared statically per family here because a provider-world family fixes these fields for every request. A path
  may name a field the component removes (`temperature` for Codex): immutability then means "stays absent". The host
  refuses such an extra when the operator saves it, generically, instead of the Codex-only check it has today
  (§13.4). Operator extras are applied after the component built the body (§11 item 5), so without this an extra
  could put back `store: true` or a removed field.
- `north_passthrough` (per family; absent = the host always renders through the north codec). Its value names a
  northbound protocol from a closed set owned by south (`chat_completions`, `responses`, `messages`), deliberately
  spelled differently from any family name. See §8. It is a new mechanism in its own right: the embeddings world's
  `NorthIdentical` is a per-request locator in a different world with an erasure protocol, and nothing here depends
  on it.

`model-capabilities` echoes `config.models`, as the four existing references do (reference.rs:600-607).

## 4. Request mapping: IR → Responses

### 4.1 Top level

| IR (`ChatRequest`, kernel:chat.rs:196-217) | Responses body | Notes |
|---|---|---|
| `model` | `model` | |
| leading `Role::System` messages | `instructions`, joined with `\n` | The join the host uses today (leaf:translate_responses.rs:483-485). The north codec turns a Responses `instructions` string into a leading System message (crates/south-north-codec/src/responses/request.rs:22-26). Only the leading run is merged; the host's Chat converter merges every system or developer message wherever it stands (leaf:translate_responses.rs:291-307), a difference listed in §13.3 |
| remaining messages | `input` (always an item list) | §4.2 |
| `tools` | `tools`: `{type: "function", name, description, parameters, strict?}` | `strict` from `extensions.responses_tool_strict[name]` (north codec tools.rs:103-105; the compatible reference reads the same key, reference.rs:285-292) |
| `tool_choice` | `"auto"` / `"none"` / `"required"`, or `{type: "function", name}` | Object forms accepted: the Chat form the north codec emits (request.rs:184) and Anthropic's `{type: "tool", name}`; any other object is a capability error |
| `extensions.parallel_tool_calls` | `parallel_tool_calls`, only alongside `tools` | Same rule as reference.rs:297-304 |
| `response_format` | `text.format`: `{type: "text"}`, `{type: "json_object"}`, or `{type: "json_schema", name, schema, strict?, description?}` | Inverse of north codec request.rs:130-162 |
| `extensions.reasoning_effort` | `reasoning.effort` | Same per-model gate as the compatible reference (reference.rs:323-338): sent when the model declares `reasoning_effort` or declares no parameter set; an effort that came from an Anthropic thinking translation needs the explicit declaration |
| `extensions.responses_reasoning_summary` | not sent in v1 | The north codec carries it (request.rs:74-76), but acting on it would be a new use of an `extensions` key (below); the upstream's default summary setting applies |
| `sampling.temperature`, `sampling.top_p` | `temperature`, `top_p` | Dropped for `openai-codex` as §4.3 says |
| `sampling.max_output_tokens` | `max_output_tokens` | Not sent for `openai-codex` (§4.3) |
| `sampling.stop` | dropped | The host's converter does not forward it either (leaf:translate_responses.rs:489-493). Dropping follows the kernel `Sampling` contract (kernel:chat.rs:130-145) |
| `stream` | `stream: true` when set, otherwise omitted | |
| — | `store: false`, always | §4.4 |

Descriptor: `POST`, the family's URL (§3.3), header `content-type: application/json`, `auth` per §10.

**Extension keys.** Three rows above act on `extensions` keys the north codec writes: `responses_tool_strict`,
`parallel_tool_calls` and `reasoning_effort`. S0 D5 says a component must not behave on an `extensions` key
(canonical-ir-inventory.md, D5); umbrella §16 Q14 asks for the route that replaces such keys. These three are
**existing precedent**, not new uses: the compatible reference already acts on exactly these three
(reference.rs:285-304, 631-636), and v1 follows it so that the two OpenAI packages behave alike. v1 adds no new use:
it does not act on `responses_reasoning_summary`, which the compatible reference does not read, and the first draft's
proposed `responses_previous_response_id` key is withdrawn (§4.4). Promoting the three keys to typed IR fields
through the kernel chain, for both packages at once, is R-Q15, which umbrella Q14 names.

### 4.2 Input items

| IR message | Responses input item |
|---|---|
| `Role::User`, `Content::Text` | `{role: "user", content: [{type: "input_text", text}]}` |
| `Role::User`, parts | `Text` → `input_text`; `ImageUrl` → `{type: "input_image", image_url: <url>, detail?}`; `Unknown` of type `input_audio` → forwarded unchanged; `Unknown` of type `image_url` carrying `file_id` (how the north codec preserves it, request.rs:341-350), `Unknown` of type `input_file`, and any other `Unknown` → capability error (below) |
| `Role::User`, no content | `{role: "user", content: []}` (as the host does, leaf:translate_responses.rs:377-379) |
| `Role::System` after the first non-system message | `{role: "system", content: [{type: "input_text", text}]}` |
| `Role::Assistant`, text | `{role: "assistant", content: [{type: "output_text", text}]}`, omitted when empty |
| `Role::Assistant`, each `ToolCall` | `{type: "function_call", call_id, name, arguments}`, after the text item |
| `Role::Tool` | `{type: "function_call_output", call_id: tool_call_id, output: <text>}`; parts that are all text are concatenated; a non-text part is a capability error; a missing `tool_call_id` is a capability error |
| `Thinking` / `RedactedThinking` parts on an assistant turn | not sent (§4.5) |

Message items carry no `type` key and no `id`: this is the shape the host's two converters produce
(leaf:translate_responses.rs:19-22, 312-315, 448-453), so dual-run bodies agree on it.

**Why `input_file` and `file_id` are refused in v1.** Both are references: a `file_id` names a file stored in the
upstream account, and an `input_file` may carry a `file_id` or a `file_url` the upstream fetches. A few bytes in the
request can stand for a document of many thousands of tokens, so the prompt is no longer bounded by the northbound
request, which §7.4's input check and the reservation both assume (umbrella §6.3: the host computes every bound
from the northbound request). A second reason: the upstream account is shared by every tenant
routed to the row, and a `file_id` is scoped to that account, not to the tenant (inferred; the host's admission does
not look at either field today — a search of `gateway/src` finds no handling of `file_id` or `input_file` on the
inference path — so the native leg forwards them). Admitting them later needs a host-configured allowance for this
media part in the umbrella's bound and a tenant-scoped file store; neither is this package's to supply. This is a
difference from the native leg, listed in §13.3.

### 4.3 Family `openai-codex`

Copied from the host's normalization (leaf:translate_responses.rs:10-35) and its call site
(server:…/text_admission/sender/responses.rs:432-453):

- `max_output_tokens` is not sent. The authorized cap still exists in the IR; the manifest declares `output_cap: []`,
  so the host's seal check has nothing to look for.
- `temperature` is not sent. (`top_p` is sent if present: the host removes only `temperature`.)
- `instructions` defaults to the string `You are a helpful assistant.` when the request has none.
- `stream: true` always. A request with `stream = false` is a capability error at build time — zero upstream calls
  (owner ruling R-Q5). Today the host does not normalize a non-streaming Codex body at all
  (server:…/text_admission/sender/responses.rs:438-440), although its own comment says the backend requires streaming;
  the upstream's answer to such a request was not measured.
- `input` is always an item list (the host lifts a string input for Codex, leaf:translate_responses.rs:16-24; this
  component never emits the string form for either family).
- `store`, `stream`, `instructions`, `temperature` and `max_output_tokens` are the family's `immutable_body_paths`
  (§3.4), so an operator extra cannot undo any of the above.

**Why Codex is refused rather than buffered, when Kiro is buffered.** Umbrella §5.2 pins the buffered path for
always-streaming upstreams to packages that declare `aws-eventstream` framing **and** `request_facts.stream: "none"`:
the host buffers the stream, deframes it and hands `parse-response` the canonical re-encoding. Kiro takes that path
because its wire has no stream switch at all. The Codex wire has one (`stream` in the body, declared
`{"body": "/stream"}`), and this package's framing is SSE `bytes`, so the buffered path does not apply to it; only
the backend's requirement that `stream` be `true` makes a non-streaming Codex call impossible, and the owner ruled
that such callers are refused at build time (R-Q5). This record keeps that ruling. Serving non-streaming Codex
callers would need a design of its own and is not proposed here.

Whether the backend really rejects `max_output_tokens` is asserted by the host's normalizer, its comment and its
test (leaf:translate_responses.rs:8, 25, 553-555; server:…/text_admission/sender/responses.rs:2197-2213). The first
draft read server:…/text_admission.rs:1201-1203 as a contradiction; it is not one. That comment classifies Codex as
a bounded-token provider for pricing: the gateway accepts the client's cap on its Codex Responses surface and writes
it into the sealed canonical body, which the normalizer then strips from the wire (sender/responses.rs:432-437). It
says nothing about the upstream. The measurement is still wanted before implementation, because all three host
statements are assertions, not recorded observations; if the backend accepts the field, `openai-codex` declares
`output_cap: ["/max_output_tokens"]` and needs no empty list (R-Q5).

### 4.4 State: `store` and `previous_response_id`

The IR has no place for either. The north codec validates `previous_response_id` for shape and then drops it
(request.rs:21, 114-123); it never reads `store`. So on the IR path a request that relies on server-side history would
be sent with the current turn only and answered without error. The host's native leg avoids this only by forwarding
the client's body as is.

v1 rule: the component always sends `store: false` and never sends `previous_response_id`; the owner ruled that v1
does not support upstream state (R-Q4).

What happens to a client's `previous_response_id` is decided by the host, on its northbound side, before the IR is built
— not by this component. The north codec is designed for a host that keeps continuation history itself: it accepts an
empty input list "for a host that resolves continuation history" (crates/south-north-codec/src/responses.rs:27) and
returns a terminal IR snapshot "for host-owned continuation history" (responses/stream.rs:98; the codec record's
interface notes put cache, TTL and scope on the host). A host that resolves the id expands the history into the IR and
drops the field; a host that does not refuses the request. Either way the IR the component receives carries no upstream
state, so a refusal inside the component would be wrong for the first host and redundant for the second. The first
draft's follow-up NC-3 (carry the id into `extensions` for the component to refuse) is withdrawn. The closed host keeps
no continuation history, so it refuses the field in its admission before building the IR (§11 item 2); today its native
leg forwards it instead, as part of the client's body.

Stateful Responses against the upstream (sending `store: true` and the upstream's own `previous_response_id`) stays
rejected for v1 (§15).

### 4.5 Reasoning items

| Case | v1 behavior |
|---|---|
| Assistant turn carries `reasoning_replay_protocol_family = claude-signed-thinking` | Capability error, as the compatible reference does (reference.rs:614-621) |
| Plain thinking text (`Thinking` without signature) | Not sent. The Responses wire accepts reasoning only as items the upstream itself issued |
| OpenAI reasoning items (`id`, `summary`, `encrypted_content`) | Not sent in v1 |

The third row is a limitation of the IR path, not a choice. Replaying OpenAI reasoning faithfully needs every
reasoning item of a turn, in order, with its position relative to the turn's text and tool calls. The north codec
keeps only the last item's `id` and `encrypted_content`, as two single-valued extensions that a later item overwrites
when adjacent assistant items are merged (request.rs:429-445, 612-621), and keeps no position. P15 A2 §2–§3 record the
same overwrite as a defect; it was fixed only for Claude carriers (request.rs:433-441). Follow-up NC-1 (§9) is the
request half of the fix; the component would then replay the items for a model declaring the proposed word
`reasoning_replay.openai.v1` and add `include: ["reasoning.encrypted_content"]`. P15 A2 §4 already rules that an
unmarked OpenAI opaque value may go only to a native Responses target; the word is how a target says it is one without
the host looking at provider types (server:…/text_admission/reasoning_replay.rs:136-175 today).

### 4.6 What never reaches the component

Request fields the native leg forwards today because it forwards the whole body, and that the IR does not carry:
`include`, `metadata`, `user`, `prompt_cache_key`, `truncation`, `store`, `previous_response_id`, item `id`s, tool
types other than client functions (the host's admission already refuses those:
server:…/token_counter/authorize.rs:563-584), and any field the north codec does not name. None of them is on the
component's side of the boundary to restore; each is either a north codec follow-up or accepted as dropped (§13.3).
Dropping `include` has a consequence for §8: without `include: ["reasoning.encrypted_content"]` a stateless request
gets no `encrypted_content` back from OpenAI's API (inferred from its published semantics), so pass-through
delivery cannot hand it to the client either.

## 5. Response mapping: non-streaming

`parse-response` accepts a 2xx body only if:

- `object` is `response`; `error` is absent or null;
- `status` is `completed`, or `incomplete` with a usage object and an `incomplete_details.reason` from the closed set
  below (§6.3). `failed`, `cancelled`, `in_progress`, `queued` and any other value are `provider_protocol_error`;
- `output` is an array whose items are all mappable (below);
- usage passes §7.

| Output item | IR |
|---|---|
| `message` with `output_text` parts | assistant text; several parts are concatenated in order |
| `message` with a `refusal` part | assistant text (lossy: the IR has no refusal marker; R-Q10) |
| `function_call` | `ToolCall { id: call_id, name, arguments }`; `call_id`, `name` and a string `arguments` are required |
| `reasoning` | one `ContentPart::Thinking { signature: None }` per `summary_text` / `reasoning_text` part, placed before the text; `id` and `encrypted_content` are not carried in v1 (§8.3) |
| anything else | `provider_protocol_error` — the item cannot be represented, and dropping it would hide content or cost. The host refuses the same set today (with a message meaning "not admitted for cost", leaf:usage_evidence.rs:868-892) |

Result: one `Choice` with index 0. `finish_reason`: `ToolCalls` if any `function_call` item is present; otherwise
`Stop` for `completed`; for `incomplete`, `Length` when `incomplete_details.reason` is `max_output_tokens` (owner
ruling R-Q2) and `ContentFilter` when it is `content_filter` (the ruling's extension of 2026-10-01, as every other
dialect does). Any other reason, or a missing one, is `provider_protocol_error`: the first draft mapped it to
`Other(reason)` and so settled it as a success, which is wider than the owner's ruling and would charge for an
outcome nobody has classified. `ChatResponse.id` and `.model` are the upstream's.

`openai-codex` never takes this path (§4.3).

`map-provider-error` (non-2xx) uses the compatible reference's table unchanged (reference.rs:738-790): the
`error.code` checks for content policy and context length, then the status table, `provider_message` kept when at most
256 characters, `retry-after` in seconds.

## 6. Stream mapping

The component splits SSE frames itself, as the three SSE references do (reference.rs:519-528), and holds one state
machine per stream. It splits them with `decode_sse_v1`, the pure SSE decoder south supplies with golden vectors in
`south-contracts`, stated in umbrella §5.2 as the SSE sibling of the eventstream deframer and released with the image
world's minor. The host never uses it to parse a provider's stream for the component — under `stream_framing:
bytes` the component still receives the upstream bytes unchanged. The host uses it for one thing only: under
`north_passthrough` it splits the northbound frames to find the terminal frame (§8.2). Both sides therefore agree on
every frame boundary by construction rather than by two implementations happening to match.

*(Amended 2026-10-10, umbrella §13.13: lv placed `decode_sse_v1` in the host-only crate `south-host-grammars`, which no
component may link, and it ships in 0.53.0. The component therefore splits its stream with its own code, as the three
SSE references do, and the two sides no longer share one function. What keeps them aligned under `north_passthrough`
is §8.2's delivery: the host cuts the upstream bytes into whole frames with `SseDecoderV1::position` and hands the
component one frame per call, and the gate ③ assertion that the component holds no partial frame after a call fails
closed (the host forwards nothing more and parks) if the component's splitter does not recognize a frame the decoder
cut, for instance one ended by bare CRs. The component's tests can run the decoder's golden vectors as a
dev-dependency. R1 itself no longer needs the decoder; the host's pass-through does.)*

### 6.1 Events

| Upstream event | IR events |
|---|---|
| `response.created`, `response.in_progress`, `response.queued` | none; binds `response.id` |
| `response.output_item.added` with a `function_call` item | `ToolCallDelta { index: <tool ordinal>, id: call_id, name, arguments_delta: <arguments already on the item, or ""> }` |
| `response.function_call_arguments.delta` | `ToolCallDelta { index, arguments_delta }` |
| `response.function_call_arguments.done`, `response.output_item.done` for a call | If the final `arguments` extends what was emitted, the remainder is emitted once; if it contradicts it, `provider_protocol_error` |
| `response.output_text.delta` | `Delta { index: 0, content }`; an empty delta emits nothing |
| `response.refusal.delta` | `Delta { index: 0, content }` (lossy, as §5) |
| `response.reasoning_summary_text.delta`, `response.reasoning_text.delta` | `ThinkingDelta { index: 0, block_index, thinking_delta }` |
| `response.output_item.added` / `.done` for `message` and `reasoning`, `response.content_part.*`, `response.output_text.done`, `response.refusal.done`, `response.reasoning_*.done`, `response.reasoning_summary_part.*`, `response.output_text.annotation.added` | none; identity checks only (§6.2) |
| `response.completed` | `Finish { finish_reason }`, `Usage { … }`, `Done {}` |
| `response.incomplete` | the same three, with the finish reason of §5 (§6.3) |
| `response.failed`, `error`, and a frame of any other type that carries a top-level `error` object | `Usage` when a valid usage object is present (§6.4), then `Error { error }`, then nothing |

Tool ordinals count tool calls only, in first-seen order — the same translation from `output_index` the host's stream
converter makes (leaf:translate_responses_sse.rs:34-43; called only from tests at the baseline). `block_index` is
assigned in first-seen order to each (item, summary or content index) pair, so that two summary parts of one reasoning
item stay two blocks (R-Q11 asks whether that reading of kernel:stream.rs:51-60 is accepted).

Encrypted reasoning content is **not** emitted as `RedactedThinking` or `ThinkingSignatureDelta`. Those events mean
Claude material to the north codec: its stream renderer collects them into a `claude-signed-thinking` carrier
(crates/south-north-codec/src/responses/stream.rs:350-360, 392, 603-605), which would offer OpenAI's opaque value to
Claude targets — the relabelling P15 A1 I13 and A2 §4 rule out. Stream contract 2 has no event for an opaque value of
another family (§8.3).

### 6.2 Evidence rules

The component enforces what the host's state machine enforces today (leaf:usage_evidence.rs:1959-2271); a violation is
`provider_protocol_error` from `parse-stream-chunk`. A failure frame (§6.4) is recognised **before** these rules run,
as the host does today ("this must be checked before strict usage validation",
server:…/text_admission/sender/responses.rs:2026-2029): rules 2–5 are not applied to it, so a Codex rejection without
a `sequence_number` still yields its error envelope instead of a protocol error. Rules 1 and 4 still apply to what
follows and to a `response` object the failure frame carries.

1. Exactly one terminal frame; any frame after it is an error (:1969-1979).
2. The event type is in the closed list of §6.1 (:1980-2014). An unlisted `response.*` event is an error, not ignored.
   The sibling Kiro record chose the opposite for its wire (its fixture `stream.unknown-event-ignored`), so the two
   records now differ on purpose: the Responses wire is a published, versioned event list whose unknown events can
   carry content or cost (a new output item type, a hosted-tool event), while Kiro's has no published list. Under
   pass-through (§8.2) the choice also decides whether an unknown frame reaches the client; strict means it does not.
   R-Q9 keeps the question open for the maintainers.
3. `sequence_number` is present and strictly increasing (:2015-2024).
4. `response.id` is non-empty and the same in every event that carries it (:2125-2143).
5. An output item's `added` and `done` agree on id, `output_index` and type, and `done` arrives once (:2145-2180); the
   same for content parts (:2182-2214).
6. Item and part types are within §5's table (:868-955).
7. The terminal `response` object passes §5 and §7.
8. A clean end of stream before any terminal frame: `finish()` returns `transport_truncated`. No `Done` is emitted, so
   the host cannot settle it as complete.

### 6.3 Terminal frames and what counts as complete evidence

Complete evidence is the sequence `Finish` (with a reason), `Usage`, `Done`, once, with nothing after it. It is
produced by:

- `response.completed` whose `response.status` is `completed`;
- `response.incomplete`, **if** it carries a usage object that passes §7 and an `incomplete_details.reason` in the
  closed set of §5 (`max_output_tokens` and `content_filter`, R-Q2 and its 2026-10-01 extension); any other reason
  is `provider_protocol_error`.

The second case is a change from the host, made by the owner's ruling R-Q2 and its extension. Today a
non-streaming body whose status is not `completed` is refused (leaf:usage_evidence.rs:547-549) and a
`response.incomplete` frame poisons the stream (:2035-2041), so a generation truncated at the cap is parked in
`delivery_unknown`; the converter's `incomplete →
length` branch (leaf:translate_responses.rs:683-686) sits behind that check — the usage parse at
server:…/text_admission/sender.rs:3736-3766 runs before the conversion at :3779-3784 — and so is not reached (code
reading, not run). Every other dialect, and the north codec's own renderer
(crates/south-north-codec/src/responses/output.rs:157-169), treat a length stop as a normal finish with usage. A
truncated generation consumed tokens and its usage is reported; the owner ruled that it settles (R-Q2). A
`response.incomplete` without usage stays an error.

### 6.4 Error frames

**Recognition.** Three shapes, the three the host recognises today
(server:…/text_admission/sender/responses.rs:2030-2043, 2062-2084):

| Frame | Where the error object is |
|---|---|
| `type: "response.failed"` | `response.error`, else a top-level `error` (:2033-2036) |
| `type: "error"` | top-level `error` (:2037) |
| any other `type`, or none, with a top-level `error` object (the host's "fallback provider" case, :2059-2061) | top-level `error` (:2038) |

Recognition runs before the evidence rules of §6.2. The Codex backend delivers request rejections this way under
HTTP 200 (leaf:translate_responses_sse.rs:217-225; server:…/text_admission/sender/responses.rs:2026-2029).

**Code.** The envelope's code comes from the error object's `code` through a closed table: first the content-policy
and context-length checks `map-provider-error` uses (reference.rs:741-747), then the codes OpenAI publishes for
response errors — at least `rate_limit_exceeded` → `rate_limit`, `server_error` → `upstream_unavailable`,
`invalid_prompt` → `content_policy`, `insufficient_quota` → `payment_required` — and `internal` for anything else
("An unmappable failure is `internal`, never an invented code", provider-adapter.wit:129-131). The full table is
pinned by the documentation-derived judge (§12.3) from OpenAI's published list, not from this record; Codex-specific
codes (for example a plan usage limit) are added only from measurement. With a mapped code, the host's generic
credential cooldown applies to an in-band rate limit the same way it does to a 429 (inferred; §11 item 7).

**Message.** The envelope's `message` is the fixed text of that code, as in `map-provider-error` (reference.rs:761-777);
the upstream's own text goes only to `provider_message`, under the 256-character rule (:780-783). What the client is
shown is the host's decision (§8.2).

**Usage on a failure.** A `response.failed` whose `response.usage` passes §7 yields a `Usage` event before `Error`.
It does not make the exchange a success — the host still settles it as a failure — but it gives the host exact
evidence of what the upstream consumed for the manual review a failed stream goes to today
(`mark_stream_delivery_unknown`, server:…/text_admission/sender/responses.rs:1672). A usage object that is present but
fails §7 is ignored rather than turned into a protocol error, so the failure keeps its code.

## 7. Usage

### 7.1 Mapping

| Wire (`usage` of the terminal `response`) | IR `Usage` (kernel:usage.rs:16-49) | Rule |
|---|---|---|
| `input_tokens` | `input_tokens` | Required. The whole prompt, cached part included |
| `output_tokens` | `output_tokens` | Required. Includes reasoning tokens |
| `total_tokens` | — | Required; must equal `input_tokens + output_tokens`, else `provider_protocol_error` |
| `input_tokens_details.cached_tokens` | `cache_read_tokens` | Optional; a subset of `input_tokens` |
| `input_tokens_details.cache_write_tokens` | `cache_write_tokens` | Optional; a subset of `input_tokens`. Read because the host reads it (leaf:usage_evidence.rs:576-580); its existence on this wire must be confirmed by the documentation-derived judge |
| `output_tokens_details.reasoning_tokens` | `reasoning_tokens` | Optional; a subset of `output_tokens` |
| `tool_usage` (response-level) | — | Absent, null, or zero in every counter; otherwise `provider_protocol_error` (§7.3) |

Further rules, all taken from the host's parser (leaf:usage_evidence.rs:543-619): every count is a non-negative
integer; a `*_details` value that is present and not an object is an error; `cached + cache_write ≤ input`; `reasoning
≤ output`. These are the kernel partition contract (kernel:usage.rs:51-61) applied to this wire. The host's own
conversion from IR usage subtracts `cache_write_tokens` from the input and treats `cache_read_tokens` as a subset
(leaf:usage_evidence.rs:1413-1433), which is what its native parser does for this wire (:611-618), so the two paths
produce the same `TokenUsage`.

**Reasoning tokens.** On this wire they sit inside `output_tokens` — the convention kernel:usage.rs:54-56 states, and
the one the host checks (leaf:usage_evidence.rs:605-609). This package's mapping does not change with the outcome of
boundary Q13, which concerns whether the convention can be frozen for every dialect (Gemini is the open case). The
documentation-derived judge (§12) must still pin it from OpenAI's published semantics rather than from this
implementation.

### 7.2 What is unknown rather than zero

- The three required counts: missing means **error**. A terminal without usage is never turned into zeros — the WIT's
  rule (provider-adapter.wit:110-115), which the three lenient references do not yet follow (boundary §6.1).
- The three detail buckets: missing means **not reported**, which the IR can only write as 0 (kernel:usage.rs:14-15).
  An unreported cache bucket is then priced as ordinary input. An unreported `reasoning_tokens` should leave the
  host's thinking marker (which selects mode-dependent output rates) to its second signal: the component emits
  `ThinkingDelta` whenever reasoning text arrives, and S0 froze the rule "marker = `reasoning_tokens > 0` or thinking
  deltas seen" (canonical-ir-inventory.md:215-221). **The host does not implement that second signal yet**: its
  component-IR accumulator ignores `ThinkingDelta` (leaf:usage_evidence.rs:1782-1786) and its IR usage conversion
  sets the marker from the count alone (:1432). That is a host to-do (§11 item 7), not a component matter; until it
  lands, this package is no worse than the host's native parser for this wire, which also uses the count alone
  (:616).
- A truthful report of `input_tokens = 0` and `output_tokens = 0` is refused by the host on the IR path, because it
  cannot tell it from "nothing reported" (leaf:usage_evidence.rs:1400-1402). Not a component matter; listed in §13.3.

### 7.3 `tool_usage`

The host validates a response-level `tool_usage` object: every tool's counters must be zero, except `image_gen`, whose
positive usage it admits and prices separately from a rate card it looks up by two hard-coded model names
(leaf:usage_evidence.rs:648-713; server:…/token_counter/media.rs:224-278). The IR's `Usage` has no place for a second
meter, and folding image tokens into `output_tokens` would price them at the text rate. v1 therefore refuses any
non-zero `tool_usage`. The host's admission already refuses every tool that is not a client function
(server:…/token_counter/authorize.rs:563-584), and its output-item check refuses the item an image tool would produce
(leaf:usage_evidence.rs:868-892), so whether the positive branch can be reached today is doubtful (§13.5). If the
owner wants hosted-tool usage billed, it needs a vocabulary of its own (R-Q3).

### 7.4 The host's generic checks that still apply

From boundary §6.3, unchanged: `reasoning_tokens ≤ output_tokens`; `cache_read + cache_write ≤ input_tokens`;
`input_tokens ≤ g(request)`, with g computed by the host from the northbound request alone (its bytes plus a fixed,
host-configured allowance per media part; this package supplies no bound of its own); exactly one terminal state and
no `Usage` after `Done`; a `reported` family with no `Usage` cannot be settled; settled amount ≤ reservation.

The input bound holds here for three reasons, and only while all three hold: requests are stateless (§4.4), so no
upstream history is added; references whose size the request does not show (`input_file`, `file_id`) are refused
(§4.2); and the only text the component adds beyond the northbound request is the Codex default instruction
(§4.3), 28 bytes, far less than the JSON framing of any northbound request that reaches it (inferred).

`output_tokens ≤ authorized cap` needs care for `openai-codex`: the upstream is never told the cap (§4.3), so an
honest response can exceed it. The owner's ruling R-Q5 settles what follows, keyed on declarations only: for a
family declaring `output_cap: []` with reported usage the host does not cut the stream (cutting would discard the
upstream's usage report), and a settlement above the reservation goes to manual review on the existing generic path.
That matches today: the host's "still enforced locally" (server:…/text_admission/sender/responses.rs:660-663) refers
to the seal-time check of the canonical body, and the host's stream meter only records forwarded output for
checkpoints, it does not cut (server:…/text_admission/sender/checkpoint.rs:1-20).

The undetectable zone is as boundary §6.3 states it: under-reporting, and deviation within bounds, cannot be detected
by the host.

## 8. Delivery on the Responses northbound surface

### 8.1 The question

When the client speaks Responses and the upstream speaks Responses, the host today forwards the upstream's bytes:
non-streaming bodies unchanged (server:…/text_admission/sender/responses.rs:1441-1468), stream frames unchanged with
`response.completed` withheld until the settlement is persisted (:1691-1697, 1799-1805). The component in this record
produces IR. If the host then renders that IR with the north codec, the client receives a different document: minted
ids (`msg_…`, `rs_…`, `fc_…`; output.rs:74-75, 96; tools.rs:193-196), no annotations, refusal text as `output_text`,
the codec's event sequence instead of the upstream's.

### 8.2 Pass-through delivery, by declaration (owner ruling R-Q1)

A family may declare `north_passthrough: "<protocol>"` (§3.4). The host **may** then deliver the upstream's own bytes
to the client, instead of rendering the IR, when all of the following hold for this request:

1. The inbound surface is that protocol.
2. **The request mapped losslessly.** The north codec renames client tools it cannot pass as plain functions
   (namespaced and custom tools, among others) and restores the client's names when it renders
   (crates/south-north-codec/src/responses/stream.rs:575-576; tools.rs:190-196); upstream bytes carry the renamed
   form. So the host passes bytes through only when the codec's tool restore map for this request is the identity,
   and renders otherwise. On the closed host this is always true today, because its admission accepts only plain
   client functions (server:…/token_counter/authorize.rs:568-577); on a host that admits more it is a per-request
   test.
3. The host does not need the codec's render state for this request (for example to take a continuation snapshot,
   §4.4); a host that keeps continuation history either renders, or runs the codec on the same IR events and
   discards its frames.

When pass-through applies:

- The request is still built by the component from IR.
- **The host splits northbound frames only to find the terminal frame** (umbrella §5.2). It runs `decode_sse_v1`
  (§6; amended 2026-10-10: the incremental `SseDecoderV1` of `south-host-grammars`, whose `position` marks each frame's
  end, and no longer a function the component also uses), over the upstream bytes; it does not parse the frames
  for the component, which still receives the bytes unchanged (`stream_framing: bytes`). The host hands the component
  the bytes chunked at those frame boundaries, one whole frame per `parse-stream-chunk` call — any chunking is
  admissible under `bytes` — so that each call's events belong to one frame. After each call the component holds no
  partial frame (a gate ③ assertion, §12.4). A frame is forwarded only after its call returned without error.
  Non-streaming: the body is forwarded only after `parse-response` succeeded.
- **Terminal withholding needs two signals.** A frame is the terminal frame when its northbound type is a terminal
  type of the protocol (`response.completed`, `response.incomplete` — northbound knowledge the host already applies,
  server:…/text_admission/sender/responses.rs:1993-1998) **and** its own call returned `Done`. The host withholds it
  until settlement is persisted, as today (:1691-1697, 1799-1805). If either signal comes without the other — a
  terminal-type frame whose call did not return `Done`, or a `Done` from a call whose frame is not of a terminal type
  — the host forwards nothing more and parks the stream. This keeps the settlement-before-visibility guarantee even if
  the component misbehaves.
- **Upstream failure frames are not relayed** (owner ruling of 2026-10-01, extending the sibling's N-Q3 ruling from
  failures the gateway originates to relayed upstream failures; recorded under R-Q1 here). When a call returns
  `Error`, the host does not forward the upstream frame. It ends the stream with its own failure event — the stateless
  `responses_error_event` of the sibling record (its §5), with a fixed public message — and settles the exchange as a
  failure. This is one of the conditions of pass-through, enabled by declaration as ruled under R-Q1, and is part of
  the pass-through gate ③ suite (§12.4). The reason: the first draft forwarded the upstream frame, as the host does
  for Codex today (:1666-1676), which exposed the upstream's own text on this path while the rendered path shows only
  the envelope's fixed message (§6.4), so the two delivery paths disagreed about what a client may see.
- A call that fails forwards nothing and parks the stream; the client is shown the same host failure event.
- Evidence is the component's IR events and nothing else.

Why this is not provider knowledge: the northbound protocol is the host's product surface, so "the upstream already
answers in the client's protocol" is a fact about that surface, and the host chooses by declaration, never by
provider identity (boundary R2). It is a new host execution mechanism with a gate ③ suite (§12.4). It is its own
mechanism, not the embeddings world's `NorthIdentical` locator.

Two hosts: the north codec's aim is that both hosts render the same bytes from the same IR. Pass-through does not
weaken that: where it applies, both hosts forward the same upstream bytes through the same decoder, and where it does
not, both render. A host that does not implement pass-through renders always, which is correct, only less faithful.

### 8.3 Without it: what re-rendering loses

| Lost | Why |
|---|---|
| Upstream item ids and the upstream response id as issued | The codec mints ids from its context |
| `encrypted_content` of reasoning items, in streams | No IR stream event can carry an opaque value of this family (§6.1). Non-streaming could use `Message.extensions` plus a codec change (NC-2); `openai-codex` only streams |
| Annotations on output text | No IR field |
| The `refusal` part type | No IR field |
| The upstream's event sequence and `sequence_number`s | The codec numbers its own frames |

Losing `encrypted_content` means a client that replays reasoning items gets nothing to replay. The first draft said
pass-through avoids the response half of that loss. In v1 it does not: the component never sends
`include: ["reasoning.encrypted_content"]` (§4.5, §4.6), and without it a stateless request gets no
`encrypted_content` from the upstream at all (inferred from OpenAI's published semantics), so there is nothing to pass
through. Encrypted reasoning becomes available only when NC-1 lands and the component sends `include` for a model
declaring `reasoning_replay.openai.v1`; from then on pass-through delivers it on the Responses surface, while
re-rendering still needs a kernel change — a family-tagged opaque reasoning event (R-Q8) — and NC-2. This is why the
owner's cutover ruling (R-Q14) keeps Codex rows, whose clients rely on replay, on the native leg until NC-1.

What pass-through does deliver in v1: the upstream's ids, annotations, the `refusal` part type and the upstream's
event sequence.

The Chat and Messages surfaces always re-render; for them the losses above are the normal cost of crossing protocols
and match what the host's converters keep today (text, tool calls and usage only:
leaf:translate_responses.rs:624-728).

## 9. What is shared with `south-north-codec`

Boundary §11 says "wire types can be shared but the mapping cannot". At the baseline there are **no Responses wire
types to share**: the codec works on `serde_json::Value` throughout; its public types are options, context, frames,
stream state and the Claude carrier (crates/south-north-codec/src/responses.rs:24-62; responses/replay.rs:19-29).

| Can be shared | How |
|---|---|
| IR extension key names (`responses_tool_strict`, `responses_reasoning_summary`, `reasoning_effort`, `parallel_tool_calls`, `responses_transient_instructions`, …) | Today string literals on both sides (request.rs:24, 70, 75, 82; tools.rs:104; reference.rs:287, 301, 631). One constants module; its home is R-Q13 |
| The event and item vocabulary | Same |
| Fixtures | A request fixture for this package is a north codec output; a response fixture is a north codec input |
| A round-trip judge | In-repo test depending on both crates: north parse ∘ south build yields the canonical request of §4; south parse ∘ north render preserves text, tool calls, finish reason and every usage bucket |

| Cannot be shared | Why |
|---|---|
| Mapping functions | Opposite directions with different loss rules: the codec mints ids and lifecycle events the upstream never sent; the component checks ids and lifecycle events the upstream did send |
| Stream state machines | The codec's state orders output and numbers frames; the component's state is evidence checking (§6.2) |
| Error mapping | The codec renders IR codes for a client; the component classifies upstream codes for the router |
| The `tsr.c1.` carrier | Claude-family material; this component only refuses it |

North codec follow-ups this record depends on (same repository, separate changes):

- **NC-1** Keep every OpenAI reasoning input item in order with its position, instead of two single-valued extensions
  (§4.5).
- **NC-2** Render reasoning items a component placed in `Message.extensions` back onto the Responses wire
  (non-streaming). This is itself a behaviour on an `extensions` key, so it waits for the same D5 answer as R-Q15.

The first draft's NC-3 (carry `previous_response_id` into the IR for the component to refuse) is withdrawn: the
decision belongs to the host's northbound side (§4.4).

## 10. Auth and credentials

### 10.1 `openai-responses`

`Auth::bearer(provider_api_key)`; the slot is `static`. Descriptor auth admission (boundary §4.2) maps it to the
bearer arm. Nothing else.

### 10.2 `openai-codex`: credential recipe

The host's `CodexMint` (server:…/token_refresh.rs:114-233) and the credential page's import
(server:gateway/src/modules/admin_ui/handler/credentials/mod.rs:181-197), written in the recipe v1 vocabulary of
umbrella §3.3. This section is the authoritative form of the Codex recipe; the umbrella's coverage table (§3.9)
summarises it.

```json
"credentials": {
  "schema": "south.credential-recipe.v1",
  "fields": {
    "access_token":  { "secret": true,  "required": false },
    "refresh_token": { "secret": true,  "required": false },
    "account_id":    { "secret": false, "required": false, "syntax": "token" }
  },
  "require_one_of": [["access_token", "refresh_token"]],
  "import": { "codex_auth_json": {
    "access_token":  { "pointers": ["/tokens/access_token"] },
    "refresh_token": { "pointers": ["/tokens/refresh_token"] },
    "account_id":    { "pointers": ["/tokens/account_id"] },
    "seed": { "present": { "pointer": "/tokens/access_token", "secret": true },
              "expires_at": { "jwt_exp": "/tokens/access_token" } } } },
  "slots": { "codex_access_token": { "minted": "codex_oauth" } },
  "recipes": { "codex_oauth": {
    "steps": [ { "id": "token", "kind": "oauth2_token", "encoding": "json",
      "endpoint": "https://auth.openai.com/oauth/token",
      "params": { "grant_type":    { "const": "refresh_token" },
                  "refresh_token": { "field": "refresh_token" },
                  "client_id":     { "const": "app_EMoamEEZ73f0CkXaXp7hrann" },
                  "scope":         { "const": "openid profile email offline_access" } },
      "extract": { "access_token":  { "pointer": "/access_token",  "secret": true },
                   "refresh_token": { "pointer": "/refresh_token", "secret": true, "optional": true },
                   "account_check": { "jwt_claim": { "token": "/id_token",
                                       "pointer": "/https:~1~1api.openai.com~1auth/chatgpt_account_id" },
                                      "optional": true, "must_equal_field": "account_id" },
                   "expires_at":    { "fixed_window": true } } } ],
    "present": "token.access_token",
    "rotates_refresh_material": true,
    "write_back": { "refresh_token": "token.refresh_token" },
    "fixed_validity_seconds": 3000,
    "default_seconds": 0,
    "refresh_margin_seconds": 300,
    "without_refresh_material": "use_stored",
    "attributes": { "account_id": { "field": "account_id", "export": true } }
  } }
}
```

Every value comes from host code: endpoint (server:…/token_refresh.rs:42), JSON body and its four parameters
(:183-191), rotation (:218-222), the 50-minute window (:210-212), the 300-second margin (:52), the JWT `exp` read for
a token with no stored expiry (:137-146), using the stored token when there is no refresh token (:149-165), the
account id claim path (:69-82), the refusal of a refresh that returns another account (:573-581).
`provider_api_key` is not listed under `slots`: absent means `static`. Every form used — `require_one_of`, import
`seed`, `optional` extraction, `must_equal_field`, `fixed_window`, `write_back` — is in the umbrella §3.3 vocabulary.

Choices made here, each against the host's current behaviour:

- **Clock: a fixed 3000-second window.** After a refresh the host ignores any expiry the response or the token
  carries and stores now + 50 minutes, because the backend returns no explicit expiry and its tokens live about an
  hour (:210-212). The recipe keeps exactly that (`fixed_window`, `fixed_validity_seconds: 3000`) instead of reading
  the token's own `exp`: the host's window is the behaviour that has run in production, and a dual run can then
  compare refresh timing one for one. It lies inside the host's provider-agnostic TTL clamp (60 s … 24 h), so the
  recipe declares no clamp of its own.
- **Seeded token.** An imported access token is seeded with its JWT `exp`, which is what the host reads today for a
  row with no stored expiry (:137-146). For a seeded token that is not a decodable JWT, the host today assumes it is
  valid until the upstream rejects it (:98-103); the recipe declares `default_seconds: 0`, which the host raises to its
  60-second minimum (umbrella §3.5). That is inside the 300-second refresh margin, so with a refresh token the token is
  refreshed on first use; without one `use_stored` sends it as today. Listed in
  §13.3.
- **Rotation and write-back.** Codex rotates its refresh token on every use (:218-222), so the recipe declares
  `rotates_refresh_material: true` and `write_back`, naming the field that receives the new refresh token. The
  host's invariant is independent of the declaration (umbrella §3.5): it never overwrites non-empty refresh material
  with an empty value — the `refresh_token` extraction is `optional`, so a response without one keeps the stored token
  — and it keeps the previous generation so a wrong rotation can be rolled back.
- **The exported account id comes from the stored field only** (umbrella §3.4: export only from fields declared
  non-secret, never from an exchange response). `attributes.account_id` reads the stored `account_id` field, set from
  the imported `auth.json` or entered by the operator. The `id_token` claim is used only as a `must_equal_field`
  check. This matches the host: the host never persists the claim — `account_id` is a column written when the
  credential is created and is not part of the refresh write-back (server:…/token_refresh.rs:357-362) — and it
  refuses a refresh whose claim names a different account than the stored one, because "the bound account is part of
  the credential's identity" (:573-581). When both values are present and differ, the refresh fails as
  `reauth_required` and nothing is written back. One difference remains: when no account id is stored, the host
  today sends the claim of a fresh refresh for that one request (:591); the recipe sends no header then. Listed in
  §13.3.
- **The token endpoint is confirmed by the operator** (umbrella §3.4 rule 1). `https://auth.openai.com/oauth/token`
  is shown to the operator when a credential for this package is created, or matched against a host-side allowlist;
  a recipe endpoint is not trusted because the manifest declares it.
- **Signing gate** (umbrella §3.4 rule 5). Recipes are enabled only for south's own released packages until package
  signing (umbrella Q11) exists; this package qualifies as one of them.
- **At least one token.** The host's import requires `access_token`
  (server:gateway/src/modules/admin_ui/handler/credentials/mod.rs:182), and its minting refuses a credential with
  neither token (server:…/token_refresh.rs:153-164). `require_one_of` states that as a save-time rule, so a
  credential with neither cannot be stored.
- **Status mapping.** The host classifies every non-2xx from the token endpoint as an operator-state error
  (:224-232); the recipe keeps the `on_status` defaults (`4xx` → `reauth_required`, `5xx` → `transient`), so a 5xx
  becomes retryable. Listed in §13.3.

The host's test seam that redirects the endpoint through credential extras (:179-181) has no recipe form, by design:
test endpoints are a build feature of the host executor, not configuration (umbrella §3.4 rule 6). Tests use the
reference interpreter's fake endpoint.

### 10.3 Client-identification material (DP7, ruled 2026-09-30: south takes it in; the host keeps no special case)

Everything the host sends to the Codex backend today that identifies the client or binds the account, and where it
goes. No other identifying header exists in host code: there is no `user-agent` entry for this type
(server:…/south_adapter.rs:268-281) and no constant companion header (:338-341).

| Item | Request | Host today | In this design | Declared as |
|---|---|---|---|---|
| OAuth `client_id` (the Codex CLI's public client id) | token refresh, JSON body | server:…/token_refresh.rs:39-40, 189 | recipe parameter, constant | `credentials.recipes` |
| OAuth `scope` | token refresh, JSON body | server:…/token_refresh.rs:190 | recipe parameter, constant | `credentials.recipes` |
| `content-type: application/json` | token refresh | server:…/token_refresh.rs:185 | implied by `encoding: json` | recipe |
| `authorization: Bearer <access token>` | inference | server:…/upstream.rs:266; server:…/text_admission/sender.rs:825-829 | descriptor `Auth::bearer(codex_access_token)` | auth arm `bearer` + minted slot |
| `chatgpt-account-id: <account id>` | inference | server:…/upstream.rs:267-269; server:…/text_admission/sender.rs:822-824 | the component receives the exported `account_id` attribute through whatever channel umbrella Q14 settles on (below) and writes an ordinary descriptor header; omitted when the attribute is absent | ordinary header; not reserved, not secret |
| Request-body conventions (`instructions` present, `store: false`, `stream: true`) | inference | leaf:translate_responses.rs:27-32 | component (§4.3) | family behavior |

**How the attribute reaches the component is open** (umbrella §3.3 and §16 Q14): the current `ProviderConfig`
fence admits no channel for it. The umbrella recommends a typed field through the kernel chain (S0 D5's own
promotion path); the alternative is an explicit, argued amendment of the fence and D5. Either way the host strips
any client-supplied key that collides with a reserved name. This record only needs the attribute to arrive; it
takes whichever route Q14 settles. *(Settled 2026-10-08, umbrella §13.8: the attribute arrives in
`ProviderConfig.declared` under its name; its field must declare a value syntax.)*

Host code that retires with this: the account-header companion (server:…/text_admission/sender.rs:814-830),
`apply_codex_auth` (server:…/upstream.rs:260-271), and the `chatgpt-account-id` entry in the host's list of headers
operators may not override (server:…/request_extras.rs:34-37). The last needs a generic replacement in the host: an
operator's extra header may never collide with a header the descriptor already carries.

If measurement shows the backend needs more constant headers, any name outside `RESERVED_HEADERS`
(crates/south-contracts/src/lib.rs:232-256) is an ordinary descriptor header the component can add in a package
release, with no contract change.

### 10.4 Instances in closed vocabularies (boundary Q1, ruled: a pin bump counts as modifying the host)

| Vocabulary | What this dialect uses | New compiled-in value needed? |
|---|---|---|
| Secret headers (`SecretHeaderV1`, south-contracts lib.rs:884-895) | None in v1 (both families are bearer). A later Azure family uses `api-key`, already in the set | No. Under boundary §10 the Azure family declares `secret_headers: ["api-key"]` |
| Query parameters (`QueryParameterV1`) | None. (The host's dated Azure URL form with `?api-version=`, server:…/upstream.rs:629-633, is outside this record) | No |
| Quota headers (`ProviderQuotaMetadataFieldV1`, lib.rs:1688-1707) | `x-ratelimit-limit-tokens`, `-remaining-tokens`, `-reset-tokens`, which the host reads for cooldown (server:…/credential_cooldown.rs:50-63). No Codex-specific header is read anywhere in the host | No. Under boundary §10 the package declares the three names |
| Kernel credential header catalog (boundary §10) | Not consulted: both families use `Auth::bearer`, which names no header | No |
| `user-agent` (`ControlledUserAgentV1`, lib.rs:1229-1246) | Not sent today | **Would be**: the type requires a `&'static str` from host program text, and the host supplies values from a table keyed by provider type (server:…/south_adapter.rs:268-281). A package cannot declare one today. If a client `user-agent` is ever needed for this family, it uses the umbrella's single proposal in its §10 (`DeclaredUserAgentV1`, a package-declared value validated by gate ① against the existing value grammar); this record carries no version of its own (R-Q6) |

So J2 for this package is not blocked by boundary §10, provided it sends no `user-agent`.

## 11. What the host's generic executor is expected to do

Nothing here is specific to this package; each item is a generic mechanism the boundary record already requires, plus
new ones (items 5, 7 and 8).

1. Select the package and family from the provider row (the owner ruled for a second provider row over a model-row
   override, R-Q12); mint per the recipe when the slot is `minted` (umbrella §3.6), and pass exported attributes by
   the channel umbrella Q14 settles on (§10.3); a failure is a pre-admission error.
2. Parse the northbound request to IR with the north codec. Resolve `previous_response_id` from history the host
   keeps, or refuse it; the closed host refuses (§4.4).
3. Pass the model's declared `supported_parameters` unchanged (boundary §7.4).
4. `build-http-request`; a capability error is a 400 with zero upstream calls.
5. `ProviderConfig::authorize` (kernel:provider.rs:361), descriptor auth admission (boundary §4.2), the
   `request_facts` seal check — including the empty-cap case — and operator extras outside what the descriptor
   carries and outside the family's `immutable_body_paths`; the latter are refused when the operator saves them (new).
6. Reserve, write the dispatch marker, send; apply `stream_framing` (`bytes`: feed unchanged).
7. Take evidence only from the component's output; apply the generic checks of §7.4; settle or park. Derive the
   thinking marker from `reasoning_tokens > 0` or `ThinkingDelta` seen, the S0 rule its component-IR path does not
   implement yet (§7.2; new for the host, not for south). Apply credential cooldown from the envelope's code for an
   in-band failure as for an HTTP one (§6.4).
8. Deliver: render with the north codec, or, where the family declares `north_passthrough` for the inbound protocol
   and the request mapped losslessly, forward upstream bytes under §8.2's rules (new).

What no longer exists in the host: the questions "does this target speak Responses" and "is this Codex"; the two
flags; the Responses usage state machine; the two converters; the Codex normalizer, error-frame handling, mint
strategy, URL arm and account header.

## 12. Conformance

### 12.1 Fixture rows (gate ②, `south.provider-component.v1`)

Rows the boundary record requires by name (§6.2 item 2) are marked ★. Rows that expect an error depend on the
boundary's B1 work, which gives fixtures an expected-error form.

| Row | Asserts |
|---|---|
| `request.text`, `request.instructions-and-history`, `request.tool-result-turn`, `request.image-input` | Canonical body of §4.1–§4.2, URL, `store: false` |
| `request.tools-and-tool-choice`, `request.structured-output` | Tool flattening, `strict`, the three `tool_choice` forms, `text.format` |
| `request.reasoning-effort`, `request.reasoning-effort-withheld` | The per-model gate |
| `request.stop-is-dropped`, `request.thinking-is-not-replayed` | The two documented drops |
| `request.refused-claude-carrier`, `request.refused-unmappable-part`, `request.refused-unknown-tool-choice`, `request.refused-input-file`, `request.refused-file-id` | Capability errors |
| `request.codex-stream` | No cap, no `temperature`, default `instructions`, `stream: true`, account header from the attribute, Codex URL |
| `request.codex-without-account`, `request.codex-non-stream-refused` | Header omitted; capability error |
| `response.usage` ★, `response.cached-usage` ★, `response.reasoning-usage` | §7.1 mapping, every bucket |
| `response.missing-usage` ★, `response.total-mismatch`, `response.subset-violation`, `response.nonzero-tool-usage` | Protocol errors |
| `response.tool-call`, `response.reasoning-summary`, `response.refusal` | §5 mapping |
| `response.incomplete-max-output`, `response.incomplete-content-filter`, `response.incomplete-unknown-reason`, `response.failed-status`, `response.unmapped-output-item` | `Length` with usage; `ContentFilter` with usage (R-Q2, extended 2026-10-01); three protocol errors |
| `stream.usage-terminal` ★, `stream.no-usage` ★ | Terminal with usage; `response.completed` without usage is an error |
| `stream.text`, `stream.tool-call`, `stream.tool-call-prebuffered-arguments`, `stream.reasoning-summary` | §6.1 |
| `stream.incomplete`, `stream.incomplete-unknown-reason` | §6.3 |
| `stream.failed`, `stream.failed-with-usage`, `stream.error-event`, `stream.error-event-without-sequence`, `stream.typeless-error-object`, `stream.error-code-rate-limit` | §6.4: all three shapes, recognition before the evidence rules, `Usage` before `Error`, the code table |
| `stream.duplicate-terminal`, `stream.event-after-terminal`, `stream.sequence-regression`, `stream.response-id-change`, `stream.item-identity-mismatch`, `stream.unknown-event-type`, `stream.missing-terminal` | One per evidence rule of §6.2 — the adversarial set S0 D1 asked for |
| `error.rejected-credential`, `error.rate-limit`, `error.context-length` | `map-provider-error` |
| `capabilities.declared` | Echo |

### 12.2 Checks

Existing: `Coverage`, `FixtureMatch`, `Determinism`, `UnknownFieldTolerance`, `StreamIncrementality` (every byte
split), `EndpointConfinement`, `AuthErrorsAreNotRetriable`. From the boundary record: `DescriptorAuthWithinManifest`
(§4.4), `RequestFactsHonoured` (§7.6), `UsageNeverDefaulted` (§6.2 item 3).

Two additions this package needs from those checks:

- `UsageNeverDefaulted` **on stream fixtures**. The boundary defines `usage_pointer` for response fixtures. For
  `openai-codex` the stream is the only path, so the mutation must also run there: a stream fixture names the chunk
  holding the terminal frame and the pointer inside it (`/response/usage`); the suite deletes it and requires an
  error, never a `Usage` of zeros.
- `RequestFactsHonoured` for `output_cap: []` (the Codex family): as umbrella §7.6 defines it for that case.
- `ImmutablePathsHonoured` (new with `immutable_body_paths`): every request fixture of the family leaves each declared
  path in the state the family fixes (for Codex: `temperature` and `max_output_tokens` absent, `store`, `stream` and
  `instructions` present).

### 12.3 Beyond gate ②

- **Credential fixtures** (`credential.*`, umbrella §3.7): `codex-refresh` (rendered request), `codex-rotation`
  (with `write_back`), `codex-fixed-window-clock`, `codex-seed-jwt-exp` (and a non-JWT seed counting as expired),
  `codex-on-status`, `codex-account-exported-from-field` (the stored field is exported; the
  claim is not), `codex-account-mismatch` (a refresh naming another account fails and writes nothing back),
  `codex-use-stored`, `codex-neither-token-refused`.
- **Documentation-derived judge** (boundary §6.2 item 5): rows in the `usage_ir_contract` style for this wire — the
  prompt total is `input_tokens` with the cached part inside it; reasoning inside output; total = input + output —
  with expectations taken from OpenAI's published usage semantics and numbers chosen so a wrong field cannot land on
  them (crates/south-component-conformance/tests/usage_ir_contract_v1.rs:1-30). The same judge pins the §6.4 error
  code table from OpenAI's published list.
- **Per-upstream fixture packs** (§3.3): for every upstream other than OpenAI that a host is to route to
  `openai-responses`, a pack captured from that upstream's traffic, passing the same judges.
- **Round-trip judge** with the north codec (§9).
- **Sandbox parity**: the wasm build equals the native reference on the frozen pack, as for the other packages.
- **Fuzz**: the stream parser is an untrusted parser and gets a fuzz target.

### 12.4 Host obligations (gate ③)

| Mechanism | Suite | Status |
|---|---|---|
| Credential recipe execution | `south.credential-recipe.v1` (umbrella §3.7) | Required before `openai-codex` |
| Descriptor auth admission | Covered by the boundary's gate ② check plus T21 `rogue-arm` | Required |
| `request_facts` seal, including the empty cap | T21 `rogue-cap`, plus one case for a family declaring no cap | Required |
| Pass-through delivery | New: `south.north-passthrough-delivery.v1` — a fake upstream stream; asserts frames split by `decode_sse_v1` and forwarded unchanged, one frame per call with no partial frame held after it, the terminal frame withheld until settlement and only when its northbound type and `Done` agree (both mismatch directions park), an upstream failure frame replaced by the host's fixed-message failure event and never forwarded, a component failure forwards nothing, a request with a non-identity tool restore map rendered instead, the non-streaming body forwarded only after a successful parse | Required before any host passes bytes through (owner ruling R-Q1, with the failure-frame replacement of §8.2 ruled on 2026-10-01; the south half of R-Q1 is still open) |
| Operator extras vs `immutable_body_paths` | New case in the same suite family: an extra touching a declared path is refused at save time | Required |

Per boundary R4, each is marked `verified` under `host_capabilities` only once both hosts pass it.

## 13. Migration and dual runs

### 13.1 Steps

| Step | Side | Content | Acceptance |
|---|---|---|---|
| R0 | Host | Owner rulings R-Q1 to R-Q5 are recorded (§16), including the 2026-10-01 extensions under R-Q1 (relayed failure frames) and R-Q2 (`content_filter`); the south halves of R-Q1 and R-Q5 remain. Fix or accept the suspected defects of §13.5 in the native leg first (P21 §9 practice) | — |
| R1 | South | Package with `openai-responses`: reference implementation, fixtures, judges, wasm build. Needs umbrella B1 (usage strictness), B2 (`request_facts`, descriptor auth admission) and B3 (the package declares `runtime_abi` and must be listed in the release index), and `decode_sse_v1` from the image world's minor (amended 2026-10-10: R1 does not need it, since the component splits its own stream; the host's pass-through needs it, from 0.53.0, umbrella §13.13) | Suite green; listed in the release index with the upstreams that have a fixture pack (§3.3). **Implemented 2026-10-10, not released (§17)**: suite green; OpenAI's pack only (R-Q16); the release index entry comes with the release |
| R2 | Host | Route provider rows to the family, one upstream at a time and only upstreams with a fixture pack; refuse `previous_response_id` on the northbound side; dual run; remove `supports_responses` / `upstream_requires_responses` branching for covered rows | §13.2 |
| R3 | South + host | `openai-codex`: needs boundary B4 (recipes) and the host's recipe executor | Credential fixtures and gate ③ suite green; §13.2 on Codex rows |
| R4 | Host | Retire the code of §13.4 | J1 count falls; removing the package leaves the host compiling, testing and starting (J3) |

### 13.2 Dual-run reconciliation

The same request goes through the native leg and the component leg, once under each billing form
(`GATEWAY_TEST_BILLING_FORM=balance|quota`), over a corpus restricted to requests the IR can represent, written in the
canonical shape of §4 (item-list `input`, explicit `store: false`, client function tools only, leading system
messages only). Both legs talk to a **replaying fake upstream**: it records what each leg sends and answers both
with the same recorded bytes for the same corpus entry. Two live calls could never be compared byte for byte — the
upstream mints ids and timestamps, generates different text, and echoes request parameters that the two legs send
differently (§13.3). Compared:

1. **Upstream request**: method, URL, non-auth headers, body by JSON equality. For Codex rows also the account header.
   For Chat-surface "Responses only" rows the native body is the host converter's output, compared for non-streaming
   requests (the native leg never streams upstream there).
2. **Northbound response**: under pass-through delivery, byte equality on the Responses surface, except that an
   upstream failure frame is now replaced (§8.2); otherwise, and on the Chat surface, equality of text, tool calls (id,
   name, argument bytes), finish reason and `usage`.
3. **Funds**: reservation, settled amount, the thinking marker, `tokens_estimated = 0`, `quantity_estimated = 0`.
4. **Failure fixtures**: terminal without usage; total mismatch; duplicate terminal; frame after terminal; unknown
   event; unmapped output item; in-band `error` and `response.failed`; 401; 429; 5xx; truncated stream. Each must end
   in the same funds state on both legs, except where §13.3 says otherwise.
5. **Token refresh** (Codex): the rendered refresh request equals the host's; rotation writes back; two concurrent
   requests refresh once.

### 13.3 Intentional differences (on record, not reconciliation failures)

- **Truncated and filtered generations settle** (§6.3): native parks `incomplete`; the component reports `Length`
  with usage for `max_output_tokens`, per the owner's ruling R-Q2, and `ContentFilter` with usage for
  `content_filter`, per its extension of 2026-10-01. Other `incomplete` reasons stay errors.
- **Non-zero `image_gen` tool usage is refused** (§7.3): native admits and prices it. Owner ruling R-Q3.
- **`store`**: always `false`; native forwards a client's `store: true`, including to Codex, because the normalizer
  only inserts the key when absent (leaf:translate_responses.rs:27). The fields of §4.6 are not forwarded.
- **`previous_response_id`, `input_file`, `file_id`**: refused — the first by the host on its northbound side
  (§4.4), the other two by the component as capability errors with zero upstream calls (§4.2); native forwards all
  three.
- **`input` form**: always an item list; native forwards a string input to non-Codex upstreams.
- **Chat surface, "Responses only" models**: the component leg streams upstream when the client streams and carries
  `response_format`, `parallel_tool_calls` and reasoning effort; the host's converter forwards none of these
  (leaf:translate_responses.rs:489-493, 559-565) and always calls non-streaming. It merges every system or developer
  message into `instructions` wherever it stands (:291-307), where the component merges only the leading run and
  sends later ones as `system` input items (§4.1, §4.2). Array-form system content is no longer a difference: the host
  fixed it in `a82de329` (its issue #61).
- **Messages surface**: models on this family become reachable; today they are not (§1).
- **Non-streaming Codex requests**: refused at build time instead of being sent un-normalized (owner ruling R-Q5).
- **Codex seeded token that is not a JWT, and 5xx classification** (§10.2): counted as expired instead of valid;
  a token-endpoint 5xx is retryable instead of an operator-state error. Refresh timing does not differ: the recipe
  keeps the host's fixed 50-minute window.
- **Codex account header with no stored account id**: native sends the claim of a fresh refresh for that request;
  the recipe sends no header (§10.2).
- **Upstream failure frames on the Responses surface**: native relays Codex's frame byte for byte; the component leg
  sends the host's fixed-message failure event (§8.2).
- **Zero-token responses**: refused on the IR path (§7.2).
- **Encrypted reasoning replay**: not available until NC-1 (§4.5, §8.3), on either delivery path.

### 13.4 Host code that retires

Line counts measured at `a82c852b`; paths are relative to the token-station-server root.

| File | Lines | Retires when |
|---|---|---|
| `crates/gateway-provider-protocol/src/translate_responses.rs` | 728 (whole file). Production callers exist for three functions only: `normalize_codex_responses_body` :10-35, `openai_chat_to_responses` :283-568, `codex_responses_to_openai_chat` :624-728; the other nine are called from tests only | R2 (Chat surface) and R3 (Codex) |
| `crates/gateway-provider-protocol/src/translate_responses_sse.rs` | 250 (whole file; called from tests only) | Any time |
| `crates/gateway-provider-protocol/tests/golden/translate_responses.rs`, `…/translate_responses_sse.rs`; `gateway/src/modules/inference/engine/translate/tests/responses.rs` | 858, 387, 81 | With the files above |
| `crates/gateway-provider-protocol/src/usage_evidence.rs`, Responses ranges :176-202, 543-619, 621-955, 1496-1513, 1531-1539, 1671-1675, 1959-2271 | 784 of 2,523 | R4, after every upstream of §3.3 is covered |
| `gateway/src/modules/inference/engine/text_admission/sender/responses.rs` | Native seal arm :412-455 (44), Codex normalizer :660-674 (15), native frame loop :1646-1698 (53), Codex error-frame helpers :2026-2130 (105) — of 2,423. The frame loop survives in generic form for pass-through (§8.2); the error-frame helpers do not, since failure frames are no longer relayed | R4 |
| `gateway/src/modules/inference/engine/text_admission/chat.rs` :501-510 and the `ResponsesBurst` contract (`sender.rs` :3726-3741, 3779-3800) | about 50 | R2 |
| `gateway/src/modules/inference/engine/token_refresh.rs` :54-103 (JWT helpers), :114-250 (`CodexMint` and its entry point) | 187 of 2,377 | R3, with the recipe executor (umbrella §3.6) |
| `gateway/src/modules/inference/engine/upstream.rs` :260-271, 535-539; `text_admission/sender.rs` :814-830 | 34 | R3 |
| `gateway/src/modules/inference/engine/token_counter/media.rs` :214-278 (Codex image-tool usage and its rate card) | 65 | R4, subject to R-Q3 |
| `gateway/src/modules/inference/engine/request_extras.rs` :424-434 (the Codex-only save-time refusal of a `temperature` body extra) and the `chatgpt-account-id` entry at :37 | 12 | R3, replaced by the generic `immutable_body_paths` check and the descriptor-header collision rule (§3.4, §10.3) |
| The flags `supports_responses` and `upstream_requires_responses`: config, catalog columns, admin form, and the branch sites listed in §1 | not counted | R4; columns need forward migrations |

### 13.5 Suspected host defects found while reading (code reading, not run)

Evidence is the cited code only; no impact is claimed.

1. `response.incomplete` and a non-`completed` status are rejected before the converter's `incomplete → length` branch
   can run (leaf:usage_evidence.rs:547-549, 2035-2041 against leaf:translate_responses.rs:683-686), so a generation
   stopped at the output cap appears to end in `delivery_unknown` on every native Responses path.
2. **Fixed.** A system or developer message whose `content` is a parts array contributed nothing to `instructions`
   on the Chat surface. Confirmed by a failing test and fixed in the host at `a82de329` (its issue #61; now
   leaf:translate_responses.rs:294-307).
3. `response_format`, `reasoning_effort`, `stop` and `parallel_tool_calls` are not carried by the Chat → Responses
   converter (:489-493, 559-565).
4. A client's `store: true` survives the Codex normalizer (:27), although its documentation says it sets `store:
   false`.
5. Non-streaming Codex requests skip the normalizer (server:…/text_admission/sender/responses.rs:438-440, 664-674) and
   are sent with `max_output_tokens` and without `stream: true`, which the normalizer's own comments say the backend
   rejects.
6. **Withdrawn.** The first draft listed two host comments as disagreeing on whether Codex accepts
   `max_output_tokens`. They do not: server:…/text_admission.rs:1201-1203 is about the gateway accepting the client's
   cap for pricing, not about the upstream (§4.3).
7. The separately priced `image_gen` branch (leaf:usage_evidence.rs:664-672) sits behind an output-item check that
   refuses every item type except message, function call and reasoning (:868-892). Whether an upstream ever reports
   positive image-tool usage without such an item was not verified.
8. A model flagged `upstream_requires_responses` cannot be served on `/v1/messages`
   (server:…/text_admission.rs:1157-1176; inferred).

## 14. Versioning

- New package `provider-openai-responses` 1.0.0 in the existing world: no WIT change, no new world, no contract
  number. South minor. The existing thirteen packages are untouched.
- It depends on manifest fields from the umbrella (`usage_evidence`, `stream_framing`, `request_facts` including
  `output_cap: []`, `credentials` with the §3.3 vocabulary this package uses) and adds two proposals of its own
  (`immutable_body_paths`, `north_passthrough`). All are manifest schema changes, hence south minor each (R5); absent
  `north_passthrough` means "always render" and absent `immutable_body_paths` means "no declared paths", which is what
  every existing package gets today.
- `decode_sse_v1` is a new pure function in `south-contracts` with golden vectors and a fuzz target, stated in
  umbrella §5.2 as the SSE sibling of the eventstream deframer and released with the image world's minor, before R1.
  *(Amended 2026-10-10: it is in the host-only crate `south-host-grammars` and ships in 0.53.0, early and alone
  (Q-B6-6); it is a host dependency of pass-through, not of the package. Umbrella §13.13.)*
- Phasing: R1 after umbrella B1, B2 and B3; R3 after B4. Both are inside the umbrella's B6, which this package
  enters without needing B7a (it declares no new instance, §10.4).
- Host link layer: the recipe executor and pass-through delivery are one-time generic mechanisms (P21 §1.4). After
  them, adding or removing a Responses-speaking upstream is a package-layer change. Under the ruling on boundary Q1,
  the only thing in this record that would still force a pin bump is a `user-agent` instance (§10.4).
- Dialect words: v1 defines none of its own. It reads the existing `reasoning_effort` word and refuses the
  `reasoning_replay.claude.v1` family's carriers. `reasoning_replay.openai.v1` is proposed for the release that
  follows NC-1. A per-model request difference measured later gets a word in an `openai_responses.` namespace — a
  package-layer change.
- Until the release index of boundary §9 exists, the host's fetch script lists package names
  (server:scripts/fetch_south_components.sh:60-70 per boundary §9.1), so a fourteenth package is a J2b④ red item
  there, not here.

## 15. Rejected alternatives

- **A Responses family inside `provider-openai-compatible`** (§3.1): the response-side functions get no family, so the
  package would have to guess the wire from each body; and its identity would churn for every OpenAI-compatible
  provider.
- **"Responses only" as a dialect word** (§3.2): a word cannot move a model to another package's wire.
- **A new world or new WIT functions for Responses**: nothing in the mapping needs one. What the world cannot express
  is outside the WIT — opaque reasoning in the IR stream, a second usage meter, request state — and none of it would
  be fixed by a new function.
- **Emitting `encrypted_content` as `RedactedThinking` or as a thinking signature** (§6.1): it would be re-labelled as
  Claude material by the north codec.
- **Folding image-tool tokens into `output_tokens`** (§7.3): prices a different meter at the text rate.
- **Keeping the host's strict Responses validator next to the component**: two parsers for one wire is what DP1
  removes, and the J1 count would not fall.
- **The `oauth` auth arm for Codex**: umbrella §3.8 deprecates it; it leaves every provider fact of the exchange in
  the host.
- **Supporting `store: true` and `previous_response_id` in v1** (§4.4): the prompt would no longer be bounded by the
  outbound body, so the host's input bound (§7.4) would fire on honest responses; and the IR has no field for either.
- **Deriving this mapping by inverting the north codec** (§9): the two directions have different loss rules and
  different state.
- **Aggregating a Codex stream in the host for non-streaming callers**: possible as a generic host feature, but it is
  host product behavior, not something this package should assume; the owner ruled for refusal (R-Q5, §4.3).
- **Refusing `previous_response_id` inside the component** (first draft's NC-3): wrong for a host that resolves
  continuation history itself, redundant for one that refuses it (§4.4).
- **Relaying upstream failure frames under pass-through** (first draft): it exposes the upstream's own text on one
  delivery path and not the other, and bypasses the host's ruling on failure messages (§8.2).
- **Exporting the account id from the refresh response** (first draft): the recipe rule forbids exporting from an
  exchange response; the stored field plus an equality check does the same job and matches the host (§10.2).
- **One `openai-responses` family for every Responses upstream on OpenAI's documentation alone** (first draft): the
  judge cannot vouch for other upstreams' usage conventions (§3.3).

## 16. Open questions

Tags: S = south maintainers, L = lv, K = kernel. Boundary Q13 (reasoning-token convention) remains open and is not
repeated; this package's mapping does not depend on it (§7.1).

On 2026-09-30 lv ruled on the L-tagged questions: as recommended, on condition that each recommendation fits the
final goal DP0 (no provider-specific logic in the host). One recommendation was adjusted to meet that condition
(R-Q5); the rulings are recorded under each question. On 2026-10-01 lv extended the rulings under R-Q1 (relayed
failure frames) and R-Q2 (`content_filter`).

- **R-Q1 (S, L)** Delivery on the Responses surface: north-identical bytes by declaration (recommended, §8.2), or
  always re-render and accept §8.3's losses.
  **Ruled for the host side (lv, 2026-09-30): north-identical delivery, by declaration.** The south maintainers'
  half remains open.
  Note (2026-10-01): the ruling stands; the declaration is renamed `north_passthrough`, and review added conditions that
  narrow it without reversing it — pass-through only when the request mapped losslessly, terminal withholding by
  northbound type and `Done` together (§8.2). Its v1 benefit is also smaller than
  first stated: no `encrypted_content` until NC-1 (§8.3). Review also proposed replacing a relayed upstream failure
  frame with the host's fixed-message failure event, extending the sibling's N-Q3 ruling from gateway-originated
  failures to relayed ones (§8.2); that is ruled below.
  **Ruled (lv, 2026-10-01): a relayed upstream failure frame is replaced by the host's fixed-message failure
  event**, extending the N-Q3 ruling to relayed failures. Pass-through is enabled by declaration as ruled above, with
  this replacement as one of its conditions (§8.2).
  **Ruled for the south side (lv, 2026-10-10): accepted as ruled for the host** — `north_passthrough` by
  declaration, with the conditions of §8.2, including the replacement of relayed failure frames.
- **R-Q2 (L)** Does a generation truncated at the cap (`response.incomplete` with usage) settle as a success with
  finish reason `length` (recommended: yes, as on every other wire), or stay in `delivery_unknown` as the native leg
  does today?
  **Ruled (lv, 2026-09-30): it settles as a success with finish reason `length`.**
  Note (2026-10-01): the ruling names `length` only; every other reason is now a protocol error rather than
  `Other(reason)` (§5). Review also proposed settling `incomplete_details.reason = content_filter` as a success with
  finish reason `content_filter`, as every other dialect does; that is ruled below.
  **Ruled (lv, 2026-10-01): `incomplete_details.reason = content_filter` also settles as a success, with finish
  reason `content_filter`**, in addition to `max_output_tokens` → `length`; any other reason stays a protocol error.
- **R-Q3 (L)** Hosted-tool usage (`tool_usage.image_gen`): refuse any non-zero value (recommended for v1; the host
  admits no hosted tool today), or model a second meter — a south-local response extension plus host pricing.
  **Ruled (lv, 2026-09-30): version 1 refuses any non-zero value.**
- **R-Q4 (L)** Stateful Responses (`store: true`, `previous_response_id`): refuse (recommended for v1), or support
  with a redesign of the input bound.
  **Ruled (lv, 2026-09-30): version 1 refuses.**
- **R-Q5 (L, S)** Codex and the output cap: first measure whether the backend accepts `max_output_tokens`. If it does
  not, accept that this family cannot be told the cap and that over-cap responses go to manual review (recommended:
  accept; it is today's behavior), which needs `request_facts.output_cap: []` in south. Same question for
  non-streaming Codex callers: refuse at build time (recommended) or have the host aggregate.
  **Ruled for the host side (lv, 2026-09-30), adjusted for DP0:** measure first. If the backend does not accept the
  field, one rule for every family that declares `output_cap: []`, keyed on declarations and
  never on provider identity. With `usage_evidence: absent` the host enforces the authorized cap on its own output
  meter and ends the answer with `length` (after cutover; today's behavior during the dual run). With reported usage
  the host does not cut, because cutting would discard the upstream's usage report; a settlement above the
  reservation goes to manual review, which is the existing generic path.
  The same rule as the Kiro record's K-Q1. Non-streaming Codex callers are refused at build time.
  **Ruled for the south side (lv, 2026-10-10): accepted as ruled for the host.** South provides
  `request_facts.output_cap: []` (the umbrella's form) if the measurement shows Codex refuses the field. The
  measurement gates R3 only; R1 (`openai-responses`) does not wait for it.
- **R-Q6 (S)** A package-declared `user-agent` value (§10.4): this record needs none today and carries no proposal of
  its own. If one is needed, it uses the umbrella's single proposal (its §10: a new owned type validated by gate ①
  against the existing value grammar, which reopens the 2026-08-20 controlled-user-agent ruling); the questions are
  asked there as umbrella Q15 and Q16.
  **No ruling needed (2026-10-10):** v1 sends no `user-agent`; the umbrella's Q15 and Q16 carry the question.
- **R-Q7 (S)** Recipe details (§10.2): every form this recipe uses is in the umbrella §3.3 vocabulary, and the clock
  is decided (a fixed 3000-second window, for the reason given in §10.2). What remains is one behaviour: a seeded
  access token that is not a decodable JWT is refreshed on first use (`default_seconds: 0`, raised to the host's
  60-second minimum, which lies inside the refresh margin; recommended) rather than trusted until the upstream rejects
  it, as the host does today.
  **Ruled (lv, 2026-10-10): refresh on first use** (`default_seconds: 0`, raised to the host's 60-second minimum).
  Affects R3 only.
- **R-Q8 (S, K)** OpenAI opaque reasoning through the IR: a family-tagged opaque reasoning event in the stream
  contract (kernel chain), the `reasoning_replay.openai.v1` word, and north codec follow-ups NC-1 and NC-2 — or rely
  on pass-through delivery for the response half (effective only once NC-1 lets the component send `include`, §8.3)
  and do only NC-1.
  **Ruled (lv, 2026-10-10): v1 does NC-1 only and relies on pass-through delivery for the response half.** No
  kernel change for v1; a family-tagged opaque reasoning event stays a later option. Codex rows are not cut over
  before NC-1 (R-Q14).
- **R-Q9 (S)** Unknown `response.*` event types: error (this record, matching the host) or ignore (what the Converse
  reference does with unknown events, reference_bedrock_converse.rs:736-739, and what the sibling Kiro record chose
  for its wire). Strict means a new inert upstream event stops streams until a package release; lenient means content
  or cost could pass unseen and, under pass-through, reach the client unparsed. Recommended: strict here, because
  this wire has a published event list; the difference from Kiro is argued in §6.2.
  **Ruled (lv, 2026-10-10): strict.** An unknown `response.*` event type is an error, as recommended.
- **R-Q10 (S)** Refusal text: ordinary text (this record), an error, or a finish reason.
  **Ruled (lv, 2026-10-10): ordinary text**, as this record maps it (§5, §6.1). Pass-through keeps the `refusal`
  part type; a re-rendered answer carries it as `output_text`.
- **R-Q11 (S, K)** `block_index` for a wire with two-level indices: first-seen counter (this record) or another rule.
  **Ruled (lv, 2026-10-10): first-seen counter**, as this record does.
- **R-Q12 (L)** How a "Responses only" model on a mixed provider is declared in the host: a second provider row (no
  new mechanism) or a generic model-row family override (recommended if such providers are common; host work only).
  **Ruled (lv, 2026-09-30): a second provider row.** It is operator data and needs no new host mechanism; a generic
  model-row family override is reconsidered only if such providers turn out to be common.
- **R-Q13 (S)** Where shared Responses vocabulary constants live (§9), given that guests depend on the conformance
  crate and the north codec may depend only on the kernel IR (ARCHITECTURE.md:70-80).
  **Ruled (lv, 2026-10-10):** the constants live in one module of the conformance crate, which guests already
  depend on. The north codec keeps its literals (it may depend only on the kernel IR), and an in-repo test that
  depends on both crates asserts the two spellings agree.
- **R-Q14 (L)** Cutover order: may the Responses surface be cut over before NC-1, losing encrypted reasoning replay
  for clients that use it (recommended: no for Codex rows; yes for rows with no such clients), and which upstreams of
  §3.3 (Azure, Bedrock with SigV4) are in production use and must be covered before R4.
  **Ruled (lv, 2026-09-30): as recommended.** Which upstreams are in production use is answered by a read-only query
  before R4.
- **R-Q15 (S, K)** The three `extensions` keys this package acts on (§4.1), exactly the three the compatible
  reference already acts on: promote them to typed IR fields through the kernel chain (recommended; S0 D5's own
  promotion path, done once for both packages), or record them as an explicit, argued exception to D5. v1 follows
  the existing precedent, adds no key and does not act on `responses_reasoning_summary`. Umbrella Q14 covers the
  general question and names this one.
  **Ruled (lv, 2026-10-10): v1 follows the existing precedent** — the same three keys the compatible reference
  reads, no new key, no behaviour on `responses_reasoning_summary`. Promotion to typed IR fields goes through the
  kernel chain as a separate change, done once for both packages.
- **R-Q16 (S)** Third-party Responses upstreams (§3.3): admit each to `openai-responses` by a captured-traffic
  fixture pack in this package (recommended while their wire and usage match OpenAI's), or give every non-OpenAI
  upstream its own family from the start. Either way no upstream is routed without its own evidence.
  **Ruled (lv, 2026-10-10): admitted to `openai-responses`, each with its own captured-traffic fixture pack**, while
  its wire and usage match OpenAI's; an upstream that departs from them gets its own family. No upstream is routed
  without its own evidence.

## 17. Implementation of R1 (2026-10-10)

Step R1 is implemented on branch `responses-r1`, stacked on the rulings of 2026-10-10 (#176); nothing is released.
Only the `openai-responses` family is built. The `openai-codex` family, its credential recipe and the §12.3 credential
fixtures are step R3, and the host's pass-through (§8.2) and the gate ③ suites of §12.4 are host work.

### 17.1 What landed

- **The package.** `components/provider-openai-responses` 1.0.0: a wit-bindgen shell around
  `south_component_conformance::reference_openai_responses`, family `openai-responses`, the `bearer` arm on the static
  slot `provider_api_key`, `usage_evidence: reported`, `stream_framing` absent (`bytes`), `request_facts` as §3.4
  (`output_cap: ["/max_output_tokens"]`, model `/model`, stream `/stream`), `immutable_body_paths: {"openai-responses":
  ["store"]}` and `north_passthrough: {"openai-responses": "responses"}` (R-Q1). Build script
  `scripts/build-openai-responses-component.sh`; release workflow, sandbox parity and CI cache entries.
- **The reference** implements §4–§7 as written for the family: a stateless body (`store: false` always, never
  `previous_response_id`, D4 / R-Q4), leading system messages as `instructions`, the §4.2 items, `input_file` and any
  `file_id` refused (D10), exactly the three precedent `extensions` keys (R-Q15); the response and stream mappings with
  `incomplete` settling only for `max_output_tokens` (`length`) and `content_filter` (`content_filter`) (R-Q2), refusal
  parts as text (R-Q10), a first-seen `block_index` (R-Q11), strict unknown events (R-Q9), the three failure-frame
  shapes recognised before the evidence rules with `Usage` before `Error`, strict usage, and any non-zero `tool_usage`
  refused (R-Q3). `map-provider-error` is the OpenAI-compatible reference's, unchanged.
- **SSE.** The component splits its stream with `south_component_conformance::sse_split`, the WHATWG rules the host's
  `decode_sse_v1` implements (LF, CR and CRLF; one BOM; comments; joined `data`; end of input dispatches). It links no
  host-only crate. Its tests take `south-host-grammars` as a dev-dependency only: the decoder's 45 golden vectors give the
  splitter the same events whole, byte by byte and at every split; a property test holds the two equal on arbitrary
  input; and frames cut at `SseDecoderV1::position` over every stream row each give the splitter exactly one event and
  leave it idle — the "no partial frame after a call" assertion of §8.2, on the component side.
- **Vocabulary (R-Q13).** `south_component_conformance::responses_vocabulary` holds the event, item, part, incomplete-reason
  and `extensions`-key spellings; the OpenAI-compatible reference reads its three keys from it. The north codec keeps its
  literals; `openai_responses_vocabulary_v1`, which depends on both crates, asserts the codec writes the same keys and
  renders only events, items, parts and reasons the module names.
- **Judges.** The round-trip judge of §9 (north parse then south build gives the codec-derived request rows; south parse,
  north render, south parse keeps text, tool calls, finish reason and every usage bucket, for responses and for
  streams, the latter through the component's strict stream rules). The documentation-derived usage judge of §12.3
  (`usage_ir_contract_v1`: the cached part inside `input_tokens`, reasoning inside `output_tokens`, the total, every
  strictness case, `tool_usage`) and the §6.4 code table. The fixture-wide prompt sweep covers the new pack.
- **Fixtures.** `fixtures-openai-responses/`, 69 cases with their sources in the pack's README: 21 request rows (7 of
  them north codec outputs), 18 response, 26 stream, 3 error, 1 capabilities.
- **Fuzz.** `contract_parsers` gains the Responses stream parser (chunking never changes its events or first error; the
  splitter agrees with `decode_sse_v1` on every input; the same bytes as a 2xx body never panic) and a seed.
- **Gate ①** (south-provider-api): the two proposed manifest fields of §3.4. `immutable_body_paths` per family, under the
  task world's immutable-path grammar (`are_immutable_body_paths`, bounds mirrored from `south-contracts`);
  `north_passthrough` per family, a closed `NorthProtocolV1` (`chat_completions`, `responses`, `messages`). Both name
  only declared families and are provider-world declarations (`ManifestErrorV1::DeliveryIsAProviderWorldDeclaration`,
  `InvalidImmutableBodyPaths`, `InvalidNorthPassthrough`).
- **Gate ②** (additive; every existing pack passes unchanged): a request case may expect a refusal
  (`{"error": <envelope>}`), as B1 gave response cases and B6-2 stream cases, so §12.1's `request.refused-*` rows are
  fixture rows; such a case skips the checks that judge a descriptor. New check `ImmutablePathsHonoured` (§12.2).

### 17.2 Versions (Q47)

`south-provider-api` and `south-component-conformance` changed, so both move 0.51.0 → 0.52.0 (minor: a public field on
`ComponentManifestV1`, new `ManifestErrorV1` and `CheckV1` variants), with the requirements on them (runtime, provider
conformance, fuzz) and every component lockfile. `south-contracts` is unchanged. Every existing package takes a patch
bump with unchanged behavior and keeps its `south_runtime` (`shipped_packages_v1::the_responses_package_retires_every_
published_053_package_identity`). The workspace version and `compatibility.json` are not touched; this is not a release.

`provider-openai-responses` declares `south_runtime` 0.53.0, the version being built. The oldest runtime that admits it
is the release that ships this change, because no released gate ① admits `immutable_body_paths` or `north_passthrough`
(unknown manifest fields are refused); following `embeddings-gemini` before 0.51.0 (#167), the package declares the
version being built until the release commit raises it, and `scripts/check-declared-runtime.sh --build` loads it under
this tree meanwhile. B6-2 declared an older runtime because it used only declarations Converse already used; this
package cannot.

### 17.3 Decisions this implementation took where the record is silent

1. **`ImmutablePathsHonoured`** is defined as: every request a family's fixtures build holds each declared path in one
   state, present in all or absent from all (for `openai-responses`, `store` present; the package test also pins it to
   `false` under adversarial `extensions`). A family with paths and no built request fails the check.
2. **The `error` event's error object.** OpenAI's reference puts `code` and `message` at the top level of the `error`
   event; the host (§6.4 table) reads a top-level `error` object. The component reads the `error` object when present,
   else the frame itself.
3. **Failure envelopes** carry HTTP status 502, as the Anthropic reference's in-band failures do.
4. **After a failure frame** any further frame is a protocol error (§6.2: rules 1 and 4 still apply to what follows);
   the end of stream after it is clean.
5. **An empty chunk before any byte** is tolerated (gate ② feeds one at split 0); a clean end of stream after the
   stream began and before a terminal is `transport_truncated` (§6.2 rule 8).
6. **Stricter than the host in five places**, each a protocol error: an SSE `event:` name that disagrees with the
   payload's `type`; arguments for a call never opened or already finished; a call whose `call_id` or `name` changes
   between `added` and `done`; a `response.completed` / `response.incomplete` whose `response.status` says otherwise; a
   `function_call` item without `call_id` or `name`.
7. **The §6.4 table** maps OpenAI's published response error codes beyond the four the record names:
   `vector_store_timeout` → `timeout`, `image_content_policy_violation` → `content_policy`, and the image input codes
   (`invalid_image`, `invalid_image_format`, `invalid_base64_image`, `invalid_image_url`, `image_too_large`,
   `image_too_small`, `image_parse_error`, `invalid_image_mode`, `image_file_too_large`,
   `unsupported_image_media_type`, `empty_image_file`, `failed_to_download_image`, `image_file_not_found`) →
   `invalid_request`. Anything else stays `internal`.
8. **Parts the tables do not list** are capability errors: thinking parts in a user message, any non-text part in a
   system or tool message, and an image in an assistant message. A system message's text parts are concatenated without
   a separator; the leading messages are joined with `\n` (§4.1). An assistant turn's text parts are concatenated into
   one `output_text`.
9. **Non-streaming reasoning** gives one `Thinking` part per summary part, then per content part, item by item, before
   the text; `ToolCalls` wins as the finish reason whenever a `function_call` item is present, `incomplete` included.
10. **Stream fixtures** carry a compact `response` object inside lifecycle and terminal events (identity, state, output
    and usage, without the request echo), to keep gate ②'s every-byte split fast; the non-streaming rows carry the full
    object.

### 17.4 Rulings on the implementation (lv, 2026-10-10)

- **`input_tokens_details.cache_write_tokens`: keep reading it**, as the host does (§7.1), so the two paths produce the
  same `TokenUsage` in the dual run. OpenAI's published usage object does not document the field, so the
  documentation-derived judge pins host parity only; revisit when a capture shows it.
- **Ratified:** the additive gate ② request-refusal form (§17.1) and the definition of `ImmutablePathsHonoured`
  (§17.3 item 1).
- **`UsageNeverDefaulted` on stream fixtures** (§12.2) moves to R3, where Codex motivates it.
- **The package's `south_runtime`** holds the runtime under construction (0.53.0, as `embeddings-gemini` did in #167)
  and is set to the release's version by the release commit, since no published runtime admits the two new manifest
  fields.

## Revision note (2026-10-01)

- Header: host baseline moved to `a82c852b`; host line numbers in `translate_responses.rs` updated for the #61 fix.
- §2: D2, D4, D6, D7, D8 rewritten; D10 (refuse `input_file` / `file_id`) and D11 (`immutable_body_paths`) added.
- §3.3: `openai-responses` narrowed to OpenAI plus upstreams with their own captured-traffic fixture pack (R-Q16).
- §3.4: manifest field renamed `north_passthrough` and defined on its own; `immutable_body_paths` added, replacing
  the host's Codex-only extras check.
- §4.1 / §4.2: instructions-merge difference from the host recorded; `extensions` keys tied to S0 D5 (R-Q15);
  `input_file` and `file_id` refused, with the reason.
- §4.3: Codex refusal of non-streaming callers kept (owner ruling R-Q5) and contrasted with Kiro's buffered path; the
  misread host comment corrected.
- §4.4 / §9 / §11: the `previous_response_id` decision moved to the host's northbound side; NC-3 withdrawn.
- §5 / §6.3: `incomplete` settles as success only for `max_output_tokens` and `content_filter`; others are errors.
- §6 / §6.2 / §6.4: SSE split by `decode_sse_v1`; failure frames (three shapes) recognised before the evidence rules,
  with a public code table and `Usage` before `Error`; the unknown-event choice argued against Kiro's.
- §7.2 / §7.4 / §11: the thinking marker's second signal recorded as a host to-do; the input bound restated on the
  northbound request.
- §8: pass-through conditions (lossless mapping, terminal type plus `Done`, failure frames replaced by the host's
  fixed-message event); the `encrypted_content` claim corrected.
- §10.2 / §10.3 / §10.4: account id exported from the stored field only, refresh claim used as an equality check;
  endpoint confirmation, rotation invariant, signing gate, "at least one token"; attribute channel and user-agent
  deferred to the umbrella.
- §12: fixtures and gate ③ suite extended for all of the above.
- §13: dual run uses a replaying fake upstream; intentional differences updated; #61 marked fixed; §13.5 item 6
  withdrawn; line counts at `a82c852b`.
- §15 / §16: four rejected alternatives added; notes added under R-Q1 and R-Q2 without changing either ruling; R-Q6,
  R-Q7, R-Q8, R-Q9 updated; R-Q15 and R-Q16 added.
- Round 2 (consistency): §12.2 `RequestFactsHonoured` for `output_cap: []` now points to umbrella §7.6.
- Round 2: §10.2 is the authoritative Codex recipe — fixed 3000-second window with its reason, `write_back` because
  Codex rotates, account id only from the stored field, the `id_token` claim only as `must_equal_field`, import
  `seed` with `jwt_exp` and `default_seconds: 0`; R-Q7 narrowed to the non-JWT seed; §13.3 updated to match.
- Round 2: §3.3 / §3.4 / D5 — `usage_evidence` and `stream_framing` are package-level scalars (umbrella R6).
- Round 2: recipe forms named as in umbrella §3.3 (`write_back`, `require_one_of`, `must_equal_field`, `optional`,
  `seed`, `on_status` class keys); no recipe clamp, the host's 60 s … 24 h clamp applies.
- Round 2: citations into umbrella §3 renumbered (recipe v1 §3.3, trust model §3.4, invariants §3.5, executor §3.6,
  conformance §3.7, `oauth` deprecation §3.8); every scratch-file decision label replaced by the umbrella section.
- Round 2: §4.1 keeps only the three `extensions` keys the compatible reference reads, as existing precedent;
  `responses_reasoning_summary` is not sent in v1; §10.3, §11 and R-Q15 cite umbrella Q14 and no longer name the
  withdrawn attribute key.
- Round 2: §6 / §8.2 — the host splits northbound frames only to find the terminal frame (umbrella §5.2 wording);
  `decode_sse_v1` released with the image world's minor.
- Round 2: §3.2 / §3.4 / §14 no longer describe the pre-revision umbrella; `output_cap: []` is the umbrella's form,
  not this record's proposal.
- Round 2: §13.1 R1 and §14 add umbrella B3.
- Round 2: D6, §5, §6.3, §12.1, §13.3 and the R-Q2 note present the `content_filter` extension as a proposal awaiting
  the owner; until ruled, it is a protocol error. The notes under R-Q1 and R-Q2 are headed "Note (2026-10-01)".
- Round 2: §2 D7, §8.2 and the R-Q1 note present replacing relayed upstream failure frames as a proposal awaiting
  the owner (an extension of the sibling's N-Q3 ruling); pass-through waits for that ruling.
- Round 2: §4.3 no longer suggests Codex could reuse the buffered path (Codex is SSE `bytes`; umbrella §5.2).
- Rulings of 2026-10-01:
  - R-Q2 extension ruled by lv: `incomplete_details.reason = content_filter` also settles as a success with finish
    reason `content_filter`; any other reason stays a protocol error. D6, §5, §6.3, §12.1, §13.1, §13.3 and the
    R-Q2 note state it.
  - Relayed upstream failure frames ruled by lv (under R-Q1, extending the sibling's N-Q3): replaced by the host's
    fixed-message failure event. The condition that no host enables pass-through until this is ruled is dropped;
    pass-through is enabled by declaration as ruled under R-Q1, with the replacement as one of its conditions. D7,
    §8.2, §12.4, §13.1 and the R-Q1 note state it.
- 2026-10-10, lv ruling (umbrella §13.13): `decode_sse_v1` lives in the host-only crate `south-host-grammars` and ships
  in 0.53.0. §6, §8.2, §13.1 R1 and §14 gain notes: the component splits its own stream, the host cuts whole frames
  for pass-through with `SseDecoderV1::position`, and R1 no longer waits for the decoder.
- 2026-10-10, lv rulings on the remaining questions: R-Q1 and R-Q5 accepted for the south side as ruled for the
  host (R-Q5's measurement gates R3 only); R-Q7 refresh on first use; R-Q8 NC-1 only, relying on pass-through; R-Q9
  strict; R-Q10 ordinary text; R-Q11 first-seen counter; R-Q13 constants in the conformance crate, north codec
  literals checked by a cross-crate test; R-Q15 existing precedent for v1, typed fields later through the kernel
  chain; R-Q16 one fixture pack per third-party upstream. R-Q6 needs no ruling. The record is accepted.
- 2026-10-10, step R1 implemented (§17, new): the header and §13.1 note it; nothing else in the record changes.
