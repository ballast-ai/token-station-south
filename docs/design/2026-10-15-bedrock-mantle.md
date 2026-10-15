# Provider world: Bedrock Mantle, the `bedrock-mantle` family of `provider-openai-responses`

Status: accepted under lv's 2026-10-15 directive ("claude-code and Mantle too, by the existing code": migrate by the
native server code's logic, speed first, no real-account captures and no production queries; where native behaviour is
doubtful, reproduce native and register the doubt as unverified). Each open question in §10 carries a recommended
default, and the implementation follows the default. BM-Q1 to BM-Q5 were accepted at their defaults on 2026-10-15. Implemented (see Implementation).

Date: 2026-10-15

Baseline: south `origin/main` = `6f4a555` (after #232, provider-vertex, and #233). Host references carry a `server:`
prefix and refer to token-station-server `origin/dev-v2` as read on 2026-10-15; `server:…/` abbreviates
`server:gateway/src/modules/inference/engine/`. Statements marked **unverified** come from reading code or published
documentation, not from running anything against Mantle or capturing its traffic.

Related records: [OpenAI Responses upstream](2026-09-30-openai-responses-upstream-component.md) (R2; §3.3 names
"Bedrock's OpenAI-shaped endpoint", R-Q16 the fixture-pack rule, §13.3 the intentional differences),
[R3](2026-10-12-responses-r3-codex-and-request-fields.md) (the second family and its separate pack, the precedent this
follows), [boundary record](2026-09-30-host-zero-vendor-boundary.md) §7.3 (endpoint templates and `config_schema`) and
§13.12 (Q47), `provider-bedrock-converse-bearer` (the AWS region template precedent).

## 1. Why

The host's speedup plan row T3b reads "Responses / Bedrock Mantle: the `openai-responses` family, missing a Mantle
fixture pack; chat / messages can already go through a component". Today the host's `aws_bedrock_openai` rows serve the
Responses surface only through the native pass-through arm, which retires with R4 once every upstream it serves is
covered (R2 §13.4). This record covers Mantle so that arm and the `AwsBedrockOpenai` type can go.

## 2. Native behaviour (transcribed, not back-derived)

| What | Where | Behaviour |
|---|---|---|
| Host and URL | server:…/bedrock.rs:129-133, 141-170 | Base `https://bedrock-mantle.{region}.api.aws`; chat `{base}/v1/chat/completions`, Responses `{base}/v1/responses`. Streaming uses the same URL; the switch is `"stream": true` in the body. The model id travels in the body, never the URL |
| Region | server:…/text_admission.rs:793-802, 1393-1400; server:…/execution_plan.rs:200-216; server:`gateway/src/infra/config/providers.rs`:758-763, 935-955 | `provider.region` is required: without it the target is statically skipped (`MissingRegion`) and sealing answers 503. Preset default `us-east-1`; `AWS_REGION` / `AWS_DEFAULT_REGION` fill an unset region when the config loads |
| Auth | server:…/upstream.rs:181-188; server:…/south_adapter.rs:165-177; server:`gateway/src/modules/admin_ui/handler/credentials/mod.rs`:403-407 | `Authorization: Bearer <api_key>` only; never SigV4; a credential with a `secret_key` is refused at save time |
| Responses surface | server:…/text_admission/sender/responses.rs:469-516; server:…/execution_plan.rs:186-198 | Native pass-through: the canonical client body is cloned, only `model` is replaced by the row's upstream id, the cap (`max_output_tokens`) is validated, and the body is sent as is. Response contract `Native`; the stream is read by the strict Responses accumulator (`StreamingUsageWire::OpenAiResponses`) and relayed |
| Replay | server:…/text_admission/reasoning_replay.rs:146-150, 223-244 | The pass-through leg is `ReplayLeg::Native`, so OpenAI encrypted reasoning (`NativeOpaque`) is forwarded to Mantle; the Claude carrier is refused |
| Chat and messages | server:…/text_admission/chat.rs:634-695; server:…/text_admission/messages.rs:286-300 | OpenAI Chat wire: with no `south_dialect` the canonical body is cloned with `model` replaced, `max_completion_tokens` kept unless the row sets `accepts_max_completion_tokens = false`, and `stream_options.include_usage` added when streaming. Messages goes through the Anthropic ↔ Chat translator to the same URL |
| Cap | server:…/text_admission.rs:1531-1533, 1590-1615 | `max_output_tokens` admitted on the Responses wire; `max_completion_tokens` on the chat wire |
| Chat stream end | server:…/text_admission/sender.rs:5513-5549, 6121-6130; server:`gateway/KNOWLEDGE.md`:855-869 | For an OpenAI Chat stream the host requires the upstream's `data: [DONE]`; a stream that ends without it fails with "Chat SSE missing [DONE]". The host synthesizes the sentinel only for component IR evidence and Gemini native. KNOWLEDGE records, from a deleted test-only helper, that Mantle's chat stream closes **without** `[DONE]`; the repo's one Mantle chat stream test feeds a fixture that ends with it |
| Responses stream end | server:…/text_admission/sender/responses.rs:2172-2192 | Every `data:` must be JSON; a trailing `data: [DONE]` parks the stream ("invalid Responses SSE JSON") |
| Extras | server:…/body_patch.rs:500-536; server:…/request_extras.rs:1050-1057 | The `-high` model suffix becomes `reasoning: {"effort": "high"}` (Responses) or `reasoning_effort: "high"` (chat); header extras are refused for Mantle rows (body extras apply) |
| Tests | server:`gateway/tests/responses_providers.rs`:937-1025; server:`gateway/tests/bedrock_surface.rs`:375-478; server:…/bedrock.rs:472-500 | `responses_native_json`, `responses_native_sse`, `responses_request`, `openai_chat_sse` and `chat_request`; the URL unit test pins us-east-1, us-west-2 and eu-west-1 |

## 3. Decision: a `bedrock-mantle` family in `provider-openai-responses`

The Responses wire, event list and usage convention the host applies to Mantle are exactly the ones it applies to
OpenAI: one pass-through arm, one strict accumulator (§2). By R-Q16 such an upstream is admitted to `openai-responses`
by a fixture pack of its own. What Mantle adds is only where the endpoint comes from: native derives the host from the
row's region and never reads the row's endpoint. A family is the one place a package can say that (boundary §7.3:
`endpoint` and `config_schema` are per family), so Mantle gets a family whose behaviour is `openai-responses`'s
unchanged (BM-Q1):

```text
providers:             [..., "bedrock-mantle"]
endpoint:              bedrock-mantle: "https://bedrock-mantle.{region}.api.aws"
config_schema:         bedrock-mantle: region (aws_region, required)
request_facts:         bedrock-mantle: output_cap ["/max_output_tokens"]; model {body /model}; stream {body /stream}
immutable_body_paths:  bedrock-mantle: ["store"]
north_passthrough:     bedrock-mantle: "responses"
credentials:           unchanged (families ["openai-codex"]): bedrock-mantle keeps the static `provider_api_key`
```

The reference treats `bedrock-mantle` exactly as `openai-responses` (`FAMILY | MANTLE_FAMILY` in the family match);
the URL is `base_url.resolve(Responses)`, which for the filled origin is `{base}/v1/responses`, the native URL. This is
the Converse Bearer precedent (`bedrock-bearer`: the same region template on the runtime host).

## 4. Surfaces, URL, region and auth

| Native | Component |
|---|---|
| Base `https://bedrock-mantle.{region}.api.aws` from `provider.region` | The host fills the template from the row's `region` config value; an operator-entered base URL is admitted only if it is that host for an AWS region (`endpoint_admits`) |
| Missing region: static skip / 503 | No value, no endpoint: the row cannot be configured (`fill_endpoint` refuses) |
| `Authorization: Bearer <api_key>` | `auth_arms: ["bearer"]` on the static slot `provider_api_key`; no other header but `content-type` |
| Responses: `{base}/v1/responses`, client body forwarded | `bedrock-mantle`: `{base}/v1/responses`, body built from the IR (§7 D1) |
| Chat / messages: `{base}/v1/chat/completions` | See BM-Q2 |

**Which package serves which surface.** A `component` row names one family, so one row serves all three surfaces
through one wire:

- **`bedrock-mantle` row** (the default, BM-Q2): Responses, chat and messages all go to `/v1/responses` (R2 §3.2: a
  Responses-family row serves every surface through the Responses wire). The Responses surface keeps what only the
  Responses wire carries (reasoning items, NC-1 replay when declared); chat and messages change upstream endpoint (D3).
- **`openai-compatible` row** (already servable today, no south change): the operator enters the origin
  `https://bedrock-mantle.<region>.api.aws` as the row endpoint; `resolve` adds `/v1/chat/completions`, the native chat
  URL; bearer on `provider_api_key`; model and stream in the body. Chat and messages are then native-shaped; the
  Responses surface goes through the IR to chat completions, losing the Responses wire. The two rows
  `provider.request.bedrock-mantle-chat-stream` and `provider.stream.bedrock-mantle-chat-without-done` in `fixtures/`
  pin this form. No chat-side family is added (BM-Q5).

## 5. The `[DONE]` behaviour

- **Responses surface: reproduced.** Native parks a stream with a trailing `data: [DONE]` (§2); the component refuses it
  as a frame with invalid JSON after the terminal it has already seen. Row `provider.stream.mantle-trailing-done-refused`.
  A Responses stream needs no sentinel on either side: it ends on its terminal event.
- **Chat surface: not reproduced; a likely native defect fixed (D2, unverified).** Native fails an OpenAI Chat stream that ends without
  `[DONE]`. On a `component` row the host takes evidence from the IR and synthesizes the sentinel itself
  (server:…/text_admission/sender.rs:5513-5549); the `openai-compatible` parser ends the stream on the usage frame that
  follows a finish, and at a clean end of stream releases a held finish. A Mantle-only "require `[DONE]`" rule cannot be
  written: the stream parser receives no configuration (`provider-adapter.wit`, R2 §3.1), and the same parser serves
  every OpenAI-compatible upstream. Reproducing native would need a WIT change or a sibling package, and would
  reproduce a failure the host itself recorded as likely (U1). Accepted (BM-Q3): the component's behaviour, a fix of a
  likely native defect (D2), with no WIT change. Test
  `the_chat_stream_ends_the_same_with_or_without_done` pins it. On a `bedrock-mantle` row (BM-Q2) chat streams over
  the Responses wire, so the question does not arise there.

## 6. The fixture pack

`crates/south-component-conformance/fixtures-bedrock-mantle/`, every row named `mantle-*`, judged together with the
Responses and Codex packs as one package pack (the R3 precedent: the response-side rows are family-neutral and live in
`fixtures-openai-responses/`). No row is a capture of Mantle's traffic. Rows transcribed from the host's native tests:
`request.mantle-host-pinned` and `-stream` (`responses_request`, with the IR checked against the north codec),
`response.mantle-host-pinned` (`responses_native_json`), `stream.mantle-host-pinned` (`responses_native_sse`: data-only
frames, deltas without `output_item.added`), `stream.mantle-trailing-done-refused`, and `request.mantle-region-us-west-2`
(the URL unit test's region). Rows derived from native's verbatim forwarding and the host's KNOWLEDGE notes:
`request.mantle-request-fields`, `request.mantle-tools-and-reasoning-effort`. Rows from the family's rules:
`request.mantle-stateful-fields-never-sent`, `request.mantle-reasoning-replay-undeclared` (BM-Q4). Rows in the documented
OpenAI shape for gpt-oss's raw reasoning (`reasoning_text`) with cache and reasoning buckets:
`response.mantle-reasoning-text-and-cache`, `stream.mantle-reasoning-text`. Usage sidecars on the two response and two
stream rows that report usage. The pack's README records each source.

## 7. Differences from native

- **D1 The Responses request is built, not forwarded.** Every intentional difference of R2 §13.3 for `openai-responses`
  applies unchanged: `store: false` always, `input` always an item list, `stream: false` omitted, `previous_response_id`
  refused on the north side (R48), `input_file` / `file_id` refused, keys outside the IR dropped, `incomplete` settled
  for the two ruled reasons, non-zero `tool_usage` refused, failure frames rendered as the host's fixed event, non-2xx
  mapped by the OpenAI-compatible table.
- **D2 Chat stream without `[DONE]`: a fix of a likely native defect, not a loss (unverified).** Native fails closed on
  a Mantle chat stream that lacks `[DONE]`. The server's own notes say real Mantle chat streams do not send it
  (server:`docs/plans/2026-09-20-target-architecture-progress.md`:1376-1380; server:`gateway/KNOWLEDGE.md`:855-869). If
  that is right, the native chat stream arm never completes on the real upstream. The component path, where the host
  synthesizes the sentinel from IR evidence, then fixes that defect. Whether Mantle omits the sentinel is unverified
  (U1). No WIT change is made (§5).
- **D3 Chat and messages on a `bedrock-mantle` row** go to `/v1/responses`, not `/v1/chat/completions` (BM-Q2).
- **D4 Chat on an `openai-compatible` row** sends the cap as `max_tokens` (the component's key; the host applies no
  Chat-wire policy to component rows, server:…/text_admission/chat.rs:743-753) where native sends
  `max_completion_tokens`.
- **D5 Encrypted reasoning replay**: native forwards OpenAI opaque reasoning to Mantle; the family refuses it unless the
  model declares `reasoning_replay.openai.v1` (BM-Q4).
- **D6 The `-high` model suffix**: native injects `reasoning.effort` for Mantle by provider type; a component row gets
  it only through a generic host mechanism (H5).
- **D7 Header extras**: native refuses them on Mantle rows; on a component row the host's generic descriptor-header
  rules apply (H6).
- **D8 Region**: a missing or malformed region is a configuration refusal on the row rather than a per-request static
  skip; the `AWS_REGION` fallback becomes a cutover step (H2).

## 8. Unverified doubts (none blocks)

- **U1** Whether Mantle's chat stream ends without `[DONE]` (the host's only source is a deleted helper; its own test
  fixture sends the sentinel).
- **U2** Whether Mantle's `/v1/responses` accepts `store: false`, `include`, the four client fields (`user`,
  `metadata`, `prompt_cache_key`, `truncation`), `reasoning.effort`, function tools and `max_output_tokens`. Native
  forwards whatever the client sends, so it has the same exposure.
