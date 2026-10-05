# The Kiro provider component (`provider-kiro`)

Status: proposed — drafted for review by the host team (token-station-server P21), not accepted

Date: 2026-09-30

Revised: 2026-10-01 after independent review (see the revision note at the end).

Predecessors: `2026-09-30-host-zero-vendor-boundary.md` ("the boundary record"; this record expands the Kiro row of
its §11 and depends on its §3 credential recipe v1, §4 descriptor auth admission, §5 `stream_framing`, §6 usage and
`usage_evidence`, §7 `request_facts`, §8 compatibility range and §10 instance declaration),
`2026-08-21-canonical-ir-inventory.md` (S0: D1 usage belongs to the component, D2 chunks are bytes, D5 `extensions`
are data, not contract), `2026-08-20-controlled-user-agent.md` (the `user-agent` value is a host compile-time
literal), `2026-09-30-embeddings-contract.md` (the sibling record whose structure this one follows).

Origin: token-station-server plan P21 (`docs/product-review-v2/plans/2026-09-29-P21-*.md`) — §2.5 (the
three-hop translation), §5 S3–S5, §8.4 DP6 ("migrate Kiro into south", approved 2026-09-30) and DP7, §8.5 (the
owner's rulings on the boundary record's Q1, Q10 and Q12), Appendix B.2 / B.4; and
plan P9 (`2026-09-24-P9-*.md`, the main plan) T2 ("Kiro stays out of south", ruled 2026-09-24, reopened by DP6).

Baseline: south `origin/main` = v0.42.0 (`3135e36`); kernel `f585bc83` (protocol 0.4.0); host `a82c852b` for every
code citation. The first draft cited host `4d5bb4e5`; of the host files cited here only
`…/engine/token_refresh.rs` changed between the two (the `region` validation of host `a82c852b`'s parent series),
so its line numbers were updated and every other host citation holds unchanged. The owner rulings of 2026-09-30
cited below are recorded in host `c4bd45e5`, a docs-only commit. Host citations carry a `server:` prefix;
`…/engine/` abbreviates `gateway/src/modules/inference/engine/` and `leaf/` abbreviates
`crates/gateway-provider-protocol/src/`; once a host file has been cited with its path, later citations give
`server:` and the file name only. Kernel citations carry `kernel:` and refer to `crates/protocol/src/` at the
baseline revision.

Three rulings this record is written on (lv, 2026-09-30; server P21 §8.4 DP7 and §8.5):

- **DP7 / boundary Q10**: south takes in client-identification headers; the host keeps no special case. Headers on
  the inference request are written by the component, headers on the token-exchange request by the credential
  recipe, and names are declared by the package.
- **Boundary Q1**: bumping the south / kernel pin counts as modifying the host. Any instance Kiro needs from a
  closed vocabulary is therefore declared by the package (boundary §10), never added to a compiled-in enum.
- **Boundary Q12 / embeddings E-Q3**: the two estimate conventions coexist. For a chat-family provider with
  `usage_evidence: absent`, the host's generic estimator makes the estimate.

**Reading guide.** §2–§11 are the south contract: what the package is, what it declares and what gates ① and ②
check. §12 is the dual run that accepts it. Host-side material — the native arm as it is today, what the host's
generic path must do, the migration steps, the retirement list and the defects found in the native arm — is in
Appendices A–D at the end. Sections 10 and 15 are kept as short pointers so that section numbers cited by other
records stay valid.

## 0. Summary

Kiro is served today by a dedicated host arm: a three-hop translation (IR → Anthropic Messages → the upstream's
`conversationState` body; upstream events → Anthropic SSE → IR → the northbound surface), its own send path, its
own token refresh and its own health probe (Appendix A). This record moves all provider knowledge into one package.

| # | Topic | Decision | Section |
|---|---|---|---|
| 1 | Package | `provider-kiro`, family `kiro`, world `provider-adapter-v2`; no WIT signature change | §3 |
| 2 | Request | One hop, IR → `conversationState`; version 1 reproduces the native turn layout (one history entry per IR message); lossy and refused cases enumerated; the model id is passed verbatim | §4 |
| 3 | Client identification | Three ordinary headers and two body fields written by the component; the `user-agent` value declared by the package through the boundary record's single user-agent proposal (its §10) | §4.6 |
| 4 | Framing | Package-level `stream_framing: aws-eventstream`; the component parses the canonical re-encoding of both `exception` and `error` frames, on the streaming and the buffered non-streaming path | §5 |
| 5 | Usage | Package-level `usage_evidence: absent`; the component never emits `Usage`; the host's generic estimator counts (a new host settlement meter), and the ledger marks the row estimated | §6 |
| 6 | Auth | Bearer arm; the slot is `minted` by one of two recipes (`social`, `idc`) chosen by an ordered rule list; rotation declared, an absent rotated token keeps the old one; `profile_arn` exported | §7 |
| 7 | Request facts | Model in the body; **no stream switch and no cap location**, declared with the boundary record's `stream: "none"` and `output_cap: []` (its §7.2); the cap is enforced by the host (ruled) | §8 |
| 8 | Errors | Status-based mapping plus the AWS exception-name arm; exception and error frames become `StreamEvent::Error` | §9 |
| 9 | Migration | Dual run against the native arm on a replaying fake upstream, direct and routed paths, both billing forms; about 2,700 host lines retire | §12, Appendix C |

§3.3 maps what this package needs to where the boundary record now provides it; the first draft's proposals P-1…P-7
have been taken into that record, and the labels are kept so that citations stay valid. What remains this record's
own is one probe rule (P-8b), the two cases of the ruled cap rule (§8.3) and the Kiro-specific questions of §16.
The attempt id waits on boundary Q14 (P-4, K-Q16).

## 1. Problem

### 1.1 Today, in one paragraph

The host builds the IR from the northbound Chat body, has the **Anthropic** component turn it into an Anthropic
Messages body, seals that intermediate body, and at prepare time refreshes the token and translates the intermediate
body into the upstream's `conversationState` body. The upstream answers every request — streaming or not — with a
binary AWS eventstream carrying no token counts; the host deframes it, turns each event into Anthropic SSE, feeds
that to the Anthropic component's stream parser and renders the IR. It bills a host-side character estimate at the
model's token rate card and labels the row estimated. Credentials are minted by one of five host `MintStrategy`
implementations, and the health probe special-cases the provider. Only Chat is served. Appendix A gives each step
with its citations.

### 1.2 Why the provider world cannot simply absorb it today

The WIT's function signatures are sufficient (§3.2). Six things outside them are not, in south as released today;
the right column says where the boundary record supplies each:

| Gap in south today | Supplied by |
|---|---|
| The `user-agent` value must be a literal in the host's program text: `ControlledUserAgentV1` stores a `&'static str`, is `Copy` and returns `&'static str` (crates/south-contracts/src/lib.rs:1244-1276); the ordinary header channel refuses the name (:232-256) | Boundary §10 (`DeclaredUserAgentV1`), its Q15 and Q16; §4.6 here |
| The manifest cannot say that the upstream has no stream switch and no top-level `stream` | Boundary §7.2 (`stream: "none"`) |
| The manifest cannot say that the upstream body has no output-cap field | Boundary §7.2 (`output_cap: []`) and §6.3 |
| A 2xx body is binary even for a non-streaming request; `HttpResponseParts.body` is text, and the kernel's doc comment says a binary response "would need a `-v2` field rather than a lossy encoding here" (kernel:http.rs:379-382) | Boundary §5.2 (the buffered path) and §14 (the comment amendment) |
| No south function deframes eventstream or re-encodes its `exception` and `error` frames; the host's own deframer folds both kinds into one error (server:leaf/aws_eventstream.rs:184-198) | Boundary §5.2 |
| The request carries a per-request random id: gate ②'s `Determinism` check forbids randomness (crates/south-component-conformance/src/report.rs:27-32), and S0 D5 forbids a component from acting on an `extensions` key (canonical-ir-inventory.md:249-255) | Open: boundary Q14 (§4.5 here) |

## 2. Decisions

- **D1 One package, one family, the existing world.** `provider-kiro`, family `kiro`, `provider-adapter-v2`. No
  new world and no WIT signature change (the doc-comment amendments are boundary §14; §13 here).
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
  `user-agent` value in the manifest through the boundary record's user-agent proposal (§4.6).
- **D8 A family with no stream switch gets the buffered path** (boundary §5.2, §7.2). The host buffers the
  complete body, deframes it and hands `parse-response` the same canonical re-encoding the stream parser sees
  (§5.4). This differs from the Codex family on purpose: that family has a stream switch (§5.4).
- **D9 The upstream has no cap and no stream switch, and the manifest says so** (§8). The host enforces the
  authorized cap on its own meter (K-Q1, ruled).
- **D10 Every northbound surface the host's generic component path serves becomes available**; nothing in the
  package is surface-specific. The host keeps no per-provider switch for surfaces (K-Q3, ruled 2026-09-30).
- **D11 Version 1 reproduces the native request layout.** Where the native arm's layout is a choice rather than a
  defect — notably one history entry per IR message, tool results included (§4.2) — the component sends the same
  body, so the dual run can compare bodies field by field. Changing the layout is a later package release, argued
  from a capture (K-Q18).

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

### 3.2 The world's signatures are sufficient

`provider-adapter-v2` exports `build-http-request`, `parse-response`, `parse-stream-chunk`, `map-provider-error`
and `model-capabilities` (crates/south-provider-api/wit/provider-adapter.wit:98-132). Each is used with its
current signature:

- `build-http-request` returns a JSON POST descriptor naming one bearer slot.
- `parse-stream-chunk` takes bytes (provider-adapter.wit:20-25, 118-125); with the package-level
  `stream_framing: aws-eventstream` the bytes are the canonical re-encoding of boundary §5.2.
- `parse-response` takes `HttpResponseParts`, whose body is text; §5.4 gives it text.
- The component does not call `host.sign`. The world imports `host` (provider-adapter.wit:138-141), and nothing
  obliges a component to call what its world imports.

Two normative doc comments do not fit as written: the WIT's "never a zero" usage rule (provider-adapter.wit:110-116,
for the all-zero usage of an `absent` package) and the kernel's "binary responses would need a `-v2` field"
(kernel:http.rs:379-382, for the buffered path). Neither changes a signature; boundary §14 lists both amendments,
the second going through the kernel chain (K-Q17).

### 3.3 What this package takes from the boundary record

This package needs no mechanism of its own. The labels P-1…P-8 are kept from the first draft so that other records'
citations stay valid; each now points to where the boundary record states the rule.

| # | What the package needs | Where it is stated | Used in |
|---|---|---|---|
| P-1 | `request_facts.stream: "none"` — no stream switch, a 2xx is always a stream | Boundary §7.2 | §8.2 |
| P-2 | `request_facts.output_cap: []` — no cap location; `RequestFactsHonoured` for an empty list is the cap mutation check | Boundary §7.2, §7.6; the cap rule §6.3 | §8.3, §11.2 |
| P-3 | The `user_agent` value | Boundary §10 (`DeclaredUserAgentV1`), Q15, Q16 | §4.6 |
| P-4 | A host-minted attempt id, fresh per upstream attempt, for `conversationId` | Open: boundary Q14 (per-request case); K-Q16 | §4.5 |
| P-5 | The buffered path: an `aws-eventstream` package whose family declares `stream: "none"` gets its 2xx eventstream body deframed whole and handed to `parse-response` as the canonical re-encoding | Boundary §5.2; the kernel comment amendment in §14 | §5.4 |
| P-6 | Recipe vocabulary: `http_exchange` with a JSON body, recipe-level `select` with presence predicates, `requires`, `write_back` with an `optional` extraction, `on_status` class keys, import `seed`, field `default`, `endpoint_params`, value syntaxes `aws_region` and `aws_arn` | Boundary §3.3; trust §3.4; host invariants §3.5; host executor §3.6 | §7 |
| P-7 | Package-level `usage_evidence: absent`, the all-zero `usage` rule and the gate ② check `AbsentFamilyEmitsNoUsage` | Boundary §6.2 items 2 and 4; the WIT comment amendment in §14 | §6, §11 |
| P-8 | Probe rules: (a) a probe never forces a refresh on a recipe that rotates; (b) a package that exports no catalog world gets no upstream liveness request | (a) boundary §3.6; (b) **this record's proposal**, building on the catalog world of boundary §11 | §7.6 |

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
  "stream_framing": "aws-eventstream",
  "usage_evidence": "absent",
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

