# KC1, south half: canonical IR 4 (prompt-cache markers, top_k, typed reasoning, the R-Q15 promotions)

Status: accepted (lv, 2026-10-12: every question in §9 ruled as recommended; KC-Q10 ruled: R3 ships first as 0.56.0, KC1 ships as 0.57.0). Implemented on branch `kc1-canonical-ir-4` (§11), merged as #188; released in 0.57.0 (`docs/design/2026-10-12-release-0.57.0.md`). The owner rulings it builds on (lv, 2026-10-09) are binding and
are not re-opened here; the questions in §9 are the decisions those rulings left open.

Date: 2026-10-12

Baseline: south `origin/main` = `7ef9995` (release 0.55.0). South line numbers refer to that commit. Kernel references
carry a `kernel:` prefix and refer to `ballast-ai/token-station` at the merge of PR #42 (`c0bca10c`, protocol 0.6.0,
router core 0.6.0), read from a clean clone. Host references carry a `server:` prefix and refer to token-station-server
as read on 2026-10-12. Statements marked **unverified** are conclusions from reading code or published documentation,
not from running anything or capturing traffic.

Predecessors:
[`2026-10-08-kernel-repin-protocol-0.5.0.md`](2026-10-08-kernel-repin-protocol-0.5.0.md) (the previous re-pin: every
package changes identity, exact `kernel_contracts` comparison, `south_runtime` moved at the release),
[`2026-09-30-openai-responses-upstream-component.md`](2026-09-30-openai-responses-upstream-component.md) (R-Q15: the
three `extensions` keys components act on, to be promoted through the kernel chain),
[`2026-10-11-responses-reasoning-replay-and-passthrough-suite.md`](2026-10-11-responses-reasoning-replay-and-passthrough-suite.md)
(NC-1: the replay markers this change must leave alone),
[`2026-09-30-host-zero-vendor-boundary.md`](2026-09-30-host-zero-vendor-boundary.md) (§13.8 the `host_values` channel,
§13.12 Q47 crate versions, §13.6 declared runtime),
[`2026-08-21-canonical-ir-inventory.md`](2026-08-21-canonical-ir-inventory.md) (D5: `extensions` are data),
[`2026-10-08-bedrock-invoke-anthropic-component.md`](2026-10-08-bedrock-invoke-anthropic-component.md) (which records
the Messages-surface losses this change removes).