- **U3** Whether Mantle's Responses stream carries `sequence_number`, a closed event list, the terminal `usage` with
  `input_tokens_details.cached_tokens` / `output_tokens_details.reasoning_tokens`, and no trailing sentinel. Native's
  accumulator requires the same, so a departure fails both.
- **U4** The model id form: the host's type comment and KNOWLEDGE say ids omit `-1:0` (`openai.gpt-oss-120b`), its tests
  use `openai.gpt-oss-120b-1:0`. The id is the row's operator data, sent verbatim either way.
- **U5** Mantle's error body shape (AWS-style or OpenAI-style), which decides what the OpenAI-compatible error table
  extracts.
- **U6** gpt-oss reasoning as `reasoning_text` items and `response.reasoning_text.*` events (the documented gpt-oss
  shape, not a Mantle capture).
- **U7** Whether GovCloud regions use the same `api.aws` host (the template admits any `aws_region`).

## 9. Host prerequisites for cutover

- **H1** Re-pin to the release carrying `provider-openai-responses` 1.4.0.
- **H2** Per `aws_bedrock_openai` row, a `component` row: `south_dialect` = `bedrock-mantle` (or `openai-compatible`
  per BM-Q2), `config_values.region` = `providers.region` with the `AWS_REGION` fallback resolved at migration, the
  credential's `api_key` as the static `provider_api_key`, models moved with their `upstream_model` unchanged.