`stream_framing` and `usage_evidence` are package-level scalars (boundary R6: the response-side functions receive no
configuration, so they cannot be told a family); `request_facts` and `user_agent` are per family (boundary §7.2,
§10). The `user_agent` value is the one the host sends today. `config_schema` (boundary §7.3) is empty: the
component reads no operator configuration key; the profile ARN is a credential field (§7.5), not a configuration
key. The inference endpoint is the provider row's `base_url`; the host's default for it is
`https://q.us-east-1.amazonaws.com` (server:gateway/src/infra/config/providers.rs:721), which becomes catalog data
(boundary §7.5).

## 4. Request mapping: IR → wire

### 4.1 The wire shape

As the host builds it today (server:leaf/translate_kiro.rs:121-156, 160-190, 225-264, 287-300), for a conversation
of a user turn, an assistant turn with two tool calls, and the two tool results:

```json
{
  "conversationState": {
    "chatTriggerType": "MANUAL",
    "conversationId": "<attempt id, §4.5>",
    "currentMessage": {
      "userInputMessage": {
        "content": "",
        "modelId": "<ChatRequest.model>",
        "origin": "AI_EDITOR",
        "userInputMessageContext": {
          "tools": [ { "toolSpecification": { "name": "…", "description": "…", "inputSchema": { "json": {} } } } ],
          "toolResults": [ { "toolUseId": "call_b", "content": [ { "text": "…" } ], "status": "success" } ]
        }
      }
    },
    "history": [
      { "userInputMessage": { "content": "<system text>\n\n<first user text>", "origin": "AI_EDITOR" } },
      { "assistantResponseMessage": { "content": "…",
                                      "toolUses": [ { "toolUseId": "call_a", "name": "…", "input": {} },
                                                    { "toolUseId": "call_b", "name": "…", "input": {} } ] } },
      { "userInputMessage": { "content": "", "origin": "AI_EDITOR",
                              "userInputMessageContext": { "toolResults": [ { "toolUseId": "call_a", "…": "…" } ] } } }
    ]
  },
  "profileArn": "<exported attribute, when the credential has one>"
}
```

`userInputMessageContext`, `toolUses`, `history` and `profileArn` are omitted when empty, as today
(server:translate_kiro.rs:126-135, 147-155, 173-175, 182-187). The descriptor is
`POST {base_url}/generateAssistantResponse` with `Auth::Bearer { provider_api_key }`.

### 4.2 Mapping

The native arm's layout is the composition of two hops: the Anthropic reference's `conversation_of`
(crates/south-component-conformance/src/reference_anthropic.rs:197-263), which maps **each IR message to one
Anthropic message and merges nothing**, and the host's second hop, which maps each Anthropic message but the last to
one history entry (server:translate_kiro.rs:72-75, 160-190). The component reproduces that composition (D11):

| IR (`ChatRequest`, kernel:chat.rs:197-217) | Wire |
|---|---|
| `model` | `currentMessage.userInputMessage.modelId`, verbatim (§4.4) |
| `System` messages, wherever they occur | Their text — a text content, or the non-empty text parts — collected in order and joined by `\n` (reference_anthropic.rs:77-89, 315). The result is prepended as `<system>\n\n<content>` to the first `userInputMessage` of `history`, or to the current message when `history` has none; when that content is empty the system text replaces it with no separator (server:translate_kiro.rs:88-113) |
| Every non-system message but the last | One `history` entry each, in order |
| The last non-system message | `currentMessage.userInputMessage`; it must be a `User` or `Tool` message (§4.3) |
| `User` message | `userInputMessage { content, origin }`; `content` is its text parts concatenated with no separator (server:translate_kiro.rs:210-221) |
| `Assistant` message | `assistantResponseMessage { content, toolUses? }`; `content` is its text parts concatenated, `""` when there are none; `toolUses[] { toolUseId ← id, name, input ← arguments parsed as a JSON object }` in call order |
| `Tool` message | Its own `userInputMessage { content: "", origin, userInputMessageContext: { toolResults: [ { toolUseId ← tool_call_id, content: [{ text }], status: "success" } ] } }`. Consecutive tool results are **not** merged; a following `User` message is its own entry. `text` is the text parts concatenated (reference_anthropic.rs:161-175) |
| `tools` | `currentMessage.userInputMessage.userInputMessageContext.tools[] { toolSpecification { name, description?, inputSchema: { json ← parameters } } }` — only the current message carries tools |
| `tool_choice: auto` or absent | Nothing to write |
| `tool_choice: none` | `tools` withheld from the body — as the Anthropic reference does (reference_anthropic.rs:274-276) and the Converse package does (fixture `provider.request.tool-choice-none-withholds-the-whole-config`) |
| Attempt id (§4.5) | `conversationId` |
| Exported `profile_arn` (§7.5) | Top-level `profileArn`, when present |
| `stream` | Ignored: the wire has no switch (§8.2) |

Whether the upstream would prefer consecutive tool results merged into one entry is unknown; nothing in the host's
code or tests shows a capture of a multi-result turn. Version 1 keeps the native layout; K-Q18 asks the dual-run
captures to settle it.

### 4.3 Lossy and refused cases

"Native" is what the host arm does today for the same input (both hops).

| Input | Component | Native today |
|---|---|---|
| Image part (`ContentPart::ImageUrl`) | **Refused**: capability error, 400 before admission | Refused, but late: inside prepare, after the token refresh (server:translate_kiro.rs:80-84, 163-167; Appendix D N-3) |
| `ContentPart::Unknown` | **Refused**: capability error | Passed to the intermediate body verbatim (reference_anthropic.rs:118-120), then dropped unless it is a `text` block (server:translate_kiro.rs:210-221) or refused if it is an `image` block |
| `Thinking` / `RedactedThinking` parts in earlier turns | **Dropped** — the wire has no block for them; the same rule as the Converse reference for parts a dialect cannot carry out (reference_bedrock_converse.rs:109-114) | Dropped by the second hop |
| `response_format` other than text | **Refused**: capability error; the package does not declare `json_schema` | Never reaches the upstream (the second hop reads only `messages`, `system`, `tools`) |
| `tool_choice: required` or a named tool (`ToolChoice::Other`) | **Refused**: capability error — the wire cannot force a call | Silently dropped |
| Final message is an assistant turn (prefill) | **Refused**: capability error | Sent as the current **user** message (server:translate_kiro.rs:77-86 reads the last message without checking its role) |
| `ToolCall.arguments` that is not a JSON object | **Refused**: invalid request | Sent: a non-object JSON value as is, unparseable text as a JSON string (reference_anthropic.rs:191-192) |
| A tool call with an empty `id` or `name`; a `Tool` message with no `tool_call_id` | **Refused**: capability error | Refused by the first hop (reference_anthropic.rs:177-186, 249-254) |
| Empty `messages` | **Refused**: invalid request | Refused (server:translate_kiro.rs:62-68) |
| `sampling.temperature`, `top_p`, `stop` | **Dropped** — no slot on the wire; documented, and pinned by a fixture | Dropped |
| `sampling.max_output_tokens` | **Not written** — no slot on the wire (§8.3) | Validated on the intermediate body, then not sent (Appendix D N-1) |
| A tool result that reports an error | **Lossy**: `status` is always `"success"`. The IR has no field for a tool-result error (K-Q14) | Also always `"success"`: the second hop reads `is_error` (server:translate_kiro.rs:252-256), but the first hop never writes it (reference_anthropic.rs:255-261) |
| Operator request extras (body) | Applied by the host to the component's body, i.e. to the real upstream shape | Applied to the intermediate body; keys other than `messages` / `system` / `tools` never reach the upstream (Appendix D N-10) |

Refusals that differ from native behavior are intentional differences for the dual run (§12.2). Following the
practice of the embeddings record and of server P21 §9, the native arm is corrected first where the difference
is a defect of the native arm (Appendix C, step K0).

### 4.4 Model id

The host rewrites the configured upstream id before sending: a trailing `-<digits>-<digits>` becomes
`-<digits>.<digits>` (`kiro_model_id`, server:leaf/translate_kiro.rs:34-47). The component does not: it writes
`ChatRequest.model` verbatim, and the operator's model row (or the family's catalog, boundary §7.5) carries the
upstream's own spelling. Reasons:

- The rule is a guess about spelling, and it fires on any id that ends in two numeric segments (Appendix D N-4).
- `request_facts.model` lets the host check the body value against the IR model value by value (boundary §7.2); a
  component that rewrites the id fails that check by design.

Before cutover the host rewrites the model rows with the native function's own output (Appendix C, K0); the
function is idempotent for ids already dotted (test at server:translate_kiro.rs:843-856), so dual-run bodies are
then identical in this field.

### 4.5 The conversation id (P-4; route undecided)

The host writes a fresh random UUID into `conversationId` on every request (server:translate_kiro.rs:139-142). A
component cannot mint one: gate ② runs every case twice and requires identical output
(crates/south-component-conformance/src/suite.rs:368-379). The value has to come from the host.

A reserved key in `ChatRequest.extensions` would collide with S0 D5, which is ruled: components "must not *behave*
on an extensions key", and the promotion path for one they need is a typed IR field through the kernel chain
(canonical-ir-inventory.md:249-255; the policy fence at :155-164). The route is boundary Q14, which covers this
per-request case as well as the per-provider values (credential attributes, configuration keys); K-Q16 records
what this package needs from the answer:

- **Recommended there — a typed field.** For this package: an optional attempt identifier the host fills on every
  provider-world call, a UUID (RFC 4122 version 4, lowercase, hyphenated), minted fresh for each upstream attempt, so
  a failover attempt gets a new one. Provider-agnostic; this package is the first that reads it. It goes through
  D5's own promotion path (community protocol release → kernel sync → `schema_id` bump).
- **Alternative — an argued amendment of the fence and D5.** The precedent is the task world, where the host hands
  the component a host-minted identifier through a typed contract (`HostMintedValuesV1`,
  crates/south-contracts/src/task.rs:111-114) — which argues for the typed route.

Either way, **the host strips client-supplied keys that collide with any reserved name** (boundary Q14) before it
builds the IR's call: the north codec copies every top-level client key it does not model into `extensions` verbatim
(south-north-codec/src/request.rs:116-124), so without stripping a client could choose its own `conversationId`.
The component refuses to build a request when the attempt id is absent (boundary R5: no silent third outcome),
unless K-Q11's capture shows the upstream accepts a request without `conversationId`, in which case the component
omits the field and P-4 is not needed by this package. Deriving the id from the request content is rejected (§14).

### 4.6 Client identification (DP7, ruled 2026-09-30: south takes them in; the host keeps no special case)

**On the inference request** — written by the component:

| Item | Value today | Channel | Closed-vocabulary instance? |
|---|---|---|---|
| `x-amz-target` | `AmazonCodeWhispererStreamingService.GenerateAssistantResponse` (server:…/engine/upstream.rs:331, 348) | Ordinary descriptor header | No |
| `amz-sdk-request` | `attempt=1; max=1` (server:upstream.rs:349) | Ordinary descriptor header | No |
| `x-amzn-kiro-agent-mode` | `vibe` (server:upstream.rs:350) | Ordinary descriptor header | No |
| `accept` | `application/vnd.amazon.eventstream` (server:…/engine/kiro.rs:70) | Ordinary descriptor header; it replaces the transport's default `*/*` (crates/south-transport-reqwest/src/lib.rs:533-539) | No |
| `user-agent` | `aws-sdk-js/1.0.0 KiroIDE` (server:upstream.rs:333, 351) | **Reserved**: see below | **Yes** |
| Body `origin` | `AI_EDITOR` on the current message and on every user entry of `history` (server:translate_kiro.rs:26, 125, 180) | Body | No |
| Body `chatTriggerType` | `MANUAL` (server:translate_kiro.rs:138) | Body | No |

None of the three `x-amz*` / `amz-*` names is reserved: south reserves only the signing headers
`x-amz-content-sha256`, `x-amz-date` and `x-amz-security-token` (crates/south-contracts/src/lib.rs:232-256), and
the kernel's `SafeHeaders` refuses only credential headers and transport-owned names (kernel:http.rs:262-296;
kernel:lib.rs:86-96).

`content-type` is not sent today: the request is built with a raw byte body and no explicit content type
(server:kiro.rs:66-72; server:upstream.rs:345-352), and the transport sets none for a JSON descriptor body
(south-transport-reqwest/src/lib.rs:540-552). The component leaves it out as well, so that the dual run compares
equal; whether it should be sent is K-Q10.

**The `user-agent` value is the one closed-vocabulary instance this package needs (P-3).** The name is on south's
reserved list, so a descriptor cannot carry it. The only channel is `ControlledUserAgentV1`, which stores a
`&'static str`, is `Copy` and returns `&'static str` — by the 2026-08-20 ruling (its D1), a value must exist in
host program text. Today the host picks values from a table keyed on provider type
(server:…/engine/south_adapter.rs:260-281); Kiro is not in that table, and its value is set by the native arm's
auth helper (server:upstream.rs:340-352). Under the ruling on Q1, a new value compiled into the host is a host
change.

This record carries no user-agent mechanism of its own; a constructor on `ControlledUserAgentV1` would not do, since
the type's representation and its `as_str` signature would have to change or the string be leaked. Boundary §10
states the one proposal: a new owned type, `DeclaredUserAgentV1`, constructed only from a manifest value that passed
gate ① against the existing value grammar, with a fuzz obligation on its parser. That proposal explicitly reopens the
2026-08-20 ruling (boundary Q15) and asks whether south publishes impersonation values at all (boundary Q16; the
2026-08-20 record declined to "encode host impersonation policy into this library", controlled-user-agent.md:76-80).
This package declares its value through that proposal and depends on it (phase B7a, §12.1). If the south maintainers
decline Q15 or Q16, this package cannot meet DP0, and the conflict with lv's Q10 ruling goes back to lv, as boundary
Q10 says.

**On the token-exchange request** — written by the recipe: nothing. The host sends only
`content-type: application/json` on both forms (server:…/engine/token_refresh.rs:1356-1362), which the recipe's
`encoding: json` produces.

**Forwarded by the host, not by the component**: `x-request-id`, `traceparent` and the span headers
(server:kiro.rs:71-78). These are host-generic and stay in the host.

**Other closed vocabularies** (boundary §10): this package needs no secret header (it is on the bearer arm), no
controlled query parameter, and no quota header — the host applies no provider-specific rate-limit header parsing
to this upstream today (the generic snapshot at server:…/engine/text_admission/sender.rs:3492-3504 is all there
is). If a capture finds rate-limit headers worth normalizing, they are declared by the package per boundary §10,
not added to `ProviderQuotaMetadataFieldV1`.

## 5. Response and stream mapping

### 5.1 What the component sees

The package declares `stream_framing: aws-eventstream`. The host deframes (prelude, both CRCs, the frame-length
bound) with the deframer south supplies (boundary §5.2: `deframe_aws_eventstream_v1` and `reencode_eventstream_v1`,
pure functions with golden vectors in `south-contracts`, under a fuzz obligation) and feeds `parse-stream-chunk` one
canonical SSE frame per message frame, the payload re-encoded as compact JSON:

```text
event: assistantResponseEvent
data: {"content":"Hel","modelId":"…"}

```

A frame whose `:message-type` is `exception` arrives as `event: exception:<:exception-type>`. A frame whose
`:message-type` is `error` — the other failure shape the host's deframer knows
(server:leaf/aws_eventstream.rs:191-198, which reads `:error-code`) — arrives as `event: error:<:error-code>` with
`data: {"message": …}` carrying the `:error-message` header, as boundary §5.2 specifies. The component treats any
event name starting with `exception:` or `error:` as an in-band failure (§5.2).

The component therefore contains an SSE frame splitter with a partial-frame tail — the same code shape as the
Converse reference (reference_bedrock_converse.rs:511-533, 745-777) — and no eventstream decoder. It must tolerate
any byte split (`StreamIncrementality`, suite.rs:287-325).

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
| `exception:<type>` or `error:<code>` | `StreamEvent::Error` with the envelope of §9; nothing is emitted afterwards |
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

The last two rules differ from the native arm, which completes both cases (§12.2).

Two limits of this wire follow, and the component cannot remove them:

1. **`Length` is never reported by the upstream.** An answer the upstream cut short is indistinguishable from one
   that ended. (`length` can still reach the caller when the host enforces the cap, §8.3.)
2. **A body cut exactly at a frame boundary looks complete.** The host's deframer catches a cut inside a frame
   (leftover bytes at end of body must be an error — a requirement on the deframer of boundary §5.2); nothing
   catches a cut between frames unless the dialect has terminal evidence.

`meteringEvent` may be that evidence: in every captured sequence in the host's tests it is the last event. If a
capture during the dual run confirms it is present on every completed answer, the component should require it —
EOF without it emits no `Done`, the rule the Converse reference applies to a stream that ends before `metadata`
(reference_bedrock_converse.rs:745-760). Until that is confirmed the component does not require it, which matches
the native arm. See K-Q5.

### 5.4 The non-streaming path (P-5; the buffered path of boundary §5.2)

The upstream answers a non-streaming request with the same event stream. Today the host buffers and decodes it
(server:kiro.rs:136-171) and serves non-streaming Chat callers from the fold.

**The buffered path (boundary §5.2).** The trigger is the package's `aws-eventstream` framing **and** the family's
`stream: "none"` (§8.2). For a 2xx response on such a family the host deframes the complete buffered body with the
same deframer and hands `parse-response` an `HttpResponseParts` whose `body` is the concatenated canonical
re-encoding. The rule is keyed on those two declarations only; the host never chooses the path by sniffing a content
type (boundary §5.2). A package whose non-streaming answer is JSON (Converse) is unaffected, because its family
declares a stream switch. `stream: "none"` tells the host before sending that the answer will be
binary, so it uses the buffered binary transport.

`parse-response` then runs the component's own stream parser over that text, flushes EOF, and folds the events:

- `choices[0].message.content` — the concatenated text; `None` when there is none.
- `choices[0].message.tool_calls` — one per tool-use id, in first-seen order, `arguments` being the concatenated
  fragments **verbatim** (the IR carries arguments as a string, kernel:chat.rs:72-76).
- `finish_reason` — as in §5.3. If the fold ends without a `Done` (no frames, or an open call), or meets an
  `exception:` / `error:` frame, the result is an error, not a response.
- `id` and `model` — left empty for the host to fill, as the Converse and Gemini references do
  (reference_bedrock_converse.rs:37-41).
- `usage` — all zeros (§6.1).

One parser serves both paths, so a streaming and a non-streaming answer to the same upstream bytes fold to the
same content.

**The kernel's doc comment.** `HttpResponseParts.body` is documented as text "because every provider modelled by
`v1` speaks JSON or SSE", and "binary responses would need a `-v2` field rather than a lossy encoding here"
(kernel:http.rs:379-382). The re-encoding is not an ad-hoc encoding of arbitrary bytes: it is the canonical,
south-defined transform the stream path already uses, with golden vectors, and it discards only framing the
component has no use for (the prelude, the CRCs and headers other than the type names). It is still a departure from
that sentence, so boundary §14 lists the comment for amendment through the kernel chain (K-Q17). No field or
signature changes.

**Why the Codex family differs** (boundary §5.2). The owner ruled that non-streaming callers of the Codex family
are refused at build time (Responses record, R-Q5). That family has a stream switch (`stream: {"body": "/stream"}`)
and SSE `bytes` framing, so it does not declare `stream: "none"`, and a non-streaming client maps to a request the
family itself refuses. This family has no switch: the upstream answers
every request with the same stream, and the native arm serves non-streaming callers today, so refusing them would be
a product regression. Both outcomes follow from declarations — `stream: "none"` selects the buffered path — and
neither names a provider. The alternative for this family — the host always drives the stream path and folds IR
events itself — is K-Q7.

### 5.5 Errors inside a 2xx

Exception and error frames are the in-band failures this record knows of (§5.1). Today the streaming path parks the
stream for manual review (server:sender.rs:4920-4929) and the non-streaming path answers 503 after moving the
request to `delivery_unknown` (server:kiro.rs:156-161; server:sender.rs:3977-3987). With the component, both paths
see `StreamEvent::Error` / an error from `parse-response`; what the host does with a failure after dispatch is
unchanged host policy. A deframer that did not re-encode `error` frames would let the component ignore them as
unknown events and settle the truncated answer as complete; that is why boundary §5.2 covers both frame kinds and
§11.2 pins both.

## 6. Usage

### 6.1 The component's side

The manifest declares the package-level `usage_evidence: absent` (boundary R6, §6.2 item 4). Consequences:

- `parse-stream-chunk` never emits `StreamEvent::Usage`.
- `parse-response` returns `usage` with every field zero, and the host never reads it (boundary §6.2 item 4).
  `ChatResponse.usage` is not optional (kernel:chat.rs:271-281), and the WIT's rule "a 2xx whose body cannot yield
  exact usage is an error, never a zero" (provider-adapter.wit:110-116) is written without a qualification;
  boundary §14 lists that doc comment for amendment so that it applies to `reported` packages (K-Q17). The
  amendment changes no signature.
- The credit total of `meteringEvent` does not cross the boundary. The IR has no field for it, `StreamEvent`
  variants carry no extensions (kernel:stream.rs:22-29), and the host discards it today (Appendix A.3).

### 6.2 The host's side (ruled: the host's generic estimator)

South does not define the estimator; Appendix B.1 states what the migration needs from it. Two points bear on
south's reading of this package:

- **It is a new host settlement mechanism.** The host's provider-agnostic output meter today
  (`ForwardedOutputMeter`, server:…/engine/text_admission/sender/checkpoint.rs:1-20, 41-100) parses forwarded SSE
  frames and feeds mid-stream checkpoint evidence, not settlement; nothing yet counts a non-streaming answer by the
  same rule. The first draft called it a meter "of exactly this kind"; it is a starting point, and the settlement
  meter gets its own gate ③ suite (§11.3).
- **Produced, not delivered.** The native arm counts what the upstream produced, as it decodes it (server:sender.rs:
  4896); a meter over forwarded frames counts what the client received. They differ when a client disconnects.
  Output is counted as produced: the settlement meter counts the IR events the component emitted from the
  upstream's answer — independent of the northbound surface and of the client's reading — not the frames delivered
  to the client (K-Q19, ruled by lv on 2026-10-01).

### 6.3 The host's generic checks that still apply

