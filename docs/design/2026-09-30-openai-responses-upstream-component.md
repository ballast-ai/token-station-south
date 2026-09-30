# The OpenAI Responses upstream dialect component (`provider-openai-responses`)

Status: proposed — drafted for review by the host team (token-station-server P21), not accepted

Date: 2026-09-30

Predecessors: `2026-09-30-host-zero-vendor-boundary.md` (the umbrella; this record expands the first row of its §11
and depends on its §3 credential recipe, §4 descriptor auth admission, §5 `stream_framing`, §6 usage discipline, §7
`request_facts` and dialect words, §8 compatibility range, §10 instance declaration),
`2026-09-30-embeddings-contract.md` (structure of this record; the `NorthIdentical` precedent reused in §8),
`2026-09-28-responses-north-codec.md` and its validation record (the northbound Responses mapping in
`south-north-codec`), `2026-09-29-claude-model-dialect.md` (dialect words in `supported_parameters`),
`2026-08-21-canonical-ir-inventory.md` (S0 D1: usage extraction is component translation; D3: no provenance on
`StreamEvent`), `2026-08-23-renderer-refusal-for-unmappable-blocks.md` (a renderer refuses what it cannot spell).
Sibling, drafted in parallel: `2026-09-30-north-codec-render-gaps.md` — northbound only; it renders the client-facing
failure event (its G3) and the single-burst Chat stream (its G4), while this record owns recognising the upstream's
failure frames (§6.4) and produces the IR both consume.

Origin: token-station-server plan P21 (`docs/product-review-v2/plans/2026-09-29-P21-*.md`) (§2.5, §5 S3–S5,
Appendix B.2 and B.4, where this component is listed as scope only, with no design), and
plan P15 (`2026-09-28-P15-Responses*.md`) with annexes A1 (I12–I19, O03–O06) and A2 (§4 target matrix).
Owner rulings applied here, as relayed by the host team on 2026-09-30:
**DP7 / boundary Q10** — south takes in client-identification headers, the host keeps no special case; **boundary Q1**
— bumping the south or kernel pin counts as modifying the host; **boundary Q12 / embeddings E-Q3** — the two estimate
conventions may coexist (not raised again here; this dialect reports usage). Boundary Q13 (the reasoning-token
convention) is still open.

Baseline: south `origin/main` = v0.42.0 (`3135e36`); kernel `f585bc83` (protocol 0.4.0 / kernel v0.3.0). Host line
numbers refer to token-station-server `4d5bb4e5` and carry a `server:` prefix; kernel line numbers carry `kernel:` and
refer to `crates/protocol/src/` at `f585bc83`. South line numbers refer to the baseline. `server:…/` abbreviates
`server:gateway/src/modules/inference/engine/`; `leaf:` abbreviates `server:crates/gateway-provider-protocol/src/`.
Statements marked "inferred" are conclusions from reading code, not from running it.

## 1. Problem

The OpenAI Responses API is one of the two text upstream dialects the host still translates and meters in its own code
(the other is Kiro; P21 §3.1, boundary §11). Four pieces of host code carry it:

| Piece | Where | What it knows |
|---|---|---|
| Native pass-through leg of `/v1/responses` | server:…/text_admission/sender/responses.rs:412-455, 1385, 1646-1698 | Which targets speak Responses (`supports_responses` on the provider row, `upstream_requires_responses` on the model row, or the types `OpenaiCodex` / `AwsBedrockOpenai`); forwards the client's body with only `model` and the cap rewritten; forwards upstream frames unchanged and withholds `response.completed` until settlement |
| Chat → Responses conversion for "Responses only" models | server:…/text_admission/chat.rs:501-510; leaf:translate_responses.rs:283-559, 615-719 | Converts a Chat Completions body to a Responses body, always calls the upstream non-streaming, converts the answer back, and replays it to a streaming client as one burst (server:…/text_admission/sender.rs:3779-3800) |
| Strict usage evidence for the Responses wire | leaf:usage_evidence.rs:543-955, 1959-2271 (plus :176-202, 1496-1513, 1531-1539, 1671-1675) | Terminal status, usage arithmetic, the closed list of `response.*` events, item and part identity, `tool_usage` |
| Codex backend specifics | leaf:translate_responses.rs:10-35; server:…/text_admission/sender/responses.rs:438-453, 660-674, 1666-1676, 2026-2130; server:…/token_refresh.rs:114-250; server:…/upstream.rs:260-271, 535-539; server:…/text_admission/sender.rs:806-830 | Request rules (no cap, no `temperature`, `store`, default `instructions`, forced `stream`), the URL suffix, in-band error frames under HTTP 200, the OAuth refresh, the account header |

The usage part measures **784 lines** at `4d5bb4e5` (734 excluding blank and comment lines) across the seven ranges
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
- **D2 Two families in v1**: `openai-responses` (any upstream that speaks the public Responses API with a static key)
  and `openai-codex` (the Codex subscription backend: minted bearer, account header, its own request rules). Both
  share one response and stream wire, which is what lets them share a package (§3.1).
- **D3 The component maps IR ⇄ Responses wire in both directions and is the only source of usage evidence** (§4–§7).
  Evidence rules the host enforces today are moved into the component unchanged in strength.
- **D4 Requests are always stateless**: `store: false`, no `previous_response_id`. A request that asks for server-side
  state is refused, not silently sent without it (§4.4).
- **D5 Usage is strict**: `usage_evidence: reported` for both families; a terminal without exact usage is an error,
  never a zero; a non-zero `tool_usage` is an error because the IR has no bucket for it (§7).