- **H3** A cutover preflight that lists rows without a region and models declaring `reasoning_replay.openai.v1`.
- **H4** A dual run against the native arm on all three surfaces, stream and non-stream, with D1–D8 asserted; the pack's
  host-pinned rows are the native test bodies, so the replay can reuse them.
- **H5** The model-suffix reasoning injection for component rows (generic, by declaration), or accept D6.
- **H6** A decision on header extras for the migrated rows (D7).
- **H7** After cutover, delete the native arm: the Mantle half of `bedrock.rs`, the two `AwsBedrockOpenai` URL arms in
  `text_admission.rs`, the auth arm in `upstream.rs`, the type in `sender/responses.rs`, `execution_plan.rs`, `chat.rs`,
  `messages.rs`, `body_patch.rs`, `request_extras.rs` and the credential page, and the variant itself at S7.

## 10. Open questions, with the default implemented

- **BM-Q1 Family or plain row.** (a) a `bedrock-mantle` family with the region template; (b) an ordinary
  `openai-responses` row whose operator enters the regional origin (R2 §3.3's inference). **Default: (a)**: it keeps
  native's derivation of the host from the region and gives the cutover a family to name; its cost is a minor version.
- **BM-Q2 Which family serves a Mantle model.** **Default: `bedrock-mantle` for every surface**, the only choice that
  keeps the Responses surface on its wire; operators whose Mantle traffic is chat only may use an `openai-compatible`
  row (§4), which keeps chat native-shaped.
- **BM-Q3 Chat `[DONE]`.** **Accepted (2026-10-15): the component's behaviour (D2)**, as the fix of a likely native
  defect; no WIT change.
- **BM-Q4 Reasoning replay.** **Default: no Mantle model declares `reasoning_replay.openai.v1`** until a multi-turn
  replay is seen to work on Mantle (the standing rule for Codex, R3 §3.1).
- **BM-Q5 A chat-side `bedrock-mantle` family in `provider-openai-compatible`.** **Default: no**; the row endpoint
  already gives the native chat URL, and BM-Q2 does not route chat there by default.

## 11. Versioning (Q47)

`south-component-conformance` changes (the reference's family match and a vocabulary constant), but it already moved to
0.65.0 in this unreleased cycle (#232), so it takes no further step and no other package moves.
`provider-openai-responses` gains a family, which is new behaviour: **1.3.10 → 1.4.0**; it keeps `south_runtime` 0.60.0
(`endpoint` and `config_schema` are older than its newest declaration) and declares no contract change.
`provider-openai-compatible` only gains two fixture rows, which are not part of its bytes, so it does not move.

## Implementation

### What landed

- **Reference** (`reference_openai_responses.rs`): `MANTLE_FAMILY` (`responses_vocabulary::mantle::FAMILY`,
  `bedrock-mantle`) joins `FAMILY` in the family match; nothing else in the reference reads the family.
- **`provider-openai-responses` 1.4.0**: the declarations of §3. `south_runtime` stays 0.60.0.
- **`fixtures-bedrock-mantle/`**: the twelve rows of §6 and a README; joined to the Responses and Codex packs by
  `tests/support/openai_responses_packs.rs`, so the native gate ②, the sandbox parity test and the release report
  judge all three directories. Two chat rows in `fixtures/` (§4).
- **Tests**: `tests/bedrock_mantle_conformance_v1.rs` (the decided rows; every request row's base URL is the template
  filled from its declared region; `UsageNeverDefaulted` on the four sidecar rows; the declarations; the template equals
  native's base for five regions, `endpoint_values` inverts it, foreign hosts are refused; the family builds exactly
  what `openai-responses` builds over every non-Codex request row of the package pack; the host-pinned IR is the north
  codec's output; the two `[DONE]` facts of §5). The Responses, Codex, sandbox-parity, usage-judge and
  shipped-packages tests take the third family and the version.

### Interpretations

1. **Expectations were written by hand from the reference's rules before any build**, then gate ② confirmed every one
   unchanged.
2. **The model id.** Host-pinned rows keep the host tests' `openai.gpt-oss-120b-1:0`; the other rows use
   `openai.gpt-oss-120b` (U4). Both are declared in the rows' model list.
3. **The minor version in the open cycle** follows `provider-kiro` (0.65.0 cycle) and `provider-anthropic` (#236): the
   retire test's tuple names 1.4.0 with a comment.

### Gate results

Run locally on the final tree (macOS, toolchain 1.96.0, root lockfile with wasmtime 49.0.3 and rustls 0.23.46), with
`set -o pipefail`:

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass |
| `PROPTEST_CASES=32 cargo nextest run --workspace --all-features` | 2193 passed, 4 skipped (9 in `bedrock_mantle_conformance_v1`; `openai_responses_sandbox_parity_v1` runs the `bedrock-mantle` rows through the wasm build) |
| `cargo test --workspace --doc --all-features` | pass |
| `cargo test --workspace --no-default-features` | 2016 passed |
| `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --all-features` | pass |
| `cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked` | pass |
| `rustup run 1.96.0 cargo check --workspace --all-targets` | pass |
| `scripts/check-boundaries.sh --self-test` and `scripts/check-boundaries.sh` | pass |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | 72 passed |
| `scripts/check-language.sh` (self-test, files, commits) | pass |
| `cargo machete` (root and `fuzz/`) | pass |
| `cargo deny check` and `cargo audit` (root and `fuzz/`) | pass |
| every `scripts/build-*-component.sh`, then `scripts/check-declared-runtime.sh --build` | pass |

Mutations (each applied to the source, the Mantle, Responses, Codex and compatible suites run, the file restored from a
copy and the tree checked unchanged):

| Mutation | Red tests |
|---|---|
| Template host on `amazonaws.com` | `the_family_declares_the_native_host…`, `the_mantle_pack_holds_mantle_rows_only_with_the_filled_endpoint`, `the_template_is_the_native_mantle_host_for_every_region` |
| Mantle URL without `/v1` | `gate_two_passes_over_the_shipped_pack`, `the_family_builds_what_openai_responses_builds…`, `the_template_is_the_native_mantle_host…` |
| `region` made optional | both gate ① tests, `gate_two_passes…`, `the_family_builds…`, `the_family_declares…`, `the_template_is…` |
| No bearer for Mantle | `gate_two_passes…`, `the_family_builds…` |
| Family check drops Mantle | `gate_two_passes…`, `only_the_three_families_are_served`, `the_family_builds…`, `the_template_is…` |
| Mantle treated as Codex | `gate_two_passes…`, `the_family_builds…`, `the_template_is…` |
| Responses parser tolerates a trailing `[DONE]` | `a_trailing_done_on_the_responses_stream_is_refused_as_native_refuses_it`, `gate_two_passes…` |
| Chat parser requires `[DONE]` at end of stream | `the_chat_stream_ends_the_same_with_or_without_done`, `the_reference_implementation_passes_the_component_behavior_suite` |
