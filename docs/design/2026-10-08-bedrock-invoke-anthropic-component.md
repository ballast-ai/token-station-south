# The Bedrock InvokeModel Anthropic component (`provider-anthropic-bedrock-invoke`)

Status: proposed — drafted for review by the south maintainers and the host owner (token-station-server P21 B6-2),
not accepted. Documentation only: no code, manifest, fixture or release changes with this record.

Date: 2026-10-08

Predecessors: `2026-09-30-host-zero-vendor-boundary.md` ("the boundary record"; this record expands the
InvokeModel row of its §11 and uses its §5 `stream_framing` and `signing`, §6 usage, §7 `request_facts`,
`endpoint` and `config_schema`, §13.6 SF13 / SF16 and §13.9 SF26), `2026-08-22-anthropic-provider-component.md`
(the Messages reference this package shares its source with), `2026-09-29-claude-model-dialect.md` (the Claude
dialect words), `2026-09-30-kiro-provider-component.md` (the sibling component record whose structure this one
follows). There is no standalone Converse component record: the Converse package's design lives in the boundary
record (§5.1–§5.4, §13.6 SF13–SF16, §16 Q28–Q31) and in `reference_bedrock_converse.rs`'s module comment, and that
is the closest precedent used here.

Origin: lv approved B6 work on 2026-10-08. Host plan `docs/product-review-v2/plans/2026-10-08-P21-south-B6-*.md`
(token-station-server), §2 block B6-2 and §3.2, which asks for this record before any south work, and its questions
Q-B6-3 (Bearer form) and Q-B6-4 (production evidence first).

Baseline: south `origin/main` = v0.49.0 (`daca924`); host `f303ebd2`. South line numbers refer to `daca924`. Host
citations carry a `server:` prefix and refer to `f303ebd2`; `…/engine/` abbreviates
`gateway/src/modules/inference/engine/`. Statements marked **(inference)** were reasoned from code or documentation
and not run or captured.

**Reading guide.** §2–§10 are the south contract: what the package is, what it declares and what gates ①–③ check.
§11 is the host's migration (dual run, then deletion of the native arm), §12 versioning, §14 the questions that need
a ruling. Host material is cited, not restated.

## 0. Summary

Bedrock serves Claude through two runtime APIs on the same origin. Converse is already a south package
(`provider-bedrock-converse`, and its Bearer sibling). **InvokeModel** carries the Anthropic Messages body almost
unchanged and is served today by a native host arm (`ProviderType::AwsClaude`, Appendix A) that borrows the
`provider-anthropic` component for the body, patches it, signs it, deframes the eventstream and base64-decodes each
chunk itself. This record moves that knowledge into one package.

| # | Topic | Decision | Section |
|---|---|---|---|
| 1 | Package | `provider-anthropic-bedrock-invoke` 1.0.0, family `anthropic-bedrock-invoke`, world `provider-adapter-v2`; a separate package because `host_signed` stands alone and `provider-anthropic` is on `header_secret` (R6); source shared with `provider-anthropic` | §3 |
| 2 | Request | The Messages body `provider-anthropic` builds, minus `model` and `stream`, plus `anthropic_version: "bedrock-2023-05-31"`; model in the URL as one encoded segment; the Converse headers | §4 |
| 3 | Request facts | `output_cap: ["/max_tokens"]`, `model: {url: "/model/{model}/invoke"}`, `stream: "url"` | §4.4 |
| 4 | Framing | `stream_framing: aws-eventstream`; the component unwraps `{"bytes": base64(...)}` itself, because the deframer is dialect-neutral and does not decode it | §5 |
| 5 | Usage | Reported and strict, Anthropic's buckets including the 5m / 1h cache-write tiers; the stream folds `message_start` and `message_delta` as `provider-anthropic` does | §6 |
| 6 | Auth | `host_signed`, `aws-sigv4`, service `bedrock`, region from the endpoint template, credentials as Converse declares them (SF13); no Bearer sibling now (I-Q2) | §7 |
| 7 | Errors | Bedrock exception names first (shared with Converse), Anthropic error types second, status last; exception and error frames end the stream | §8 |
| 8 | Conformance | Gate ① needs nothing new; a new gate ② pack; gate ③ needs no new host suite | §9 |
| 9 | Migration | Evidence first (Q-B6-4); then route model rows to the component, dual run as Converse C9, delete the native arm | §11 |

Twelve questions are open (§14), two of them preconditions for doing the work at all (I-Q2, I-Q3).

## 1. Problem

### 1.1 Today, in one paragraph

For an `aws_claude` row the host builds the IR into a Messages body with the `provider-anthropic` component (Chat
and Responses surfaces) or forwards the client's own Messages body (Messages surface), then strips `model` and
`stream` and inserts `anthropic_version` (server:…/engine/bedrock.rs:274-283). It builds
`https://bedrock-runtime.{region}.amazonaws.com/model/{id}/invoke[-with-response-stream]` (bedrock.rs:77-90), picks
SigV4 or Bearer **by whether the stored credential has a secret key** (bedrock.rs:193-273, the branch at :213),
signs, sends, and for a stream deframes the eventstream, base64-decodes each `chunk` payload
(server:…/engine/bedrock_durable_stream.rs:15-33) and reads usage with its own Messages accumulator. The provider's
dialect name, URL, body patch, base64 envelope and credential-shape switch are all host code, keyed on
`ProviderType::AwsClaude`.

