# The Kiro provider component (`provider-kiro`)

Status: proposed — drafted for review by the host team (token-station-server P21), not accepted

Date: 2026-09-30

Predecessors: `2026-09-30-host-zero-vendor-boundary.md` ("the boundary record"; this record expands the Kiro row of
its §11 and depends on its §3 credential recipe v1, §4 descriptor auth admission, §5 `stream_framing`, §6 usage and
`usage_evidence`, §7 `request_facts`, §8 compatibility range and §10 instance declaration),
`2026-08-21-canonical-ir-inventory.md` (S0: D1 usage belongs to the component, D2 chunks are bytes),
`2026-08-20-controlled-user-agent.md` (the `user-agent` value is a host compile-time literal),
`2026-09-30-embeddings-contract.md` (the sibling record whose structure this one follows).

Origin: token-station-server `docs/product-review-v2/plans/2026-09-29-P21-供应商接入只动South.md` — §2.5 (the
three-hop translation), §5 S3–S5, §8.4 DP6 ("migrate Kiro into south", approved 2026-09-30) and DP7, §8.5 (the
owner's rulings on the boundary record's Q1, Q10 and Q12), Appendix B.2 / B.4; and
`2026-09-24-P9-南向翻译扩面.md` T2 ("Kiro stays out of south", ruled 2026-09-24, reopened by DP6).

Baseline: south `origin/main` = v0.42.0 (`3135e36`); kernel `f585bc83` (protocol 0.4.0); host `4d5bb4e5` for every
code citation. The owner rulings of 2026-09-30 cited below are recorded in host `c4bd45e5`, a docs-only commit on
top of that. Host citations carry a `server:` prefix; `…/engine/` abbreviates
`gateway/src/modules/inference/engine/` and `leaf/` abbreviates `crates/gateway-provider-protocol/src/`; once a
host file has been cited with its path, later citations give `server:` and the file name only. Kernel
citations carry `kernel:` and refer to `crates/protocol/src/` at the baseline revision.

Three rulings this record is written on (lv, 2026-09-30; server P21 §8.4 DP7 and §8.5):

- **DP7 / boundary Q10**: south takes in client-identification headers; the host keeps no special case. Headers on
  the inference request are written by the component, headers on the token-exchange request by the credential
  recipe, and names are declared by the package.
- **Boundary Q1**: bumping the south / kernel pin counts as modifying the host. Any instance Kiro needs from a
  closed vocabulary is therefore declared by the package (boundary §10), never added to a compiled-in enum.
- **Boundary Q12 / embeddings E-Q3**: the two estimate conventions coexist. For a chat-family provider with
  `usage_evidence: absent`, the host's generic estimator makes the estimate.

## 0. Summary

Kiro is served today by a dedicated host arm: a three-hop translation (IR → Anthropic Messages → the upstream's
`conversationState` body; upstream events → Anthropic SSE → IR → the northbound surface), its own send path, its
own token refresh and its own health probe. This record moves all provider knowledge into one package.

| # | Topic | Decision | Section |
|---|---|---|---|
| 1 | Package | `provider-kiro`, family `kiro`, world `provider-adapter-v2`; **no WIT change** | §3 |
| 2 | Request | One hop, IR → `conversationState`; lossy and refused cases enumerated; the model id is passed verbatim | §4 |
| 3 | Client identification | Three ordinary headers and two body fields written by the component; the `user-agent` value declared by the package instead of compiled into the host | §4.6 |
| 4 | Framing | `stream_framing: aws-eventstream`; the component parses the canonical re-encoding, on both the streaming and the non-streaming path | §5 |
| 5 | Usage | `usage_evidence: absent`; the component never emits `Usage`; the host's generic estimator counts, and the ledger marks the row estimated | §6 |
| 6 | Auth | Bearer arm; the slot is `minted` by one of two recipes (`social`, `idc`) chosen by an ordered rule list; rotation declared; `profile_arn` exported | §7 |
| 7 | Request facts | Model in the body; **no stream switch and no cap location** — two small additions to boundary §7.2 | §8 |
| 8 | Errors | Status-based mapping plus the AWS exception-name arm; exception frames become `StreamEvent::Error` | §9 |
| 9 | Migration | Dual run against the native arm on Chat, both billing forms; about 2,600 host lines retire | §12 |

What this record needs beyond the four existing records is collected in §3.3 (eight proposals, each minimal).
§15 lists observations on the native arm made while reading it; §16 lists the open questions.

## 1. Problem: what the host does today

### 1.1 Request path: three hops

1. The northbound Chat body becomes IR (`canonical_to_ir`, server:…/engine/text_admission/chat.rs:383-391).
2. The **Anthropic** component turns IR into an Anthropic Messages body — the host maps the Kiro provider type to
   the Anthropic dialect (server:…/engine/south_component.rs:267-276) and calls it
   (server:…/engine/text_admission/chat.rs:581-583), then removes `stream` (:584-586).
3. That intermediate body is sealed as `TextOperation::Kiro`, with the output cap checked at top-level
   `max_tokens` (server:…/engine/text_admission.rs:890-892, 1114-1115) and "no `stream` field" required
   (:933-936).
4. At prepare time the host parses the sealed body back, refreshes the token, and translates Anthropic Messages
   into the upstream body (server:…/engine/text_admission/sender.rs:2614-2656; server:…/engine/kiro.rs:42-53;
   `anthropic_messages_to_kiro`, server:leaf/translate_kiro.rs:55-157).
5. It posts to `{base_url}/generateAssistantResponse` (server:…/engine/upstream.rs:547-552) with
   `Authorization: Bearer`, `x-amz-target`, `amz-sdk-request`, `x-amzn-kiro-agent-mode`, `user-agent`
   (server:…/engine/upstream.rs:329-352) and `accept: application/vnd.amazon.eventstream`
   (server:…/engine/kiro.rs:66-72).

### 1.2 Response path: three hops back

- **Streaming**: the host deframes AWS eventstream (server:…/engine/text_admission/sender.rs:4827-4887), feeds each
  event to `KiroSseState::on_event`, which emits Anthropic SSE frames as JSON (server:leaf/translate_kiro.rs:385-566),
  re-encodes each frame as SSE text and feeds it to the **Anthropic** component's stream parser
  (server:sender.rs:4735-4747; server:…/engine/south_component.rs:849-856), and renders the resulting IR events as
  Chat chunks.
- **Non-streaming**: the upstream always answers with an event stream. The host reads the whole body, decodes it
  (`decode_kiro_event_stream`, server:…/engine/kiro.rs:136-171), folds it into one Anthropic message
  (`kiro_accumulate_to_anthropic_response`, server:leaf/translate_kiro.rs:573-646) and renders that through the
  Anthropic dialect (server:sender.rs:3977-4023).

### 1.3 Usage

The upstream reports no token counts — only a `meteringEvent` carrying credits
(server:leaf/translate_kiro.rs:302-321). The durable path bills a host-side estimate at the model's ordinary token
rate card and marks the frozen payload `estimated: true` (server:sender.rs:1905-1932), which lands as
`usage_records.tokens_estimated = 1` (server:leaf/usage_types.rs:131-147). The estimator is `ceil(chars / 4)`
(server:…/engine/token_counter/estimate.rs:19-35), applied to three different materials:

| Quantity | Material counted today |
|---|---|
| Input | The sealed **Anthropic intermediate** body, serialized (server:sender.rs:2657, 2690-2692) |
| Output, streaming | Text deltas, plus the tool name and input fragment of every `toolUseEvent` (server:sender.rs:4749-4766, 4983-4989) |
| Output, non-streaming | The serialized `content` array of the folded Anthropic message (server:sender.rs:2694-2701, 3994-3998) |