- **D6 `response.incomplete` with usage is complete evidence of a truncated generation** (finish reason `length` or
  `content_filter`), not a failure. This differs from the host's native leg and needs the owner's confirmation (§6.3,
  R-Q2).
- **D7 On the Responses northbound surface the recommended delivery is the upstream's own bytes** ("north-identical"),
  declared by the family and executed generically by the host, with the component's IR events as the only evidence
  (§8). Re-rendering through the north codec is the fallback and is what the Chat and Messages surfaces get by
  construction.
- **D8 Codex credentials use credential recipe v1** (boundary §3.8), and the client-identification material the
  backend is sent today is designed in: the OAuth client id and scope in the recipe, the account header in the
  component (DP7, ruled 2026-09-30: south takes them in; the host keeps no special case) (§10).
- **D9 This dialect needs no new instance in any compiled-in vocabulary** (secret header, query name, quota header).
  The one instance it could need later — a `user-agent` value — has no package-declared form today (§10.4).

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

Boundary §7.4 lists "Responses only" as a per-model dialect word. A dialect word can change the request shape
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
| Upstream | Any upstream speaking the public Responses API | The Codex subscription backend |
| URL | `base_url.resolve(ProviderApi::Responses)` (kernel:provider.rs:126-133, 138-161) | `{base_url}/codex/responses` — the URL the host builds today (server:…/upstream.rs:535-539) |
| Auth | `Auth::bearer`, slot `provider_api_key` (static) | `Auth::bearer`, slot `codex_access_token` (minted, §10.2) |
| Extra request header | none | `chatgpt-account-id`, when the credential exports an account id (§10.3) |
| Output cap on the wire | `max_output_tokens` | none (§4.3) |
| Non-streaming request | supported | refused at build time (§4.3) |
| `usage_evidence` | `reported` | `reported` |
| `stream_framing` | `bytes` (absent) | `bytes` (absent) |

Upstreams the host's native leg also serves today, and how each is covered:

- **Azure OpenAI** (`api-key` header; server:…/upstream.rs:183-189, 606-633): one more family in this package,
  differing only in auth presentation, exactly as `azure-openai-v1` does in the compatible package. Not in v1; the
  host treats its Responses wire as identical (it uses the same strict parser), which a fixture pack must confirm.
- **Bedrock's OpenAI-shaped endpoint** (server:…/text_admission.rs:664-670): with an API key it is an ordinary
  `openai-responses` row whose base URL is the regional endpoint (inferred); with SigV4 it needs a sibling package,
  because `host_signed` admits no other arm in the same manifest (crates/south-provider-api/src/manifest.rs:383-385).

The host's native Responses code retires only when every upstream it serves in production is covered (§13.4).

### 3.4 Manifest sketch