### 1.2 Why it is not a family of an existing package

- **Not a family of `provider-anthropic`.** That package's only arm is `header_secret`
  (components/provider-anthropic/manifest.json); `host_signed` admits no other arm
  (crates/south-provider-api/src/manifest.rs:806-808, `HostSignedAdmitsNoOtherArm`), and `stream_framing` is
  package-level (boundary R6): `provider-anthropic` is `bytes` (SSE), InvokeModel is `aws-eventstream`.
- **Not a family of `provider-bedrock-converse`.** Its response side parses the Converse wire. A different response
  wire is a separate package, never a dialect word (boundary §7.4).

So the boundary record's §11 row stands: a separate package sharing source with `provider-anthropic`.

## 2. Decisions

- **D1 One package, one family, the existing world.** No WIT signature change, no new manifest field, no new
  contract number (§3).
- **D2 The body is `provider-anthropic`'s, adjusted exactly as the native arm adjusts it.** The package does not
  re-implement the Messages request; it takes the shared builder's body and applies the same three edits as
  server:bedrock.rs:274-283, so on the Chat and Responses surfaces the upstream body is the native arm's by
  construction (§4.2, §11.2).
- **D3 The model is operator data, in the URL, one segment.** Encoded with the shared `url_segment::encode`
  (crates/south-component-conformance/src/url_segment.rs), so an inference-profile ARN stays one segment (SF26,
  sendable since v0.47.0).
- **D4 The component unwraps the base64 envelope.** The deframer and canonical re-encoding stay dialect-neutral
  (crates/south-contracts/tests/eventstream_contract_v1.rs:322-337 pins that a `chunk` payload passes through
  undecoded); unwrapping is dialect knowledge and lives in the component (§5).
- **D5 Usage is reported and strict.** The shared Messages usage parser applies unchanged (§6).
- **D6 Signing is declared exactly as Converse declares it.** Same `endpoint`, `config_schema`, `credentials`,
  `emits` and `signing` (§7).
- **D7 SigV4 only in version 1.** A Bearer sibling follows only if production evidence shows Bearer-shaped
  credentials on `aws_claude` rows (I-Q2, host Q-B6-3).
- **D8 No work before evidence.** If production carries no InvokeModel traffic, the host deletes the native arm
  instead of migrating it and this package is not built (I-Q3, host Q-B6-4).

## 3. Package identity, world and manifest

### 3.1 Identity