The credits are summed and then discarded (`_upstream_credits`, server:sender.rs:3989; `KiroSseState::credits()` has no
caller outside the leaf crate's tests).

### 1.4 Credentials

One `MintStrategy` of five (server:…/engine/token_refresh.rs:1277-1372) on the shared skeleton (:480-613):

- Two forms, chosen from the credential's `extras` (:1189-1214): an explicit `authMethod` of `idc` / `enterprise` /
  `iam_identity_center` (case-insensitive) selects IdC; any other explicit value selects social; with no
  `authMethod`, IdC is selected exactly when both `clientId` and `clientSecret` are present.
- Endpoints built from a region template (:1135-1137, 1254-1271): social
  `https://prod.{region}.auth.desktop.kiro.dev/refreshToken` with body `{"refreshToken"}`; IdC
  `https://oidc.{region}.amazonaws.com/token` with body `{"refreshToken","clientId","clientSecret","grantType":
  "refresh_token"}`. Both are **JSON with camelCase keys**; `idcRegion` wins over `region`, default `us-east-1`
  (:1215-1220). An `extras.refreshUrl` override replaces the endpoint (:1167-1171, 1270).
- Response `{accessToken, refreshToken, expiresIn}` (snake_case aliases accepted, :1242-1250); `expiresIn` is
  relative seconds, defaulted to 3600 and clamped to 60 s – 24 h (:1349-1357).
- The refresh token is single-use and rotating (:1362-1365); the refresh margin is 300 s (:52, 1296-1299); a
  terminal rejection is HTTP 400 / 401 (:252-270).
- `profileArn` is a static field of the credential and goes into the request body when present (:1151-1166;
  server:leaf/translate_kiro.rs:153-155).
- The sign-in file is recognized by shape and its six static keys copied into `extras`
  (server:gateway/src/modules/admin_ui/handler/credentials/mod.rs:215-267).
- The health probe special-cases this provider so that a probe never burns a rotating refresh token
  (server:…/engine/health_probe.rs:323-335, 353-421).

### 1.5 Northbound surfaces

Only Chat. The Messages plan refuses the provider ("has no bounded Messages sender contract",
server:…/engine/text_admission/messages.rs:357-369; pinned by the test at server:text_admission.rs:1480-1505), and the
target builder refuses every operation other than the dedicated one (server:text_admission.rs:637-640, 801-813; test at
server:sender.rs:7618-7666). Responses has no arm for it (inferred: `sender/responses.rs` contains no reference to the
provider, so a Responses target falls into that same refusal).

### 1.6 Why the provider world cannot simply absorb it today

The WIT is sufficient (§3.2). Five things outside the WIT are not:

| Gap | Where |
|---|---|
| The `user-agent` value must be a literal in the host's program text | `ControlledUserAgentV1` takes `&'static str` (crates/south-contracts/src/lib.rs:1244-1276); the ordinary header channel refuses the name (:232-256) |
| The upstream has no stream switch, and the body has no top-level `stream` | boundary §7.2 offers only `{"body": …}` or `"url"` |
| The upstream body has no output-cap field | boundary §7.2 requires exactly one declared cap location to carry the cap |
| A 2xx body is binary even for a non-streaming request | `HttpResponseParts.body` is text (kernel:http.rs:379-391) |
| The request carries a per-request random id | gate ②'s `Determinism` check forbids randomness (crates/south-component-conformance/src/report.rs:27-32) |

## 2. Decisions

- **D1 One package, one family, the existing world.** `provider-kiro`, family `kiro`, `provider-adapter-v2`. No
  new world and no WIT change.
- **D2 One hop each way.** The component translates IR directly to and from the upstream wire. The Anthropic
  intermediate disappears, and with it the dependency of this provider's behavior on another package's version.
- **D3 Faithful or refused, never silently different** — for anything that changes what the model is asked to do.
  Fields the wire has no slot for and that only tune sampling are dropped and documented (§4.3).
- **D4 The model id is operator data.** The component writes `ChatRequest.model` verbatim; the host's id-rewriting
  heuristic is not carried over (§4.4).
- **D5 `usage_evidence: absent`.** The component never emits `Usage`, and its non-streaming response carries an
  all-zero `usage` the host must not read. The credit meter is used only as terminal evidence (§5.3, §6).
- **D6 Bearer arm, minted slot.** The descriptor uses `Auth::Bearer`; the slot is minted by one of two recipes;
  `profile_arn` reaches the component as an exported attribute (§7).
- **D7 Client identification lives in the package** (DP7): headers and body markers in the component, the
  `user-agent` value in the manifest (§4.6).
- **D8 The non-streaming path stays buffered.** The host deframes the complete body and hands `parse-response` the
  same canonical re-encoding the stream parser sees (§5.4).
- **D9 The upstream has no cap and no stream switch, and the manifest says so** (§8). What the host does about an
  upstream it cannot cap is host policy, surfaced as K-Q1.
- **D10 Every northbound surface the host's generic component path serves becomes available**; nothing in the
  package is surface-specific. Whether to switch Messages and Responses on for this provider is K-Q3.

## 3. Package identity, world and manifest

### 3.1 Identity

| Property | Value |
|---|---|
| Package | `provider-kiro`, version `1.0.0` |
| Family (`providers`) | `kiro` |
| World / `api_version` | `provider-adapter-v2` (`token-station:adapter@2.0.0`) |
| Behavior suite | `south.provider-component.v1` |
| Capabilities | `chat`, `stream`, `tool_call` — not `json_schema` (the wire has no slot, §4.3) |
| Auth arms | `bearer` |
| Secrets | `provider_api_key` (the slot name `provider-anthropic` uses, components/provider-anthropic/manifest.json) |
| Reference implementation | `crates/south-component-conformance/src/reference_kiro.rs`, compiled to wasm by a wit-bindgen shell as the other four are (components/provider-bedrock-converse/src/lib.rs:1-13) |
| Fixtures | `crates/south-component-conformance/fixtures-kiro/` |

The family is named after the product, not after the upstream service whose wire it speaks: the package carries
the product's sign-in endpoint, its client identification and its sign-in file format as well as the wire (K-Q9).

### 3.2 The world is sufficient

`provider-adapter-v2` exports `build-http-request`, `parse-response`, `parse-stream-chunk`, `map-provider-error`
and `model-capabilities` (crates/south-provider-api/wit/provider-adapter.wit:98-132). Each is used as is:

- `build-http-request` returns a JSON POST descriptor naming one bearer slot.
- `parse-stream-chunk` takes bytes (provider-adapter.wit:20-25, 118-125); with `stream_framing: aws-eventstream`
  the bytes are the canonical re-encoding of boundary §5.2.
- `parse-response` takes `HttpResponseParts`, whose body is text; §5.4 gives it text.
- The component does not call `host.sign`. The world imports `host` (provider-adapter.wit:138-141), and nothing
  obliges a component to call what its world imports.

Everything this record needs is at the manifest and contract level.

### 3.3 What this record needs beyond the four existing records

Each item is a **proposal**, kept to the smallest change that makes the package expressible. None contradicts the
boundary record; each refines a field that record proposes or a contract type that exists today.

| # | Proposal | Refines | Section |
|---|---|---|---|
| P-1 | `request_facts.stream: "none"` — the dialect has no stream switch and a 2xx is always a stream | boundary §7.2 | §8.2 |
| P-2 | `request_facts.output_cap: []` — the dialect has no cap location | boundary §7.2 | §8.3 |
| P-3 | A per-family `user_agent` declaration; `ControlledUserAgentV1` gains a constructor from a manifest-admitted value | boundary §10 (which does not list this closed instance set) | §4.6 |
| P-4 | Reserved key `south_attempt_id` in `ChatRequest.extensions`: a host-minted UUID, fresh per upstream attempt | the precedent of `south_credential_attributes` (boundary §3.3) and of `HostMintedValuesV1` in the task world | §4.5 |
| P-5 | For an `aws-eventstream` family, a 2xx body whose content type is `application/vnd.amazon.eventstream` reaches `parse-response` as the canonical re-encoding | boundary §5.2 (which defines the feed for `parse-stream-chunk` only) | §5.4 |
| P-6 | Recipe details: `select` as an ordered rule list; declared parameter names on `oauth2_token`; `default_seconds`; an explicit rotation target; import with ordered pointers and a seeded token; one more value syntax | boundary §3.3 | §7 |
| P-7 | Gate ② check `AbsentFamilyEmitsNoUsage`; the all-zero `usage` rule for `parse-response` of an `absent` family | boundary §6.2 items 2 and 4 | §6, §11 |
| P-8 | Host rule: a probe never forces a refresh on a recipe that declares `rotates_refresh_material: true` | boundary §3.5 | §7.6 |

### 3.4 Manifest sketch

Field names follow the boundary record's drafts. `credentials` is given in §7.2.

```json
{
  "name": "provider-kiro",
  "version": "1.0.0",
  "api_version": "provider-adapter-v2",
  "providers": ["kiro"],
  "capabilities": ["chat", "stream", "tool_call"],
  "auth_arms": ["bearer"],
  "permissions": { "network": false, "filesystem": false, "secrets": ["provider_api_key"] },
  "conformance": { "required_suite": "south.provider-component.v1", "fixtures": "fixtures-kiro/" },
  "compatibility": {
    "ir_schema_id": "token-station-protocol@0.4.0/v0.3.0",
    "kernel_version": "0.3.0",
    "kernel_revision": "6822aab1dea54ef646cb2206595cd4955ff9764a",
    "wit_package": "token-station:adapter@2.0.0",
    "south_runtime": "<the release that ships it>",
    "runtime_abi": 1,
    "contracts": {}
  },
  "stream_framing": { "kiro": "aws-eventstream" },
  "usage_evidence": { "kiro": "absent" },
  "request_facts": {
    "kiro": {
      "output_cap": [],
      "model": { "body": "/conversationState/currentMessage/userInputMessage/modelId" },
      "stream": "none"
    }
  },
  "user_agent": { "kiro": "aws-sdk-js/1.0.0 KiroIDE" },
  "credentials": { "schema": "south.credential-recipe.v1", "…": "see §7.2" }
}
```

`config_schema` (boundary §7.3) is empty: the component reads no operator configuration key. The inference
endpoint is the provider row's `base_url`; the host's default for it is `https://q.us-east-1.amazonaws.com`
(server:gateway/src/infra/config/providers.rs:721), which becomes catalog data (boundary §7.5).

## 4. Request mapping: IR → wire

### 4.1 The wire shape

As the host builds it today (server:leaf/translate_kiro.rs:121-156, 160-190, 225-264, 287-300):

```json
{
  "conversationState": {
    "chatTriggerType": "MANUAL",
    "conversationId": "<south_attempt_id>",
    "currentMessage": {
      "userInputMessage": {
        "content": "<system text>\n\n<text of the final user turn>",
        "modelId": "<ChatRequest.model>",
        "origin": "AI_EDITOR",
        "userInputMessageContext": {
          "tools": [ { "toolSpecification": { "name": "…", "description": "…", "inputSchema": { "json": {} } } } ],
          "toolResults": [ { "toolUseId": "…", "content": [ { "text": "…" } ], "status": "success" } ]
        }
      }
    },
    "history": [
      { "userInputMessage": { "content": "…", "origin": "AI_EDITOR",
                              "userInputMessageContext": { "toolResults": [] } } },
      { "assistantResponseMessage": { "content": "…",
                                      "toolUses": [ { "toolUseId": "…", "name": "…", "input": {} } ] } }
    ]
  },
  "profileArn": "<exported attribute, when the credential has one>"
}
```

`userInputMessageContext`, `history` and `profileArn` are omitted when empty, as today
(server:translate_kiro.rs:126-135, 147-155). The descriptor is `POST {base_url}/generateAssistantResponse` with
`Auth::Bearer { provider_api_key }`.

### 4.2 Mapping

| IR (`ChatRequest`, kernel:chat.rs:197-217) | Wire |
|---|---|
| `model` | `currentMessage.userInputMessage.modelId`, verbatim (§4.4) |
| `System` messages | Their text, joined by `\n`, prepended with `\n\n` to the content of the first user turn — the first user entry of `history`, or the current message when there is none (today: server:translate_kiro.rs:88-113, 193-206) |
| All turns but the last | `history`, in order: a user turn → `userInputMessage { content, origin }`, an assistant turn → `assistantResponseMessage { content }` |
| The last turn | `currentMessage.userInputMessage`; it must be a user turn (§4.3) |
| `Content::Text` / `ContentPart::Text` | Concatenated without a separator into `content` (today: server:translate_kiro.rs:210-221) |
| `Message.tool_calls` on an assistant turn | `toolUses[] { toolUseId ← id, name, input ← arguments parsed as a JSON object }` |
| `Tool` messages | `toolResults[] { toolUseId ← tool_call_id, content: [{ text }], status: "success" }` inside the `userInputMessageContext` of the user turn they belong to. A run of consecutive `Tool` messages is one user turn; a `User` message immediately after the run joins it and supplies `content`, otherwise `content` is `""` |
| `tools` | `currentMessage.userInputMessage.userInputMessageContext.tools[] { toolSpecification { name, description?, inputSchema: { json ← parameters } } }` — only the current message carries tools |
| `tool_choice: auto` or absent | Nothing to write |
| `tool_choice: none` | `tools` withheld from the body — the same answer the Converse package gives (fixture `provider.request.tool-choice-none-withholds-the-whole-config`) |
| `extensions.south_attempt_id` | `conversationId` (§4.5) |
| `ProviderConfig.extensions.south_credential_attributes.profile_arn` | Top-level `profileArn`, when present |
| `stream` | Ignored: the wire has no switch (§8.2) |

The exact turn-merging rule — how consecutive same-side messages are grouped — is pinned by the dual-run
fixtures of §12: for every IR input that contains no refused construct, the component must produce what hops 2–4
of §1.1 produce today. The rule above is what the second hop does with the intermediate body; what the first hop
(the Anthropic package) does when merging turns was not re-read for this record.

### 4.3 Lossy and refused cases

"Native" is what the host arm does today for the same input.

| Input | Component | Native today |
|---|---|---|
| Image part (`ContentPart::ImageUrl`) | **Refused**: capability error, 400 before admission | Refused, but late: inside prepare, after the token refresh (server:translate_kiro.rs:80-84, 163-167; §15 N-3) |
| `ContentPart::Unknown` | **Refused**: capability error | Dropped (only `text` blocks are read, server:translate_kiro.rs:210-221) |
| `Thinking` / `RedactedThinking` parts in earlier turns | **Dropped** — the wire has no block for them; the same rule as the Converse reference for parts a dialect cannot carry out (reference_bedrock_converse.rs:109-114) | Dropped |
| `response_format` other than text | **Refused**: capability error; the package does not declare `json_schema` | Never reaches the upstream (inferred: the second hop reads only `messages`, `system`, `tools`) |
| `tool_choice: required` or a named tool (`ToolChoice::Other`) | **Refused**: capability error — the wire cannot force a call | Silently dropped |
| Final turn is an assistant turn (prefill) | **Refused**: capability error | Sent as the current **user** message (server:translate_kiro.rs:77-86 reads the last message without checking its role) |
| `ToolCall.arguments` that is not a JSON object | **Refused**: invalid request | Decided by the first hop (not re-read) |
| Empty `messages` | **Refused**: invalid request | Refused (server:translate_kiro.rs:62-68) |
| `sampling.temperature`, `top_p`, `stop` | **Dropped** — no slot on the wire; documented, and pinned by a fixture | Dropped |
| `sampling.max_output_tokens` | **Not written** — no slot on the wire (§8.3) | Validated on the intermediate body, then not sent (§15 N-1) |
| A tool result that reports an error | **Lossy**: `status` is always `"success"`. The IR has no field for a tool-result error (K-Q14) | `"error"` only if the intermediate body carries `is_error` (server:translate_kiro.rs:252-256); no south reference implementation emits that key (inferred from a repository search) |
| Operator request extras (body) | Applied by the host to the component's body, i.e. to the real upstream shape | Applied to the intermediate body; keys other than `messages` / `system` / `tools` never reach the upstream (§15 N-10) |

Refusals that differ from native behavior are intentional differences for the dual run (§12.2). Following the
practice of the embeddings record and of server P21 §9, the native arm is corrected first where the difference
is a defect of the native arm.

### 4.4 Model id

The host rewrites the configured upstream id before sending: a trailing `-<digits>-<digits>` becomes
`-<digits>.<digits>` (`kiro_model_id`, server:leaf/translate_kiro.rs:34-47). The component does not: it writes
`ChatRequest.model` verbatim, and the operator's model row (or the family's catalog, boundary §7.5) carries the
upstream's own spelling. Reasons:

- The rule is a guess about spelling, and it fires on any id that ends in two numeric segments (§15 N-4).
- `request_facts.model` lets the host check the body value against the IR model value by value (boundary §7.2); a
  component that rewrites the id fails that check by design.

Migration step: before cutover, model rows whose upstream id is in the dashed form are rewritten to the dotted
form. The rewrite is idempotent for ids already dotted (test at server:translate_kiro.rs:843-856), so dual-run
bodies are then identical in this field.

### 4.5 The conversation id

The host writes a fresh random UUID into `conversationId` on every request (server:translate_kiro.rs:139-142). A
component cannot: gate ② runs every case twice and requires identical output
(crates/south-component-conformance/src/suite.rs:368-379).

**Proposal P-4.** The host sets a reserved key `south_attempt_id` in `ChatRequest.extensions`
(kernel:chat.rs:215-216) on every provider-world call: a UUID (RFC 4122 version 4, lowercase, hyphenated), minted
fresh for each upstream attempt, so a failover attempt gets a new one. It is provider-agnostic — every package
receives it, and this one uses it. The component writes it to `conversationId` and refuses to build a request
when it is absent (boundary R5: no silent third outcome). The IR does not change: `extensions` is an open bag,
and the key is a south convention in the same way `south_credential_attributes` is.

The precedent is the task world, where the host hands the component a host-minted identifier
(`HostMintedValuesV1`, crates/south-contracts/src/task.rs:111-114). Deriving the id inside the component from the
request content is rejected in §14.

### 4.6 Client identification (DP7, ruled 2026-09-30: south takes them in; the host keeps no special case)

**On the inference request** — written by the component:

| Item | Value today | Channel | Closed-vocabulary instance? |
|---|---|---|---|
| `x-amz-target` | `AmazonCodeWhispererStreamingService.GenerateAssistantResponse` (server:upstream.rs:331, 348) | Ordinary descriptor header | No |
| `amz-sdk-request` | `attempt=1; max=1` (server:upstream.rs:349) | Ordinary descriptor header | No |
| `x-amzn-kiro-agent-mode` | `vibe` (server:upstream.rs:350) | Ordinary descriptor header | No |
| `accept` | `application/vnd.amazon.eventstream` (server:kiro.rs:70) | Ordinary descriptor header; it replaces the transport's default `*/*` (crates/south-transport-reqwest/src/lib.rs:533-539) | No |
| `user-agent` | `aws-sdk-js/1.0.0 KiroIDE` (server:upstream.rs:333, 351) | **Reserved**: see below | **Yes** |
| Body `origin` | `AI_EDITOR` on the current message and on every user entry of `history` (server:translate_kiro.rs:26, 125, 180) | Body | No |
| Body `chatTriggerType` | `MANUAL` (server:translate_kiro.rs:138) | Body | No |

None of the three `x-amz*` / `amz-*` names is reserved: south reserves only the signing headers
`x-amz-content-sha256`, `x-amz-date` and `x-amz-security-token` (crates/south-contracts/src/lib.rs:232-256), and
the kernel's `SafeHeaders` refuses only credential headers and transport-owned names (kernel:http.rs:262-296;
kernel:lib.rs:86-96).

`content-type` is not sent today: the request is built with a raw byte body and no explicit content type
(server:kiro.rs:66-72; server:upstream.rs:345-352). The component leaves it out as well, so that the dual run
compares equal; whether it should be sent is K-Q10.