From boundary §6.3, for an `absent` package (every bound is the host's, computed from the northbound request):

| Check | Applies? |
|---|---|
| Settled amount ≤ reservation | Yes, unchanged |
| Exactly one terminal state; nothing after `Done` | Yes |
| `output_tokens` ≤ authorized cap × choices | Applies to the host's own estimate. Holds by construction once the host enforces the cap (§8.3, K-Q1 ruled); during the dual run it can fire on a long answer |
| `input_tokens` ≤ input bound | Vacuous: both sides are the host's own count from the northbound request |
| Cache and reasoning consistency | Vacuous: the buckets are zero |
| An `absent` package that emits `Usage`, or returns a non-zero `usage` | The package contradicts its manifest → manual review, never settled from that number |

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
is `minted`. `Auth::OAuth` is not used — the boundary record deprecates it in favor of minted slots (its §3.8).

### 7.2 Recipes

```json
"credentials": {
  "schema": "south.credential-recipe.v1",
  "fields": {
    "refresh_token": { "secret": true,  "required": false },
    "client_id":     { "secret": false, "required": false, "syntax": "token" },
    "client_secret": { "secret": true,  "required": false },
    "auth_method":   { "secret": false, "required": false, "syntax": "token" },
    "auth_region":   { "secret": false, "required": false, "syntax": "aws_region", "default": "us-east-1" },
    "profile_arn":   { "secret": false, "required": false, "syntax": "aws_arn" }
  },
  "import": {
    "kiro-auth-token.json": {
      "refresh_token": ["/refreshToken"],
      "client_id":     ["/clientId"],
      "client_secret": ["/clientSecret"],
      "auth_method":   ["/authMethod"],
      "auth_region":   ["/idcRegion", "/region"],
      "profile_arn":   ["/profileArn"],
      "seed": { "present": { "pointer": "/accessToken", "secret": true },
                "expires_at": { "rfc3339_or_epoch_seconds": "/expiresAt" } }
    }
  },
  "slots": { "provider_api_key": { "minted": "kiro" } },
  "recipes": {
    "kiro": {
      "select": [
        { "when": { "field_in": { "field": "auth_method",
                                  "values": ["idc", "enterprise", "iam_identity_center"] } }, "recipe": "idc" },
        { "when": { "field_present": "auth_method" },                                             "recipe": "social" },
        { "when": { "all_present": ["client_id", "client_secret"] },                              "recipe": "idc" },
        { "recipe": "social" }
      ]
    },
    "social": {
      "steps": [ {
        "id": "token", "kind": "http_exchange", "method": "POST", "encoding": "json",
        "endpoint": "https://prod.{region}.auth.desktop.kiro.dev/refreshToken",
        "endpoint_params": { "region": { "field": "auth_region" } },
        "params": { "refreshToken": { "field": "refresh_token" } },
        "on_status": { "400": "reauth_required", "401": "reauth_required", "4xx": "transient" },
        "extract": {
          "access_token":  { "pointer": "/accessToken",  "secret": true },
          "refresh_token": { "pointer": "/refreshToken", "secret": true, "optional": true },
          "expires_at":    { "relative_seconds": "/expiresIn" }
        }
      } ],
      "present": "token.access_token",
      "rotates_refresh_material": true,
      "write_back": { "refresh_token": "token.refresh_token" },
      "default_seconds": 3600,
      "refresh_margin_seconds": 300,
      "without_refresh_material": "fail",
      "attributes": { "profile_arn": { "field": "profile_arn", "export": true } }
    },
    "idc": {
      "steps": [ {
        "id": "token", "kind": "http_exchange", "method": "POST", "encoding": "json",
        "endpoint": "https://oidc.{region}.amazonaws.com/token",
        "endpoint_params": { "region": { "field": "auth_region" } },
        "params": {
          "refreshToken": { "field": "refresh_token" },
          "clientId":     { "field": "client_id" },
          "clientSecret": { "field": "client_secret" },
          "grantType":    { "const": "refresh_token" }
        },
        "requires": ["refresh_token", "client_id", "client_secret"],
        "on_status": { "400": "reauth_required", "401": "reauth_required", "4xx": "transient" },
        "extract": { "…": "as in social" }
      } ],
      "present": "token.access_token",
      "rotates_refresh_material": true,
      "write_back": { "refresh_token": "token.refresh_token" },
      "default_seconds": 3600,
      "refresh_margin_seconds": 300,
      "without_refresh_material": "fail",
      "attributes": { "profile_arn": { "field": "profile_arn", "export": true } }
    }
  }
}
```

Every value comes from Appendix A.4. The slot is minted by the selecting recipe `kiro`, whose four `select` rules
are the host's rule, in the host's order (server:token_refresh.rs:1189-1214), including "an explicit method wins over
shape" (test at :1799-1809). The recipe declares no expiry clamp: the host's executor always applies its
provider-agnostic clamp of 60 s – 24 h (boundary §3.5), which is today's clamp (:1374).

Three points where a recipe could break credentials that work today, and how this one avoids them:

- **No refresh token.** `refresh_token` is not required. A sign-in file without one imports; the seeded access
  token serves until it is within the margin of expiry, and then `without_refresh_material: "fail"` answers with a
  re-authorization error. That is today's behavior: the host accepts such a file
  (server:gateway/src/modules/admin_ui/handler/credentials/mod.rs:229-232), serves the stored token while it is
  fresh (the skeleton's step ②, server:token_refresh.rs:516-524), and only then fails (:1333-1346).
- **No rotated token in the response.** `write_back` names an **optional** extraction; when the response carries
  no `refreshToken`, the stored refresh token is kept. Today's write-back does the same: it updates `refresh_token`
  only when a value is present (server:gateway/src/modules/catalog/repo/credentials.rs:796-797). The host also keeps
  this as an invariant independent of any declaration (boundary §3.5; §7.6).
- **Which statuses are terminal.** Only 400 and 401 mark the credential as needing re-authorization today
  (server:token_refresh.rs:252-270). The boundary record's default maps every 4xx to `reauth_required`, so the
  recipe states 400 and 401 and maps the rest of the class to `transient`; 5xx keeps the default `transient`.

### 7.3 Which boundary §3.3 vocabulary the sketch uses (P-6)

Every form below is in the recipe vocabulary of boundary §3.3, under that name; this package adds none.

| Form | Why this package needs it |
|---|---|
| `http_exchange` step with `encoding: json` and declared `params` | Neither form is an RFC 6749 token request: both are JSON with camelCase keys, and the social form has no grant type (server:token_refresh.rs:1277-1294). `oauth2_token` keeps the RFC's parameter names, so these are `http_exchange` steps |
| `endpoint_params` with value syntax `aws_region` | The endpoints are region templates. The host has validated the same shape on its native path since `a82c852b` (server:token_refresh.rs:1221-1230, 1249-1262; Appendix D N-5) |
| Recipe-level `select`, predicates `field_in` (ASCII case-insensitive), `field_present`, `all_present` | The host's choice is a four-step decision, one step of which tests the **presence** of two fields, one of them secret; presence predicates may test secret fields, never their values |
| `requires` per step | Today the IdC form checks its client fields before any network call (server:token_refresh.rs:1347-1355) |
| `default_seconds` on `relative_seconds` | A missing `expiresIn` is treated as 3600 s (:1374) |
| `write_back` with an `optional` extraction | Rotation must name the field it replaces; an absent rotated token leaves the field unchanged |
| `on_status` with class keys `4xx` / `5xx`, an exact code winning | Only 400 and 401 are terminal today; the vocabulary's default makes every 4xx terminal |
| Import with ordered pointers | `idcRegion` wins over `region` |
| Import `seed` with clock form `rfc3339_or_epoch_seconds` | The sign-in file carries a usable access token and its expiry, which is an ISO-8601 string or an epoch number (server:credentials/mod.rs:234-243); a credential with no refresh token works today until that token expires (§7.2) |
| Field `default` | `auth_region` defaults to `us-east-1` (:1133, 1215-1220) |
| Value syntax `aws_arn` | `profile_arn` contains `:` and `/`; the value goes only into the JSON body |

### 7.4 Intentional differences from the native arm

- **No endpoint override.** `extras.refreshUrl` has no equivalent: the boundary record forbids taking a host
  from credential contents (its §3.3) and makes test endpoints a test-build feature (its §3.4 rule 6). The override
  is the test seam of `gateway/tests/kiro_refresh_lock.rs` and, by its own comment, an escape hatch for relays; the
  seam moves to the gate ③ suite's fake endpoint (boundary §3.7). It is
  reachable only through admin-entered extras — the sign-in file import copies six other keys
  (server:credentials/mod.rs:244-256). Dropping it is ruled (K-Q4).
- **5xx is transient.** The native strategy maps every non-2xx to a 400-class error (Appendix D N-6). Which
  statuses mark the credential terminal is unchanged (§7.2).
- **camelCase only.** The snake_case aliases the host accepts in the response are not carried over; the host's
  own comment says both endpoints answer camelCase (server:token_refresh.rs:1262-1264).
- **A missing field is a configuration error before any network call** in both forms (`requires`); today only
  the IdC form checks its client fields up front (:1347-1355).

### 7.5 Exported attribute

`profile_arn` is a static, non-secret credential field with value syntax `aws_arn`, exported as an attribute
(boundary §3.3); it is not a `config_schema` key. An attribute takes its value only from a field declared non-secret,
never from an exchange response (boundary §3.3, §3.4 rule 4); this one complies. The component writes it to
`profileArn` when present and omits it otherwise — exactly today's behavior (server:translate_kiro.rs:153-155).
Which form requires it is not settled by the host's code: one comment says it is required for social sign-in
(server:translate_kiro.rs:51-54), another that it is sent for IdC and intentionally omitted for social
(server:token_refresh.rs:1151-1166). The component takes no position; it forwards what the credential has.

How the attribute reaches the component is boundary Q14 — the same S0 D5 question as the attempt id of §4.5. This
package reads the attribute through whatever route that question settles.

### 7.6 Trust, host invariants and probes

The recipe is data the host executes; the manifest is untrusted input. What this package relies on from the
boundary record's trust model (§3.4) and host invariants (§3.5):

- **Endpoints are confirmed** (§3.4 rule 1, default per Q18). The two endpoint templates
  (`prod.{region}.auth.desktop.kiro.dev`, `oidc.{region}.amazonaws.com`, with `region` restricted to `aws_region`)
  are shown to the operator per package digest when the credential is created, or are on a host-side allowlist.
- **First-party only until signing** (§3.4 rule 5). The host enables recipes only for packages it verifies as south
  first-party releases by digest against the release index; this package is published by south, which is why it
  needs the release index (phase B3, §12.1).
- **Refresh material is never lost** (§3.5). The host never overwrites non-empty refresh material with an empty
  value, whatever a recipe declares, and keeps the previous generation so a wrong rotation can be rolled back.
- **One lock per credential across the migration.** The host's refresh lock is named `"{prefix}:{credential id}"`
  (server:token_refresh.rs:535) and the native Kiro strategy's prefix is `kiro-refresh` (:1311-1313). While native
  code that can mint for a Kiro credential still exists — the probe special case and the routed-dispatch path
  (Appendix A.5, A.6) — and the recipe executor also mints for it, both must take **the same lock**; otherwise two
  refreshes of one single-use rotating token can run at once, and CAS write-back protects only the database, not
  the upstream spend. Step K0 therefore renames the native prefix to the executor's name before any row is routed
  to the package (Appendix C.1), and gate ③ pins it (§11.3).
- **Probes (P-8).** Two rules, keyed on two different declarations. (a) A probe never forces a refresh on a recipe
  that declares `rotates_refresh_material: true` (boundary §3.6); once the stored token has expired, the probe runs
  the real refresh and reports its result. (b) **This record's proposal**: a package that exports no catalog world
  (boundary §11) gets no upstream liveness request; its health reflects the credential state only. Together they
  are today's special case (server:…/engine/health_probe.rs:323-335, 353-421), which makes no network call while
  the token is unexpired because this upstream has no free liveness request. Keying both on rotation would silence
  the liveness request of a future rotating provider that has one.

Appendix B.2 lists what else the host's executor inherits.

## 8. Request facts

### 8.1 Model

`{"body": "/conversationState/currentMessage/userInputMessage/modelId"}`. The host compares it with the IR model
value; §4.4 makes them equal by construction.

### 8.2 Stream (P-1)

The body has no `stream` key and the URL does not vary, so the family declares `stream: "none"` (boundary §7.2):
the dialect has no stream switch; the request is the same for both modes; **a 2xx is always a stream**. Without the
declaration, the absent default means the top-level `stream` field, under which a streaming request with no such
field fails the host's check (server:…/engine/text_admission.rs:916-938 shows today's provider-keyed exemption).

The host then (a) checks that no switch appears, which is vacuous here, and (b) serves a non-streaming northbound
request through the buffered path of §5.4. Gate ① refuses a package in which a family declares `stream: "none"`
unless the package's `stream_framing` is other than `bytes`, since only a declared framing lets the host turn the
buffered answer into text for `parse-response`.

### 8.3 Output cap (P-2)

The body has no cap field the host knows of, so the family declares `output_cap: []` (boundary §7.2): this dialect
cannot be told a cap, and the host's seal check has nothing to find in the body. Gate ② checks the declaration with
`RequestFactsHonoured` as boundary §7.6 defines it for an empty list (§11.2).

**Enforcement (K-Q1, ruled; boundary §6.3).** For every family that declares `output_cap: []` in a package with
`usage_evidence: absent`, the
host enforces the authorized cap on its own output meter and ends the answer with `length` — after cutover; during
the dual run both arms keep today's behavior (nothing bounds the output; a settlement above the reservation goes to
manual review). The ruling leaves two cases to be specified; this record proposes:

- **Streaming, cut inside a tool call.** The cut takes effect at the first IR event boundary at which the meter
  reaches the cap. A tool call already open is delivered as far as it was streamed, and the answer ends with
  `length` — the same thing a caller sees from any upstream that stops at its token limit inside a call.
- **Non-streaming, the buffered path.** The host holds the whole body, so "stop reading" means cutting the folded
  answer: text is kept up to the point where the estimate reaches the cap, a tool call that is not complete within
  the cap is removed rather than delivered with partial arguments (a non-streaming caller cannot use half an
  argument string), and `finish_reason` is `length`. Settlement counts what is delivered after the cut.

Both are host behavior selected by declaration and pinned by the gate ③ row "Host-enforced cap" (§11.3).

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
  For an in-band `exception:` / `error:` frame it is the payload's `message` under the same rule, or the type name.
- A 500 from this upstream does not always mean the upstream is unhealthy: the host's own comment records that a
  malformed body is answered with an opaque 500 (server:kiro.rs:88-93). The component cannot tell the two apart from
  the response; conformance (§11) is what keeps the component from producing malformed bodies.
- Signals the host needs and gets: the code (and through it `is_retriable_elsewhere`, kernel:error.rs:72-87),
  `retry_after_ms`, and the HTTP status. Credential-level signals come from the recipe: `reauth_required` on a
  400 / 401 from the token endpoint — the host's terminal marker today (server:token_refresh.rs:252-270) — and
  `transient` otherwise. A mint failure is a pre-admission error that moves no money and lets the host try the
  next credential, as today (server:sender.rs:2648-2655).
- Whether exhaustion of the subscription's allowance has a distinguishable status or exception name is unknown;
  if it does, it maps to `payment_required` or `rate_limit` (K-Q6).

## 10. What the host's generic path is expected to do

Moved to Appendix B.3. In short: nothing in the host's path names the provider. The host routes the row to the
package, mints per the recipe, supplies the attempt id and the exported attribute, calls `build-http-request`,
admits the descriptor and the request facts, sends with the declared user-agent, deframes per `stream_framing`,
parses, renders, meters, enforces the cap and settles from its own estimate.

## 11. Conformance

### 11.1 Gate ①

This package relies on the gate ① validation the boundary record defines, and adds nothing to it: the `user_agent`
value (§10); `request_facts` including `stream: "none"` and `output_cap: []` (§7.2); the recipe structure — `select`
rules, `write_back` when rotating, `on_status` keys, `requires`, `http_exchange` parameters, import pointers and
`seed`, and the trust rules (§3.3, §3.4, §3.7). The one consistency rule this package depends on is stated at package
level: a package in which any family declares `stream: "none"` must declare a package-level `stream_framing` other
than `bytes`, since that family's 2xx is always a stream (§8.2).

### 11.2 Gate ② — fixture pack `fixtures-kiro/`

Stream and response inputs are written in the canonical re-encoding, as boundary §5.4 requires of
`aws-eventstream` packages; response fixtures use it too, because of the buffered path (P-5).

| Row | Asserts |
|---|---|
| `request.chat` | Single user turn → the §4.1 shape; headers; bearer slot; `conversationId` from the attempt id |
| `request.system-joins-the-first-user-entry` | With and without `history`; a first user entry with empty content receives the system text alone |
| `request.tools-and-parallel-tool-results` | `toolSpecification`, `toolUses`; two tool results become two user entries, the last one the current message (§4.2) |
| `request.tool-result-then-user` | A `User` message after a tool result is its own entry |
| `request.tool-choice-none-withholds-tools` | §4.2 |
| `request.profile-arn-from-exported-attribute` / `request.no-profile-arn` | Present and omitted |
| `request.sampling-has-no-slot` | `temperature`, `top_p`, `stop`, `max_output_tokens` leave no trace in the body |
| `request.model-is-verbatim` | A dashed id is **not** rewritten |
| `request.refused-*` | One row each: image, unknown part, `required` tool choice, assistant-final turn, non-object arguments, empty tool-call id, tool result without `tool_call_id`, missing attempt id → the stated error code |
| `response.text` / `response.tool-use` | The fold of §5.4; `usage` all zeros; `id` and `model` empty |
| `response.no-frames` / `response.open-tool-call` / `response.exception-frame` / `response.error-frame` | An error, not a response |
| `stream.text` / `stream.tool-use` / `stream.text-then-tool` | The events of §5.2 and the EOF flush of §5.3 |
| `stream.metering-is-never-usage` | A `meteringEvent` is present; **no `Usage` event** in the output |
| `stream.unknown-event-ignored` / `stream.empty-content-ignored` | §5.2 |
| `stream.exception-frame` / `stream.error-frame` | `event: exception:<type>` and `event: error:<code>` → `Error`, and nothing after |
| `stream.open-tool-call-at-eof` / `stream.no-frames` | No `Done` |
| `error.rejected-credential` (401, 403) / `error.throttled-carries-retry-after` / `error.opaque-500` | §9 |
| `capabilities.declared` | Echoes the operator's models, as the other packages do (reference_bedrock_converse.rs:791-798) |
| `credential.social` / `credential.idc` / `credential.select-*` / `credential.rotation` / `credential.rotation-absent-keeps` / `credential.no-refresh-token-seeded` / `credential.expiry-default` / `credential.status-400` / `credential.status-403` / `credential.status-500` / `credential.idc-missing-client-fields` | The rendered exchange request and the extraction (boundary §3.7), including each of the four `select` rules; an absent rotated token yields no rotation output; 403 is `transient`; missing IdC client fields fail with no request rendered |

**Checks.** The existing eight apply unchanged (report.rs:19-64). From the boundary record:
`DescriptorAuthWithinManifest` (§4.4), `RequestFactsHonoured` (§7.6 — for this family: body-form model, no `stream`
key, and the empty-cap-list check as §7.6 defines it) and `AbsentFamilyEmitsNoUsage` (§6.2 item 4 — no stream
fixture's output contains `Usage`, every response fixture's `usage` is all zeros; it replaces the by-name usage rows
and `UsageNeverDefaulted`, which apply to `reported` packages).

**Mutation checks** specific to this pack, run by the suite on its own fixtures:

- *Metering inflation*: multiply the `meteringEvent` value by 1,000 in every stream and response fixture; the
  output must be unchanged. The component must be deaf to the number.
- *Attempt-id sensitivity*: change the attempt id; exactly one body value may change, at
  `/conversationState/conversationId`. Its input location follows boundary Q14 (K-Q16).
- *Frame reordering inside a tool call*: swap two `input` fragments; the concatenated arguments must change
  accordingly — the component must not reorder or deduplicate.

**Release discipline.** Boundary §6.2 item 5 requires a documentation-derived usage judge for each package south
publishes. This upstream has no public documentation of its wire known to this record, and the package reports
no usage; the judge is replaced by `AbsentFamilyEmitsNoUsage` and the dual run. Request and event shapes rest on
captures, which the release must archive as redacted fixtures with their capture date.

### 11.3 Gate ③ — what the host must prove