| Property | Value |
|---|---|
| Package | `provider-anthropic-bedrock-invoke`, version `1.0.0` (name from the boundary record's §11 example and the host plan; I-Q1) |
| Family (`providers`) | `anthropic-bedrock-invoke` (I-Q1) |
| World / `api_version` | `provider-adapter-v2` (`token-station:adapter@2.0.0`) |
| Behavior suite | `south.provider-component.v1` |
| Capabilities | `chat`, `stream`, `tool_call`, as `provider-anthropic` |
| Auth arms | `host_signed` alone |
| Secrets (`permissions.secrets`) | none: the component never sees a credential, as Converse |
| Reference implementation | `crates/south-component-conformance/src/reference_anthropic_bedrock_invoke.rs`, type `AnthropicBedrockInvokeReferenceV1`, compiled by a wit-bindgen shell `components/provider-anthropic-bedrock-invoke/` as the other provider packages are (components/provider-bedrock-converse/src/lib.rs:1-13) |
| Fixtures | `crates/south-component-conformance/fixtures-anthropic-bedrock-invoke/` |

The family name follows the package name rather than the host's type name (`aws_claude`): the family names a wire
and an API, not a vendor arrangement, and it leaves `anthropic-bedrock-invoke-bearer` free for a sibling (§7.3).

### 3.2 The world's signatures are sufficient

`build-http-request` returns a signed-arm descriptor with no `auth`; `parse-response` takes a JSON body;
`parse-stream-chunk` takes the canonical re-encoding of each eventstream message (boundary §5.2). Nothing calls
`host.sign`. As for Converse, the component never sees a credential.

### 3.3 Manifest sketch

Everything except identity, family, `fixtures` and `request_facts` equals `provider-bedrock-converse` 1.0.12
(components/provider-bedrock-converse/manifest.json).

```json
{
  "name": "provider-anthropic-bedrock-invoke",
  "version": "1.0.0",
  "api_version": "provider-adapter-v2",
  "providers": ["anthropic-bedrock-invoke"],
  "capabilities": ["chat", "stream", "tool_call"],
  "auth_arms": ["host_signed"],
  "endpoint": { "anthropic-bedrock-invoke": "https://bedrock-runtime.{region}.amazonaws.com" },
  "config_schema": {
    "anthropic-bedrock-invoke": {
      "region": { "syntax": "aws_region", "required": true,
                  "description": "The AWS region the Bedrock runtime is called in." }
    }
  },
  "credentials": {
    "schema": "south.credential-recipe.v1",
    "fields": {
      "access_key_id": { "secret": true, "required": true, "description": "…" },
      "secret_access_key": { "secret": true, "required": true, "description": "…" },
      "session_token": { "secret": true, "description": "…" }
    }
  },
  "permissions": { "network": false, "filesystem": false, "secrets": [] },
  "conformance": { "required_suite": "south.provider-component.v1",
                   "fixtures": "fixtures-anthropic-bedrock-invoke/" },
  "compatibility": { "…": "as provider-bedrock-converse; south_runtime per §12" },
  "emits": ["authorization", "x-amz-date", "x-amz-content-sha256", "x-amz-security-token"],
  "stream_framing": "aws-eventstream",
  "signing": {
    "scheme": "aws-sigv4", "service": "bedrock",
    "region": { "template_param": "region" },
    "credentials": { "access_key_id": "access_key_id", "secret_access_key": "secret_access_key",
                     "session_token": "session_token" }
  },
  "request_facts": {
    "anthropic-bedrock-invoke": {
      "output_cap": ["/max_tokens"],
      "model": { "url": "/model/{model}/invoke" },
      "stream": "url"
    }
  }
}
```

No `usage_evidence` (default `reported`), no `user_agent`, no `host_values`, no `secret_headers`: InvokeModel needs
none of them.

## 4. Request mapping: IR → wire

### 4.1 URL and headers

- `POST {base_url}/model/{encode(model)}/invoke`, or `…/invoke-with-response-stream` when `ChatRequest.stream`.
  `base_url` is the endpoint template filled with the `region` key. A request without a model is refused with a
  capability error before anything is built, as Converse does (reference_bedrock_converse.rs:1051-1056).
- Headers: `content-type: application/json`; `accept: application/json`, or `application/vnd.amazon.eventstream`
  when streaming; `x-amzn-bedrock-accept: application/json`. These are the native arm's values
  (server:bedrock.rs:344-354, shared by its Invoke and Converse paths) and Converse's (SF14, boundary Q29: headers in
  the component, values equal to the native arm's).
- No `anthropic-version` header: InvokeModel takes the version in the body (§4.2). No `anthropic-beta` header:
  `provider-anthropic` sends none today, and the native arm drops client beta headers on Bedrock steps
  (server:…/engine/dispatch/mod.rs:546-552).
- **Model encoding.** `url_segment::encode` keeps unreserved characters, sub-delims, `:` and `@`, so an ordinary
  Bedrock model id (`anthropic.claude-haiku-4-5-20251001-v1:0`, `global.anthropic.claude-sonnet-4-6`) is written
  byte for byte as the native arm writes it (server:bedrock.rs:68-76 notes AWS rejects `%3A`). An ARN's `/` becomes
  `%2F` inside one segment; the native arm writes it raw (bedrock.rs:85-90 does not encode). That difference is
  deliberate (boundary §13.7 item 7, SF26) and is one of the dual run's expected deltas (§11.2).

### 4.2 Body

The shared Messages builder (`body_of`, reference_anthropic.rs:306-348) produces the body; then, in this order:

1. remove `model` (InvokeModel takes it from the URL, and a body with both fails validation;
   server:bedrock.rs:262-263);
2. remove `stream` (InvokeModel rejects it with 400 "stream: Extra inputs are not permitted";
   server:bedrock.rs:264-268);
3. insert `anthropic_version: "bedrock-2023-05-31"` (a wire-protocol constant of the InvokeModel dialect, as
   `ANTHROPIC_VERSION` is for Messages, reference_anthropic.rs:31-33; server:bedrock.rs:258-261).

Everything else — `system`, `messages`, `max_tokens` (default 4096 when the IR has no cap,
reference_anthropic.rs:35-37), sampling, `stop_sequences`, `tools`, `tool_choice`, `thinking` /
`output_config`, signed thinking blocks and redacted thinking — is the shared builder's, unchanged. Recommended
shape (inference about the cheapest code that keeps `provider-anthropic` byte-identical): the Invoke reference
calls the existing builder and post-processes the `Value`, mirroring the native function, rather than threading a
wire parameter through `body_of`. Key order in the serialized body follows whatever the host's `serde_json` does
with the descriptor (§11.2 compares bytes).

### 4.3 Dialect words and reasoning replay

The Claude dialect words (`anthropic_dialect::DIALECT_PARAMETERS`, anthropic_dialect.rs:46-53) and
`reasoning_replay.claude.v1` apply exactly as in `provider-anthropic`: `Dialect::of`, `refuse_forced_tool` and the
replay checks run before the body is built, through the shared code. The host already applies Claude handling by
the **model row's declaration**, not by family name (host Q30 ruling, server commit `b628163d`, 2026-10-08), so the
new family needs nothing from the host to receive the words. The host's replay leg for `component` rows whose
dialect is not `anthropic` (`ReplayLeg::Component`, host S4 C9 follow-up "D2") also keys on the model declaration;
the dual run must include a replay case (§11.2), the case Converse's first dual run missed.

### 4.4 Request facts

| Fact | Declaration | Note |
|---|---|---|
| Output cap | `["/max_tokens"]` | The builder always writes it (default 4096). Gate ②'s cap mutation check (`RequestFactsHonoured`, crates/south-component-conformance/src/suite.rs:414-520) proves no other body location moves with the cap. |
| Model | `{ "url": "/model/{model}/invoke" }` | The check is "the URL path contains the template with the encoded model" (suite.rs:465-476), so the streaming URL `/model/{m}/invoke-with-response-stream` satisfies the same template, exactly as Converse's `/model/{model}/converse` covers `converse-stream`. **(inference)** The host seal uses the same containment rule; confirm in the dual run. |
| Stream | `"url"` | The switch is the operation name; the host checks only that the response content type matches the request (manifest.rs:614-616). No buffered path: InvokeModel has a non-streaming operation. |

### 4.5 Lossy and refused cases

Exactly `provider-anthropic`'s: anything that package drops or refuses, this one drops or refuses identically,
because the builder is shared. No InvokeModel-specific loss is introduced. **(inference)** Anthropic features the
shared builder does not emit today (beta flags, `top_k`, `metadata`, citations, `cache_control` markers that the
IR does not carry) are not added by this package; see I-Q10 for what that means on the Messages surface.

## 5. Response and stream mapping

### 5.1 What the component sees

- **Non-streaming 2xx**: the Anthropic message JSON (`id`, `type`, `role`, `model`, `content`, `stop_reason`,
  `stop_sequence`, `usage`), parsed by the shared `parse_response` (reference_anthropic.rs:817-918) unchanged.
  **(inference, from AWS documentation)** Bedrock also returns `x-amzn-bedrock-input-token-count` and
  `x-amzn-bedrock-output-token-count` headers; the component ignores them (I-Q11).
- **Streaming 2xx**: the host deframes with `AwsEventStreamDeframerV1` and feeds the canonical re-encoding. An
  InvokeModel event message arrives as

  ```
  event: chunk
  data: {"bytes":"<base64 of one Anthropic stream event's JSON>"}
  ```

  with the payload's own member order and spelling kept (boundary §5.2). Exception and error messages arrive as
  `event: exception:<type>` and `event: error:<code>` as for Converse.

### 5.2 The stream parser

A thin wrapper around the shared Messages stream state machine (`AnthropicSseParser`,
reference_anthropic.rs:495-758), which today splits SSE frames and dispatches on the SSE `event:` name:

1. Split SSE frames with the shared boundary helper; buffer across chunks as every parser does.
2. `event: chunk`: parse `data` as a JSON object; `bytes` must be a string, else protocol error. Other members are
   ignored. **(inference, not captured)** Bedrock streams carry a random-length padding member `p` beside the
   payload; a fixture must pin that it is ignored (I-Q5).
3. Decode `bytes` as standard base64 (RFC 4648 §4, padded); invalid base64 is a protocol error, never skipped.
4. The decoded bytes must be one UTF-8 JSON object whose `type` is a string; that `type` is the Anthropic event
   name (`message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`,
   `message_stop`, `ping`, `error`). Hand `(type, object)` to the shared `events_of` (reference_anthropic.rs:603-706).
   The state machine needs one small refactor so that it can be fed a parsed `(event, data)` pair as well as SSE
   bytes; `provider-anthropic`'s behavior does not change.
5. `event: exception:<type>` / `event: error:<code>`: end the stream with `StreamEvent::Error`, the code taken from
   the shared Bedrock exception table (`exception_code`, reference_bedrock_converse.rs:1097-1111, made
   crate-shared), as Converse does (reference_bedrock_converse.rs:768-778).
6. Any other top-level `event:` is ignored, as Converse ignores unknown events (reference_bedrock_converse.rs:779-781;
   I-Q12 asks whether to refuse instead).
7. The empty chunk (clean EOF) is handled by the shared state machine: a stream that ended after `message_delta`
   with a stop reason closes with `Finish` / `Done`; one that never reached it yields no `Usage`.

### 5.3 Termination and in-stream errors

Termination is the shared state machine's: `message_delta` carrying `stop_reason` and `usage` emits `Finish`, the
folded `Usage` and `Done`; `message_stop` is ignored. **(inference, from AWS documentation)** Bedrock's
`message_stop` carries `amazon-bedrock-invocationMetrics` (`inputTokenCount`, `outputTokenCount`, latencies); it is
not usage evidence here (I-Q11).

**Observation (code reading, not run).** The shared `events_of` ignores an Anthropic `error` event
(reference_anthropic.rs:703, the catch-all arm), so a Messages stream that reports `overloaded_error` in-band ends
without `Done` and is treated by the host as truncated rather than as the upstream's error. On InvokeModel Bedrock
normally reports mid-stream failures as exception frames, which step 5 maps, so the gap matters less here; fixing
it in the shared code changes `provider-anthropic` behavior too (I-Q12).

## 6. Usage

- **Evidence: reported** (the package does not declare `usage_evidence`; default). The non-streaming response must
  carry `usage`; missing required counts are a protocol error, never a zero (`WireUsage::of`,
  reference_anthropic.rs:408-458; B1).
- **Buckets**: `input_tokens` (uncached), `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`,
  and the `cache_creation.ephemeral_5m_input_tokens` / `ephemeral_1h_input_tokens` tiers, which must add up to
  `cache_creation_input_tokens`. IR `input_tokens` is the sum of uncached, cache read and cache write
  (reference_anthropic.rs:474-488). These are the buckets the native arm reads through its Messages accumulator
  (server:bedrock_durable_stream.rs:64-76, `StreamingUsageWire::AnthropicMessages`).
- **Stream fold**: `message_start` must carry the input side, `message_delta` the output side; the wire buckets are
  folded last-nonzero-wins and every report carries the whole-so-far usage (reference_anthropic.rs:590-601).
  This already covers the host's 03 #86 shape (compatible upstreams report input 0 on `message_start` and the real
  input only on the terminal `message_delta`): the terminal non-zero value wins.