**The `user-agent` value is the one closed-vocabulary instance this package needs.** The name is on south's
reserved list, so a descriptor cannot carry it; the only channel is `ControlledUserAgentV1`, whose value is a
`&'static str` "in host program text" (crates/south-contracts/src/lib.rs:1229-1276), chosen today by a table keyed
on provider type (server:…/engine/south_adapter.rs:260-281). Under the ruling on Q1, a new value compiled into the
host is a host change.

**Proposal P-3.** The manifest declares, per family, one `user_agent` value. Gate ① validates it with the frozen
grammar `try_from_static` enforces today (non-empty, at most 256 bytes, printable ASCII, no leading or trailing
space). South adds a constructor that builds `ControlledUserAgentV1` from a manifest-admitted value; the host
passes that value to the transport and never consults the provider type. What stays closed is the mechanism —
exactly one typed `user-agent` slot, outside the ordinary header channel — which is boundary R1 applied to a set
that boundary §10 does not list. The 2026-08-20 record chose `'static` provenance so that no path led from
configuration or request data to the value; a value admitted at gate ① from a digest-pinned package keeps that
property for request data and replaces "host program text" by "package manifest".

**On the token-exchange request** — written by the recipe: nothing. The host sends only
`content-type: application/json` on both forms (server:token_refresh.rs:1333-1339), which the recipe's `encoding: json`
produces.

**Forwarded by the host, not by the component**: `x-request-id`, `traceparent` and the span headers
(server:kiro.rs:71-78). These are host-generic and stay in the host.

**Other closed vocabularies** (boundary §10): this package needs no secret header (it is on the bearer arm), no
controlled query parameter, and no quota header — the host applies no provider-specific rate-limit header parsing
to this upstream today (the generic snapshot at server:sender.rs:3492-3504 is all there is). If a capture finds
rate-limit headers worth normalizing, they are declared by the package per boundary §10, not added to
`ProviderQuotaMetadataFieldV1`.

## 5. Response and stream mapping

### 5.1 What the component sees

The family declares `stream_framing: aws-eventstream`. The host deframes (prelude, both CRCs, the frame-length
bound — boundary §5.2) and feeds `parse-stream-chunk` one SSE frame per message frame:

```text
event: assistantResponseEvent
data: {"content":"Hel","modelId":"…"}

```

An exception frame arrives as `event: exception:<exception type>`. The component therefore contains an SSE frame
splitter with a partial-frame tail — the same code shape as the Converse reference
(reference_bedrock_converse.rs:511-533, 745-777) — and no eventstream decoder. It must tolerate any byte split
(`StreamIncrementality`, suite.rs:287-325).

### 5.2 Events

Event names and payload shapes are taken from the host's translator and its tests, which state they were captured
from the live upstream (server:leaf/translate_kiro.rs:302-311, 688-712); they were not re-captured for this
record.

| Upstream event | IR events |
|---|---|
| `assistantResponseEvent { content }`, non-empty | `Delta { index: 0, content }` |
| `assistantResponseEvent` with empty `content` | Nothing (today: server:translate_kiro.rs:448-454) |
| `toolUseEvent { name, toolUseId }` with no `input` | `ToolCallDelta { index, id, name, arguments_delta: "" }` — opens a call; `index` counts distinct tool-use ids from 0 |
| `toolUseEvent { input, … }` | `ToolCallDelta { index, arguments_delta: input }`, with `id` and `name` absent; if no call is open for that id, this fragment opens it and carries `id` and `name` |
| `toolUseEvent { stop: true }` | Nothing; the call is marked closed |
| `meteringEvent { unit, usage }` | Nothing; recorded as terminal evidence (§5.3). **Never a `Usage` event** |
| `contextUsageEvent` | Nothing |
| Any other event name | Nothing — a dialect may gain events |
| `exception:<type>` | `StreamEvent::Error` with the envelope of §9; nothing is emitted afterwards |
| A `data` payload that is not JSON | `provider_protocol_error` |

`id` and `name` go on the first fragment only, which is the IR's rule (kernel:stream.rs:33-43) and what the
Converse fixture `provider.stream.a-tool-call-names-itself-once` pins.

### 5.3 Termination

By the host's reading, the wire has no terminal event and no stop reason: the host synthesizes the ending at
end of body, with `tool_use` when a tool call was seen and `end_turn` otherwise (server:translate_kiro.rs:545-565). The
component does the same on the EOF flush (the empty chunk, crates/south-component-conformance/src/component.rs:56-63):

- If at least one frame has been parsed and no `Done` has been emitted: `Finish { finish_reason }` then `Done`,
  with `ToolCalls` if any call was opened and `Stop` otherwise.
- If nothing has been parsed: no events. An empty chunk before any data is not EOF — the incrementality check
  feeds one (suite.rs:296) — and a 2xx with no frames at all is not a completed answer, so it gets no `Done` and
  the host does not settle it as a success.
- If a tool call is open at EOF (a start with no `stop`): no `Done`. The body was cut inside a call.

Two limits of this wire follow, and the component cannot remove them:

1. **`Length` is never reported.** An answer the upstream cut short is indistinguishable from one that ended.
2. **A body cut exactly at a frame boundary looks complete.** The host's deframer catches a cut inside a frame
   (leftover bytes at end of body must be an error — a requirement on the deframer of boundary §5.2); nothing
   catches a cut between frames unless the dialect has terminal evidence.

`meteringEvent` may be that evidence: in every captured sequence in the host's tests it is the last event. If a
capture during the dual run confirms it is present on every completed answer, the component should require it —
EOF without it emits no `Done`, the rule the Converse reference applies to a stream that ends before `metadata`
(reference_bedrock_converse.rs:745-760). Until that is confirmed the component does not require it, which matches
the native arm. See K-Q5.

### 5.4 The non-streaming path

The upstream answers a non-streaming request with the same event stream. Today the host buffers and decodes it
(server:kiro.rs:136-171).

**Proposal P-5.** For a family that declares `aws-eventstream`, when a 2xx response's content type is
`application/vnd.amazon.eventstream`, the host deframes the complete buffered body with the same deframer and
hands `parse-response` an `HttpResponseParts` whose `body` is the concatenated canonical re-encoding. The rule is
keyed on a declared framing and a standard content type, not on the provider; a family whose non-streaming answer
is JSON (Converse) is unaffected, because its content type differs. `request_facts.stream: "none"` (§8.2) tells
the host before sending that the answer will be binary, so it uses the buffered binary transport.

`parse-response` then runs the component's own stream parser over that text, flushes EOF, and folds the events:

- `choices[0].message.content` — the concatenated text; `None` when there is none.
- `choices[0].message.tool_calls` — one per tool-use id, in first-seen order, `arguments` being the concatenated
  fragments **verbatim** (the IR carries arguments as a string, kernel:chat.rs:72-76).
- `finish_reason` — as in §5.3. If the fold ends without a `Done` (no frames, or an open call), the result is
  `provider_protocol_error`, not a response.
- `id` and `model` — left empty for the host to fill, as the Converse and Gemini references do
  (reference_bedrock_converse.rs:37-41).
- `usage` — all zeros (§6.1).

One parser serves both paths, so a streaming and a non-streaming answer to the same upstream bytes fold to the
same content. The alternative — the host always drives the stream path and folds IR events itself — is K-Q7.

### 5.5 Errors inside a 2xx

An exception frame is the only in-band failure this record knows of. Today the streaming path parks the stream for
manual review (server:sender.rs:4920-4928) and the non-streaming path answers 503 after moving the request to
`delivery_unknown` (server:kiro.rs:156-161; server:sender.rs:3977-3987). With the component, both paths see
`StreamEvent::Error` / an error from `parse-response`; what the host does with a failure after dispatch is
unchanged host policy.

## 6. Usage

### 6.1 The component's side

The manifest declares `usage_evidence: absent` for the family (boundary §6.2 item 4). Consequences:

- `parse-stream-chunk` never emits `StreamEvent::Usage`.
- `parse-response` returns `usage` with every field zero. `ChatResponse.usage` is not optional
  (kernel:chat.rs:271-281), and the WIT's rule "a 2xx whose body cannot yield exact usage is an error, never a
  zero" (provider-adapter.wit:110-116) is written for `reported` families. **Proposal P-7** states the rule for
  `absent` families: the field is all zeros, and the host must not read it.
- The credit total of `meteringEvent` does not cross the boundary. The IR has no field for it, `StreamEvent`
  variants carry no extensions (kernel:stream.rs:22-29), and the host discards it today (§1.3).

### 6.2 The host's side (ruled: the host's generic estimator)

South does not define the estimator. This record states what the migration needs from it:

1. **Input is counted over material the component did not produce** — the northbound request as the host
   normalized it, or the IR request. Then a package cannot move the input figure at all. The host already has
   this quantity for mid-stream checkpoints (`checkpoint_input_estimate`, server:sender.rs:1489-1504).
2. **Output is counted over what is delivered northbound** — text, reasoning text, tool name and tool arguments —
   and **by the same rule for streaming and non-streaming answers**. The host already has a provider-agnostic
   meter of exactly this kind (`ForwardedOutputMeter`, server:…/engine/text_admission/sender/checkpoint.rs:1-20,
   41-100), introduced "on the Kiro precedent".
3. **The row is labeled**: `tokens_estimated = 1` and `quantity_estimated = 1`, chosen by the manifest's
   declaration and no longer by family name. Today's label comes from a function documented as serving this one
   provider (server:sender.rs:1905-1932) and the ledger variant `EstimatedTokens` (server:leaf/usage_types.rs:131-147,
   300-326).