`usage_evidence` (boundary §6.2), `request_facts` (§7.2) and `credentials` (§3.3) are the boundary record's fields,
written with its draft names.

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
  "usage_evidence": { "openai-responses": "reported", "openai-codex": "reported" },
  "request_facts": {
    "openai-responses": { "output_cap": ["/max_output_tokens"], "model": { "body": "/model" },
                          "stream": { "body": "/stream" } },
    "openai-codex":     { "output_cap": [], "model": { "body": "/model" }, "stream": { "body": "/stream" } }
  },
  "north_identical": { "openai-responses": "openai-responses", "openai-codex": "openai-responses" },
  "credentials": { "schema": "south.credential-recipe.v1", "…": "see §10.2" },
  "conformance": { "required_suite": "south.provider-component.v1", "fixtures": "fixtures/" },
  "compatibility": { "…": "as the other provider packages; range fields per boundary §8" }
}
```

Two of these are **new proposals of this record**, not in the boundary record:

- `request_facts.output_cap: []` — "this family's wire has no cap field". Boundary §7.2 requires that exactly one
  declared location carries the authorized cap; it has no way to say there is none. An explicit empty list is distinct
  from an absent key (absent keeps today's three top-level fields, R5). See §4.3 and R-Q5.
- `north_identical` (per family; absent = the host always renders through the north codec). See §8.

`model-capabilities` echoes `config.models`, as the four existing references do (reference.rs:600-607).

## 4. Request mapping: IR → Responses

### 4.1 Top level

| IR (`ChatRequest`, kernel:chat.rs:196-217) | Responses body | Notes |
|---|---|---|
| `model` | `model` | |
| leading `Role::System` messages | `instructions`, joined with `\n` | The join the host uses today (leaf:translate_responses.rs:294-298, 474-476). The north codec turns a Responses `instructions` string into a leading System message (crates/south-north-codec/src/responses/request.rs:22-26) |
| remaining messages | `input` (always an item list) | §4.2 |
| `tools` | `tools`: `{type: "function", name, description, parameters, strict?}` | `strict` from `extensions.responses_tool_strict[name]` (north codec tools.rs:103-105; the compatible reference reads the same key, reference.rs:285-292) |
| `tool_choice` | `"auto"` / `"none"` / `"required"`, or `{type: "function", name}` | Object forms accepted: the Chat form the north codec emits (request.rs:184) and Anthropic's `{type: "tool", name}`; any other object is a capability error |
| `extensions.parallel_tool_calls` | `parallel_tool_calls`, only alongside `tools` | Same rule as reference.rs:297-304 |
| `response_format` | `text.format`: `{type: "text"}`, `{type: "json_object"}`, or `{type: "json_schema", name, schema, strict?, description?}` | Inverse of north codec request.rs:130-162 |
| `extensions.reasoning_effort` | `reasoning.effort` | Same per-model gate as the compatible reference (reference.rs:323-338): sent when the model declares `reasoning_effort` or declares no parameter set; an effort that came from an Anthropic thinking translation needs the explicit declaration |
| `extensions.responses_reasoning_summary` | `reasoning.summary` | Carried by the north codec (request.rs:74-76) |
| `sampling.temperature`, `sampling.top_p` | `temperature`, `top_p` | Dropped for `openai-codex` as §4.3 says |
| `sampling.max_output_tokens` | `max_output_tokens` | Not sent for `openai-codex` (§4.3) |
| `sampling.stop` | dropped | The host's converter does not forward it either (leaf:translate_responses.rs:480-484). Dropping follows the kernel `Sampling` contract (kernel:chat.rs:130-145) |
| `stream` | `stream: true` when set, otherwise omitted | |
| — | `store: false`, always | §4.4 |

Descriptor: `POST`, the family's URL (§3.3), header `content-type: application/json`, `auth` per §10.

### 4.2 Input items

| IR message | Responses input item |
|---|---|
| `Role::User`, `Content::Text` | `{role: "user", content: [{type: "input_text", text}]}` |
| `Role::User`, parts | `Text` → `input_text`; `ImageUrl` → `{type: "input_image", image_url: <url>, detail?}`; `Unknown` of type `image_url` carrying `file_id` (how the north codec preserves it, request.rs:341-350) → `{type: "input_image", file_id, detail?}`; `Unknown` of type `input_file` / `input_audio` → forwarded unchanged; any other `Unknown` → capability error |
| `Role::User`, no content | `{role: "user", content: []}` (as the host does, leaf:translate_responses.rs:368-370) |
| `Role::System` after the first non-system message | `{role: "system", content: [{type: "input_text", text}]}` |
| `Role::Assistant`, text | `{role: "assistant", content: [{type: "output_text", text}]}`, omitted when empty |
| `Role::Assistant`, each `ToolCall` | `{type: "function_call", call_id, name, arguments}`, after the text item |
| `Role::Tool` | `{type: "function_call_output", call_id: tool_call_id, output: <text>}`; parts that are all text are concatenated; a non-text part is a capability error; a missing `tool_call_id` is a capability error |
| `Thinking` / `RedactedThinking` parts on an assistant turn | not sent (§4.5) |

Message items carry no `type` key and no `id`: this is the shape the host's two converters produce
(leaf:translate_responses.rs:19-22, 303-306, 439-444), so dual-run bodies agree on it.

### 4.3 Family `openai-codex`

Copied from the host's normalization (leaf:translate_responses.rs:10-35) and its call site
(server:…/text_admission/sender/responses.rs:432-453):

- `max_output_tokens` is not sent. The authorized cap still exists in the IR; the manifest declares `output_cap: []`,
  so the host's seal check has nothing to look for.
- `temperature` is not sent. (`top_p` is sent if present: the host removes only `temperature`.)
- `instructions` defaults to the string `You are a helpful assistant.` when the request has none.
- `stream: true` always. A request with `stream = false` is a capability error at build time — zero upstream calls.
  Today the host does not normalize a non-streaming Codex body at all
  (server:…/text_admission/sender/responses.rs:438-440), although its own comment says the backend requires streaming;
  the upstream's answer to such a request was not measured.
- `input` is always an item list (the host lifts a string input for Codex, leaf:translate_responses.rs:16-24; this
  component never emits the string form for either family).

Whether the backend really rejects `max_output_tokens` is asserted by the host's normalizer and its test
(server:…/text_admission/sender/responses.rs:2197-2213) but contradicted by another host comment
(server:…/text_admission.rs:1201-1203, which says the Codex Responses surface accepts the field as is). It must be
measured before implementation; if the backend accepts the field, `openai-codex` declares `output_cap:
["/max_output_tokens"]` and the empty-list proposal is not needed for this package (R-Q5).

### 4.4 State: `store` and `previous_response_id`

The IR has no place for either. The north codec validates `previous_response_id` for shape and then drops it
(request.rs:21, 114-123); it never reads `store`. So on the IR path a request that relies on server-side history would
be sent with the current turn only and answered without error. The host's native leg avoids this only by forwarding
the client's body as is.

v1 rule: the component always sends `store: false` and never sends `previous_response_id`. For that to be a refusal
rather than a silent drop, the north codec must carry the client's `previous_response_id` into
`ChatRequest.extensions` (proposed key `responses_previous_response_id`, follow-up NC-3 in §9), and this component
answers a request carrying it with a capability error. Until NC-3 lands the host must refuse the field itself before
building the IR. Supporting stateful Responses is rejected for v1 (§15) and left to the owner (R-Q4).

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

## 5. Response mapping: non-streaming

`parse-response` accepts a 2xx body only if:

- `object` is `response`; `error` is absent or null;
- `status` is `completed`, or `incomplete` with a usage object (§6.3). `failed`, `cancelled`, `in_progress`, `queued`
  and any other value are `provider_protocol_error`;
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
`Stop` for `completed`; for `incomplete`, `Length` when `incomplete_details.reason` is `max_output_tokens`,
`ContentFilter` when it is `content_filter`, `Other(reason)` otherwise. `ChatResponse.id` and `.model` are the
upstream's.

`openai-codex` never takes this path (§4.3).

`map-provider-error` (non-2xx) uses the compatible reference's table unchanged (reference.rs:738-790): the
`error.code` checks for content policy and context length, then the status table, `provider_message` kept when at most
256 characters, `retry-after` in seconds.

## 6. Stream mapping

The component splits SSE frames itself, as the three SSE references do (reference.rs:519-528), and holds one state
machine per stream.

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
| `response.failed`, `error` | `Error { error }`, then nothing (§6.4) |

Tool ordinals count tool calls only, in first-seen order — the same translation from `output_index` the host's stream
converter makes (leaf:translate_responses_sse.rs:34-43; called only from tests at this baseline). `block_index` is
assigned in first-seen order to each (item, summary or content index) pair, so that two summary parts of one reasoning
item stay two blocks (R-Q11 asks whether that reading of kernel:stream.rs:51-60 is accepted).

Encrypted reasoning content is **not** emitted as `RedactedThinking` or `ThinkingSignatureDelta`. Those events mean
Claude material to the north codec: its stream renderer collects them into a `claude-signed-thinking` carrier
(crates/south-north-codec/src/responses/stream.rs:350-360, 392, 603-605), which would offer OpenAI's opaque value to
Claude targets — the relabelling P15 A1 I13 and A2 §4 rule out. Stream contract 2 has no event for an opaque value of
another family (§8.3).

### 6.2 Evidence rules

The component enforces what the host's state machine enforces today (leaf:usage_evidence.rs:1959-2271); a violation is
`provider_protocol_error` from `parse-stream-chunk`:

1. Exactly one terminal frame; any frame after it is an error (:1969-1979).
2. The event type is in the closed list of §6.1 (:1980-2014). An unlisted `response.*` event is an error, not ignored
   — see R-Q9.
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
- `response.incomplete`, **if** it carries a usage object that passes §7.

The second case is a change from the host. Today a non-streaming body whose status is not `completed` is refused
(leaf:usage_evidence.rs:547-549) and a `response.incomplete` frame poisons the stream (:2035-2041), so a generation
truncated at the cap is parked in `delivery_unknown`; the converter's `incomplete → length` branch
(leaf:translate_responses.rs:674-677) sits behind that check — the usage parse at
server:…/text_admission/sender.rs:3736-3766 runs before the conversion at :3779-3784 — and so is not reached (code
reading, not run). Every other dialect, and the north codec's own renderer
(crates/south-north-codec/src/responses/output.rs:157-169), treat a length stop as a normal finish with usage. A
truncated generation consumed tokens and its usage is reported; this record recommends settling it (R-Q2). A
`response.incomplete` without usage stays an error.

### 6.4 Error frames

`response.failed` and `error` map to `StreamEvent::Error`. The envelope's code comes from the frame's `error.code`
through the same content-policy and context-length checks as `map-provider-error`; with no HTTP status to fall back
on, anything else is `internal` ("An unmappable failure is `internal`, never an invented code",
provider-adapter.wit:129-131). The upstream's message is kept in `provider_message` under the 256-character rule. The
Codex backend delivers request rejections this way under HTTP 200 (leaf:translate_responses_sse.rs:217-225;
server:…/text_admission/sender/responses.rs:2026-2029); the list of codes it uses was not available from code and must
come from measurement.

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
  An unreported cache bucket is then priced as ordinary input. An unreported `reasoning_tokens` leaves the host's
  thinking marker to its second signal: the component emits `ThinkingDelta` whenever reasoning text arrives
  (canonical-ir-inventory.md:215-221). The host's native parser for this wire uses the count alone
  (leaf:usage_evidence.rs:616).
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
`input_tokens ≤ g(request)` — valid here because requests are stateless (§4.4), so the prompt is bounded by the
outbound body; exactly one terminal state and no `Usage` after `Done`; a `reported` family with no `Usage` cannot be
settled; settled amount ≤ reservation.

`output_tokens ≤ authorized cap` needs care for `openai-codex`: the upstream is never told the cap (§4.3), so an
honest response can exceed it, and the check sends it to manual review. That appears to be today's outcome too: the
host's comment says the cap is "still enforced locally" (server:…/text_admission/sender/responses.rs:660-663), and P21
§5 S5 states that the only usage-side bound today is the settlement guard (inferred). It is an accepted property of
this family, or a reason to measure whether the backend takes the field (R-Q5).

The undetectable zone is as boundary §6.3 states it: under-reporting, and deviation within bounds, cannot be detected
by the host.

## 8. Delivery on the Responses northbound surface

### 8.1 The question

When the client speaks Responses and the upstream speaks Responses, the host today forwards the upstream's bytes:
non-streaming bodies unchanged (server:…/text_admission/sender/responses.rs:1441-1468), stream frames unchanged with
`response.completed` withheld until the settlement is persisted (:1691-1697, 1799-1805). The component in this record
produces IR. If the host then renders that IR with the north codec, the client receives a different document: minted
ids (`resp_…`, `msg_…`, `fc_…`; output.rs:75, 96; tools.rs:193-196), no `encrypted_content`, no annotations, refusal
text as `output_text`, the codec's event sequence instead of the upstream's.

### 8.2 Recommendation: north-identical delivery, by declaration

A family may declare `north_identical: "<protocol>"`, the value naming a northbound protocol `south-north-codec`
implements (`openai-chat`, `openai-responses`, `anthropic-messages`; a closed set owned by south). When the inbound
surface is that protocol, the host **may** deliver the upstream's bytes. Everything else is unchanged:

- The request is still built by the component from IR.
- The component still parses every byte. Non-streaming: the host forwards the body only after `parse-response`
  succeeded. Streaming: the host splits the stream into SSE frames — the northbound protocol's own framing, which it
  already knows — and feeds one frame per `parse-stream-chunk` call; a frame is forwarded only after its call returned
  without error.
- The frame whose call returns `Done` is the terminal frame; the host withholds it until settlement, as today. A frame
  whose call returns `Error` is forwarded and ends the exchange without settling as success — what the host does for
  Codex error frames today (server:…/text_admission/sender/responses.rs:1666-1676). A call that fails forwards nothing
  and parks the stream; what the client is then shown is a northbound matter (sibling record §5: a relayed stream has
  no render state, hence its `responses_error_event`).
- Evidence is the component's IR events and nothing else.

This is the embeddings record's `NorthIdentical` (its §5) applied to a text protocol: the northbound protocol is the
host's product surface, so recognising "the upstream already answers in it" is not provider knowledge, and the host
chooses by declaration, never by provider identity (boundary R2). It is a new host execution mechanism and gets a gate
③ suite (§12.4).

### 8.3 Without it: what re-rendering loses

| Lost | Why |
|---|---|
| Upstream item ids and the upstream response id as issued | The codec mints ids from its context |
| `encrypted_content` of reasoning items, in streams | No IR stream event can carry an opaque value of this family (§6.1). Non-streaming could use `Message.extensions` plus a codec change (NC-2); `openai-codex` only streams |
| Annotations on output text | No IR field |
| The `refusal` part type | No IR field |
| The upstream's event sequence and `sequence_number`s | The codec numbers its own frames |

Losing `encrypted_content` means a client that replays reasoning items gets nothing to replay. Restoring it on the IR
path needs a kernel change — a family-tagged opaque reasoning event (R-Q8) — and the north codec follow-ups of §9.
North-identical delivery avoids the response half of that; the request half (NC-1) is needed either way before replay
works.

The Chat and Messages surfaces always re-render; for them the losses above are the normal cost of crossing protocols
and match what the host's converters keep today (text, tool calls and usage only:
leaf:translate_responses.rs:615-719).

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
  (non-streaming).
- **NC-3** Carry `previous_response_id` into the IR so a component can refuse it (§4.4).

## 10. Auth and credentials

### 10.1 `openai-responses`

`Auth::bearer(provider_api_key)`; the slot is `static`. Descriptor auth admission (boundary §4.2) maps it to the
bearer arm. Nothing else.

### 10.2 `openai-codex`: credential recipe

The host's `CodexMint` (server:…/token_refresh.rs:114-233) and the credential page's import
(server:gateway/src/modules/admin_ui/handler/credentials/mod.rs:181-197), written as recipe v1 (boundary §3.3):

```json
"credentials": {
  "schema": "south.credential-recipe.v1",
  "fields": {
    "access_token":  { "secret": true,  "required": false },
    "refresh_token": { "secret": true,  "required": false },
    "account_id":    { "secret": false, "required": false, "syntax": "token" }
  },
  "import": { "codex_auth_json": {
    "access_token":  "/tokens/access_token",
    "refresh_token": "/tokens/refresh_token",
    "account_id":    "/tokens/account_id" } },
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
                   "account_id":    { "jwt_claim": { "token": "/id_token",
                                       "pointer": "/https:~1~1api.openai.com~1auth/chatgpt_account_id" },
                                      "export": true, "optional": true },
                   "expires_at":    { "jwt_exp": "/access_token" } } } ],
    "present": "token.access_token",
    "rotates_refresh_material": true,
    "refresh_margin_seconds": 300,
    "without_refresh_material": "use_stored",
    "attributes": { "account_id": { "output": "token.account_id", "else_field": "account_id", "export": true } }
  } }
}
```

Every value comes from host code: endpoint (server:…/token_refresh.rs:42), JSON body and its four parameters
(:183-191), rotation (:218-222), the 300-second margin (:52), using the stored token when there is no refresh token
(:149-165), the account id claim path (:69-82). `provider_api_key` is not listed under `slots`: absent means `static`.
Field names follow the boundary's draft; `optional` on an extraction and the `output … else_field` attribute form are
additions this family needs.

Three points where the boundary's recipe sketch does not yet say enough (R-Q7):

1. **The exported attribute must outlive the refresh that produced it.** Most requests do not refresh, and the header
   still needs the account id; the host returns the stored value then, and prefers a freshly extracted one
   (server:…/token_refresh.rs:237-238, 520-523). `attributes` above uses a proposed `output … else_field` form; the
   recipe contract must also say that an exported extraction is persisted with the credential.
2. **Clock.** After a refresh the host does not read the token's expiry: it stores now + 50 minutes (:210-212), and
   decodes the JWT `exp` only when no expiry is stored (:137-146). `jwt_exp` with a 300-second margin refreshes at the
   token's real expiry minus five minutes instead. An intended difference, listed in §13.3. What `jwt_exp` does with a
   token that is not a decodable JWT must be specified; the host treats it as valid (:98-103).
3. **Status mapping.** The host classifies every non-2xx from the token endpoint as an operator-state error
   (:224-232); the recipe default (4xx → `reauth_required`, 5xx → `transient`) is kept here, so a 5xx becomes
   retryable. Also listed in §13.3.

The host's test seam that redirects the endpoint through credential extras (:179-181) has no recipe form, by design: a
recipe endpoint is a manifest constant (boundary §3.3). Tests use the reference interpreter's fake endpoint.

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
| `chatgpt-account-id: <account id>` | inference | server:…/upstream.rs:267-269; server:…/text_admission/sender.rs:822-824 | the component reads `south_credential_attributes.account_id` from `ProviderConfig.extensions` (boundary §3.3) and writes an ordinary descriptor header; omitted when the attribute is absent, as today | ordinary header; not reserved, not secret |
| Request-body conventions (`instructions` present, `store: false`, `stream: true`) | inference | leaf:translate_responses.rs:27-32 | component (§4.3) | family behavior |

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
| `user-agent` (`ControlledUserAgentV1`, lib.rs:1229-1246) | Not sent today | **Would be**: the type requires a `&'static str` from host program text, and the host supplies values from a table keyed by provider type (server:…/south_adapter.rs:268-281). A package cannot declare one. Boundary §10 does not list this vocabulary; if a client `user-agent` is ever needed for this or any DP7 provider, it must become a manifest-declared instance validated by the same grammar at gate ① (R-Q6) |