- **One difference from the host's #86 rule.** The host refuses a terminal non-zero bucket **smaller** than the
  start value as inconsistent (server `gateway/CLAUDE.md`, "Streaming and non-streaming bill identically", and
  `crates/gateway-provider-protocol/src/usage_evidence.rs:1128`); the shared fold accepts it. A well-behaved
  upstream never produces it, so the dual run will not see it. I-Q7 asks whether to align the shared fold.
- **Usage judge rows** (B1): a `.meta.json` with `usage_pointer: "/usage"` beside each response fixture that carries
  usage, as `fixtures-bedrock-converse/provider.response.usage.meta.json`.

## 7. Credentials and signing

### 7.1 Signing

`host_signed` alone; `signing` as §3.3. The host's finalizer, selected by `scheme`, signs the finished request for
service `bedrock` in the region filled into the endpoint template, so the signing region and the origin cannot
disagree (boundary §5.3). `signing.credentials` names the three declared secret fields; gate ① requires each to
name a declared secret field and `access_key_id` / `secret_access_key` to be required (SF13, boundary Q28). The
host collects exactly those fields; it holds no Bedrock field set of its own.

### 7.2 What the host stops doing

The native arm's credential-shape switch — SigV4 when the row has a secret key, Bearer otherwise
(server:bedrock.rs:213-273) — is exactly what boundary Q30 forbids for component rows: a host picks the package by
the row's family, never by the shape of the stored credential. With this package a SigV4 row is a
`anthropic-bedrock-invoke` row; a Bearer-shaped `aws_claude` credential has no component home until a sibling
exists (§7.3).