Against §1.3, items 1 and 2 change all three numbers the native arm produces. That is a billing-caliber change
for existing traffic and makes a byte-equal dual run of settled amounts impossible unless the native arm is moved
to the generic estimator first (K-Q2).

### 6.3 The host's generic checks that still apply

From boundary §6.3, for an `absent` family:

| Check | Applies? |
|---|---|
| Settled amount ≤ reservation | Yes, unchanged |
| Exactly one terminal state; nothing after `Done` | Yes |
| `output_tokens` ≤ authorized cap × choices | Applies to the host's own estimate. With no upstream cap (§8.3) it can fire on a long answer unless the host enforces the cap itself (K-Q1) |
| `input_tokens` ≤ g(request) | Vacuous: both sides are the host's own count |
| Cache and reasoning consistency | Vacuous: the buckets are zero |
| **New**: an `absent` family that emits `Usage`, or returns a non-zero `usage` | The package contradicts its manifest → manual review, never settled from that number |

**Undetectable zone**, restated for this case. The component reports no number, so it cannot under- or
over-report tokens. What it controls is (a) the text it emits from the upstream's events — which is also what
the caller receives, so inflating or truncating it is visible to the caller — and (b) the body it sends upstream,
which the host cannot compare with the northbound request in any dialect-independent way: a package that sent a
larger prompt than it was given would spend the operator's subscription without changing the bill. Trust for (b)
comes from the same three places as elsewhere: the pinned digest, gate ② request fixtures, and the dual run.

## 7. Credentials

### 7.1 Arm and slot

The descriptor carries `Auth::Bearer { secret: provider_api_key }` (kernel:http.rs:150-152); the manifest declares
`auth_arms: ["bearer"]`; descriptor auth admission (boundary §4.2) maps it to the contract's bearer arm. The slot
is `minted`. `Auth::OAuth` is not used — the boundary record deprecates it in favor of minted slots (its §3.7).

### 7.2 Recipes

```json
"credentials": {
  "schema": "south.credential-recipe.v1",
  "fields": {
    "refresh_token": { "secret": true,  "required": true },
    "client_id":     { "secret": false, "required": false, "syntax": "token" },
    "client_secret": { "secret": true,  "required": false },
    "auth_method":   { "secret": false, "required": false, "syntax": "token" },
    "auth_region":   { "secret": false, "required": false, "syntax": "aws_region", "default": "us-east-1" },
    "profile_arn":   { "secret": false, "required": false, "syntax": "printable_ascii" }
  },
  "import": {
    "kiro-auth-token.json": {
      "refresh_token": ["/refreshToken"],
      "client_id":     ["/clientId"],
      "client_secret": ["/clientSecret"],
      "auth_method":   ["/authMethod"],
      "auth_region":   ["/idcRegion", "/region"],
      "profile_arn":   ["/profileArn"],
      "seed": { "present": "/accessToken", "expires_at": { "rfc3339_or_epoch_seconds": "/expiresAt" } }
    }
  },
  "slots": {
    "provider_api_key": { "minted": { "select": [
      { "when": { "field_in": { "auth_method": ["idc", "enterprise", "iam_identity_center"] } }, "recipe": "idc" },
      { "when": { "field_present": "auth_method" },                                             "recipe": "social" },
      { "when": { "all_present": ["client_id", "client_secret"] },                              "recipe": "idc" },
      { "recipe": "social" }
    ] } }
  },
  "recipes": {
    "social": {
      "steps": [ {
        "id": "token", "kind": "oauth2_token", "encoding": "json",
        "endpoint": "https://prod.{region}.auth.desktop.kiro.dev/refreshToken",
        "endpoint_params": { "region": { "field": "auth_region" } },
        "params": { "refreshToken": { "field": "refresh_token" } },
        "on_status": { "400": "reauth_required", "401": "reauth_required", "429": "transient" },
        "extract": {
          "access_token":  { "pointer": "/accessToken",  "secret": true },
          "refresh_token": { "pointer": "/refreshToken", "secret": true },
          "expires_at":    { "relative_seconds": "/expiresIn", "default_seconds": 3600 }
        }
      } ],
      "present": "token.access_token",
      "rotates_refresh_material": true,
      "rotate": { "refresh_token": "token.refresh_token" },
      "refresh_margin_seconds": 300,
      "without_refresh_material": "fail",
      "attributes": { "profile_arn": { "field": "profile_arn", "export": true } }
    },
    "idc": {
      "steps": [ {
        "id": "token", "kind": "oauth2_token", "encoding": "json",
        "endpoint": "https://oidc.{region}.amazonaws.com/token",
        "endpoint_params": { "region": { "field": "auth_region" } },
        "params": {
          "refreshToken": { "field": "refresh_token" },
          "clientId":     { "field": "client_id" },
          "clientSecret": { "field": "client_secret" },
          "grantType":    { "const": "refresh_token" }
        },
        "on_status": { "400": "reauth_required", "401": "reauth_required", "429": "transient" },
        "extract": { "…": "as in social" }
      } ],
      "present": "token.access_token",
      "rotates_refresh_material": true,
      "rotate": { "refresh_token": "token.refresh_token" },
      "refresh_margin_seconds": 300,
      "without_refresh_material": "fail",
      "attributes": { "profile_arn": { "field": "profile_arn", "export": true } }
    }
  }
}
```

Every value comes from §1.4. The four `select` rules are the host's rule, in the host's order
(server:token_refresh.rs:1189-1214), including "an explicit method wins over shape" (test at :1735-1745).

### 7.3 What the sketch needs from boundary §3.3 (proposal P-6)

| Need | Why | Smallest addition |
|---|---|---|
| Declared parameter names | Neither form is an RFC 6749 token request: both are JSON with camelCase keys, and the social form has no grant type. Boundary §3.3 and §3.8 label this exchange "`oauth2_token`, RFC 6749 §6" | `oauth2_token` means "exchange refresh material for an access token" and mandates no parameter names; `params` is the whole body |
| `select` as ordered rules | The host's choice is a four-step decision, one step of which tests the **presence** of two fields | A rule list, first match wins, last rule unconditional; a closed predicate set `field_in` (ASCII case-insensitive), `field_present`, `all_present`. Presence of a secret field may be tested; its value may not |
| `default_seconds` | A missing `expiresIn` is treated as 3600 s | An optional default on `relative_seconds`. The clamp to 60 s – 24 h becomes a provider-agnostic rule of the host's executor, stated in the gate ③ suite |
| `rotate` | `rotates_refresh_material: true` does not say which extracted value replaces which field | A map from field to step output, required when rotation is declared |
| Import: ordered pointers | `idcRegion` wins over `region` | Each import entry is a list; the first pointer present wins |
| Import: seeding | The sign-in file carries a usable access token and its expiry; a credential with no refresh token works today until that token expires (the skeleton's step ②, server:token_refresh.rs:516-524; server:health_probe.rs:323-335) | An optional `seed` naming the token and its expiry; the file's `expiresAt` is an ISO-8601 string or an epoch number (server:credentials/mod.rs:234-243), hence one more clock convention for import only |
| One value syntax | `profile_arn` contains `:` and `/`, which the `token` syntax of boundary §7.3 cannot hold | `printable_ascii`, bounded length. The value goes only into the JSON body |

`aws_region` as the syntax of the template parameter is already in the boundary record (its §3.3). It closes
something the host leaves open today (§15 N-5).

### 7.4 Intentional differences from the native arm

- **No endpoint override.** `extras.refreshUrl` has no equivalent: the boundary record forbids taking a host
  from credential contents (its §3.3). The override is the test seam of `gateway/tests/kiro_refresh_lock.rs`
  and, by its own comment, an escape hatch for relays; the seam moves to the gate ③ suite's fake endpoint, and
  whether any production credential uses the override is K-Q4.
- **5xx is transient.** The recipe default maps 5xx to `transient`. The native strategy maps every non-2xx to a
  400-class error (§15 N-6).
- **camelCase only.** The snake_case aliases the host accepts in the response are not carried over; the host's
  own comment says both endpoints answer camelCase (server:token_refresh.rs:1239-1241).
- **A missing field is a configuration error before any network call**, in both forms; today only the IdC form
  checks its two client fields up front (:1324-1332).

### 7.5 Exported attribute

`profile_arn` is a static, non-secret field, exported through `south_credential_attributes` (boundary §3.3). The
component writes it to `profileArn` when present and omits it otherwise — exactly today's behavior
(server:translate_kiro.rs:153-155). Which form requires it is not settled by the host's code: one comment says it is
required for social sign-in (server:translate_kiro.rs:51-54), another that it is sent for IdC and intentionally omitted
for social (server:token_refresh.rs:1151-1166). The component takes no position; it forwards what the credential has.

### 7.6 What the host's generic executor inherits

The skeleton's seven steps stay in the host (boundary §3.5). Three provider-specific behaviors become generic
rules keyed on the recipe:

- **Probes (proposal P-8).** A health probe must not force a refresh on a recipe that declares
  `rotates_refresh_material: true` while the stored token is unexpired; once it has expired, the probe runs the
  real refresh and reports its result. This is today's special case (server:health_probe.rs:353-421) with the provider
  test replaced by the declaration.
- **Lock name.** One lock per credential id; the provider-named prefix (server:token_refresh.rs:1288-1290) goes away.
- **Attributes are read from the authoritative row at request time**, as today (server:token_refresh.rs:1381-1400), so
  an operator's edit takes effect without waiting for a refresh.

## 8. Request facts

### 8.1 Model

`{"body": "/conversationState/currentMessage/userInputMessage/modelId"}`. The host compares it with the IR model
value; §4.4 makes them equal by construction.

### 8.2 Stream — proposal P-1

The body has no `stream` key and the URL does not vary. Boundary §7.2 offers `{"body": …}` and `"url"`, and says
an absent declaration means the top-level `stream` field — under which a streaming request with no such field
fails the host's check (server:…/engine/text_admission.rs:916-938 shows today's provider-keyed exemption).