| Host suite | Proves |
|---|---|
| `south.credential-recipe.v1` (boundary §3.7) | Both recipes against a fake token endpoint: rotation write-back; a response without a rotated token keeps the stored one; the never-overwrite invariant holds even for a recipe that would write an empty value; eight concurrent callers hit the endpoint once and all get the new token (today's `kiro_refresh_lock.rs`, made generic); CAS loser re-read; no retry on `reauth_required`; 403 does not mark the credential terminal; the host's own expiry clamp applied with no recipe clamp declared; `select` evaluated from the authoritative row |
| One lock across the migration | A native mint path and the recipe executor minting for the same credential at once reach the token endpoint once (K2 only; the row retires at K3) |
| Eventstream deframer adoption (boundary §5.2, §5.4) | Frames split across chunks, both CRC errors, an oversized frame, an `exception` frame, an `error` frame, **leftover bytes at end of body**, and the buffered whole-body form of P-5 |
| Attempt id (P-4) | Present on every provider-world call; fresh per attempt; a UUID; a client-supplied key colliding with a reserved name never reaches the component |
| Declared user agent (P-3) | The wire carries exactly one `user-agent`, equal to the manifest value; the host has no per-provider table |
| Absent usage (§6) | A settled row carries both estimated flags; streaming and non-streaming answers to the same upstream bytes settle to the same token counts, output counted as the IR events produced (K-Q19, ruled 2026-10-01); a package that emits `Usage` despite `absent` goes to manual review |
| Refusal before admission | A capability error from `build-http-request` produces a 400, zero upstream calls, zero token refreshes beyond the one already needed, and no credential failover |
| Probe rules (P-8) | A probe of a rotating recipe with an unexpired token forces no refresh; a package without a catalog world gets no liveness request |
| Host-enforced cap (K-Q1, ruled) | Streaming: the cut at an event boundary, `length`, a partial tool call delivered as streamed. Non-streaming: text cut at the cap, an incomplete tool call removed, `length`; settlement counts what was delivered |

T21 (boundary §12) covers these declarations without this package installed: `t21-unseen-eventstream`
(`aws-eventstream`, `stream: "none"`, `exception` and `error` frames, eventstream answers to non-streaming requests)
and `t21-unseen-absent` (`usage_evidence: absent`, `output_cap: []`).

## 12. Migration and dual run

### 12.1 Steps

Four steps; Appendix C.1 gives their content and acceptance. K0 (host) corrects native defects the dual run would
otherwise pin, moves the native arm to the generic estimator (K-Q2, ruled), rewrites dashed model ids and renames the
native refresh lock (§7.6). K1 (south) ships the package; it depends on boundary phases B1 (`usage_evidence`), B2
(`stream_framing`, `request_facts`, descriptor auth admission, the deframer), B3 (`runtime_abi` and the release index:
the package declares `runtime_abi`, and recipes are enabled only for first-party packages verified against the index,
boundary §3.4 rule 5), B4 (recipes) and B7a (south-side instance declarations, which carry the user-agent value;
boundary §13 places B7a before B6 for this reason), and on the answer to boundary Q14 (K-Q16). K2 (host) routes rows
to the package and runs the dual run. K3 (host) deletes the native arm (Appendix C.2).

### 12.2 Dual-run reconciliation

**Harness.** The model's answer is not reproducible, so two live calls cannot show equal amounts, and a live token
exchange spends a single-use refresh token in one arm. Items 1–6 therefore run with both arms configured against:

- a **replaying fake upstream** that answers from recorded eventstream bodies (captured live, redacted, archived
  with the fixtures), selected by the request body with `conversationId` masked; and
- a **fake token endpoint** for both forms, answering from recorded responses, including one without a rotated
  token and the failure statuses of item 6.

Live traffic serves only item 7, one arm at a time, and is not compared.

**Paths.** The same northbound Chat request goes through the native arm and the component arm on both send paths the
native arm has — the direct Chat send (streaming and non-streaming) and the routed-dispatch path that routing groups
use (server:…/engine/dispatch/mod.rs:585-607) — once under each billing form (`balance`, `quota`). Compared item by
item:

1. **Upstream request**: method, URL, the non-auth headers of §4.6 (including the absence of `content-type`), and
   the body as JSON — equal except `conversationId`, which differs by construction and is compared for shape.
2. **Token exchange**: for an expired credential of each form, the exchange request (URL, body) and the state
   written back, including the response without a rotated token.
3. **Northbound response**: non-streaming — content, tool calls, finish reason; streaming — the sequence of
   deltas and the terminal chunk. `id` values are host-minted in both arms and compared for shape.
4. **Reservation**: equal. It is a function of the model row and the authorized cap only
   (server:…/engine/token_counter/authorize.rs:830-954), so neither arm's translation can move it.
5. **Settlement**: settled token counts, amount, `tokens_estimated`, `quantity_estimated` — equal, since K0 moves
   the native arm to the generic estimator first (K-Q2, ruled). Against a replaying upstream both arms see the same
   bytes, so equal counts are a real proof, not a coincidence.
6. **Failures**: 401, 429 with `retry-after`, 500, an exception frame and an error frame mid-stream, a body cut
   inside a frame, an expired token whose refresh answers 400, 403 and 500.
7. **Captures**: at least one recorded live answer per shape (text, tool call, text then tool call, a multi-result
   tool turn), archived as fixtures; the capture also answers K-Q5, K-Q6, K-Q10, K-Q11 and K-Q18.

**Intentional differences** (on record, not reconciliation failures):

- The refusals of §4.3 marked as differing from native, unless K0 has already corrected the native arm: `Unknown`
  parts, `required` or named tool choice, an assistant-final turn, non-object tool arguments.
- A 2xx with no frames: the component returns an error; the native arm answers an empty success
  (server:translate_kiro.rs:632-634, 641 on the non-streaming path; `finish()` at :545-565 on the streaming path).
- A tool call open at end of body: the component emits no `Done`; the native arm completes the call (:545-565).
- A non-JSON event payload on the non-streaming path: the component returns an error; the native decoder turns it
  into `null` and the fold ignores it (server:kiro.rs:147; Appendix D N-7).
- Unparseable tool arguments on the non-streaming path: the component passes the concatenated string; the native
  arm replaces it with `{}` (server:translate_kiro.rs:627-631).
- A refresh answered with 5xx: `transient` versus a 400-class error (§7.4).
- A request-shape refusal no longer rotates through credentials (Appendix B.3 step 3; Appendix D N-3).
- Operator body extras reach the upstream body (§4.3, last row).
- The model id is no longer rewritten (§4.4); with the rows rewritten in K0 the bodies are equal.

**Surfaces.** The dual run is on Chat only — the native arm has no other surface to compare with. Messages and
Responses are accepted separately, against the package's fixtures and a live capture (K-Q3, ruled).

### 12.3 What retires in the host

Moved to Appendix C.2: about 2,700 lines, of which about 2,000 are not test code.

## 13. Versioning

- **South**: a minor release. It adds one package, one reference implementation and one fixture pack. Every
  manifest field and contract it uses is the boundary record's and ships with that record's phases (§12.1); the
  buffered path is a convention recorded under `host_capabilities` in `compatibility.json` once a host adopts it.
- **No world, WIT signature or kernel type change.** Two normative doc comments change, both listed in boundary §14
  (K-Q17): the WIT's usage rule at provider-adapter.wit:110-116 (a south change that bumps no world) and the
  kernel's `HttpResponseParts.body` comment at http.rs:379-382 (through the kernel chain). The recommended answer
  to boundary Q14 is a kernel change: a typed field. Until Q14 is answered, version 1 cannot ship unless K-Q11's
  capture shows the upstream accepts a request without `conversationId`. K-Q14 is a kernel question that does not
  block version 1.
- **Host link layer**: one upgrade to a south that knows the new manifest fields and ships the deframer, the
  recipe interpreter's types and the declared user-agent type. After that, changing this provider's headers, body
  markers, endpoints, exception table or recipes is a package release; the host changes nothing (DP0, with the
  Q1 ruling). A change of the recipe endpoints needs the operator's confirmation again (§7.6).
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
- **Merge consecutive tool results into one user entry in version 1** (the first draft's rule). No capture shows
  the upstream prefers it, and the dual run needs the native layout to compare bodies; it waits for K-Q18.
- **Derive `conversationId` inside the component from the request content.** It would be deterministic, but two
  unrelated requests with the same opening message would share an id on the same account, and what the upstream
  does with a repeated id is unknown. Today's behavior is a fresh id per request; P-4 keeps it.
- **Carry the attempt id in `extensions` without amending D5** (the first draft). A component acting on an
  `extensions` key is what S0 D5 rules out; K-Q16 asks for the typed field or an argued amendment.
- **Convert credits to tokens in the component and report them as `Usage`.** The host already abandoned the
  credit-derived figure for billing (server:translate_kiro.rs:323-374 keeps it "only for legacy display"); the
  conversion needs prices, which do not enter south (ARCHITECTURE.md:108-112); and it would make an `absent`
  family lie about having evidence.
- **Carry the host's model-id rewrite into the component.** §4.4.
- **Keep `extras.refreshUrl` as a recipe parameter.** It is precisely the "host taken from credential contents"
  that boundary §3.3 rules out.
- **Widen `oauth2_token` to any body** (the first draft). It would let a step kind tied to RFC 6749 describe
  non-RFC exchanges; `http_exchange` with a body says what the exchange is (§7.3).
- **Key both probe rules on rotation.** Rotation says "do not force a refresh"; it says nothing about whether the
  upstream has a free liveness request (§7.6).
- **A new world for subscription-backed providers.** Nothing in the function set differs; every difference is
  expressible as a declaration.

## 15. Observations on the native arm

Moved to Appendix D, with the same labels N-1…N-11. N-5 has since been fixed by the host.

## 16. Open questions

Tags: S = south maintainers, L = lv, K = kernel. Each carries this record's recommendation.

On 2026-09-30 lv ruled on the L-tagged questions: as recommended, on condition that each recommendation fits the
final goal DP0 (no provider-specific logic in the host). Two recommendations were adjusted to meet that condition
(K-Q1, K-Q3); the rulings are recorded under each question. K-Q16 to K-Q19 were added by the 2026-10-01 revision;
of these, lv ruled K-Q19 on 2026-10-01.

- **K-Q1 (L) An upstream the host cannot cap** (§8.3). Recommendation: keep today's behavior for the dual run;
  after cutover the host enforces the authorized cap itself and ends the answer with `length` (option A). This
  changes what a caller sees on a long answer, so it needs a ruling and a notice.
  **Ruled (lv, 2026-09-30), adjusted for DP0:** one rule for every family that declares `output_cap: []`, keyed on
  declarations and never on provider identity. With `usage_evidence: absent` the host enforces the authorized cap on
  its own output meter and ends the answer with `length` (after cutover; today's behavior during the dual run). With
  reported usage the host does not cut, because cutting would discard the upstream's usage report; a settlement above
  the reservation goes to manual review, which is the existing generic path.
  This family is the first case; the Codex family of the Responses record is the second (its R-Q5).
  *Revision 2026-10-01:* §8.3 specifies the two cases the ruling leaves open (a streaming cut inside a tool call,
  the buffered non-streaming path). They apply the ruling; they need a new ruling only if lv disagrees.
- **K-Q2 (L) Aligning the estimate before the dual run** (§6.2). Moving to the generic estimator changes the
  billed token counts of existing traffic. Recommendation: move the native arm to the generic estimator first, as
  its own reviewed change, then dual-run for byte-equal settlement; the alternative is a dual run that records a
  per-row delta and proves nothing about amounts.
  **Ruled (lv, 2026-09-30): as recommended.** Moving the native arm to the generic estimator removes
  provider-specific estimation from the host; it changes billed token counts and ships as its own reviewed change.
- **K-Q3 (L) Messages and Responses for this provider** (Appendix A.7, D10). Recommendation: keep them off until
  the Chat dual run passes, then switch them on with their own acceptance; they are a product change, not a
  migration.
  **Ruled (lv, 2026-09-30), adjusted for DP0:** the host keeps no per-provider switch for northbound surfaces. Once
  a row is routed to this package every surface the generic component path serves is available (D10). Staging is
  done by acceptance, not by a gate in the host: the Chat dual run, plus acceptance of Messages and Responses against
  the conformance fixtures, all pass before the row is switched.
- **K-Q4 (L) Dropping `extras.refreshUrl`** (§7.4). Recommendation: drop it; first query production for
  credentials that set it.
  **Ruled (lv, 2026-09-30): drop it**, after a read-only query of production for credentials that set it.
- **K-Q5 (S, L) `meteringEvent` as required terminal evidence** (§5.3). Recommendation: require it if the
  dual-run captures show it on every completed answer; it turns a silently truncated answer into a detected one,
  at the cost of `delivery_unknown` if the upstream ever omits it.
  **Ruled for the host side (lv, 2026-09-30): as recommended.** The south maintainers' half remains open.
- **K-Q6 (S) The exception and status vocabulary of this upstream** (§9), including what an exhausted allowance
  looks like, and which `:error-code` values its `error` frames carry. To be settled by capture; until then the
  status column alone is authoritative.
- **K-Q7 (S) The non-streaming path**: buffered deframe into `parse-response` (P-5, recommended — no new suite
  rule, real `response.*` fixtures, one parser) or the host always driving the stream path and folding IR events
  itself (one less convention on `parse-response`, but a new host fold and a waiver of the `response` fixture
  family). Boundary §5.2 adopts the buffered path for every `aws-eventstream` package whose family declares
  `stream: "none"`; the Codex family is refused instead because it has a stream switch (§5.4). The south
  maintainers' acceptance of that section remains open.
- **K-Q8 (S) Accept P-8(b)** (§7.6): a package that exports no catalog world gets no upstream liveness request, as
  a rule separate from boundary §3.6's "a probe never forces a refresh on a rotating recipe". P-1…P-7 and P-8(a)
  are decided in the boundary record (§3.3 here maps each to its section).
- **K-Q9 (S) Family name**: `kiro` (recommended, §3.1) or a name for the wire alone, with the sign-in recipes and
  client identification then belonging to a family that has only one known user.
- **K-Q10 (S) `content-type` on the inference request** (§4.6): match the native arm and send none
  (recommended for the dual run), or send the media type the vendor's client sends, once a capture shows it.
- **K-Q11 (S) Is `conversationId` required, and must it be a UUID?** If the upstream accepts its absence, P-4 is
  unnecessary for this package and version 1 no longer waits for boundary Q14; the question stands on its own for any
  dialect that needs a client-generated id.
- **K-Q12 (S, L) Images.** Version 1 refuses them, as the host does. Whether the wire carries images is not known
  from the host's code; adding them later is a package change plus a capability declaration.
  **Ruled for the host side (lv, 2026-09-30): version 1 refuses images.**
- **K-Q13 (S) The model catalog for this family** (boundary §7.5, ruled a south data artifact). The ids named in
  the host's comments and tests (server:translate_kiro.rs:28-33, 843-856) are a starting list, not a verified one.
- **K-Q14 (K) A tool-result error flag in the IR** (§4.3). `Message` has no field for it; carrying one needs an
  IR convention. Not blocking: today's path never sends `"error"` either (the first hop does not write `is_error`,
  reference_anthropic.rs:255-261).
- **K-Q15 (S, community host) A second consumer.** Server P21 §7 states that the community host has its own
  implementation of this provider; this record did not read it. If so, the package is the first case where both
  hosts retire provider code for one package, and the community host's view of the attempt id (boundary Q14), the
  buffered path (boundary §5.2) and the user-agent proposal (boundary §10) should be sought before they are frozen.
- **K-Q16 (S, K) The attempt id** (§4.5). The route is boundary Q14, which covers this per-request case; this
  record adds only what the package needs from the answer: an optional value the host fills on every
  provider-world call, a UUID minted fresh per upstream attempt. Recommendation, as in Q14: a typed field through
  the kernel chain, D5's own promotion path. Either way the host strips client-supplied keys that collide with
  reserved names.
- **K-Q17 (S, K) The two doc-comment amendments** (§3.2, §5.4, §6.1), listed in boundary §14. The WIT's "never a
  zero" usage rule becomes a rule for `reported` packages (south); the kernel's `HttpResponseParts.body` comment
  admits a south-defined canonical re-encoding of an eventstream body, or the kernel rules the sentence does not
  apply. Recommendation: amend both; neither changes a signature.
- **K-Q18 (S) Merging tool results** (§4.2). Recommendation: keep the native one-entry-per-message layout in
  version 1; if the dual-run captures show the upstream mishandles consecutive user entries, a later package
  release merges them, with its own fixtures.
- **K-Q19 (L) Output counted as produced or as delivered** (§6.2, Appendix B.1). Recommendation: count the IR
  events the component emitted — what the upstream produced, as today — so that a client disconnect does not lower
  the bill below the upstream spend and the count does not depend on the northbound surface. Counting forwarded
  frames would bill only what the client received.
  **Ruled (lv, 2026-10-01): count output as produced** — the IR events the upstream produced, not what was delivered
  to the client.

## Appendix A. The native arm today (host material)

Citations are at host `a82c852b`.

### A.1 Request path: three hops

1. The northbound Chat body becomes IR (`canonical_to_ir`, server:…/engine/text_admission/chat.rs:383-391).
2. The **Anthropic** component turns IR into an Anthropic Messages body — the host maps the Kiro provider type to
   the Anthropic dialect (server:…/engine/south_component.rs:267-276) and calls it
   (server:chat.rs:581-583), then removes `stream` (:584-586).
3. That intermediate body is sealed as `TextOperation::Kiro`, with the output cap checked at top-level
   `max_tokens` (server:…/engine/text_admission.rs:890-892, 1114-1115) and "no `stream` field" required
   (:933-936).
4. At prepare time the host parses the sealed body back, refreshes the token, and translates Anthropic Messages
   into the upstream body (server:sender.rs:2614-2656; server:kiro.rs:42-53;
   `anthropic_messages_to_kiro`, server:translate_kiro.rs:55-157).
5. It posts to `{base_url}/generateAssistantResponse` (server:upstream.rs:547-552) with
   `Authorization: Bearer`, `x-amz-target`, `amz-sdk-request`, `x-amzn-kiro-agent-mode`, `user-agent`
   (server:upstream.rs:329-352) and `accept: application/vnd.amazon.eventstream` (server:kiro.rs:66-72).

### A.2 Response path: three hops back

- **Streaming**: the host deframes AWS eventstream (server:sender.rs:4827-4887), feeds each event to
  `KiroSseState::on_event`, which emits Anthropic SSE frames as JSON (server:translate_kiro.rs:385-566), re-encodes
  each frame as SSE text and feeds it to the **Anthropic** component's stream parser (server:sender.rs:4735-4747;
  server:south_component.rs:849-856), and renders the resulting IR events as Chat chunks. Exception and error frames
  park the stream (server:sender.rs:4920-4929).
- **Non-streaming**: the upstream always answers with an event stream. The host reads the whole body, decodes it
  (`decode_kiro_event_stream`, server:kiro.rs:136-171), folds it into one Anthropic message
  (`kiro_accumulate_to_anthropic_response`, server:translate_kiro.rs:573-646) and renders that through the
  Anthropic dialect (server:sender.rs:3977-4023).

### A.3 Usage

The upstream reports no token counts — only a `meteringEvent` carrying credits
(server:translate_kiro.rs:302-321). The durable path bills a host-side estimate at the model's ordinary token
rate card and marks the frozen payload `estimated: true` (server:sender.rs:1905-1932), which lands as
`usage_records.tokens_estimated = 1` (server:leaf/usage_types.rs:131-147). The estimator is `ceil(chars / 4)`
(server:…/engine/token_counter/estimate.rs:19-35), applied to three different materials:

| Quantity | Material counted today |
|---|---|
| Input | The sealed **Anthropic intermediate** body, serialized (server:sender.rs:2657, 2690-2692) |
| Output, streaming | Text deltas, plus the tool name and input fragment of every `toolUseEvent`, as decoded from the upstream (server:sender.rs:4749-4766, 4896, 4983-4989) |
| Output, non-streaming | The serialized `content` array of the folded Anthropic message (server:sender.rs:2694-2701, 3994-3998) |

The credits are summed and then discarded (`_upstream_credits`, server:sender.rs:3989; `KiroSseState::credits()` has
no caller outside the leaf crate's tests).

### A.4 Credentials

One `MintStrategy` of five (server:token_refresh.rs:1300-1395) on the shared skeleton (:480-613):

- Two forms, chosen from the credential's `extras` (:1189-1214): an explicit `authMethod` of `idc` / `enterprise` /
  `iam_identity_center` (case-insensitive) selects IdC; any other explicit value selects social; with no
  `authMethod`, IdC is selected exactly when both `clientId` and `clientSecret` are present.
- Endpoints built from a region template (:1135-1137, 1277-1294): social
  `https://prod.{region}.auth.desktop.kiro.dev/refreshToken` with body `{"refreshToken"}`; IdC
  `https://oidc.{region}.amazonaws.com/token` with body `{"refreshToken","clientId","clientSecret","grantType":
  "refresh_token"}`. Both are **JSON with camelCase keys**; `idcRegion` wins over `region`, default `us-east-1`
  (:1215-1220), and since `a82c852b` the value must be region-shaped (:1221-1230, 1249-1262). An `extras.refreshUrl`
  override replaces the endpoint (:1167-1171, 1293).
- Response `{accessToken, refreshToken, expiresIn}` (snake_case aliases accepted, :1265-1273); `expiresIn` is
  relative seconds, defaulted to 3600 and clamped to 60 s – 24 h (:1372-1380). A response without `refreshToken`
  leaves the stored one in place (server:catalog/repo/credentials.rs:796-797).
- The refresh token is single-use and rotating (:1385-1388); the refresh margin is 300 s (:52, 1319-1322); a
  terminal rejection is HTTP 400 / 401 (:252-270); the refresh lock is `kiro-refresh:{id}` (:535, 1311-1313).
- `profileArn` is a static field of the credential and goes into the request body when present (:1151-1166;
  server:translate_kiro.rs:153-155).
- The sign-in file is recognized by shape and its six static keys copied into `extras`
  (server:credentials/mod.rs:215-267); a file without `refreshToken` is accepted. The same parser serves the
  user-facing subscription binding (server:gateway/src/modules/byok/handler.rs:181-206).
- The health probe special-cases this provider so that a probe never burns a rotating refresh token and never
  spends credits (server:health_probe.rs:323-335, 353-421).

### A.5 The probe path

The token-probe path's auth helper refreshes through the same strategy (server:…/engine/provider_pool.rs:535-548),
reached only when the probe's special case runs the real refresh.

### A.6 The routed-dispatch path

Routing groups send a Kiro step through `kiro::send_request_attempt`, which translates and posts per step credential
(server:dispatch/mod.rs:585-607; test at server:dispatch/tests_a.rs:313-430).

### A.7 Northbound surfaces

Only Chat. The Messages plan refuses the provider ("has no bounded Messages sender contract",
server:…/engine/text_admission/messages.rs:357-369; pinned by the test at server:text_admission.rs:1480-1505), and
the target builder refuses every operation other than the dedicated one (server:text_admission.rs:637-640, 801-813;
test at server:sender.rs:7618-7666). Responses has no arm for it (inferred: `sender/responses.rs` contains no
reference to the provider, so a Responses target falls into that same refusal).

## Appendix B. What the host's generic path must do (host material)

### B.1 The estimator (the host's side of §6)

1. **Input is counted over material the component did not produce** — the northbound request as the host
   normalized it, or the IR request. Then a package cannot move the input figure at all. The host already has
   this quantity for mid-stream checkpoints (`checkpoint_input_estimate`, server:sender.rs:1489-1504).
2. **Output is counted by one rule for streaming and non-streaming answers** — text, reasoning text, tool name and
   tool arguments. It counts the IR events the component emitted (produced), not the frames forwarded to the client
   (delivered) (K-Q19, ruled by lv on 2026-10-01). The existing `ForwardedOutputMeter` (server:checkpoint.rs:1-20,
   41-100) counts forwarded SSE frames for mid-stream checkpoints only; the settlement meter is new and needs a
   non-streaming counterpart.
3. **The row is labeled**: `tokens_estimated = 1` and `quantity_estimated = 1`, chosen by the manifest's
   declaration and no longer by family name. Today's label comes from a function documented as serving this one
   provider (server:sender.rs:1905-1932) and the ledger variant `EstimatedTokens` (server:usage_types.rs:131-147,
   300-326).

Against Appendix A.3, items 1 and 2 change all three numbers the native arm produces. That is a billing-caliber
change for existing traffic, which is why K0 moves the native arm first (K-Q2, ruled).

### B.2 What the recipe executor inherits

The skeleton's seven steps stay in the host (boundary §3.5, §3.6). Provider-specific behaviors become generic rules:

- **Probes (P-8)**: §7.6.
- **Lock name.** One lock per credential id with a provider-independent name; the provider-named prefix
  (server:token_refresh.rs:1311-1313) goes away — in K0, not K3, so that both paths share it during K2 (§7.6).
- **Refresh material.** The never-overwrite invariant and the kept previous generation (§7.6; boundary §3.5).
- **Attributes are read from the authoritative row at request time**, as today (server:token_refresh.rs:1404-1423),
  so an operator's edit takes effect without waiting for a refresh.

### B.3 The generic path, step by step

Nothing below names the provider.

1. Route the model row to the package (pinned by digest); read the credential fields; evaluate `select`; mint per
   the recipe if the stored token is within the margin of expiry (boundary §3.6). Failure is a pre-admission error.
2. Hand the component the exported attributes and a fresh attempt id (P-4), through the route boundary Q14 settles;
   strip client-supplied keys that collide with reserved names.
3. Call `build-http-request`. A capability or invalid-request error is a 400 with zero upstream calls and **no
   credential failover** — a refusal of the request's shape is not a credential failure (contrast Appendix D N-3).
4. `ProviderConfig::authorize` (kernel:provider.rs:361-382), descriptor auth admission (boundary §4.2), and the
   `request_facts` checks: model by value, no stream switch, no cap location.
5. Apply operator request extras to the body; seal; reserve; write the dispatch marker.
6. Send with the manifest's `user_agent` (P-3): through the streaming transport for a streaming request, through
   the buffered binary transport otherwise.
7. Non-2xx → `map-provider-error`; funds per the host's existing rules for failures after dispatch.
8. 2xx, streaming → deframe, re-encode, `parse-stream-chunk`, render northbound; meter output; enforce the cap
   (§8.3).
9. 2xx, non-streaming → deframe the whole body, re-encode, `parse-response` (P-5); fill `id` and `model`; enforce
   the cap on the folded answer (§8.3); render; meter output by the same rule as step 8.
10. Settle from the generic estimate and label the row estimated (Appendix B.1); apply §6.3.

What no longer exists in the host: the provider type's arms, the dedicated send and finalize paths, the routed
dispatch arm, the leaf crate's translator, the mint strategy, the probe special case, the sign-in file recognizer
and the header constants (Appendix C.2).

## Appendix C. Migration steps and retirement (host material)

### C.1 Steps

| Step | Side | Content | Acceptance |
|---|---|---|---|
| K0 | Host | Correct the native arm where Appendix D marks a defect that would otherwise be pinned as "correct" by the dual run (at least N-2, N-3 and N-8; N-1 per K-Q1); move the native arm to the generic estimator (K-Q2, ruled; output counted as the IR events produced, K-Q19, ruled 2026-10-01); rewrite dashed model ids on the model rows with the native function's output (§4.4); rename the native refresh lock prefix to the executor's name (§7.6) | Native tests green; rulings recorded |
| K1 | South | Boundary phases B1, B2, B3, B4 and B7a, and the answer to boundary Q14; the reference implementation, the package, the fixture pack | The package passes gates ① and ②; a south minor release lists it in the release index |
| K2 | Host | The generic path of Appendix B.3 for this family; model rows routed to the package; the dual run of §12.2 on the replaying harness | §12.2 |
| K3 | Host | Delete Appendix C.2 | The J1 count falls; removing the package and its rows leaves the host compiling, testing and starting (J3) |

Until B4 lands, the host's existing mint strategy can stand in for the recipe; that is a J1 red item and must be
cleared before K3. Whichever mints, the lock is shared from K0 on.

### C.2 What retires in the host

Line counts measured at host `a82c852b`. "Whole" means the file is deleted.

| Location | Lines | Content |
|---|---|---|
| `crates/gateway-provider-protocol/src/translate_kiro.rs` | 933, whole (647 before the first test module) | Both translation directions |
| `gateway/src/modules/inference/engine/kiro.rs` | 171, whole | Request preparation, the single-attempt sender, the buffered decoder |
| `…/engine/text_admission/sender.rs` | 615 in seven spans: 2611-2685, 2687-2701, 2726-2728, 3540-3544, 3878-4057, 4093-4113, 4735-5050 | Prepare, the two estimate helpers, the two dispatch branches, non-streaming finalize, the streaming path |
| `…/engine/text_admission.rs`, `text_admission/chat.rs`, `messages.rs`, `sender/messages.rs`, `reasoning_replay.rs` | About 55 in total | The operation and transport variants and their match arms (server:text_admission.rs:92, 103, 637-640, 802, 890-892, 933, 1114-1122, 1204-1206; server:chat.rs:61, 71, 325-334, 581-588; server:messages.rs:358; sender/messages.rs:678-684; reasoning_replay.rs:166) |
| `…/engine/token_refresh.rs` | 306 (1118-1423), plus 146 of tests (1703-1848) | The mint strategy (now including the region check), replaced by the recipes |
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
`absent` packages. `leaf/aws_eventstream.rs` (755 lines) is shared with the Bedrock arms and retires when the host
adopts the south-provided deframer (boundary §5.2), not with this package.

Counting whole files, the listed spans and their tests, about 2,700 lines leave the host, of which about 2,000
are not test code.

## Appendix D. Observations on the native arm (code reading, not run)

Noted while reading the host at the baseline. None was executed or reproduced; none is a statement about impact.
They matter here because a dual run pins whatever the native arm does as "correct".

- **N-1 The authorized cap is not sent upstream.** The seal validates `max_tokens` on the intermediate body
  (server:text_admission.rs:890-892, 1114-1115); the upstream body is built from `messages`, `system` and `tools`
  only (server:translate_kiro.rs:55-157). The provider type is nevertheless classed `BoundedToken`
  (server:text_admission.rs:1204-1206).
- **N-2 Streaming and non-streaming answers are counted over different material** (Appendix A.3). On the
  streaming path the tool name is added once per `toolUseEvent`, including the start and stop events
  (server:sender.rs:4756-4762), so the count depends on how the upstream fragments a call.
- **N-3 A request-shape refusal is handled as a credential failure.** `prepare_request` refreshes the token and
  then translates (server:kiro.rs:42-53); any error from it becomes `RetryCredential`
  (server:sender.rs:2648-2655). A request with an image would, by this reading, be tried against every credential,
  refreshing each that is stale.
- **N-4 The model-id rewrite matches any id ending in two numeric segments** (server:translate_kiro.rs:34-47) — a
  date-suffixed id such as `name-4-20250514` would become `name-4.20250514`.
- **N-5 (fixed by the host in `a82c852b`'s series; kept for the record)** The region was substituted into the token
  endpoint without validation, and the sign-in file's `region` / `idcRegion` are copied into `extras` verbatim
  (server:credentials/mod.rs:244-256) by a parser that also serves the user-facing subscription binding. The host
  now refuses any value that is not a single region-shaped label (server:token_refresh.rs:1221-1230, 1249-1262;
  test at :1716-1756). `extras.refreshUrl` still replaces the endpoint unvalidated, but the sign-in file import does
  not copy it, so only admin-entered extras reach it (K-Q4 drops it). The recipe's `aws_region` syntax (§7.3) keeps
  the same property on the component arm.
- **N-6 Every non-2xx from the token endpoint is classified as a 400-class error**
  (server:token_refresh.rs:1390-1394), while the trait's contract says 5xx is upstream jitter and should be 503
  (:451-453).
- **N-7 An unparseable event payload is handled differently on the two paths**: the buffered decoder turns it
  into `null` and the fold ignores it (server:kiro.rs:143-149); the streaming path raises an error without parking
  the stream (server:sender.rs:4889-4895), as do its render errors (4903-4910, 4954-4956), leaving the terminal
  transition to the drop path.
- **N-8 Leftover bytes at end of body are not checked** on either path (server:kiro.rs:141-169;
  server:sender.rs:4864-4933): a body that ends inside a frame, if the transport reports a clean end, is folded as
  a complete answer.
- **N-9 A final assistant turn is sent as the current user message** (§4.3); `tool_choice` is dropped for every
  value; unparseable tool input becomes `{}` on the non-streaming path (server:translate_kiro.rs:627-631).
- **N-10 Operator body extras target the intermediate body** (server:text_admission.rs:350-370, 410); only changes
  to `messages`, `system` or `tools` survive the last hop.
- **N-11 Stale comments**: the pricing arm says the upstream meter "is retained for cost observability"
  (server:text_admission.rs:1204-1205), but nothing on the durable path records it (Appendix A.3); the two comments
  on `profileArn` contradict each other (§7.5).

## Revision note (2026-10-01)

- Header: host baseline moved to `a82c852b` (only `token_refresh.rs` citations changed); reading guide added.
- §1: host detail moved to Appendix A; the gap table gains the kernel's binary-body comment, `error` frames and S0 D5.
- §2: D8 rewritten for the buffered path (boundary §5.2); D11 added — version 1 reproduces the native turn layout.
- §3.3: P-3 now points to the boundary record's single user-agent proposal (boundary §10); P-4 becomes an open
  route (boundary Q14); P-5 and P-7 are marked as doc-comment amendments; P-6 and P-8 revised.
- §4.2 / §4.3 / §4.1: turn layout taken from `reference_anthropic.rs` (one entry per IR message, no merging); the
  "native" column corrected for unknown parts, non-object arguments, empty ids and tool-result errors.
- §4.5: the attempt id conflicts with S0 D5; route is K-Q16; the host strips colliding client keys.
- §4.6: the first draft's constructor claim withdrawn; the value depends on the boundary record's §10 proposal.
- §5.1 / §5.2 / §5.5: the deframer (in `south-contracts`, boundary §5.2) re-encodes `error` frames too; the
  component maps both to `Error`.
- §5.4: P-5 needs the kernel comment amended; why the Codex family differs is stated.
- §6.2: the generic meter is a new host mechanism; produced versus delivered is K-Q19; detail moved to Appendix B.1.
- §7.2 / §7.3 / §7.4: `refresh_token` optional with a seed; an absent rotated token keeps the old one; only 400/401
  terminal (`4xx` class key); `http_exchange` with a body instead of a widened `oauth2_token`; `requires`.
- §7.6: new — endpoint confirmation and third-party gate (boundary §3.4), the never-overwrite invariant, one lock
  across K2, the split probe rule.
- §8.3: `RequestFactsHonoured` for `output_cap: []` defined by mutation; the two cases K-Q1 left open specified.
- §11 / §12.2: fixture and gate ③ rows for all of the above; the dual run moves to a replaying fake upstream and a
  fake token endpoint, adds the routed-dispatch path and five more intentional differences.
- §10, §12.3, §15 and the old §1, §6.2, §7.6 detail moved to Appendices A–D; N-5 marked fixed; retirement count
  updated to about 2,700 lines. K-Q16–K-Q19 added; no ruling was changed.
- Round 2, §8.3 / §11.2: `RequestFactsHonoured` for `output_cap: []` now points to boundary §7.6's single
  definition; this record's own wording and the comparison with the Responses record's draft are removed.
- Round 2, §3.4 / §5 / §6 / §8.2 / §11.1: `stream_framing` and `usage_evidence` are package-level scalars (boundary
  R6); the gate ① rule is stated at package level (a family's `stream: "none"` needs the package's framing).
- Round 2, §7.2 / §7.3: the recipe uses the boundary §3.3 vocabulary names — `write_back` (was `rotate`),
  `requires`, `on_status` class keys, a recipe-level `select` with presence predicates, `seed` with
  `rfc3339_or_epoch_seconds`, field `default`, `endpoint_params`, `optional` extraction; §7.3 is now a usage table,
  not a list of proposals.
- Round 2, §7.2 / §11.3: the recipe declares no clamp; the host executor's provider-agnostic clamp applies
  (boundary §3.6).
- Round 2, §7.2 / §7.5: `profile_arn` uses value syntax `aws_arn` and is a credential field, not a `config_schema` key.
- Round 2, throughout: citations into boundary §3 renumbered (vocabulary §3.3, trust §3.4, invariants §3.5, executor
  §3.6, conformance §3.7, `oauth` deprecation §3.8, coverage §3.9).
- Round 2, §4.5 / §7.5 / K-Q16: the attempt id and the credential attribute cite boundary Q14; the withdrawn
  attribute channel is no longer named.
- Round 2, §1.2 / §3.3 / §5 / K-Q8: descriptions of the boundary record before its revision removed; P-1…P-7 and
  P-8(a) now map to the boundary sections that state them; only P-8(b) remains this record's proposal (K-Q8).
- Round 2, §12.1 / Appendix C.1: K1 adds boundary phase B3 (`runtime_abi`, release index).
- Round 2, throughout: citations of review-process labels replaced by the boundary sections that state each
  rule (§3.4, §3.5, §5.2, §6.3, §10, §13, §14, Q14).
- Rulings of 2026-10-01:
  - K-Q19 ruled by lv: output is counted as produced (the IR events the upstream produced), not as delivered to the
    client. §6.2 and Appendix B.1 state it.
  - The §11 absent-usage fixture row and the Appendix C K0 row state the ruled rule (output counted as the IR events
    produced) instead of referring to K-Q19 as a choice.