### 7.3 The Bearer form (not in version 1)

Bedrock accepts an API key as `Authorization: Bearer` on InvokeModel as on Converse. If evidence shows such
credentials on `aws_claude` rows (I-Q2), the sibling follows the SF16 precedent exactly: package
`provider-anthropic-bedrock-invoke-bearer`, family `anthropic-bedrock-invoke-bearer`, the `bearer` arm with slot
`provider_api_key`, every other declaration equal and pinned by a test, the reference delegating to this one and
differing only in identity, family and the descriptor's auth, a fixture pack derived from this one with the auth
delta and a drift test. Until then, a Bearer-shaped `aws_claude` row cannot be migrated, which constrains the
deletion step (§11.3).

## 8. Errors, retry and cooldown

`map_provider_error` for a non-2xx answer, in order:

1. The Bedrock exception name from the `x-amzn-errortype` header or the body's `__type`, normalized as Converse
   normalizes it (reference_bedrock_converse.rs:954-966), through the shared `exception_code` table.
2. **(inference)** Otherwise, an Anthropic `error.type` if the body has the Messages error shape, through
   `provider-anthropic`'s table (reference_anthropic.rs:920-935). Whether InvokeModel ever returns that shape, or
   always wraps model errors in Bedrock exceptions (`ModelErrorException` with the model's status), needs a
   capture (I-Q5).
3. Otherwise the status mapping shared by both references.

`provider_message` comes from the body's `message` (Bedrock) or `error.message` (Anthropic), at most 256
characters; `retry_after_ms` from `retry-after`. In-stream exceptions and errors: §5.2 step 5. Retry and cooldown
are the host's generic policy on the IR error code; nothing is InvokeModel-specific.

## 9. Conformance

### 9.1 Gate ①

Nothing new: every rule this manifest meets already exists and is met by Converse — `host_signed` stands alone with
a non-empty `emits` from the signed-header vocabulary (manifest.rs:799-822); `signing` only on a `host_signed`
package and compatible with `emits`; every `signing.credentials` entry names a declared secret field, required
inputs name required fields (SF13); `stream_framing` from the closed set; the endpoint template carries the
`region` parameter (manifest.rs:983-986); the model URL template has exactly one `{model}` (manifest.rs:652-657).

### 9.2 Gate ② — fixture pack `fixtures-anthropic-bedrock-invoke/`

Proposed rows (I-Q6 asks the owner to confirm the set). Stream inputs are written in the canonical re-encoding, as
Converse's are; `chunk` payloads carry real base64 of the Anthropic event JSON.

| Group | Rows |
|---|---|
| request | `chat` (no `model` / `stream` in the body, `anthropic_version`, `/invoke`, the three headers); `stream-uses-the-stream-operation` (`/invoke-with-response-stream`, `accept` eventstream, still no `stream` member); `default-max-tokens-when-the-caller-sets-none`; `model-id-with-a-slash-stays-one-segment` (#138 / SF26, as in the Converse packs); the four dialect rows Converse has (`dialect-sampling-none…`, `…sampling-exclusive…`, `…adaptive-thinking…`, `…budget-thinking…`); `reasoning-replay-multiple-blocks`; `tool-choice-none…`; `parallel-tool-results…` |
| response | `text`, `tool-use`, `usage` (+ `.meta.json`), `cached-usage` with the 5m / 1h tiers (+ `.meta.json`), `missing-usage` (protocol error), `reasoning-replay-multiple-blocks`, `unknown-stop-reason-survives` |
| stream | `text`; `usage-terminal` with cache tiers; `terminal-delta-carries-the-real-input` (the 03 #86 shape); `padding-member-is-ignored`; `chunk-without-bytes-is-refused`; `chunk-with-invalid-base64-is-refused`; `chunk-that-decodes-to-non-json-is-refused`; `exception-ends-the-stream`; `error-frame-ends-the-stream`; `no-usage` (EOF before `message_delta`); `reasoning-replay-multiple-blocks`; `a-tool-call-names-itself-once` |
| error | `throttling-carries-retry-after` (header exception name), `rejected-credential`, `validation-is-invalid-request` |
| capabilities | `declared` |

The suite's generic checks (`EndpointConfinement`, `DescriptorAuthWithinManifest`, `RequestFactsHonoured`,
`UsageNeverDefaulted`, the delete-the-usage mutation) apply to every row without new code. A sandbox parity test
(`anthropic_bedrock_invoke_sandbox_parity_v1`) and a gate ② report join the release discipline as for every
package.

### 9.3 Gate ③ — what the host must prove

**No new host suite.** The package uses only executors the host has already proved against south's suites:
`south.request-signing.v1` (seven cases) and `south.eventstream-framing.v1` (nine cases), both `verified` for
`token-station-server` in `compatibility.json`. The signing suite already covers path segments encoded twice for
an inference-profile ARN (boundary §13.6 SF15). Base64 unwrapping is inside the component and needs no host proof.
The buffered path is not used (§4.4).

### 9.4 T21

Nothing new (I-Q8). The `t21-unseen-eventstream` guest already proves `host_signed` with declared `signing`, the
`aws-eventstream` framing including exception and error frames, and a URL-template model (boundary §13.6 SF18).
InvokeModel adds no host-visible mechanism; its base64 envelope is invisible to the host by design.

## 10. Source sharing

Proposed layout (all inside `south-component-conformance`, where gate ② and the sandbox parity tests require the
native reference to live):

- `reference_anthropic.rs`: make the body builder, the usage parser, the stream state machine's
  `(event, data)` entry point and the error table `pub(crate)`; no behavior change, pinned by the unchanged
  `fixtures-anthropic/` pack.
- `reference_bedrock_converse.rs`: make `exception_code`, `message_of` and the Bedrock error-name normalization
  `pub(crate)`; no behavior change, pinned by the unchanged Converse packs.
- `reference_anthropic_bedrock_invoke.rs` (new): the URL, the three body edits, the chunk wrapper, the composed error
  map.
- A crate-private base64 module: the standard-alphabet strict decoder, generalizing the private base64url decoder
  already in `credential_recipe.rs:1005-1020` rather than adding a runtime dependency to every guest (I-Q4).

## 11. Migration (host material)

### 11.1 Steps

0. **Evidence first** (host Q-B6-4, S4 C0.6, authorized but not yet run because the production database was not
   reachable): does production carry InvokeModel traffic, and are any `aws_claude` credentials Bearer-shaped (no
   secret key)? If there is no traffic, the host deletes the native arm without migrating (P25 precedent) and this
   record is withdrawn. If there are Bearer-shaped credentials, I-Q2 decides whether a sibling is built first.
1. South releases the package (§12); the host re-pins.
2. The operator creates a `component` row for family `anthropic-bedrock-invoke` with the same keys stored as the
   declared credential fields, and points the model rows at it (a routing change, as in the Converse cutover
   handbook, host `2026-10-07-P21-S4-Bedrock-*.md`).
3. Dual run (§11.2) before any production model row moves.
4. Cutover per model row; then delete the native arm (§11.3), with a frozen-expectation parity test replacing the
   dual run, as Converse C11 did.

### 11.2 Dual run

Modelled on Converse C9 (`component_converse_dual_run`): one gateway with a native `aws_claude` row and a signed
`component` row, same model, same prices including cache read and write, one mock upstream that independently
recomputes SigV4, the three northbound surfaces × streaming and non-streaming, each sent to both rows. Compare the
URL, body bytes, header set (except `authorization`, `x-amz-date`, `x-request-id`), the northbound body, the usage
row and the amount. Include from the start the three cases Converse's first dual run missed or added later:
reasoning replay inputs (C9 "D2"), P19 dialect declarations with an absolute assertion that they took effect, and
an ARN model.

Expected deltas, each to be recorded rather than fixed silently:

- **ARN model URL**: `%2F` in one segment (component) against a raw `/` (native, server:bedrock.rs:85-90).
- **Messages surface (inference, the largest expected delta)**: the native arm forwards the **client's own**
  Messages body with only the three edits (server:…/engine/text_admission/messages.rs:225-236), while a `component`
  row goes through the IR (messages.rs:404-420). Fields the IR does not carry are lost on the component row. This is
  not new to InvokeModel — every `component` row serving the Messages surface has it — but for `aws_claude` it is a
  regression from passthrough; I-Q10.
- **Usage**: none expected for well-behaved upstreams (§6).

### 11.3 What retires in the host

From the host plan §3.2 and the files read for this record (server `f303ebd2`): `TextOperation` /
`TextTransport::BedrockInvoke` and their dispatch (…/engine/text_admission.rs:121, :131, :696-702, :2220);
`ChatOutbound::BedrockInvoke` and `ChatResponseContract::BedrockInvoke` (text_admission/chat.rs:75, :85, :399-407,
:754-766); the Messages and Responses `AwsClaude` arms (text_admission/messages.rs:225-236,
text_admission/sender/responses.rs:456, :522, :560-572); the Invoke references in `sender.rs` (19 lines naming
`BedrockInvoke` / `InvokeAnthropic`, among them :2016 and :3374) and `sender/messages.rs` (:658, :673, :1584);
`ReplayLeg::BedrockInvoke` (text_admission/reasoning_replay.rs:126-127, :152); `BedrockApi::Invoke`,
`augment_anthropic_body_for_bedrock` and the credential-shape switch for Invoke (bedrock.rs:56, :79-80, :193-283);
`unwrap_invoke_chunk`, `BedrockDurableWire::InvokeAnthropic` and `BedrockRawEvent::InvokeAnthropic`
(bedrock_durable_stream.rs:15-43, :137-151); the routed-dispatch `AwsClaude` arms (dispatch/mod.rs:54-70,
:512-526, :546-563); the `AwsClaude → anthropic` dialect mapping (south_component.rs:592). The `base64` use in
`bedrock_durable_stream.rs` goes with it.

**J1** (`check_vendor_identifiers.sh`): the baseline holds `ident:awsclaude` in 26 files, 51 occurrences
(`scripts/arch/vendor_identifier_baseline.txt`, counted at `f303ebd2`), of which 8 are historical migration files
(11 occurrences) that never change. **(inference)** Most of the other 40 go with the native arm; the
`ProviderType::AwsClaude` variant, its CHECK lists, the seed and the admin front end stay until S7, as
`ProviderType::AwsBedrock` did after C11. A deleted arm should refuse `aws_claude` text requests before sealing
with zero upstream calls and a pointer to the `component` row, as C11's R43 does for `aws_bedrock`.

## 12. Versioning

- **South**: a minor release adding one package, one reference and one fixture pack. No world, WIT, manifest
  field, contract number or kernel change; not breaking for hosts on re-pin.
- **`south_runtime`**: the package declares the oldest runtime whose gate ① admits it. Every declaration it uses is
  one `provider-bedrock-converse` 1.0.12 uses under `south_runtime` 0.46.0, so **(inference)** 0.46.0, confirmed by
  `scripts/check-declared-runtime.sh --build` at release time. Sending an ARN model needs a host on runtime 0.47.0
  or later (SF26), which is a host condition, not a package one.
- **The other packages**: every change to `south-component-conformance` has so far changed every guest's
  `component.wasm` (releases 0.47.0 §3, 0.48.0 §4, 0.49.0 §3), and the refactor of §10 touches it. So all
  seventeen existing packages take a patch bump with unchanged behavior and keep their `south_runtime`. That cost
  does not depend on how small the refactor is; it can only be shared. Recommended (I-Q9): keep the reference in
  the conformance crate (gate ② needs it there), make the shared-code changes strictly visibility-only, and release
  B6-2 in the same minor as another change that already touches the shared crates, so the seventeen bumps happen
  once.
- **Package identity** of the new package bumps whenever the body edits, headers, error tables or the chunk
  wrapper change; a change to the shared Messages builder bumps both this package and `provider-anthropic`.

## 13. Rejected alternatives

- **A second family of `provider-anthropic`.** Refused by `HostSignedAdmitsNoOtherArm` and by R6 (package-level
  framing).
- **A family of `provider-bedrock-converse`.** Different response wire (boundary §7.4).
- **Base64 decoding in the deframer or the host.** It would make the deframer dialect-aware, which the eventstream
  contract test forbids (eventstream_contract_v1.rs:322-337), and keep provider knowledge in the host (DP0).
- **A `base64` runtime dependency in `south-component-conformance`.** Recommended against (I-Q4): every guest links
  the crate, every guest's lockfile and supply-chain checks would grow, for twenty lines of decoder.
- **SigV4 and Bearer in one package, chosen by credential shape.** Refused by boundary Q30.

## 14. Open questions

Tags: S = south maintainers, L = lv. Each carries this record's recommendation.

- **I-Q1 (S, L) Names.** Package `provider-anthropic-bedrock-invoke` (the boundary record's example, already used
  in the host plan) with family `anthropic-bedrock-invoke`; or `provider-bedrock-invoke-anthropic` /
  `bedrock-invoke-anthropic`, which sorts beside the Converse packages. Recommended: the first, to match the
  existing references.
- **I-Q2 (L) The Bearer API-key form** (host Q-B6-3). Recommended: SigV4 only in version 1; a `-bearer` sibling
  (§7.3) only if the evidence of I-Q3 shows Bearer-shaped `aws_claude` credentials in production. If such
  credentials exist and no sibling is built, the native arm cannot be deleted without stranding them.
- **I-Q3 (L) Is there InvokeModel traffic at all** (host Q-B6-4)? Recommended: no south work until the evidence of
  S4 C0.6 is in. With no traffic, the host deletes the native arm and this record is withdrawn.
- **I-Q4 (S) Base64 in the reference.** Recommended: generalize the private decoder in `credential_recipe.rs` into
  a crate-private module with both alphabets, strict padding and refusal of non-zero trailing bits; RFC 4648 §10
  vectors; a property test against the `base64` crate as a dev-dependency only (the workspace already pins
  `base64` 0.23.1 for `south-north-codec`). Open: whether CONTRIBUTING's "untrusted parsers require a scheduled
  fuzz target" applies to a decoder inside a component reference; no existing stream parser in this crate has one.
- **I-Q5 (S, L) Fixture source.** Recommended: request and non-streaming rows from AWS's and Anthropic's
  documentation and the host's existing native-arm test bodies, marked as such; before the host's cutover (not
  before the south release), one capture of a real InvokeModel stream, a throttled stream and a model error, to
  confirm the `p` member, `amazon-bedrock-invocationMetrics`, the exception spelling and the non-2xx body shape.
  Who captures, and in which environment, is the owner's call (as Kiro's host Q-B6-5).
- **I-Q6 (S) The gate ② rows** of §9.2. Recommended as listed; the row for I-Q7 is added only if I-Q7 is taken.
- **I-Q7 (S, L) Align the stream usage fold with the host's 03 #86 rule** (refuse a terminal non-zero bucket smaller
  than the start value)? Recommended: yes, in the shared `WireUsage::absorb`, so the component and the host judge
  the same evidence the same way; this makes `provider-anthropic` stricter too (a behavior change and an identity
  bump, with one stream fixture in each pack).