`"none"` means: the dialect has no stream switch; the request is the same for both modes; **a 2xx is always a
stream**. The host then (a) checks that no declared-elsewhere switch appears, which is vacuous here, and
(b) handles a non-streaming northbound request per §5.4.

### 8.3 Output cap — proposal P-2

The body has no cap field the host knows of. Boundary §7.2 requires that exactly one declared location carry the
authorized cap, which an upstream without a cap field cannot satisfy.

`output_cap: []` means: this dialect cannot be told a cap. The host's seal check then has nothing to find in the
body, and the cap must be enforced — if it is to be enforced at all — by the host. The options:

- **A (recommended after cutover)**: the host stops reading when its output meter reaches the authorized cap and
  ends the answer with `length`. Provider-agnostic, selected by the declaration, and a new host execution
  mechanism, so it gets a gate ③ suite (boundary R4).
- **B (today, by code reading)**: nothing bounds the output; the reservation's headroom absorbs what it can, and
  a settlement above the reservation goes to manual review.
- **C**: refuse to serve a bounded-token model through a family that declares no cap location.

During the dual run the component arm keeps B so the two arms are comparable. The choice is K-Q1.

## 9. Errors, retry and cooldown

`map-provider-error` maps a non-2xx onto the closed catalog (kernel:error.rs:13-48). The host has no
provider-specific mapping today: a non-2xx after dispatch is returned as an upstream error and the request moves
to `delivery_unknown` (server:sender.rs:3897-3930, 4081-4091), and cooldown is recorded from the status, `retry-after`
and the generic quota snapshot (server:sender.rs:3492-3504). The table below is therefore new knowledge in the package.

The status column is certain. The exception-name arm reuses the Converse reference's handling of the AWS error
shape — the `x-amzn-errortype` header or the body's `__type` (reference_bedrock_converse.rs:950-986); **which
names this upstream actually emits is not verified** (K-Q6).

| Status (fallback) | Exception name, if present | Code | Retriable elsewhere |
|---|---|---|---|
| 400, 404, 422 | `ValidationException`, `ResourceNotFoundException` | `invalid_request` | No |
| 401, 403 | `AccessDeniedException`, `UnrecognizedClientException` | `auth` | No — pinned by `AuthErrorsAreNotRetriable` |
| 402 | — | `payment_required` | No |
| 408 | `ModelTimeoutException` | `timeout` | Yes |
| 429 | `ThrottlingException` | `rate_limit` | Yes |
| 500, 502, 503, 504 | `InternalServerException`, `ServiceUnavailableException` | `upstream_unavailable` | Yes |
| Anything else | — | `internal` | No |

- `retry_after_ms` comes from `retry-after` when present.
- `provider_message` is the body's `message`, at most 256 characters, never the raw body (kernel:error.rs:90-96).
- A 500 from this upstream does not always mean the upstream is unhealthy: the host's own comment records that a
  malformed body is answered with an opaque 500 (server:kiro.rs:88-93). The component cannot tell the two apart from the
  response; conformance (§11) is what keeps the component from producing malformed bodies.
- Signals the host needs and gets: the code (and through it `is_retriable_elsewhere`, kernel:error.rs:72-87),
  `retry_after_ms`, and the HTTP status. Credential-level signals come from the recipe: `reauth_required` on a
  400 / 401 from the token endpoint — the host's terminal marker today (server:token_refresh.rs:252-270) — and
  `transient` otherwise. A mint failure is a pre-admission error that moves no money and lets the host try the
  next credential, as today (server:sender.rs:2648-2655).
- Whether exhaustion of the subscription's allowance has a distinguishable status or exception name is unknown;
  if it does, it maps to `payment_required` or `rate_limit` (K-Q6).

## 10. What the host's generic path is expected to do

Nothing below names the provider.

1. Route the model row to the package (pinned by digest); read the credential fields; evaluate `select`; mint per
   the recipe if the stored token is within the margin of expiry (boundary §3.5). Failure is a pre-admission error.
2. Put the exported attributes into `ProviderConfig.extensions.south_credential_attributes`, and a fresh
   `south_attempt_id` into `ChatRequest.extensions` (P-4).
3. Call `build-http-request`. A capability or invalid-request error is a 400 with zero upstream calls and **no
   credential failover** — a refusal of the request's shape is not a credential failure (contrast §15 N-3).
4. `ProviderConfig::authorize` (kernel:provider.rs:361-382), descriptor auth admission (boundary §4.2), and the
   `request_facts` checks: model by value, no stream switch, no cap location.
5. Apply operator request extras to the body; seal; reserve; write the dispatch marker.
6. Send with the manifest's `user_agent` (P-3): through the streaming transport for a streaming request, through
   the buffered binary transport otherwise.
7. Non-2xx → `map-provider-error`; funds per the host's existing rules for failures after dispatch.
8. 2xx, streaming → deframe, re-encode, `parse-stream-chunk`, render northbound; count delivered output.
9. 2xx, non-streaming → deframe the whole body, re-encode, `parse-response` (P-5); fill `id` and `model`; render;
   count delivered output by the same rule as step 8.
10. Settle from the generic estimate and label the row estimated (§6.2); apply §6.3.

What no longer exists in the host: the provider type's arms, the dedicated send and finalize paths, the leaf
crate's translator, the mint strategy, the probe special case, the sign-in file recognizer and the header
constants (§12.3).

## 11. Conformance

### 11.1 Gate ① additions

Validation of the manifest fields this record adds or refines: `user_agent` against the frozen grammar (P-3);
`request_facts.stream` ∈ `{body, url, none}` and `output_cap` possibly empty (P-1, P-2); the `select` rule list
(closed predicates, every rule names an existing recipe, the last rule unconditional); `rotate` present exactly
when rotation is declared; import pointers syntactically valid (P-6); and consistency — a family that declares
`stream: "none"` must declare a `stream_framing`, since its 2xx is always a stream.

### 11.2 Gate ② — fixture pack `fixtures-kiro/`

Stream and response inputs are written in the canonical re-encoding, as boundary §5.4 requires of
`aws-eventstream` families.

| Row | Asserts |
|---|---|
| `request.chat` | Single user turn → the §4.1 shape; headers; bearer slot; `conversationId` from `south_attempt_id` |
| `request.system-joins-the-first-user-turn` | With and without `history` |
| `request.tools-and-parallel-tool-results` | `toolSpecification`, `toolUses`, a run of tool results in one user turn |
| `request.tool-choice-none-withholds-tools` | §4.2 |
| `request.profile-arn-from-exported-attribute` / `request.no-profile-arn` | Present and omitted |
| `request.sampling-has-no-slot` | `temperature`, `top_p`, `stop`, `max_output_tokens` leave no trace in the body |
| `request.model-is-verbatim` | A dashed id is **not** rewritten |
| `request.refused-*` | One row each: image, unknown part, `required` tool choice, assistant-final turn, non-object arguments, missing `south_attempt_id` → the stated error code |
| `response.text` / `response.tool-use` | The fold of §5.4; `usage` all zeros; `id` and `model` empty |
| `response.no-frames` / `response.open-tool-call` | `provider_protocol_error` |
| `stream.text` / `stream.tool-use` / `stream.text-then-tool` | The events of §5.2 and the EOF flush of §5.3 |
| `stream.metering-is-never-usage` | A `meteringEvent` is present; **no `Usage` event** in the output |
| `stream.unknown-event-ignored` / `stream.empty-content-ignored` | §5.2 |
| `stream.exception-frame` | `event: exception:<type>` → `Error`, and nothing after |
| `stream.open-tool-call-at-eof` / `stream.no-frames` | No `Done` |
| `error.rejected-credential` (401, 403) / `error.throttled-carries-retry-after` / `error.opaque-500` | §9 |
| `capabilities.declared` | Echoes the operator's models, as the other packages do (reference_bedrock_converse.rs:791-798) |
| `credential.social` / `credential.idc` / `credential.select-*` / `credential.rotation` / `credential.expiry-default` / `credential.status-400` / `credential.status-500` | The rendered exchange request and the extraction (boundary §3.6), including each of the four `select` rules |

**Checks.** The existing eight apply unchanged (report.rs:19-64). From the boundary record:
`DescriptorAuthWithinManifest` (§4.4) and `RequestFactsHonoured` (§7.6) — the latter extended so that a family
declaring `stream: "none"` has no `stream` key and one declaring `output_cap: []` has no cap anywhere in the
body's declared locations. New:

- **`AbsentFamilyEmitsNoUsage` (P-7)**: for every family declaring `usage_evidence: absent`, no stream fixture's
  output contains `Usage` and every response fixture's `usage` is all zeros. It replaces the by-name usage rows
  and `UsageNeverDefaulted`, which apply to `reported` families.

