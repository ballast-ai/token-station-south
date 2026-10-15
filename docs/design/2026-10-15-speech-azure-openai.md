# Azure OpenAI speech: the `azure-openai-v1` and `azure-openai-dated` families of `speech-openai-compatible`

Status: accepted under lv's 2026-10-15 ruling (speech on Azure OpenAI and Azure Foundry rows needs a south speech
family following the embeddings `azure-openai-v1` precedent: the `api-key` header,
`{base}/openai/deployments/{deployment}/audio/speech?api-version={dated version}` for synthesis, `/audio/transcriptions`
on the same shape, translation if native serves it; not in 0.66.0, shipped in the next release) and lv's standing
directive of the same day (migrate by the native code's logic, speed first; where native is doubtful, reproduce it and
register the doubt as **unverified**; no real-key captures, no production queries). Every open question (§9) carries a
recommended default, and the implementation (§7) implements the defaults.

Date: 2026-10-15

Baseline: south `origin/main` = `6e3fca7` (after #240, which added `azure-openai-dated` to `provider-openai-compatible`).
Host references carry a `server:` prefix and refer to token-station-server `origin/dev-v2` at `70f9aa5c`;
`server:…/` abbreviates `server:gateway/src/modules/inference/`. Statements marked **unverified** are conclusions from
reading code, not from running anything against an upstream or capturing traffic.

Predecessors: the speech world record [`2026-09-30-speech-world.md`](2026-09-30-speech-world.md); batch records
[V3-1](2026-10-14-speech-v3-1-tts.md) (§2, the `speech-openai-compatible` synthesis; SV-Q1 (a), "Azure as a later minor
with its own fixtures when a row needs it") and [V3-3](2026-10-14-speech-v3-3-asr.md) (§2, its recognition); the T9
record [`2026-10-15-openai-compatible-t9.md`](2026-10-15-openai-compatible-t9.md) (`azure-openai-dated` in the provider
world: the `api_version` key, the `api_version_date` syntax, the default `2024-10-21`, the escaped deployment segment,
the `api-key` header); the embeddings contract record [`2026-09-30-embeddings-contract.md`](2026-09-30-embeddings-contract.md)
§6 (`azure-openai-v1` in `embeddings-openai-compatible`).

Not to be confused with `speech-azure` (family `azure-speech`, V3-3 §5 and V4): that package serves the **Azure Speech
service** (`<resource>.cognitiveservices.azure.com`, `ocp-apim-subscription-key`, SSML synthesis and
`speechtotext/transcriptions:transcribe`), native provider type `azure_speech`. This record is about the **Azure OpenAI**
and **Azure Foundry** resources (`<resource>.openai.azure.com`, `<resource>.services.ai.azure.com`), native provider
types `azure_openai` and `azure_foundry`, which speak the OpenAI audio protocol.

## 1. What this delivers

| Item | Where | Change |
|---|---|---|
| `azure-openai-v1` family | `speech-openai-compatible` 1.0.1 → **1.1.0** | The GA v1 surface: `audio/speech`, `audio/transcriptions` and `audio/translations` below the base `{endpoint}/openai/v1`, `header_secret` `api-key` |
| `azure-openai-dated` family | same | The deployment-addressed surface: `openai/deployments/{deployment}/audio/{speech,transcriptions,translations}?api-version={version}` below the row endpoint, `header_secret` `api-key`, the config key `api_version` (`api_version_date`, default `2024-10-21`) |
| Fixtures | `crates/south-component-conformance/fixtures-speech-openai-compatible/` | Eleven new rows and the wrong-family row moved off `azure-openai-v1` (§6) |
| Tests | `tests/speech_azure_openai_v1.rs`; `shipped_packages_v1` | Native URLs, auth, refusals and the family check pinned (§6) |
| Versions | every package, `south-component-conformance` | The 0.67.0 cycle's Q47 round (§8) |

## 2. Native behaviour (server `origin/dev-v2` `70f9aa5c`)

Native has no Azure-OpenAI-specific speech arm. An `azure_openai` or `azure_foundry` row falls into the catch-all arms
of both speech surfaces, and only the URL builder and the auth function know the provider type.

### 2.1 Arm selection

| Behaviour | Native | Source |
|---|---|---|
| TTS arm | `TtsUpstream::resolve` tests Vertex, the `xai` / `xiaomi` names, Bailian, MiniMax, `azure_speech`, the `groq` name and ElevenLabs; anything else, `azure_openai` and `azure_foundry` included, is `TtsUpstream::OpenaiCompat` | `server:…/handler/audio/tts.rs:107-176` (fallback `:174`) |
| ASR arm | `AsrUpstream::resolve` tests ElevenLabs, the `xai` name and `azure_speech`; anything else is `AsrUpstream::OpenaiCompat` | `server:…/handler/audio.rs:91-101` |
| Translation | Only `OpenaiCompat` supports `/v1/audio/translations`; it forwards the entry path unchanged | `audio.rs:84-89`, refusal for other arms `:204-211`, endpoint `:302-311` |
| Provider types | `azure_openai`: "deployment-name addressing and `?api-version=`", hosts `<resource>.openai.azure.com` and `<resource>.services.ai.azure.com`; `azure_foundry`: host `<resource>.services.ai.azure.com`, `api-key` | `server:gateway/src/infra/config/providers.rs:153-160, 209-219` |

### 2.2 URL

| Behaviour | Native | Source |
|---|---|---|
| Canonical paths | TTS `/v1/audio/speech`; ASR the entry path `/v1/audio/transcriptions` or `/v1/audio/translations` | `tts.rs:692-700`; `audio.rs:302-313` |
| Builder | `build_upstream_url(provider, Some(model), path)`, the endpoint with trailing slashes trimmed | `tts.rs:741-745`; `audio.rs:313`; `server:…/engine/upstream.rs:425-430` |
| `azure_foundry` | `{endpoint}/openai/v1{path}` with `/v1` stripped from the path (`/mai/v1` only for `/images/…`, not a speech path); never dated | `upstream.rs:537-550` |
| `azure_openai` version | `model.azure_api_version` > `provider.api_version` > `AZURE_OPENAI_DEFAULT_API_VERSION` = `2024-10-21` | `upstream.rs:5-10, 569-572`; `server:gateway/src/infra/config/models.rs:326` |
| `azure_openai`, version `v1` (any case) | `{endpoint}/openai/v1{path}`, the deployment carried in the body's `model` | `upstream.rs:573-575` |
| `azure_openai`, any other version | `{endpoint}/openai/deployments/{upstream_model}{path}?api-version={version}`; neither the deployment nor the version is escaped or validated; the endpoint used verbatim (no host sniffing) | `upstream.rs:551-581` |

So for speech: `{endpoint}/openai/deployments/{d}/audio/speech?api-version={v}`,
`…/audio/transcriptions?api-version={v}` and `…/audio/translations?api-version={v}` on a dated `azure_openai` row; and
`{endpoint}/openai/v1/audio/{speech,transcriptions,translations}` on a `v1` `azure_openai` row and on every
`azure_foundry` row.

### 2.3 Auth and headers

`api-key: <api_key>` for both types, no Bearer (`upstream.rs:190-196`), applied by `apply_provider_auth` on both
surfaces (`tts.rs:779`, `audio.rs:337`). The TTS request is `content-type: application/json` (`tts.rs:751-760`); the
ASR request forwards the client's multipart `content-type` (`audio.rs:337-342`).

### 2.4 Bodies, responses, metering, errors

Exactly the `openai-compatible` family's, because they are the same arms:

- **TTS** (`tts.rs:692-700`): the client's JSON with `model` replaced by the upstream model (the deployment name on an
  Azure row, in both URL forms); `stream_format: "sse"` when the model is token-priced (`openai_token_tts`,
  `tts.rs:395-405`; only this arm may be token-priced, `:52-63`). The binary answer is billed by the request's
  characters; the SSE answer by `speech.audio.done` usage. Owned body fields `input`, `model`, `stream_format`,
  `response_format` (`tts.rs:86`).
- **ASR** (`audio.rs:252-261`): the client's multipart with the gateway alias spliced to the upstream model; owned form
  fields `model`, `file`, `response_format`, `stream` (`audio.rs:76`). The answer is passed through and billed by its
  `duration` when present, else 60 seconds marked estimated (`audio.rs:585-611`).
- **Errors**: the upstream status and body as for every `OpenaiCompat` row.

V3-1 §2 and V3-3 §2 transcribe these arms in full; nothing in them depends on the provider type.

## 3. Family mapping

**Decision (AO-Q1): two families of `speech-openai-compatible`, not a new package.** The wire is the OpenAI audio
protocol byte for byte; what differs from `openai-compatible` is the route (path, query) and the auth header, which is
exactly what a family selects. Every precedent puts Azure OpenAI in the package that speaks the wire as a family:
`provider-openai-compatible` (`azure-openai-v1`, then `azure-openai-dated` in T9) and `embeddings-openai-compatible`
(`azure-openai-v1`). SV-Q1 (a) already ruled "Azure as a later minor with its own fixtures when a row needs it". A new
package would duplicate the reference and its pack for a two-line difference, and `speech-azure` is a different product
(see the preamble).

**Decision (AO-Q2): the provider world's family names.** A host row has one dialect for every world; an `azure_openai`
row migrated to `azure-openai-dated` for text (T9) must find a speech package declaring the same family, and likewise
`azure-openai-v1` (T8). lv's ruling speaks of "a speech `azure-openai` family"; the two names below are that family in
its two URL forms, named as the provider and embeddings worlds name them.

**Decision (AO-Q3): both URL forms.** Native serves both from one provider type (the version value selects), and
`azure_foundry` rows are v1 only. Covering only the dated form would leave every Foundry row and every `v1`
`azure_openai` row without a speech target.

### 3.1 `azure-openai-v1`

- **Base.** `{endpoint}/openai/v1`, as the provider and embeddings families of the same name (T8). The API-root rule
  `speech-openai-compatible` already applies (the base's own path, `v1` for an origin) then gives `audio/speech`,
  `audio/transcriptions` and `audio/translations` below it: native's `{endpoint}/openai/v1{path}`.
- **Auth.** `{"arm": "header_secret", "header": "api-key", "slot": …}` on the configured slot.
- **Query.** None.
- **Everything else** is `openai-compatible`'s: the body or parts, `model` the upstream model (the deployment), the
  metering forms (`characters` and `tokens` for synthesis, `seconds` for recognition), translation on a row carrying
  `speech.translate`, the `speech.verbose_json` rows, readers, outcomes and errors.

### 3.2 `azure-openai-dated`

- **Base.** The row endpoint verbatim (`<resource>.openai.azure.com` or `<resource>.services.ai.azure.com`).
- **Path.** `openai/deployments/{deployment}/audio/speech`, `…/audio/transcriptions` or `…/audio/translations`,
  `deployment` the upstream model as one escaped path segment (the crate's `url_segment` encoder, as T9).
- **Query.** `[{"name": "api-version", "value": version}]`, `api-version` being a sanctioned query parameter
  (`QueryParameterV1::ApiVersion`), as `speech-azure` already sends it; the manifest declares no `query_parameters` and
  the package stays on `contracts.media` 1.
- **Version.** The config value `api_version`; absent, the component falls back to `2024-10-21`. The manifest declares
  the key with `provider-openai-compatible`'s `config_schema` entry byte for byte (`api_version_date`, default
  `2024-10-21`, the same description), so one host row's value serves text and speech; the reference reuses the
  provider world's constants (`reference::azure_openai_dated`) and its date check.
- **Auth.** `header_secret` `api-key`, as for `azure-openai-v1`.
- **Everything else** is `openai-compatible`'s, as above; the body still carries `model` (the deployment, unescaped),
  as native's does.

### 3.3 The family check

`model-capabilities` and `prepare` both start by mapping the configuration's family; anything other than
`openai-compatible`, `azure-openai-v1` and `azure-openai-dated` is a `capability` 400 ("this package serves the
openai-compatible, azure-openai-v1 and azure-openai-dated families"). The capabilities of an Azure row are the
`openai-compatible` family's for the same models.

## 4. Decisions taken where native has no counterpart

The T9 decisions carry over to the speech family unchanged:

- **D1. Family names** as §3 (AO-Q2).
- **D2. The default lives in two places**, the manifest `default` and the component's constant, both native's
  `2024-10-21`; a test pins them equal to the provider world's.
- **D3. A version that is not dated is refused** with `internal` 500 naming the key, before any request refusal (the
  configuration is checked first, as `image-vertex` and `provider-vertex` check theirs). The literal `v1` belongs to
  `azure-openai-v1`.
- **D4. An empty upstream model is refused** with `capability` 400 on the dated family; native would send
  `/openai/deployments//audio/speech`.
- **D5. The deployment is one escaped path segment.** Azure deployment names are letters, digits, `-`, `_` and `.`,
  which the encoder leaves unchanged.

## 5. Differences from native and unverified doubts

### 5.1 Differences

| # | Difference | Status |
|---|---|---|
| D1 | The per-model version override (`ModelConfig.azure_api_version`) has no counterpart: the version is a row value. | Host prerequisite H3 (shared with T9) |
| D2 | `v1` selects a family, not a value: a `v1` `azure_openai` row becomes an `azure-openai-v1` row with base `{endpoint}/openai/v1`. | Registered (T8, T9) |
| D3 | A version that is not `YYYY-MM-DD[-preview]` and not `v1`: native sends it; the family refuses it (§4 D3), and a host's validation refuses to save it. | Registered (T9 D6) |
| D4 | An empty deployment: native sends an empty segment; the family refuses it (§4 D4). | Registered |
| D5 | A deployment outside RFC 3986 `pchar`: native sends it raw; the family escapes it. Unreachable for valid Azure deployment names. | Registered (T9 D7) |
| D6 | Every registered difference of the `openai-compatible` family applies unchanged, because the arms are the same: V3-1 §10 and §11 (e.g. the delivered media type, the `sse` outcomes that native parks) and V3-3 §8 (part lists instead of byte splicing, `stream=true` and unknown formats refused before dispatch, a missing duration `unknown` or `null` instead of native's estimated 60 seconds, translation gated on `speech.translate`). | Inherited, as registered there |
| D7 | Operator header extras, `x-request-id` and `traceparent` forwarding, the trailing-slash trim of the endpoint: whatever the host does for component rows applies. | Host-generic |

### 5.2 Unverified doubts

| # | Doubt | Default |
|---|---|---|
| U1 | Whether Azure OpenAI honours `stream_format: "sse"` on `audio/speech`, which native sends for a token-priced model and the family reproduces. Native has no Azure test of it. | Reproduced; H6 |
| U2 | Whether the GA v1 surface serves the audio paths (`/openai/v1/audio/speech`, `/audio/transcriptions`, `/audio/translations`) on Azure OpenAI and Foundry resources (SV-Q1 already registered it). Native builds those URLs. | Reproduced |
| U3 | Whether `/audio/translations` exists on the dated surface for the deployed models at the default `2024-10-21`, and which models translate. Native forwards it; the row's `speech.translate` word decides as for every row. | Reproduced |
| U4 | Azure's error body. The 401 fixture uses Azure's documented `{"error": {"code": "401", "message": …}}` shape, not a capture; the reader takes `error.message` from either shape. | Fixture marked as such |
| U5 | Whether Azure's `gpt-4o-transcribe` deployments report `usage` with `type: "duration"` in a `json` answer, and whether `verbose_json` carries `duration` and `segments`, as V3-3 §9 already asks for OpenAI. | Same reader; same measurements |

## 6. Fixtures and tests

Eleven rows join `fixtures-speech-openai-compatible/`, transcribed from §2 (the URLs native builds, the `api-key` arm,
the bodies and parts of the shared arms) before the reference first ran:

- `prepare.azure-dated-chars` (an explicit `2025-03-01-preview`, the classic host, `mp3`), `prepare.azure-dated-default-version-tokens`
  (no value so `2024-10-21`, the `services.ai.azure.com` host, token-priced so `stream_format: "sse"`),
  `prepare.azure-v1-chars` (base `{endpoint}/openai/v1`, `wav`);
- `prepare.asr-azure-dated-transcribe` (a verbose row, `response_format` set to `verbose_json`),
  `prepare.asr-azure-dated-translate` (`audio/translations` on the row carrying `speech.translate`),
  `prepare.asr-azure-dated-text` (a plain row, the client's `text` forwarded), `prepare.asr-azure-v1-transcribe` (the
  Foundry host's v1 base);
- `capabilities.azure-dated-models`;
- `response.azure-dated-audio` (binary `audio/mpeg`, billed by characters), `response.azure-dated-status-401` (U4) and
  `response.asr-azure-dated-transcribe` (a `verbose_json` answer, `duration` billed).

`capabilities.wrong-family` named `azure-openai-v1` as the family this package refuses; it now names `azure_openai`
(the native provider type's word) and expects the three-family message.

Gate ② judges the pack natively (`openai_compatible_speech_suite`) and in the sandbox
(`openai_compatible_speech_sandbox_parity`, whose raw ABI comparison replays every call the suite makes, the Azure rows
included), so the release's gate ② report covers both families.

`speech_azure_openai_v1` pins: the families and the version key against the provider world's (the `config_schema`
entry byte for byte, the default against native's literal); native's dated URLs for every operation on both hosts with
explicit and default versions; native's v1 URLs; `api-key` on both Azure families and Bearer on `openai-compatible`,
and no auth arm without a slot; that each Azure family's prepared call differs from `openai-compatible`'s only in path,
query and auth, for all three operations; the refusals of §4 D3 (undated versions, ahead of a request refusal; the v1
family ignores the value) and D4; the escaped deployment segment with the unescaped body `model`; near-miss families
refused by both functions; the capabilities equal to `openai-compatible`'s; and that the pack's Azure rows are judged.
`shipped_packages_v1` records the families, the auth arms and the versions (§8).

## 7. Implementation

- `reference_openai_compatible_speech.rs`: a `Family` mapping of the configuration (§3.3) and a `Route` (path, query,
  auth) per family and operation, computed right after the family check, so the dated family's configuration error
  and empty deployment come before any request refusal; synthesis and recognition take the route's path, query and
  auth, and nothing else changes. Public constants `AZURE_OPENAI_V1`, `AZURE_OPENAI_DATED` (the provider world's
  constant) and `AZURE_SECRET_HEADER`.
- `asr_shared::OpenAiAsr` gains `query`, passed to the multipart descriptor (`speech-xai` passes none).
- `reference::is_dated_api_version` becomes crate-visible, so both worlds apply the same date check.
- The component crate is unchanged apart from its version: it wraps the same reference.
- The manifest: `providers` `[openai-compatible, azure-openai-v1, azure-openai-dated]`, `auth_arms`
  `[bearer, header_secret]`, the `config_schema` entry of §3.2; `south_runtime` and `contracts` unchanged (`header_secret`,
  a config key with a `default` and the `api_version_date` syntax are older than the speech world).

## 8. Versioning (Q47)

0.66.0 closes the current cycle (lv: not in 0.66.0), so this change opens the 0.67.0 cycle. It changes
`south-component-conformance` (`src/` and the pack), which every component links, so boundary record §13.12 rule 2
applies: the crate takes a version in the same change, with the requirements on it and every component lockfile, and
every package takes a version bump because every `component.wasm` changes. Following the 0.64.0 and 0.65.0 rounds
(each cycle's first change to the crate took a minor, additive changes included; this one adds public constants and
changes crate-private modules only):

| Item | Before (0.66.0) | After |
|---|---|---|
| `south-component-conformance` | 0.65.0 | **0.66.0** |
| `speech-openai-compatible` | 1.0.1 | **1.1.0** (new families) |
| every other package | as released in 0.66.0 | a patch, unchanged behaviour, `south_runtime` and contracts unchanged |
| `south-contracts`, `south-provider-api` | 0.56.0, 0.58.0 | unchanged |

`speech-openai-compatible` keeps its `south_runtime` and `{media: 1, speech: 1}`. `shipped_packages_v1` records the
round (a retirement test over every identity published with 0.66.0) and the package's minor.

## 9. Open questions (defaults implemented)

- **AO-Q1** Families of `speech-openai-compatible` rather than a new package (§3). **Default: families.**
- **AO-Q2** The provider world's names `azure-openai-v1` and `azure-openai-dated` rather than one `azure-openai`
  (§3). **Default: the provider world's names**, so one row's dialect serves every world.
- **AO-Q3** Both URL forms (§3). **Default: both.**
- **AO-Q4** Token-priced synthesis (`stream_format: "sse"`) on the Azure families, as native sends it (U1).
  **Default: served as native**; H6 gates enabling token prices on an Azure row.
- **AO-Q5** Translation on the Azure families, as native forwards it (U3). **Default: served**, gated on the row's
  `speech.translate` like every row.
- **AO-Q6** The provider world's `api_version` key, syntax, default and refusals (§4). **Default: shared, byte for
  byte.**
- **AO-Q7** Ship in 0.67.0, not 0.66.0. **Default: as lv ruled.**

## 10. Host prerequisites for cutover

- **H1. Pin.** The host re-pins to the release that ships `speech-openai-compatible` 1.1.0 (0.67.0).
- **H2. Rows.** The rows T8 and T9 migrate for text serve speech too: a dated `azure_openai` row is a `component` row
  with dialect `azure-openai-dated`, the same endpoint, `api_key` credentials and the config value `api_version` = the
  row's `api_version` (omitted when unset); a `v1` `azure_openai` row and every `azure_foundry` row are
  `azure-openai-v1` rows with base `{endpoint}/openai/v1`.
- **H3. Per-model versions.** As T9 H3: a model whose `azure_api_version` differs from its row's effective version moves
  to a row of its own before migration (D1).
- **H4. Speech routes and bindings.** Each speech model on such a row gets a `component_speech` route to this package.
  The binding's `base_url` is the row endpoint for `azure-openai-dated` and `{endpoint}/openai/v1` for
  `azure-openai-v1` (not the `openai-compatible` API root), and its `declared` carries `api_version` for the dated
  family. The media executor already sends the sanctioned `api-version` query (`speech-azure`); this family is its
  second user.
- **H5. Credentials.** The `api-key` header is a secret header the host must redact in logs and response views, as for
  the text and embeddings families of the same names.
- **H6. Token-priced synthesis.** Before a token price is set on a speech model of an Azure row, one capture of
  `stream_format: "sse"` against that deployment confirms U1; until then such models stay character-priced or on the
  native arm.
- **H7. The dual run.** A replay-style dual run of the native arms (synthesis by characters, by tokens, transcription,
  translation) on an `azure_openai` dated row, a `v1` row and an `azure_foundry` row against the families, with the
  differences of §5.1 registered; lv's 2026-10-15 ruling makes a passing dual run sufficient for cutover.

## Revision note

- 2026-10-15: first draft, accepted under lv's ruling and directive of the same day; implemented with the defaults.