- **I-Q8 (S) T21.** Recommended: no new guest or mode (§9.4).
- **I-Q9 (S, L) Release bundling** (host Q-B6-9). Recommended: ship B6-2 in the same minor as another change to the
  shared crates, so the seventeen identity bumps happen once; if B6-2 is ready alone, release it alone rather than
  wait.
- **I-Q10 (L) Messages-surface fidelity.** After cutover, Messages requests to these rows go through the IR instead
  of passthrough (§11.2). Recommended: measure it in the dual run with prompt-caching and beta-flag requests, and
  have the owner rule whether the loss is acceptable or the cutover waits for the IR to carry what is lost (a
  kernel-chain change). This is a host and kernel question that this package cannot answer.
- **I-Q11 (S) Bedrock's own token counts** (`x-amzn-bedrock-*-token-count` headers, `amazon-bedrock-invocationMetrics`).
  Recommended: ignore them; the Anthropic usage object is richer (cache tiers) and is the evidence the native arm
  bills on. A cross-check would add a second judge with no bucket for cache.
- **I-Q12 (S) Unknown frames and the in-band Anthropic `error` event.** Recommended: keep ignoring unknown
  top-level events (the Converse precedent); map a decoded Anthropic `error` event to `StreamEvent::Error` in the
  shared state machine, which also fixes the §5.3 observation for `provider-anthropic` (a behavior change for that
  package, with a fixture in each pack).