**Mutation checks**, run by the suite on the pack's own fixtures:

- *Metering inflation*: multiply the `meteringEvent` value by 1,000 in every stream and response fixture; the
  output must be unchanged. The component must be deaf to the number.
- *Attempt-id sensitivity*: change `south_attempt_id`; exactly one body value may change, at
  `/conversationState/conversationId`.
- *Frame reordering inside a tool call*: swap two `input` fragments; the concatenated arguments must change
  accordingly — the component must not reorder or deduplicate.

**Release discipline.** Boundary §6.2 item 5 requires a documentation-derived usage judge for each package south
publishes. This upstream has no public documentation of its wire known to this record, and the package reports
no usage; the judge is replaced by `AbsentFamilyEmitsNoUsage` and the dual run. Request and event shapes rest on
captures, which the release must archive as redacted fixtures with their capture date.

### 11.3 Gate ③ — what the host must prove

| Host suite | Proves |
|---|---|
| `south.credential-recipe.v1` (boundary §3.6) | Both recipes against a fake token endpoint: rotation write-back; eight concurrent callers hit the endpoint once and all get the new token (today's `kiro_refresh_lock.rs`, made generic); CAS loser re-read; no retry on `reauth_required`; the expiry clamp; `select` evaluated from the authoritative row |
| Eventstream deframer adoption (boundary §5.4) | Frames split across chunks, both CRC errors, an oversized frame, an exception frame, **leftover bytes at end of body**, and the buffered whole-body form of P-5 |
| Attempt id (P-4) | Present on every provider-world call; fresh per attempt; a UUID |
| Declared user agent (P-3) | The wire carries exactly one `user-agent`, equal to the manifest value; the host has no per-provider table |
| Absent usage (§6) | A settled row carries both estimated flags; streaming and non-streaming answers to the same upstream bytes settle to the same token counts; a package that emits `Usage` despite `absent` goes to manual review |
| Refusal before admission | A capability error from `build-http-request` produces a 400, zero upstream calls, zero token refreshes beyond the one already needed, and no credential failover |
| Probe rule (P-8) | A probe of a rotating recipe with an unexpired token makes no network call |
| Host-enforced cap | Only if K-Q1 chooses option A |

T21 (boundary §12) already plans an `aws-eventstream` mode, a minted slot and an `absent` family; it should gain
`stream: "none"` and `output_cap: []` modes so these two declarations are proven without this package installed.

## 12. Migration and dual run

### 12.1 Steps

| Step | Side | Content | Acceptance |
|---|---|---|---|
| K0 | Host | Correct the native arm where §15 marks a defect that would otherwise be pinned as "correct" by the dual run (at least N-2, N-3 and N-8; N-1 per K-Q1); decide the estimator alignment (K-Q2); rewrite dashed model ids on the model rows (§4.4) | Native tests green; rulings recorded |
| K1 | South | Boundary phases B1, B2, B4 and the user-agent part of B7, plus P-1…P-8; the reference implementation, the package, the fixture pack | The package passes gates ① and ②; a south minor release lists it in the release index |
| K2 | Host | The generic path of §10 for this family; model rows routed to the package; the dual run | §12.2 |
| K3 | Host | Delete §12.3 | The J1 count falls; removing the package and its rows leaves the host compiling, testing and starting (J3) |

K1 depends on boundary B1 (`usage_evidence`), B2 (`stream_framing`, `request_facts`, descriptor auth admission,
the deframer), B4 (recipes) and B7 (instance declaration). Until B4 lands, the host's existing mint strategy can
stand in for the recipe; that is a J1 red item and must be cleared before K3.

### 12.2 Dual-run reconciliation

The same northbound Chat request goes through the native arm and the component arm, once under each billing form
(`balance`, `quota`), streaming and non-streaming. Compared item by item:

1. **Upstream request**: method, URL, the non-auth headers of §4.6 (including the absence of `content-type`), and
   the body as JSON — equal except `conversationId`, which differs by construction and is compared for shape.
2. **Token exchange**: for an expired credential of each form, the exchange request (URL, body) and the state
   written back.
3. **Northbound response**: non-streaming — content, tool calls, finish reason; streaming — the sequence of
   deltas and the terminal chunk. `id` values are host-minted in both arms and compared for shape.
4. **Reservation**: equal. It is a function of the model row and the authorized cap only
   (server:…/engine/token_counter/authorize.rs:830-954), so neither arm's translation can move it.
5. **Settlement**: settled token counts, amount, `tokens_estimated`, `quantity_estimated` — equal **if** K-Q2
   aligns the native arm first; otherwise equal flags and a recorded per-row delta in the counts.
6. **Failures**: 401, 429 with `retry-after`, 500, an exception frame mid-stream, a body cut inside a frame, a body
   with no frames, an expired token whose refresh answers 400 and 500.
7. **Captures**: at least one recorded live answer per shape (text, tool call, text then tool call), archived as
   fixtures; the capture also answers K-Q5, K-Q6 and K-Q10.

**Intentional differences** (on record, not reconciliation failures):

- The refusals of §4.3 marked as differing from native, unless K0 has already corrected the native arm.
- Unparseable tool arguments on the non-streaming path: the component passes the concatenated string; the native
  arm replaces it with `{}` (server:translate_kiro.rs:627-631).
- A refresh answered with 5xx: `transient` versus a 400-class error (§7.4).
- A request-shape refusal no longer rotates through credentials (§10 step 3).
- Operator body extras reach the upstream body (§4.3, last row).
- The model id is no longer rewritten (§4.4); with the rows rewritten in K0 the bodies are equal.

**Surfaces.** The dual run is on Chat only — the native arm has no other surface to compare with. Messages and
Responses are accepted separately, against the package's fixtures and a live capture (K-Q3).

### 12.3 What retires in the host

Line counts measured at host `4d5bb4e5`. "Whole" means the file is deleted.

| Location | Lines | Content |
|---|---|---|
| `crates/gateway-provider-protocol/src/translate_kiro.rs` | 933, whole (647 before the first test module) | Both translation directions |
| `gateway/src/modules/inference/engine/kiro.rs` | 171, whole | Request preparation, the single-attempt sender, the buffered decoder |
| `…/engine/text_admission/sender.rs` | 615 in seven spans: 2611-2685, 2687-2701, 2726-2728, 3540-3544, 3878-4057, 4093-4113, 4735-5050 | Prepare, the two estimate helpers, the two dispatch branches, non-streaming finalize, the streaming path |
| `…/engine/text_admission.rs`, `text_admission/chat.rs`, `messages.rs`, `sender/messages.rs`, `reasoning_replay.rs` | About 55 in total | The operation and transport variants and their match arms (server:text_admission.rs:92, 103, 637-640, 802, 890-892, 933, 1114-1122, 1204-1206; server:chat.rs:61, 71, 325-334, 581-588; server:messages.rs:358; sender/messages.rs:678-684; reasoning_replay.rs:166) |
| `…/engine/token_refresh.rs` | 283 (1118-1400), plus 106 of tests (1680-1785) | The mint strategy, replaced by the recipes |
| `…/engine/health_probe.rs` | 82 (323-335, 353-421), plus about 40 of tests | The probe special case, replaced by P-8 |
| `…/engine/upstream.rs` | 25 (169, 329-333, 340-352, 547-552), plus 14 of tests | Header constants, the auth helper, the URL arm |
| `…/engine/provider_pool.rs` | 14 (535-548) | The probe-path auth branch |
| `…/engine/dispatch/mod.rs` and `dispatch/tests_a.rs` | 24 (550, 585-607) and 118 (313-430) | The routed-step arm and its test |
| `gateway/src/modules/admin_ui/handler/credentials/mod.rs` | 53 (215-267), plus its tests | The sign-in file recognizer, replaced by `credentials.import` |
| `…/engine/south_component.rs:272-276`, `south_adapter.rs:240, 349`, `south_switch.rs` (the surface variant and four arms), `request_extras.rs:947`, `gateway/src/modules/byok/service.rs:298` | A few lines each | Provider-type arms |
| `gateway/tests/kiro_refresh_lock.rs` | 99, whole | Superseded by the generic recipe suite |
| `gateway/src/infra/config/providers.rs` (the enum variant and eight arms), the two `provider_type` CHECK lists, `webv2/src/lib/providerTypes.ts:103-104` and two locale strings | — | Retire with the generic label of server P21 S7, not with this package |

The sender figure differs from the 558 lines of server P21 Appendix B.2; this record did not re-derive that
count's rule. `frozen_chat_payload_from_usage` (server:sender.rs:1905-1941) stays and becomes the generic path for
`absent` families. `leaf/aws_eventstream.rs` (755 lines) is shared with the Bedrock arms and retires when the host
adopts the south-provided deframer (boundary §5.2), not with this package.

Counting whole files, the listed spans and their tests, about 2,600 lines leave the host, of which about 2,000
are not test code.

## 13. Versioning

- **South**: a minor release. It adds one package, one reference implementation and one fixture pack; the
  manifest additions P-1, P-2, P-3 and P-6 are new optional fields or new values of proposed fields (boundary R5:
  when absent they mean today's behavior). P-3 adds a constructor to a contract type without changing the wire;
  P-4 and P-5 are conventions recorded under `host_capabilities` in `compatibility.json` once a host adopts them.
- **No world, WIT or kernel change.** Nothing here goes through the kernel chain; K-Q14 is a kernel question that
  does not block version 1.
- **Host link layer**: one upgrade to a south that knows the new manifest fields and ships the deframer, the
  recipe interpreter's types and the user-agent constructor. After that, changing this provider's headers, body
  markers, endpoints, exception table or recipes is a package release; the host changes nothing (DP0, with the
  Q1 ruling).
- **Package identity** bumps when the wire mapping, the headers, the declared `user_agent` or a recipe changes.
  The client-identification values are expected to drift with the vendor's client, so this package will bump
  more often than the four dialect packages.
- **Existing thirteen packages**: unaffected.

## 14. Rejected alternatives

- **Keep the three-hop translation and only move the last hop into a package** (a package that takes Anthropic
  Messages JSON). The provider world's input is the IR; a package that consumes another package's output would
  need a new world and would tie this provider's behavior to the Anthropic package's version.
- **Add the wire as a family of `provider-bedrock-converse` or `provider-anthropic`**. It shares eventstream
  framing with the former and the model vendor with the latter, and nothing else: the request shape, the event
  vocabulary, the auth arm and the usage discipline all differ.
- **The component deframes eventstream itself** (`stream_framing: bytes`). The WIT allows it and boundary §5.2
  does not forbid it, but it would put a second deframer and a CRC dependency in a package when south is about
  to provide one, and the non-streaming path would still need the host to hand over binary.
- **Derive `conversationId` inside the component from the request content.** It would be deterministic, but two
  unrelated requests with the same opening message would share an id on the same account, and what the upstream
  does with a repeated id is unknown. Today's behavior is a fresh id per request; P-4 keeps it.
- **Convert credits to tokens in the component and report them as `Usage`.** The host already abandoned the
  credit-derived figure for billing (server:translate_kiro.rs:323-374 keeps it "only for legacy display"); the
  conversion needs prices, which do not enter south (ARCHITECTURE.md:108-112); and it would make an `absent`
  family lie about having evidence.
- **Carry the host's model-id rewrite into the component.** §4.4.
- **Keep `extras.refreshUrl` as a recipe parameter.** It is precisely the "host taken from credential contents"
  that boundary §3.3 rules out.
- **A new world for subscription-backed providers.** Nothing in the function set differs; every difference is
  expressible as a declaration.

## 15. Observations on the native arm (code reading, not run)

Noted while reading the host at the baseline. None was executed or reproduced; none is a statement about impact.
They matter here because a dual run pins whatever the native arm does as "correct".

- **N-1 The authorized cap is not sent upstream.** The seal validates `max_tokens` on the intermediate body
  (server:text_admission.rs:890-892, 1114-1115); the upstream body is built from `messages`, `system` and `tools` only
  (server:translate_kiro.rs:55-157). The provider type is nevertheless classed `BoundedToken`
  (server:text_admission.rs:1204-1206).
- **N-2 Streaming and non-streaming answers are counted over different material** (§1.3). On the streaming path
  the tool name is added once per `toolUseEvent`, including the start and stop events (server:sender.rs:4756-4762), so
  the count depends on how the upstream fragments a call.
- **N-3 A request-shape refusal is handled as a credential failure.** `prepare_request` refreshes the token and
  then translates (server:kiro.rs:42-53); any error from it becomes `RetryCredential`
  (server:sender.rs:2648-2655). A request with an image would, by this reading, be tried against every credential,
  refreshing each that is stale.
- **N-4 The model-id rewrite matches any id ending in two numeric segments** (server:translate_kiro.rs:34-47) — a
  date-suffixed id such as `name-4-20250514` would become `name-4.20250514`.
- **N-5 The region is substituted into the token endpoint without validation** (server:token_refresh.rs:1215-1220,
  1254-1262), and the sign-in file's `region` / `idcRegion` are copied into `extras` verbatim
  (server:credentials/mod.rs:244-256). The same parser serves the user-facing subscription binding
  (server:gateway/src/modules/byok/handler.rs:181-206; this provider is on that surface,
  server:gateway/src/modules/byok/service.rs:291-300). A value containing URL syntax would change the host the
  refresh is sent to, and the endpoint's error body is included in the returned message
  (server:token_refresh.rs:1367-1371). Whether the URL parser, the HTTP client or an egress guard stops this was not
  tested. The recipe's `aws_region` syntax (§7.3) removes the question for the component arm.
- **N-6 Every non-2xx from the token endpoint is classified as a 400-class error** (server:token_refresh.rs:1367-1371),
  while the trait's contract says 5xx is upstream jitter and should be 503 (:451-453).
- **N-7 An unparseable event payload is handled differently on the two paths**: the buffered decoder turns it
  into `null` and the fold ignores it (server:kiro.rs:143-149); the streaming path raises an error without parking the
  stream (server:sender.rs:4889-4895), as do its render errors (4903-4910, 4954-4956), leaving the terminal transition
  to the drop path.
- **N-8 Leftover bytes at end of body are not checked** on either path (server:kiro.rs:141-169;
  server:sender.rs:4864-4933): a body that ends inside a frame, if the transport reports a clean end, is folded as
  a complete answer.
- **N-9 A final assistant turn is sent as the current user message** (§4.3); `tool_choice` is dropped for every
  value; unparseable tool input becomes `{}` on the non-streaming path (server:translate_kiro.rs:627-631).
- **N-10 Operator body extras target the intermediate body** (server:text_admission.rs:350-370, 410); only changes to
  `messages`, `system` or `tools` survive the last hop.
- **N-11 Stale comments**: the pricing arm says the upstream meter "is retained for cost observability"
  (server:text_admission.rs:1204-1205), but nothing on the durable path records it (§1.3); the two comments on
  `profileArn` contradict each other (§7.5).

## 16. Open questions

Tags: S = south maintainers, L = lv, K = kernel. Each carries this record's recommendation.

- **K-Q1 (L) An upstream the host cannot cap** (§8.3). Recommendation: keep today's behavior for the dual run;
  after cutover the host enforces the authorized cap itself and ends the answer with `length` (option A). This
  changes what a caller sees on a long answer, so it needs a ruling and a notice.
- **K-Q2 (L) Aligning the estimate before the dual run** (§6.2). Moving to the generic estimator changes the
  billed token counts of existing traffic. Recommendation: move the native arm to the generic estimator first, as
  its own reviewed change, then dual-run for byte-equal settlement; the alternative is a dual run that records a
  per-row delta and proves nothing about amounts.
- **K-Q3 (L) Messages and Responses for this provider** (§1.5, D10). Recommendation: keep them off until the Chat
  dual run passes, then switch them on with their own acceptance; they are a product change, not a migration.
- **K-Q4 (L) Dropping `extras.refreshUrl`** (§7.4). Recommendation: drop it; first query production for
  credentials that set it.
- **K-Q5 (S, L) `meteringEvent` as required terminal evidence** (§5.3). Recommendation: require it if the
  dual-run captures show it on every completed answer; it turns a silently truncated answer into a detected one,
  at the cost of `delivery_unknown` if the upstream ever omits it.
- **K-Q6 (S) The exception and status vocabulary of this upstream** (§9), including what an exhausted allowance
  looks like. To be settled by capture; until then the status column alone is authoritative.
- **K-Q7 (S) The non-streaming path**: buffered deframe into `parse-response` (P-5, recommended — no new suite
  rule, real `response.*` fixtures, one parser) or the host always driving the stream path and folding IR events
  itself (one less convention on `parse-response`, but a new host fold and a waiver of the `response` fixture
  family). The upstream-Responses package of boundary §11 will face the same choice; the two should agree.
- **K-Q8 (S) Accept P-1…P-8** (§3.3), in particular: `user_agent` as a declared instance (P-3), which extends
  boundary §10; `south_attempt_id` (P-4); and the recipe refinements (P-6), which revise boundary §3.3's
  description of this exchange as an RFC 6749 refresh grant.
- **K-Q9 (S) Family name**: `kiro` (recommended, §3.1) or a name for the wire alone, with the sign-in recipes and
  client identification then belonging to a family that has only one known user.
- **K-Q10 (S) `content-type` on the inference request** (§4.6): match the native arm and send none
  (recommended for the dual run), or send the media type the vendor's client sends, once a capture shows it.
- **K-Q11 (S) Is `conversationId` required, and must it be a UUID?** If the upstream accepts its absence, P-4 is
  unnecessary for this package; the proposal stands on its own for any dialect that needs a client-generated id.
- **K-Q12 (S, L) Images.** Version 1 refuses them, as the host does. Whether the wire carries images is not known
  from the host's code; adding them later is a package change plus a capability declaration.
- **K-Q13 (S) The model catalog for this family** (boundary §7.5, ruled a south data artifact). The ids named in
  the host's comments and tests (server:translate_kiro.rs:28-33, 843-856) are a starting list, not a verified one.
- **K-Q14 (K) A tool-result error flag in the IR** (§4.3). `Message` has no field for it; carrying one needs an
  IR convention. Not blocking: today's durable path does not produce it either (inferred).
- **K-Q15 (S, community host) A second consumer.** Server P21 §7 states that the community host has its own
  implementation of this provider; this record did not read it. If so, the package is the first case where both
  hosts retire provider code for one package, and the community host's view of P-3, P-4 and P-5 should be sought
  before they are frozen.