So J2 for this package is not blocked by boundary §10, provided it sends no `user-agent`.

## 11. What the host's generic executor is expected to do

Nothing here is specific to this package; each item is a generic mechanism the boundary record already requires, plus
one new (item 8).

1. Select the package and family from the provider row (or model-row override, R-Q12); mint per the recipe when the
   slot is `minted`, place exported attributes under `south_credential_attributes` (boundary §3.3, §3.5); a failure is
   a pre-admission error.
2. Parse the northbound request to IR with the north codec; refuse `previous_response_id` until NC-3 lets the
   component do it (§4.4).
3. Pass the model's declared `supported_parameters` unchanged (boundary §7.4).
4. `build-http-request`; a capability error is a 400 with zero upstream calls.
5. `ProviderConfig::authorize` (kernel:provider.rs:361), descriptor auth admission (boundary §4.2), the
   `request_facts` seal check — including the empty-cap case — and operator extras outside what the descriptor
   carries.
6. Reserve, write the dispatch marker, send; apply `stream_framing` (`bytes`: feed unchanged).
7. Take evidence only from the component's output; apply the generic checks of §7.4; settle or park.
8. Deliver: render with the north codec, or, where the family declares `north_identical` for the inbound protocol,
   forward upstream bytes under §8.2's rules.

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
| `request.refused-claude-carrier`, `request.refused-unmappable-part`, `request.refused-unknown-tool-choice` | Capability errors |
| `request.codex-stream` | No cap, no `temperature`, default `instructions`, `stream: true`, account header from the attribute, Codex URL |
| `request.codex-without-account`, `request.codex-non-stream-refused` | Header omitted; capability error |
| `response.usage` ★, `response.cached-usage` ★, `response.reasoning-usage` | §7.1 mapping, every bucket |
| `response.missing-usage` ★, `response.total-mismatch`, `response.subset-violation`, `response.nonzero-tool-usage` | Protocol errors |
| `response.tool-call`, `response.reasoning-summary`, `response.refusal` | §5 mapping |
| `response.incomplete-max-output`, `response.failed-status`, `response.unmapped-output-item` | `Length` with usage; two protocol errors |
| `stream.usage-terminal` ★, `stream.no-usage` ★ | Terminal with usage; `response.completed` without usage is an error |
| `stream.text`, `stream.tool-call`, `stream.tool-call-prebuffered-arguments`, `stream.reasoning-summary` | §6.1 |
| `stream.incomplete`, `stream.failed`, `stream.error-event` | §6.3, §6.4 |
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
- `RequestFactsHonoured` with `output_cap: []`: the check asserts that the IR's cap appears **nowhere** in the body.