Owner rulings (host design `server:docs/product-review-v2/plans/2026-10-09-P21-KC1-*.md` (the host KC1 design draft) §8;
kernel PR #42 "Rulings"):

| # | Ruling |
|---|---|
| K1 | Change the kernel IR (protocol 0.6.0, `canonical_ir` 4) together with R-Q15; every package changes identity once. |
| K2 | Block-level markers: `ContentPart::Text` / `ImageUrl` and `ToolDef` carry `cache_control`; a cached `tool_result` is a `Role::Tool` message with `Content::Parts` and the marker on the last part; a system message keeps one `Text` part per system block. |
| K3 | `metadata.user_id`: the host rewrites it to a per-tenant hash and passes it in `ChatRequest.host_values`; the package declares the key; the component maps it per dialect (Anthropic `metadata.user_id`). The hash itself is the host's design. |
| K4 | The OpenAI Chat surface accepts OpenRouter-style part-level `cache_control` into the IR. |
| K5 | `thinking` is in scope, typed, `budget_tokens` carried unconverted. |
| K6 | TTL is a closed enum (`5m`, `1h`); an unknown value is a north-decode 400. |
| K7 | Converse `cachePoint` mapping is done now; check AWS for 1h, and where 1h is unsupported map to 5m and record it. |
| K8 | Server tools are out of scope. |
| K9 (kernel) | `ReasoningEffort` stays semi-open (`Other(String)`); `responses_tool_strict` is not lifted at deserialization: producers move to `ToolDef.strict`, consumers fall back to the old key during the transition; an unknown `CacheControl` key is refused. |

## 1. What changes, in one table

| Area | Today (0.55.0, protocol 0.5.0) | After this change |
|---|---|---|
| Kernel pin | mirror `v0.4.0` (`c2581f37`), `canonical_ir` 3 | mirror `v0.5.0`, upstream `kernel-v0.6.0`, `canonical_ir` 4 (§2) |
| Messages decode | `system` joined into one string; no `cache_control`, `top_k`, `thinking`, `metadata`; unmodelled top-level keys dropped | system blocks kept as parts; markers on blocks, tools and `tool_result`; `top_k`; `thinking` / `output_config.effort` typed; other top-level keys to `extensions` (§3) |
| Chat decode | part `cache_control` dropped; `reasoning_effort` / `parallel_tool_calls` in `extensions`; `function.strict` dropped | part markers typed (K4); the two keys typed; `strict` typed (§3.2) |
| Responses decode | `reasoning.effort`, `parallel_tool_calls`, the `responses_tool_strict` map in `extensions` | typed `reasoning.effort`, `parallel_tool_calls`, `ToolDef.strict` (§3.3) |
| Anthropic and Bedrock Invoke builders | `system` always a string; markers, `top_k`, `thinking` never emitted | markers, `top_k`, verbatim `thinking`, `system` block array when marked, `metadata.user_id` from `host_values` (§4.1) |
| Converse | no `cachePoint`; flat cache-write usage | `cachePoint` (declared per model), TTL split from `cacheDetails` (§4.2) |
| OpenAI-compatible, Gemini, Responses | read the extension keys | read typed fields; markers dropped, never refused; Gemini gains `topK` (§4.3 to §4.5) |
| `host_values` vocabulary | `attempt_id` | adds `end_user_hash` (§5) |

## 2. Kernel re-pin

### 2.1 What moves

| Item | Before | After |
|---|---|---|
| Kernel mirror tag | `v0.4.0` = `c2581f37ad468201c903e0d596b59e796173e459` | `v0.5.0` (not tagged yet; **unverified** identifiers) |
| Upstream source (`mirror.source_commit`) | `kernel-v0.5.0` = `8e34f5a0…` | `kernel-v0.6.0` (not tagged yet; the PR #42 merge is `c0bca10c`, **unverified** that the tag lands there) |
| `token-station-protocol` | 0.5.0 | 0.6.0 |
| `kernel_contracts.canonical_ir` | 3 | 4 |
| `stream`, `error_catalog` | 2, 1 | unchanged (PR #42 adds no stream event and no error code) |

The only pin is the workspace dependency in the root `Cargo.toml` (`token-station-protocol`, git `rev`); the component
and fuzz lockfiles follow it. South's `compatibility.json` moves `kernel_contracts.canonical_ir` to 4; its
`contracts.canonical_ir` stays `null` (the kernel owns that number). The tuple every manifest declares becomes
`ir_schema_id = token-station-protocol@0.6.0/v0.5.0`, `kernel_version = 0.5.0`, `kernel_revision = <kernel-v0.6.0
commit>`, `kernel_contracts.canonical_ir = 4`, derived exactly as the 0.46.0 re-pin derived them (§2 of that record).

`compatibility_admits` compares `kernel_contracts` exactly, so a host whose `HostRangeV1` says `canonical_ir` 3 refuses
every package after this change, and a host at 4 refuses every earlier package. The host must move its kernel pin,
`south_host_range.json`, its embedded and test manifests and its fetch-script pins in one step
(`server:` KC1 design §5 lists them).

### 2.2 Source breaks inside south

Protocol 0.6.0 adds fields to `ContentPart::Text`, `ContentPart::ImageUrl`, `ToolDef`, `Sampling` and `ChatRequest`.
Every struct literal gains the new fields (or uses `ContentPart::text` / `ToolDef::new`) and every
`ContentPart::Text { text }` pattern gains `..`. About 83 sites across the north codec, the conformance references and
their tests (a `git grep` count of literals and patterns of those five types; not every hit needs a change). The
compiler finds all of them. Adding `..` silently is not enough at the sites listed in §4: there the new field must be
read, and a fixture pins it.

### 2.3 Readers that would go silent

The three R-Q15 keys keep their wire spelling, so a 0.6.0 deserializer lifts `reasoning_effort` and
`parallel_tool_calls` out of `extensions` into the typed fields. Every component reads its `ChatRequest` by
deserializing JSON at the WIT boundary (`provider-adapter.wit`, `build-http-request`), so after the re-pin a reader
that still looks in `request.extensions` finds nothing and silently stops sending the field. The readers today:

| Reader | Key | Moves to |
|---|---|---|
| `anthropic_dialect.rs:201` (`Dialect::thinking`, shared by Anthropic, Invoke, Converse) | `reasoning_effort` | `request.reasoning.effort` |
| `reference.rs:323-330, 778-786` (OpenAI-compatible) | `parallel_tool_calls`, `reasoning_effort` | typed fields |
| `reference.rs:308-313` | `responses_tool_strict` | `ToolDef.strict`, falling back to the map (K9) |
| `reference_openai_responses.rs:357-392` | all three | same |

The existing fixtures already guard most of this: `dialect-adaptive-thinking-carries-the-effort` and its siblings put
`reasoning_effort` at the top level of `chat_request`, so a reader left on `extensions` turns them red. §7.4 adds the
missing guard for `parallel_tool_calls` and `strict`.

`responses_tool_strict` is not lifted (K9), so a producer still writing the map keeps working against the fallback.

### 2.4 Package identities and `south_runtime`

The tuple is package content, so all twenty packages change identity (Q47, boundary record §13.12: the version moves
because the manifest changes, and because `south-contracts`, `south-provider-api` and `south-component-conformance`
change, which changes every `component.wasm`).

Final table (2026-10-12, after the rebase onto the 0.56.0 release `350ab72`, KC-Q10; the middle column is checked
against the published v0.56.0 index). R3 (#186, `45d068a`) already retired every 0.55.0 identity for the 0.56.0 release and moved `south-component-conformance` to 0.54.0; under Q47 each package and crate
KC1 changes takes its own bump on top of R3's numbers, and ships in 0.57.0.

| Package | 0.55.0 (published) | 0.56.0 (published, R3) | KC1, ships in 0.57.0 | Why |
|---|---|---|---|---|
| `provider-anthropic` | 1.0.19 | 1.0.20 | 1.1.0 | markers, `top_k`, verbatim `thinking`, `metadata.user_id` (§4.1) |
| `provider-anthropic-bedrock-invoke` | 1.0.3 | 1.0.4 | 1.1.0 | shares `body_of` (§4.1) |
| `provider-bedrock-converse` | 1.0.16 | 1.0.17 | 1.1.0 | `cachePoint`, `cacheDetails`, verbatim `thinking`, `top_k` (§4.2) |
| `provider-bedrock-converse-bearer` | 1.0.8 | 1.0.9 | 1.1.0 | same code as above |
| `provider-gemini` | 1.2.0 | 1.2.1 | 1.3.0 | `topK` (§4.4) |
| `provider-openai-compatible` | 2.5.0 | 2.5.1 | 2.6.0 | typed reads, Chat `strict` forwarded (§4.3) |
| `provider-openai-responses` | 1.1.0 | 1.2.0 | 1.3.0 | typed reads (§4.5) |
| `embeddings-openai-compatible` | 1.0.6 | 1.0.7 | 1.0.8 | tuple only |
| `embeddings-gemini` | 1.1.3 | 1.1.4 | 1.1.5 | tuple only |
| `embeddings-vertex` | 1.0.5 | 1.0.6 | 1.0.7 | tuple only |
| `image-azure` | 1.0.2 | 1.0.3 | 1.0.4 | tuple only |
| `task-kling` | 1.0.15 | 1.0.16 | 1.0.17 | tuple only |
| `task-kling-v2` | 0.32.13 | 0.32.14 | 0.32.15 | tuple only |
| `task-minimax-v2`, `task-bailian-v2` | 0.31.12 | 0.31.13 | 0.31.14 | tuple only |
| `task-byteplus-v2` | 0.36.12 | 0.36.13 | 0.36.14 | tuple only |
| `task-xai-v2`, `task-veo-v2`, `task-wan-image-v2`, `task-gmi-image-v2` | 0.35.12 | 0.35.13 | 0.35.14 | tuple only |

| Crate (Q47) | 0.55.0 | 0.56.0 | KC1 | Why |
|---|---|---|---|---|
| `south-contracts` | 0.52.0 | 0.52.0 | 0.52.0 | unchanged: it does not depend on the kernel and KC1 does not touch it |
| `south-provider-api` | 0.52.0 | 0.52.0 | 0.53.0 | the `end_user_hash` word and `is_end_user_hash` (§5) |
| `south-component-conformance` | 0.53.0 | 0.54.0 | 0.55.0 | the kernel types, the dialect changes, `typed_request` |

`crates/south-component-conformance/tests/shipped_packages_v1.rs::kc1_retires_every_r3_package_identity` pins the KC1
column and the new tuple; `r3_retires_every_published_055_package_identity` drops its "now" column, as each earlier
retirement test did when the next change came, and the 0.46.0 re-pin test no longer pins the tuple it introduced.

`south_runtime`: every package must declare the release that carries the re-pin (0.57.0). An older runtime's range
takes `canonical_ir` 3 from its own `compatibility.json`, so `scripts/check-declared-runtime.sh` refuses any lower
declaration, as in the 0.46.0 re-pin (§5 of that record). During development the declarations keep their current values
and move together at the release step, because `shipped_packages_v1` refuses a `south_runtime` newer than the workspace
version; the release also updates every retirement test that pins a package's runtime.

## 3. North codec

### 3.1 Anthropic Messages decode (`anthropic.rs`)

**System.** Each system block becomes one `ContentPart::Text` in one `Role::System` message, with the block's
`cache_control` (K2). A string `system` stays one `Content::Text` message, as today. Skipping empty text stays the
builders' job (`system_text_of`, pinned by the system fixture). `messages[]` entries with role `system` / `developer` get the same treatment.

**Content blocks.** `text` → `Text { text, cache_control }`; `image` → `ImageUrl { image_url, cache_control }`; any other
block stays `Unknown` verbatim, but its `cache_control`, if present, is checked with the kernel's `CacheControl`
deserializer first. The kernel's own `unknown_part` check runs only when JSON is deserialized, and the codec builds
`ContentPart::Unknown(part.clone())` directly; without the check an invalid marker on a `document` block would pass the
codec and fail at the component's deserializer with an internal error instead of a 400.

**`tool_result`.** When the block or any inner block carries a marker, the `Role::Tool` message's content becomes
`Content::Parts` (text parts, plus image parts as `ImageUrl`), and the block-level marker lands on the last part (K2).
Without a marker the content stays the flattened `Content::Text`, so no existing fixture changes. `is_error` has no IR
field and stays lost (§9 KC-Q9).

**Tools.** `tools[i].cache_control` → `ToolDef.cache_control`; `tools[i].strict` (Anthropic strict tool use) →
`ToolDef.strict`.

**Sampling.** `top_k` → `Sampling.top_k`; a value that is not a non-negative integer, or exceeds `u32::MAX`, is
`CodecError::out_of_range`, as `max_tokens` is.

**Reasoning (K5).**

| Messages | IR |
|---|---|
| `thinking: {type: "enabled", budget_tokens: N}` | `reasoning_mode: enabled`, `reasoning_budget_tokens: N` |
| `thinking: {type: "adaptive"}` | `reasoning_mode: adaptive` (a `budget_tokens` beside it is carried unchanged; the upstream rules on it) |
| `thinking: {type: "disabled"}` | `reasoning_mode: disabled` |
| `output_config.effort` | `reasoning_effort` (`Other` for a level the kernel does not name, such as `max`) |
| `thinking` not an object, `budget_tokens` not a `u32`, `effort` not a string | 400 |
| any other `thinking.type`, e.g. `between_tools` | §9 KC-Q2 (recommended: 400) |
| `thinking.display` | no IR field; lost (§9 KC-Q3) |

Nothing is converted: an effort is never derived from a budget, nor the reverse.

**`tool_choice.disable_parallel_tool_use`** is dropped today (`tool_choice_from_wire` keeps only `type` and `name`). It
can map to `parallel_tool_calls = !disable_parallel_tool_use` (§9 KC-Q6).

**Other top-level keys.** A loop like the Chat decode's (`request.rs:117-124`) copies every key the decoder does not
consume into `extensions`, so `service_tier`, `container`, `context_management`, the rest of `output_config` and the
like survive as data. The consumed keys are `model`, `messages`, `system`, `max_tokens`, `temperature`, `top_p`,
`top_k`, `stop_sequences`, `stream`, `tools`, `tool_choice` and `thinking`. Two keys need a decision:

- `metadata`: the raw end-user id must not reach a component (K3); the host reads it from the north body, not from the
  IR. Recommended: consumed, never copied into `extensions` (§9 KC-Q4).
- the top-level automatic `cache_control` has no IR field. In `extensions` it is data that nothing emits, so it is
  still lost (§9 KC-Q1).

**Reserved IR keys.** `extensions` is flattened, so a client key that equals an IR typed wire key is lifted into that
typed field when the component deserializes the request. A Messages client could write `reasoning_budget_tokens`,
`parallel_tool_calls`, `sampling`, `response_format`, or `host_values` at the top level and have it read as typed IR, and
a client `host_values` key next to a host-filled `host_values` serializes the key twice (§5.3). The Messages decode
refuses, with 400, a top-level key that is an IR wire key and is not part of the Messages wire:
`reasoning_mode`, `reasoning_effort`, `reasoning_budget_tokens`, `parallel_tool_calls`, `sampling`,
`response_format`, `host_values` (§9 KC-Q5 for refuse versus drop).

**Unknown TTL, kind or marker key (K6).** Every marker the codec reads (system, content, tool, `tool_result`, unknown
block) goes through `CacheControl`'s deserializer; a failure is a `CodecError` naming the path
(`messages[3].content[1].cache_control.ttl`) and becomes the host's 400. Nothing is forwarded.

### 3.2 OpenAI Chat decode (`request.rs`)

- `part_from_wire`: `text` and `image_url` parts read `cache_control` (K4); the `Unknown` fallback checks a marker as in
  §3.1. Chat `tools[i]` has no marker in OpenRouter's convention, so `ToolDef.cache_control` stays `None` here.
- `tools[i].function.strict` → `ToolDef.strict` (dropped today).
- `reasoning_effort` and `parallel_tool_calls` join `TYPED_TOP_LEVEL_KEYS` and are set on the typed fields. Leaving them
  in the `extensions` loop while also setting the typed field would serialize each key twice. A non-string effort or a
  non-boolean `parallel_tool_calls` becomes a 400: the kernel refuses them at deserialization since 0.6.0, so keeping
  them as extension data would fail later, at the component, as an internal error.
- Reserved IR keys, as §3.1, minus the keys that are this wire's own: `reasoning_mode`, `reasoning_budget_tokens`,
  `sampling`, `host_values`. **Unverified, and a finding beyond KC1:** today the Chat decode copies a client
  `sampling` key into `extensions` while the typed `sampling` is always serialized (it has no `skip_serializing_if`),
  so the IR JSON carries `sampling` twice. Whether serde then fails or keeps one is not checked; either is wrong. The
  same refusal closes it.

### 3.3 Responses decode (`responses/request.rs`, `responses/tools.rs`)

- `reasoning.effort` → typed `reasoning.effort` instead of `extensions["reasoning_effort"]`; `reasoning.summary` stays
  the `responses_reasoning_summary` extension (not promoted by PR #42).
- `parallel_tool_calls` → typed.
- `strict` → `ToolDef.strict` on each tool. Transition (K9): the codec also keeps writing the `responses_tool_strict`
  map during 0.56.x, so a consumer built before this change, and the host's own readers if any (**unverified**: none
  found in `server:gateway/src`), see what they saw; it stops when the host confirms it no longer reads the map
  (§9 KC-Q8).
- Responses has no inline cache marker on input parts; nothing changes there.

### 3.4 NC-1 and the Claude carrier

The replay markers (`reasoning_replay_protocol_family`, `reasoning_replay_block_layout`,
`responses_opaque_reasoning_items`, `responses_reasoning_replay_items`) are `Message.extensions` keys; the typed
reasoning field is request-level configuration. They do not overlap and nothing in NC-1 moves. Two contact points:

- The codec's merge of adjacent assistant messages (`merge_extensions`, NC-1 §2.2) and the `materialize_replay_message`
  step build `Text` parts; they gain `cache_control: None`. Responses input carries no markers, so a merged run never
  holds one.
- A replayed Claude `thinking` block requires `thinking` to be enabled upstream. Today the Anthropic builder adds
  `thinking` only from the effort rule; after this change a Messages client's own `thinking` is sent verbatim (§4.1),
  so a replay that worked on the native row now also has its configuration on the component row.

The promotion of the NC-1 keys themselves to typed IR (NP-Q1's note) is not in PR #42 and not in this change.

## 4. Dialect builders

### 4.1 Anthropic Messages and Bedrock Invoke (`reference_anthropic.rs`, shared `body_of`)

Invoke still only removes `model` / `stream` and adds `anthropic_version`, so everything below applies to both.

| IR | Emitted |
|---|---|
| `Text` / `ImageUrl` marker | `cache_control` on the block, verbatim (`{"type":"ephemeral"}` or with `ttl`; an explicit `5m` stays explicit) |
| `ToolDef.cache_control`, `ToolDef.strict` | `tools[i].cache_control`, `tools[i].strict` |
| `Role::Tool` with `Content::Parts` | `tool_result.content` as a block array; the last part's marker becomes the `tool_result` block's `cache_control`; a marker on an earlier part stays on its inner block |
| System parts | a string (joined with `\n`, empty text skipped) when no system part carries a marker; otherwise a block array, one `text` block per non-empty part, markers kept |
| `Sampling.top_k` | `top_k`, under the same rule as `temperature` / `top_p` (below) |
| `reasoning` with `mode` or `budget_tokens` set | `thinking: {type: <mode>, budget_tokens?: <budget>}` verbatim; `effort`, if set, as `output_config.effort` verbatim |
| `reasoning` with only `effort` | today's per-model rule (`Dialect::thinking`), reading the typed field |
| `ChatRequest.parallel_tool_calls == Some(false)` with tools | §9 KC-Q6 |
| `host_values["end_user_hash"]` | `metadata: {user_id: <value>}` (§5) |

The verbatim arm does not consult the model's declared thinking shape (`anthropic.thinking.adaptive` / `.budget`): the
caller wrote an Anthropic `thinking` object, and the native row sends it unchanged, so the component does too and the
upstream rules on it. The declared shape still governs a request whose reasoning came as an effort only (OpenAI
clients). A budget that is not below `max_tokens`, or a budget beside a forced tool choice, is sent and refused upstream,
as on the native row.

**`top_k` and the sampling rules.** `keeps_sampling` already drops `temperature` and `top_p` when thinking is on or the
model declares `anthropic.sampling.none`. `top_k` follows the same predicate: thinking is incompatible with `top_k`, and
Opus 4.7 and later reject `top_k` like the other two (Anthropic's model notes, **unverified** by capture). "Thinking on"
now means the effort rule produced a thinking form, or the typed mode is `enabled` or `adaptive`. This is a recorded
difference from the native row, which forwards `top_k` and lets the upstream refuse it; the host's dual run must not
combine `top_k` with thinking. `anthropic.sampling.exclusive` does not touch `top_k`. The doc comment of
`SAMPLING_NONE` changes to name all three parameters.

**Marker positions and limits.** The component does not count breakpoints (at most four) or enforce the TTL ordering
(a 1h entry before any 5m entry); the upstream refuses both, as on the native row. A marker on an empty text part that
the builder skips is dropped with the part (the upstream refuses a marker on empty text).

**The existing system fixture** (`provider.request.system-joins-and-skips-empty-text`) carries no marker, so its
expected string output does not change.

### 4.2 Bedrock Converse (`reference_bedrock_converse.rs`; both Converse packages)

**`cachePoint` (K7).** AWS documents `CachePointBlock {type: "default", ttl?: "5m" | "1h"}` in `system`, message
`content` and `toolConfig.tools`, placed as its own entry after the content it closes; Claude models from Sonnet 4.5 on
take both TTLs, Claude 3.7 Sonnet, Claude 3.5 Sonnet v2 and Nova take 5m only, Nova takes no tool checkpoint, and AWS
warns that a `ttl` on a 5m-only model "can result in a `ValidationException`" (AWS Bedrock user guide, "Prompt caching",
and the `CachePointBlock` API reference, read 2026-10-12; **unverified** by capture). So 1h is supported, and K7's
fallback applies per model rather than to Converse as a whole.

Converse also serves models with no explicit caching, where a `cachePoint` is a likely 400 (**unverified**). Sending one
unconditionally would turn a Messages request that succeeds today on such a row into a failure. So emission is declared
per model in `supported_parameters`, like the `anthropic.*` dialect words:

| Word (proposed) | Effect |
|---|---|
| `bedrock.converse.cache_point` | markers become `cachePoint` entries; without it every marker is dropped, as today |
| `bedrock.converse.cache_point.ttl_1h` | a `1h` marker keeps `ttl: "1h"`; without it a `1h` marker is sent with no `ttl` (5m) and the downgrade is recorded here (K7) |
| `bedrock.converse.cache_point.tools` | a tool marker becomes a `cachePoint` in `toolConfig.tools`; without it a tool marker is dropped (Nova) |

`5m` and an absent TTL both emit no `ttl` key: the AWS default is 5m, and the field is the documented failure on
5m-only models. A marker on a `Role::Tool` part becomes a `cachePoint` after that `toolResult` block in the user turn.
A marker on an `Unknown` part is dropped with the part (Converse drops unknown parts today).

The downgrade under-uses the cache rather than over-charging: a 5m write costs less than a 1h write. §9 KC-Q7 asks
whether a word, a fixed rule, or the existing `anthropic.*` words should gate this.

**Usage.** Converse reports `cacheWriteInputTokens` as one number today and `usage_of` keeps the 5m / 1h fields at
zero. `TokenUsage.cacheDetails` (an array of `{ttl: "5m" | "1h", inputTokens}`, "Sorted by TTL duration (1h before
5m)", AWS API reference) splits it. Once the component sends 1h checkpoints, a flat write billed at the 5m price is an
under-charge, so this change parses `cacheDetails` in the same step: each entry fills `cache_write_5m_tokens` or
`cache_write_1h_tokens`; a repeated TTL, an unknown TTL, or a sum different from `cacheWriteInputTokens` is a provider
protocol error (the rule `usage_of` already applies to `totalTokens`); an absent `cacheDetails` with writes stays flat,
as today. The stream's `metadata` event carries the same `TokenUsage` (**unverified** for `cacheDetails` in streams).
§9 KC-Q7 also covers how the host prices a flat write on a row that sent a 1h checkpoint.

**Reasoning and `top_k`.** Claude's fields ride in `additionalModelRequestFields`. The verbatim `thinking` arm of §4.1
applies to Converse only when the model declares an `anthropic.*` dialect word, because a Converse row may serve a model
for which a Claude `thinking` object is an error; otherwise the typed mode and budget are dropped, and the effort rule
applies as today. `top_k` follows the same gate, as `additionalModelRequestFields.top_k`, under §4.1's sampling rule
(§9 KC-Q7).

`metadata.user_id`: Converse has `requestMetadata` (string tags for invocation logs), which is not an abuse-attribution
id; nothing is mapped and the Converse packages do not declare `end_user_hash`.

### 4.3 OpenAI-compatible (`reference.rs`)

- Markers: dropped, never refused. `Text` / `ImageUrl` are rendered field by field, so the marker is left out by
  construction; an `Unknown` part of a native type (`input_audio`, `file`) that carries a marker is forwarded verbatim
  today, so the builder removes `cache_control` from it. Explicit caching on OpenAI-compatible upstreams that accept a
  marker (for example Bailian) is a later, declared feature.
- `strict`: `ToolDef.strict`, falling back to the `responses_tool_strict` map (K9). Chat-origin `strict` now reaches the
  upstream; the native OpenAI row already forwards it.
- `parallel_tool_calls`, `reasoning.effort`: typed, same rules (`parallel_tool_calls` only beside tools; the effort
  gate unchanged).
- `reasoning_effort_allowed`'s "came from an Anthropic thinking translation" test reads the
  `extensions["anthropic_thinking"]` key, which nothing in south writes (**unverified** who does). It becomes "the typed
  mode is `enabled` or `adaptive`", with the old key as a fallback. Mode and budget are not mapped to an effort (the
  kernel allows a declared rule; none is proposed).
- `top_k`: dropped (the OpenAI wire has none; the kernel's `Sampling` rule).
- `end_user_hash`: not declared (§9 KC-Q4).

### 4.4 Gemini (`reference_gemini.rs`)

- Markers: dropped (Gemini caches through `cachedContents` resources, not inline markers).
- `top_k` → `generationConfig.topK`. New: a Chat client's `top_k` was extension data before and never sent.
- Reasoning: unchanged, dropped (no `thinkingConfig` mapping exists today; adding one is separate work).
- `strict`, `parallel_tool_calls`: no Gemini equivalent; dropped.

### 4.5 OpenAI Responses (`reference_openai_responses.rs`)

- `strict`: per-tool typed, map fallback; `parallel_tool_calls` typed (still only beside tools); `reasoning.effort`
  typed, the gate as in §4.3.
- Markers dropped (Responses caches by `prompt_cache_key`, not inline); `top_k` dropped.
- NC-1 behaviour (replay items, `include`) unchanged.

## 5. The `host_values` word for `metadata.user_id`

### 5.1 The word

| | |
|---|---|
| Key | `end_user_hash` (`HOST_VALUE_END_USER_HASH`), added to `HOST_VALUES` |
| Meaning | An opaque, per-tenant pseudonym of the caller's end user, minted by the host, stable for one end user within one tenant and unlinkable across tenants. South does not define how it is computed. |
| Syntax | 1 to 128 bytes of `[A-Za-z0-9_-]` (inside the kernel `ComponentValues` grammar; Anthropic documents `metadata.user_id` as an opaque id such as a UUID or a hash, with a 256-character maximum, **unverified** against the current API reference). `south-provider-api` exports a validator for hosts, as it exports `attempt_id`'s definition. |
| When present | Only when the package declares it, the request carries an end-user id, and the host's policy produces a value. Absent means "send no `metadata`". |
| Declared by | `provider-anthropic`, `provider-anthropic-bedrock-invoke` (§5.2) |

The component validates the syntax and refuses a malformed value with an `internal` error (a host bug, not a client
error) before building anything. A client's own `metadata.user_id` never enters the IR (§3.1), so without the host
value the upstream receives no `metadata`. That is a recorded difference from today's native rows, which forward the
raw id (§9 KC-Q4 covers the transition).

### 5.2 Bedrock Invoke

The host's native `aws_claude` row forwards the client body unchanged apart from `model` / `stream` /
`anthropic_version` (`server:gateway/src/modules/inference/engine/bedrock.rs:246-273`), so `metadata` reaches
InvokeModel today. Whether InvokeModel accepts `metadata` is not established: it refuses unknown fields
("Extra inputs are not permitted" for `stream`), and no capture with `metadata` exists (**unverified**). The test plan
(§7.3) adds a capture before the release. If InvokeModel refuses it, the Invoke package does not declare the word, and
the shared builder emits `metadata` only for the Messages family.

### 5.3 What the host must do (south contract only)

- Fill `end_user_hash` only for a package that declares it, as for `attempt_id` (`server:…/south_component.rs`,
  `with_host_values`).
- Remove any `host_values` key from `ChatRequest.extensions` before handing the request over. The host clears the typed
  map today but not an extension of the same name; once a package reads `end_user_hash`, a client-sent `host_values`
  key would sit next to the host's map in the serialized JSON. §3's codec refusal closes this for requests the codecs
  build; the host strip covers any other producer.
- The hashing scheme, the salt and its rotation, and which tenants or rows get a value are the host's design (K3).

## 6. Behaviour changes a host will see

| Request | 0.55.0 component row | After |
|---|---|---|
| Messages with block / tool / `tool_result` markers → Anthropic or Invoke row | markers dropped | sent verbatim |
| Same → Converse row | dropped | `cachePoint` where declared; else dropped |
| Same → OpenAI-compatible, Gemini, Responses rows | dropped | dropped (fixtures pin "not refused") |
| Chat with OpenRouter part markers → Anthropic row | dropped | sent |
| Any marker with an unknown TTL, kind or key | dropped, 200 | 400 at decode |
| Messages `thinking` → Anthropic / Invoke row | dropped | sent verbatim |
| Messages `top_k` | dropped | sent (Anthropic, Invoke, Gemini, Converse where gated) unless thinking / `sampling.none` drops it |
| Chat `function.strict` → OpenAI-compatible | dropped | sent |
| Client top-level key equal to an IR wire key | extension data (lifted silently) | 400 |
| `metadata.user_id` | dropped | host hash where declared and supplied |

## 7. Test plan

Tests only; names are proposals.

### 7.1 North codec (`crates/south-north-codec/tests`)

- `anthropic_messages.rs`: system block array with and without markers (one part per block, a string system unchanged);
  `cache_control` with and without `ttl` on `text`, `image`, an unknown `document` block, a tool and a `tool_result`
  (string content, block content, marker on the block versus on the last inner block); `top_k` (valid, negative, above
  `u32::MAX`, non-integer); each `thinking` row of §3.1 including `budget_tokens` carried verbatim and
  `output_config.effort: "max"` as `Other`; the other-keys loop (`service_tier` lands in `extensions`, `metadata` does
  not); every reserved key refused; unknown TTL, unknown `type`, an extra marker key, each refused with its path.
- `openai_chat.rs`: part markers on `text` / `image_url` / an unknown part; `function.strict`; `reasoning_effort` and
  `parallel_tool_calls` typed and absent from `extensions`; non-string effort and non-boolean parallel refused; reserved
  keys refused (including the duplicate `sampling` case of §3.2).
- `responses.rs`: effort, parallel and `strict` typed; the strict map still written (§3.3).
- **IR wire round trip:** for each new decoder case, the `ChatRequest` serializes to a committed golden IR JSON and
  deserializes back equal; and a golden of a 0.55.0 decode with none of the new inputs is byte-identical before and
  after (the kernel's `the_0_5_chat_request_fixture_gains_no_new_wire_keys`, on south's own decoder output).
- `responses_openai_replay.rs`, `responses_replay.rs`: unchanged expectations (§3.4).
- Fuzz corpus (`fuzz/`): seed inputs with markers, `thinking` and reserved keys.

### 7.2 Gate ② fixture rows (`crates/south-component-conformance/fixtures-*`)

| Pack | New or changed cases |
|---|---|
| `fixtures-anthropic`, `fixtures-anthropic-bedrock-invoke` | `provider.request.cache-control-system-blocks`, `-message-blocks-1h`, `-tool`, `-tool-result`; `provider.request.top-k`; `provider.request.top-k-dropped-with-thinking`; `provider.request.top-k-dropped-under-sampling-none`; `provider.request.thinking-verbatim-enabled-budget`, `-adaptive-with-effort`, `-disabled`; `provider.request.strict-tool`; `provider.request.metadata-user-id-from-host-value`; `provider.request.malformed-end-user-hash-refused`; `system-joins-and-skips-empty-text` unchanged |
| `fixtures-bedrock-converse`, `-bearer` | `provider.request.cache-point-system-messages-tools`; `-1h-declared`; `-1h-downgraded`; `-undeclared-dropped`; `-tools-undeclared-dropped`; `provider.request.thinking-verbatim-claude`; `provider.request.top-k-claude`; `provider.response.cache-details-split`; `provider.response.cache-details-mismatch-refused` |
| `fixtures` (OpenAI-compatible) | `provider.request.cache-control-dropped-not-refused` (text, image and a native unknown part); `provider.request.strict-typed`; `provider.request.strict-map-fallback`; `provider.request.parallel-tool-calls-typed` |
| `fixtures-gemini` | `provider.request.cache-control-dropped-not-refused`; `provider.request.top-k` |
| `fixtures-openai-responses` | `provider.request.cache-control-dropped-not-refused`; typed strict / parallel / effort cases beside the existing extension-key cases, which become map-fallback cases |

Every existing case must keep its expected output except the ones this table lists as changed. Gate ②'s
`undeclared_values_ignored` needs no change; the `end_user_hash` cases run with a manifest that declares it.

### 7.3 Other tests

- `south-provider-api` `component_values_v1.rs`: `HOST_VALUES == [attempt_id, end_user_hash]`; the syntax validator's
  accept / refuse table; gate ① still refuses an unknown word.
- `shipped_packages_v1.rs`: §2.4's retirement test; the two Anthropic-family manifests declare `end_user_hash` (or only
  `provider-anthropic`, per §5.2).
- `anthropic_dialect.rs` unit tests: the effort rule reads the typed field; `keeps_sampling` drops `top_k`; the verbatim
  arm ignores the declared shape.
- A conformance test that every reader of §2.3 is gone from `extensions`: a request with only typed fields set produces
  the same body as today's extension-keyed fixture.
- **Captures before release** (not CI): InvokeModel with `metadata` (§5.2); Converse `cachePoint` with `ttl: "1h"` on a
  1h model and on a 5m-only model, and the `cacheDetails` it returns, streaming and not (§4.2).

## 8. Versioning, release and coordination

- Release: the next minor, 0.56.0 by default. `compatibility.json` `release.version` and `kernel_contracts.canonical_ir`
  4; all twenty package identities (§2.4); every `south_runtime` declaration moves to the release; a release record in
  the usual form.
- Normative text revised in step: the canonical IR inventory (D5 amendment: the three R-Q15 keys are promoted and no
  longer precedent), the Responses record (R-Q15 marked done), the Bedrock Invoke record (the Messages-surface loss
  list), the boundary record §13.8 (the new `host_values` word), and the Anthropic provider record's D6 ("`system`
  takes text only" now has a marked-block exception).
- Host follow-up (not south): re-pin kernel and south, `canonical_ir` 4, the `end_user_hash` value, the extension
  strip (§5.3), the new `supported_parameters` words in its reserved-namespace check, and the dual-run test the host KC1
  design §6 names.
- **Coordination, not resolved here.** Another session plans R3 (Codex) and four further Responses fields for 0.56.0.
  Both changes edit `south-north-codec/src/responses/{request,tools}.rs`, `reference_openai_responses.rs`, the
  `fixtures-openai-responses` pack and the `provider-openai-responses` version, and this one changes every package's
  identity and the IR tuple. If R3 ships first on protocol 0.5.0, its packages carry `canonical_ir` 3 and must change
  identity again when KC1 ships; if both target one release, one branch rebases onto the other and the package version
  table is settled once. Which order, and which release number each takes, is for lv and the two sessions to agree
  (§9 KC-Q10).

## 9. Questions for lv

- **KC-Q1** Top-level automatic `cache_control` on a Messages request has no IR field (PR #42 did not add one).
  Options: (a) leave it lost and record it; (b) refuse it with 400 on component rows; (c) a later kernel field.
  *Recommended: (a) now, validating its TTL like other markers so an invalid one is still a 400, and (c) as a follow-up
  if captures show clients rely on it.*
- **KC-Q2** `thinking.type` values outside the closed `ReasoningMode` (Sonnet 5.5's `between_tools`). Options: 400 at
  decode (fail-closed, consistent with K6); drop the configuration (today's outcome, the model then thinks and bills
  more); a kernel follow-up adding the mode. *Recommended: 400 now, kernel follow-up.*
- **KC-Q3** `thinking.display` (`summarized` / `omitted` / `updates`) has no IR field and is lost. *Recommended: record
  it as a known loss in KC1; request a kernel field with the KC-Q2 follow-up.*
- **KC-Q4** `metadata`. (a) The decoder keeps the raw `metadata` out of `extensions` (recommended); (b) only the two
  Anthropic-family packages declare `end_user_hash`; OpenAI-family packages do not map it to `user` /
  `safety_identifier` in this change (recommended); (c) a component row receives no `metadata` until the host ships the
  hash, while native rows forward the raw id: acceptable as a transition (recommended), or should the host hold
  Anthropic component cut-overs until the hash ships?
- **KC-Q5** A client top-level key that equals an IR wire key (`host_values`, `sampling`, the reasoning keys on surfaces
  that do not own them): 400 (recommended; no legitimate client sends them and silent lifting is the alternative) or
  drop.
- **KC-Q6** `parallel_tool_calls` on the Anthropic family: map Messages `tool_choice.disable_parallel_tool_use` to the
  typed field on decode and back on build, so a Chat client's `parallel_tool_calls: false` also reaches Claude?
  *Recommended: yes, in this change; it uses only the field PR #42 added and today's decode drops the flag.*
- **KC-Q7** Converse gating. (a) The three `bedrock.converse.cache_point*` words (recommended), or emit `cachePoint` for
  every model that declares any `anthropic.*` word; (b) the verbatim `thinking` and `top_k` gated on any `anthropic.*`
  word (recommended) or on a new word; (c) a flat cache write without `cacheDetails` on a row that sent a 1h checkpoint:
  the host prices it at the 1h write price (recommended, fail toward over-reserve) or the 5m price.
- **KC-Q8** When does the Responses codec stop writing the `responses_tool_strict` map? *Recommended: in the first
  release after every host consumer reads `ToolDef.strict`; the consumers' fallback stays until then.*
- **KC-Q9** `tool_result.is_error` and Messages `output_config.format` (structured outputs) remain lost on component rows
  (no IR field). *Recommended: record both; neither blocks the Bedrock Invoke cut-over that KC1 gates, but `is_error`
  changes model behaviour and should be next in line for a kernel field.*
- **KC-Q10** Release order with R3 (§8): KC1 in 0.56.0 and R3 after, R3 in 0.56.0 and KC1 in 0.57.0, or both in one
  release. *No recommendation; for lv and the two sessions.*
- **KC-Q11** Effort-only requests to the Anthropic family with a level outside the dialect's table (`max`, which
  Anthropic now documents): today a 400 capability error. Keep it, or send `Other` levels verbatim to models that
  declare the adaptive shape? *Recommended: keep the 400 in KC1; a Messages client's `max` already reaches the upstream
  through the verbatim arm because it comes with `thinking`.* (Not covered: a Messages client that sends
  `output_config.effort` without `thinking` goes through the effort rule; recorded.)

### 9.1 Rulings (lv, 2026-10-12)

- KC-Q1 to KC-Q9 and KC-Q11: as recommended.
- KC-Q10: R3 (`openai-codex` and the four Responses fields) ships first as 0.56.0; KC1 rebases onto it and ships as 0.57.0.
- Upstream tag `kernel-v0.6.0` now exists (annotated, on `c0bca10c`); the kernel mirror is synced to it with
  `canonical_ir` 4 and is released as mirror `v0.5.0` once its gates pass.

## 10. Unverified, collected

- *(Resolved 2026-10-12: upstream `kernel-v0.6.0` is annotated on `c0bca10c`; mirror `v0.5.0` is `356445c1`, whose
  `compatibility.json` records `canonical_ir` 4 and `source_commit` `c0bca10c`.)*
- AWS Converse `cachePoint` TTL support per model and the `cacheDetails` shape come from AWS documentation, not capture;
  whether `cacheDetails` appears in the stream's `metadata` event is not checked.
- Whether InvokeModel accepts `metadata` (§5.2).
- That Opus 4.7 and later reject `top_k` like `temperature` / `top_p` (§4.1).
- What serde does with a duplicated `sampling` or `host_values` key in the IR JSON (§3.2, §5.3).
- Who writes the `anthropic_thinking` extension key read in §4.3.
- Anthropic's `metadata.user_id` length limit (§5.1).

## 11. Implementation (2026-10-12)

Branch `kc1-canonical-ir-4`, rebased on R3 (`45d068a`). Commits, in order: the re-pin with the R-Q15 readers (§2), the
`end_user_hash` word (§5), the north codec tests then the codec (§3), the gate ② rows then the dialect builders (§4),
the top-level marker check (KC-Q1), and the records. The mirror is pinned at `356445c15b205f610f060b2f26ca9daa690cbd88`
(tag `v0.5.0`); every manifest declares `kernel_revision` `c0bca10c2d9f9a439680865feb65c9edf598fc65`.

Where the implementation is more specific than §3 to §5:

- **System prompts stay a string unless marked.** The Messages decode keeps one part per system block only when a block
  carries a marker; an unmarked array is the joined string it always was. So no unmarked request changes its IR, and
  the existing system fixture is unchanged. The same holds for `system` / `developer` turns inside `messages`.
- **A marked tool result carries text only.** As the flattened form did, a non-text block inside a `tool_result` is
  not carried; a marker on such a block is checked and dropped.
- **Markers with no IR slot.** A valid marker on a `tool_use`, `thinking` or `redacted_thinking` block is checked and
  dropped (the kernel has no slot there); recorded as a loss beside KC-Q9.
- **Dialects without inline markers render a marker-only part list as its joined text.** A system prompt or tool result
  that is a part list only because of its markers is rendered as the `\n`-joined string it would otherwise have been, by
  OpenAI-compatible (system and tool turns), Gemini (`systemInstruction`, `functionResponse`), Responses
  (`instructions`, system items, `function_call_output`) and Converse (`system` without `bedrock.converse.cache_point`,
  and every `toolResult`, which cannot hold a `cachePoint`). A marker therefore changes nothing else about a request on
  these wires; the gate ② rows pin it.
- **The codec reports the new refusals with the existing `unknown_value` and `out_of_range` codes**, with the field
  path, so a host's mapping of `CodecError` needs no new arm.
- **Converse** emits a caller's `thinking` and `top_k` only for a model that declares at least one `anthropic.*` dialect
  word (KC-Q7 b), and exports `CACHE_POINT_PARAMETERS` for hosts that validate the reserved namespace.
- **Bedrock Invoke declares `end_user_hash`.** The evidence is indirect: the host's native `aws_claude` row forwards the
  client body, `metadata` included, and serves Claude Code traffic. A capture is still owed (§10).
- **`south_runtime`** stayed unchanged on the branch and moved to 0.57.0 in the release commit (§2.4); until then
  `scripts/check-declared-runtime.sh` refused the packages under their old declarations, as it did during the 0.46.0
  re-pin.

### 11.1 Failing first, and the checks

- North codec: with the codec sources of the re-pin commit and the new tests, all 15 tests of
  `south-north-codec/tests/canonical_ir_4.rs` fail; with the change they pass.
  `tests/unchanged_ir.rs` pins, byte for byte, the IR the codecs produced before the change for an unmarked Messages
  request and an unmarked Chat request.
- Gate ②: with the conformance sources of the commit before the dialect change and the new rows, six suites fail
  (`gate_two_passes_over_the_shipped_pack` for Anthropic, Bedrock Invoke, Converse, Gemini and Responses, and the
  OpenAI-compatible pack through `gemini_openai_compatible_v1`); with the change they pass.
- The release replay (components built by the release scripts, gate ② reports over those bytes, `release_index.py
  generate` with `--require-gate2-reports`, then `compare` against the published v0.55.0 index) passes: every
  package changed version, the catalog is unchanged. `check_crate_versions.py` reports `south-provider-api` and
  `south-component-conformance` moved and `south-contracts` unchanged.
- `scripts/check-declared-runtime.sh --build` fails for every group, as expected until the release step (§2.4): each
  older runtime records `canonical_ir` 3, and the 0.46.0 runtime also does not know `end_user_hash`.