## Appendix A. The native arm today (host material, server `f303ebd2`)

- **Selection**: `ProviderType::AwsClaude` → `TextOperation::BedrockInvoke` / `TextTransport::BedrockInvoke`
  (…/engine/text_admission.rs:696-702); the request dialect is `anthropic` (south_component.rs:588-596); the
  response dialect is `anthropic` for `ChatResponseContract::BedrockInvoke` (text_admission/sender.rs:2014-2016).
- **Body**: Chat (text_admission/chat.rs:754-766) and Responses (text_admission/sender/responses.rs:545-572) build
  with the `provider-anthropic` component and then call `augment_anthropic_body_for_bedrock`; Messages forwards the
  client body and calls it (text_admission/messages.rs:225-236); routed dispatch does the same on the raw body
  (dispatch/mod.rs:512-526).
- **URL**: `build_bedrock_url` (bedrock.rs:77-90), model not encoded.
- **Headers and auth**: `prepare_bedrock_runtime_request_with_headers` (bedrock.rs:308-398): `content-type`,
  `accept`, `x-amzn-bedrock-accept`; SigV4 via `aws_sigv4::sign_bedrock_request_with_headers` or Bearer, chosen by
  the credential's shape (bedrock.rs:193-273).
- **Stream**: south's deframer through `stream_framing::EventStreamFrameReader`, then `unwrap_invoke_chunk`
  (bedrock_durable_stream.rs:15-33) and the Messages usage accumulator; terminal frames (`message_delta`,
  `message_stop`) are held back until the stream finishes (:137-151).
- **Usage**: `StreamingUsageWire::AnthropicMessages` (bedrock_durable_stream.rs:64-76), the 03 #86 state machine.