### 12.3 Beyond gate ②

- **Credential fixtures** (`credential.*`, boundary §3.6): `codex-refresh` (rendered request), `codex-rotation`,
  `codex-jwt-exp-clock`, `codex-on-status`, `codex-account-claim` (claim exported; fallback to the stored field),
  `codex-use-stored`.
- **Documentation-derived judge** (boundary §6.2 item 5): rows in the `usage_ir_contract` style for this wire — the
  prompt total is `input_tokens` with the cached part inside it; reasoning inside output; total = input + output —
  with expectations taken from OpenAI's published usage semantics and numbers chosen so a wrong field cannot land on
  them (crates/south-component-conformance/tests/usage_ir_contract_v1.rs:1-30).
- **Round-trip judge** with the north codec (§9).
- **Sandbox parity**: the wasm build equals the native reference on the frozen pack, as for the other packages.
- **Fuzz**: the stream parser is an untrusted parser and gets a fuzz target.

### 12.4 Host obligations (gate ③)

| Mechanism | Suite | Status |
|---|---|---|
| Credential recipe execution | `south.credential-recipe.v1` (boundary §3.6) | Required before `openai-codex` |
| Descriptor auth admission | Covered by the boundary's gate ② check plus T21 `rogue-arm` | Required |
| `request_facts` seal, including the empty cap | T21 `rogue-cap`, plus one case for a family declaring no cap | Required |
| North-identical delivery | New: `south.north-identical-delivery.v1` — a fake upstream stream; asserts bytes forwarded unchanged, the terminal frame withheld until settlement, an `Error` frame forwarded without success, a component failure forwards nothing, the non-streaming body forwarded only after a successful parse | Required only if §8.2 is accepted |

Per boundary R4, each is marked `verified` under `host_capabilities` only once both hosts pass it.

## 13. Migration and dual runs

### 13.1 Steps

| Step | Side | Content | Acceptance |
|---|---|---|---|
| R0 | Host | Rulings on R-Q1 to R-Q5; fix or accept the suspected defects of §13.5 in the native leg first (P21 §9 practice) | — |
| R1 | South | Package with `openai-responses`: reference implementation, fixtures, judges, wasm build. Needs boundary B1 (usage strictness) and B2 (`request_facts`, descriptor auth admission) | Suite green; listed in the release index |
| R2 | Host | Route provider rows to the family; dual run; remove `supports_responses` / `upstream_requires_responses` branching for covered rows | §13.2 |
| R3 | South + host | `openai-codex`: needs boundary B4 (recipes) and the host's recipe executor | Credential fixtures and gate ③ suite green; §13.2 on Codex rows |
| R4 | Host | Retire the code of §13.4 | J1 count falls; removing the package leaves the host compiling, testing and starting (J3) |

### 13.2 Dual-run reconciliation

The same request goes through the native leg and the component leg, once under each billing form
(`GATEWAY_TEST_BILLING_FORM=balance|quota`), over a corpus restricted to requests the IR can represent, written in the
canonical shape of §4 (item-list `input`, explicit `store: false`, client function tools only). Compared:

1. **Upstream request**: method, URL, non-auth headers, body by JSON equality. For Codex rows also the account header.
   For Chat-surface "Responses only" rows the native body is the host converter's output, compared for non-streaming
   requests (the native leg never streams upstream there).
2. **Northbound response**: under north-identical delivery, byte equality on the Responses surface; otherwise, and on
   the Chat surface, equality of text, tool calls (id, name, argument bytes), finish reason and `usage`.
3. **Funds**: reservation, settled amount, the thinking marker, `tokens_estimated = 0`, `quantity_estimated = 0`.
4. **Failure fixtures**: terminal without usage; total mismatch; duplicate terminal; frame after terminal; unknown
   event; unmapped output item; in-band `error` and `response.failed`; 401; 429; 5xx; truncated stream. Each must end
   in the same funds state on both legs, except where §13.3 says otherwise.
5. **Token refresh** (Codex): the rendered refresh request equals the host's; rotation writes back; two concurrent
   requests refresh once.

### 13.3 Intentional differences (on record, not reconciliation failures)

- **Truncated generations settle** (§6.3): native parks `incomplete`; the component reports `Length` with usage.
  Pending R-Q2.
- **Non-zero `image_gen` tool usage is refused** (§7.3): native admits and prices it. Pending R-Q3.
- **`store`**: always `false`; native forwards a client's `store: true`, including to Codex, because the normalizer
  only inserts the key when absent (leaf:translate_responses.rs:27). `previous_response_id` and the fields of §4.6 are
  not forwarded.
- **`input` form**: always an item list; native forwards a string input to non-Codex upstreams.
- **Chat surface, "Responses only" models**: the component leg streams upstream when the client streams, carries
  `response_format`, `parallel_tool_calls` and reasoning effort, and keeps system messages whose content is a parts
  array. The host's converter forwards none of these (leaf:translate_responses.rs:294-298, 480-484, 550-556) and
  always calls non-streaming.
- **Messages surface**: models on this family become reachable; today they are not (§1).
- **Non-streaming Codex requests**: refused at build time instead of being sent un-normalized.
- **Codex refresh timing and 5xx classification** (§10.2).
- **Zero-token responses**: refused on the IR path (§7.2).
- **Encrypted reasoning replay**: not available until NC-1 (§4.5); under re-rendering, not delivered either (§8.3).

### 13.4 Host code that retires

Line counts measured at `4d5bb4e5`; paths are relative to the token-station-server root.

| File | Lines | Retires when |
|---|---|---|
| `crates/gateway-provider-protocol/src/translate_responses.rs` | 719 (whole file). Production callers exist for three functions only: `normalize_codex_responses_body` :10-35, `openai_chat_to_responses` :283-559, `codex_responses_to_openai_chat` :615-719; the other nine are called from tests only | R2 (Chat surface) and R3 (Codex) |
| `crates/gateway-provider-protocol/src/translate_responses_sse.rs` | 250 (whole file; called from tests only) | Any time |
| `crates/gateway-provider-protocol/tests/golden/translate_responses.rs`, `…/translate_responses_sse.rs`; `gateway/src/modules/inference/engine/translate/tests/responses.rs` | 858, 387, 81 | With the files above |
| `crates/gateway-provider-protocol/src/usage_evidence.rs`, Responses ranges :176-202, 543-619, 621-955, 1496-1513, 1531-1539, 1671-1675, 1959-2271 | 784 of 2,523 | R4, after every upstream of §3.3 is covered |
| `gateway/src/modules/inference/engine/text_admission/sender/responses.rs` | Native seal arm :412-455 (44), Codex normalizer :660-674 (15), native frame loop :1646-1698 (53), Codex error-frame helpers :2026-2130 (105) — of 2,423. The frame loop survives in generic form if §8.2 is accepted | R4 |
| `gateway/src/modules/inference/engine/text_admission/chat.rs` :501-510 and the `ResponsesBurst` contract (`sender.rs` :3726-3741, 3779-3800) | about 50 | R2 |
| `gateway/src/modules/inference/engine/token_refresh.rs` :54-103 (JWT helpers), :114-250 (`CodexMint` and its entry point) | 187 of 2,313 | R3, with the recipe executor (boundary §3.5) |
| `gateway/src/modules/inference/engine/upstream.rs` :260-271, 535-539; `text_admission/sender.rs` :814-830 | 34 | R3 |
| `gateway/src/modules/inference/engine/token_counter/media.rs` :214-278 (Codex image-tool usage and its rate card) | 65 | R4, subject to R-Q3 |
| `gateway/src/modules/inference/engine/request_extras.rs` :424-434 and the `chatgpt-account-id` entry at :37 | 12 | R3 |
| The flags `supports_responses` and `upstream_requires_responses`: config, catalog columns, admin form, and the branch sites listed in §1 | not counted | R4; columns need forward migrations |

### 13.5 Suspected host defects found while reading (code reading, not run)

Evidence is the cited code only; no impact is claimed.

1. `response.incomplete` and a non-`completed` status are rejected before the converter's `incomplete → length` branch
   can run (leaf:usage_evidence.rs:547-549, 2035-2041 against leaf:translate_responses.rs:674-677), so a generation
   stopped at the output cap appears to end in `delivery_unknown` on every native Responses path.
2. A system or developer message whose `content` is a parts array contributes nothing to `instructions` on the Chat
   surface: the converter reads it with `as_str` only (leaf:translate_responses.rs:294-298).
3. `response_format`, `reasoning_effort`, `stop` and `parallel_tool_calls` are not carried by the Chat → Responses
   converter (:480-484, 550-556).
4. A client's `store: true` survives the Codex normalizer (:27), although its documentation says it sets `store:
   false`.
5. Non-streaming Codex requests skip the normalizer (server:…/text_admission/sender/responses.rs:438-440, 664-674) and
   are sent with `max_output_tokens` and without `stream: true`, which the normalizer's own comments say the backend
   rejects.
6. Two host comments disagree on whether Codex accepts `max_output_tokens` (leaf:translate_responses.rs:8, 25 and the
   test at server:…/text_admission/sender/responses.rs:2197-2213, against server:…/text_admission.rs:1201-1203).
7. The separately priced `image_gen` branch (leaf:usage_evidence.rs:664-672) sits behind an output-item check that
   refuses every item type except message, function call and reasoning (:868-892). Whether an upstream ever reports
   positive image-tool usage without such an item was not verified.
8. A model flagged `upstream_requires_responses` cannot be served on `/v1/messages`
   (server:…/text_admission.rs:1157-1176; inferred).

## 14. Versioning

- New package `provider-openai-responses` 1.0.0 in the existing world: no WIT change, no new world, no contract
  number. South minor. The existing thirteen packages are untouched.
- It depends on manifest fields from the boundary record (`usage_evidence`, `request_facts`, `credentials`) and adds
  two proposals (`output_cap: []`, `north_identical`). All are manifest schema changes, hence south minor each (R5);
  absent `north_identical` means "always render", which is what every existing package gets today.
- Phasing: R1 after boundary B1 and B2; R3 after B4. Both are inside the boundary's B6.
- Host link layer: the recipe executor and north-identical delivery are one-time generic mechanisms (P21 §1.4). After
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
- **The `oauth` auth arm for Codex**: boundary §3.7 deprecates it; it leaves every provider fact of the exchange in
  the host.
- **Supporting `store: true` and `previous_response_id` in v1** (§4.4): the prompt would no longer be bounded by the
  outbound body, so the host's input bound (§7.4) would fire on honest responses; and the IR has no field for either.
- **Deriving this mapping by inverting the north codec** (§9): the two directions have different loss rules and
  different state.
- **Aggregating a Codex stream in the host for non-streaming callers**: possible as a generic host feature, but it is
  host product behavior, not something this package should assume (R-Q5).

## 16. Open questions

Tags: S = south maintainers, L = lv, K = kernel. Boundary Q13 (reasoning-token convention) remains open and is not
repeated; this package's mapping does not depend on it (§7.1).

On 2026-09-30 lv ruled on the L-tagged questions: as recommended, on condition that each recommendation fits the
final goal DP0 (no provider-specific logic in the host). One recommendation was adjusted to meet that condition
(R-Q5); the rulings are recorded under each question.

- **R-Q1 (S, L)** Delivery on the Responses surface: north-identical bytes by declaration (recommended, §8.2), or
  always re-render and accept §8.3's losses.
  **Ruled for the host side (lv, 2026-09-30): north-identical delivery, by declaration.** The south maintainers'
  half remains open.
- **R-Q2 (L)** Does a generation truncated at the cap (`response.incomplete` with usage) settle as a success with
  finish reason `length` (recommended: yes, as on every other wire), or stay in `delivery_unknown` as the native leg
  does today?
  **Ruled (lv, 2026-09-30): it settles as a success with finish reason `length`.**
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
- **R-Q6 (S)** A package-declared `user-agent` instance (§10.4): add it to boundary §10 now, since DP7 providers other
  than Codex already need one, or leave it until a package needs it.
- **R-Q7 (S)** Recipe details (§10.2): persistence and fallback of exported attributes; `jwt_exp` on a non-JWT token;
  whether a fixed-window clock form is wanted.
- **R-Q8 (S, K)** OpenAI opaque reasoning through the IR: a family-tagged opaque reasoning event in the stream
  contract (kernel chain), the `reasoning_replay.openai.v1` word, and north codec follow-ups NC-1 and NC-2 — or rely
  on north-identical delivery for the response half and do only NC-1.
- **R-Q9 (S)** Unknown `response.*` event types: error (this record, matching the host) or ignore (what the Converse
  reference does with unknown events, reference_bedrock_converse.rs:736-739). Strict means a new inert upstream event
  stops streams until a package release; lenient means content or cost could pass unseen.
- **R-Q10 (S)** Refusal text: ordinary text (this record), an error, or a finish reason.
- **R-Q11 (S, K)** `block_index` for a wire with two-level indices: first-seen counter (this record) or another rule.
- **R-Q12 (L)** How a "Responses only" model on a mixed provider is declared in the host: a second provider row (no
  new mechanism) or a generic model-row family override (recommended if such providers are common; host work only).
  **Ruled (lv, 2026-09-30): a second provider row.** It is operator data and needs no new host mechanism; a generic
  model-row family override is reconsidered only if such providers turn out to be common.
- **R-Q13 (S)** Where shared Responses vocabulary constants live (§9), given that guests depend on the conformance
  crate and the north codec may depend only on the kernel IR (ARCHITECTURE.md:70-80).
- **R-Q14 (L)** Cutover order: may the Responses surface be cut over before NC-1, losing encrypted reasoning replay
  for clients that use it (recommended: no for Codex rows; yes for rows with no such clients), and which upstreams of
  §3.3 (Azure, Bedrock with SigV4) are in production use and must be covered before R4.
  **Ruled (lv, 2026-09-30): as recommended.** Which upstreams are in production use is answered by a read-only query
  before R4.
