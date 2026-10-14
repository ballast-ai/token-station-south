# Speech world batch V4: Bailian Qwen-TTS, Vertex Gemini TTS and Azure Speech SSML TTS

Status: accepted (lv reviewed it on 2026-10-15 and ruled SV-Q10 and SV-Q11 as recommended, §12); implemented
(Implementation, below); declared in `contracts.speech` 1 at speech step S-S-5, to be released in 0.65.0 (`speech-vertex`
1.1.0 is not part of it).

Date: 2026-10-15

Baseline: south `origin/main` = `40db8c8` (release 0.64.0, the V3-1 implementation #221, Kiro #220, the accepted V3-2 and
V3-3 records #222 / #223). The V3-2 implementation (PR #224, branch `origin/speech-v3-2` = `d5cc959`, not yet merged) is
read as the latest precedent for packages, shared modules and `south_runtime` choices. Host references carry a `server:`
prefix and refer to token-station-server `origin/dev-v2` = `98de0f64`, read with `git show`, not run. The speech files
cited here (`handler/audio/*`, `engine/upstream.rs`, `engine/vertex.rs`, `tests/media_arms.rs`) are byte-identical to
`6a946491`, the revision V3-1 cites (`git diff --stat 6a946491 98de0f64` over them is empty), so the V3 records' line
numbers agree with these. `server:…/` abbreviates `server:gateway/src/modules/inference/`; `tts.rs` and
`tts_providers.rs` are `server:…/handler/audio/tts.rs` and `server:…/handler/audio/tts_providers.rs`, `upstream.rs` is
`server:…/engine/upstream.rs`, `vertex.rs` is `server:…/engine/vertex.rs`, `safe_fetch.rs` is
`server:…/engine/safe_fetch.rs`, `request_extras.rs` is `server:…/engine/request_extras.rs`, `media_arms.rs` is
`server:gateway/tests/media_arms.rs`, and `azure_tts_tests.rs`, `vertex_tts_tests.rs` and `mimo_tts_tests.rs` (which
holds the Qwen-TTS unit tests too) are in `server:…/handler/audio/`. Statements marked **unverified** are conclusions
from reading code, host comments or vendor documentation as recalled, not from running anything or capturing traffic;
§13 collects them.

Parent record: [`2026-09-30-speech-world.md`](2026-09-30-speech-world.md) ("the record"): §3 (auth arms, the minted
slot), §5 D3a (the SSML text body, escaping in the component), §6 (the `url`, `inline` and `body` forms, `container`),
§7 (immutability; SSML gives `null`), §8 (facts, the weighted `upper`), §9 (metering sources, host checks), §10 (second
hop, verify-then-settle), §11 (suite rows, `ssml_escaping`, `second_hop_artifact`), §12 (the three V4 rows), §13 (order,
dual run), §16 (Q7, Q10), §17.3 (Vertex: value channel, recipe), §18 S-S-4, and the S-S-1 to S-S-3 implementation
sections with their resolutions (media type parameters, lv 2026-10-14). Sibling records treated as precedent:
[`2026-10-14-speech-v3-1-tts.md`](2026-10-14-speech-v3-1-tts.md) ("V3-1", SV-Q1 to SV-Q6, Implementation),
[`2026-10-14-speech-v3-2-tts.md`](2026-10-14-speech-v3-2-tts.md) ("V3-2", SV-Q7, Implementation on #224),
[`2026-10-14-speech-v3-3-asr.md`](2026-10-14-speech-v3-3-asr.md) ("V3-3", SV-Q8, SV-Q9; `speech-azure`),
[`2026-10-13-image-i3-3-vertex.md`](2026-10-13-image-i3-3-vertex.md) ("I3-3": location, project, base URL, recipe,
VI-Q1, VI-Q2, V1 to V7), [`2026-10-12-image-i3-1-minimax-and-qwen.md`](2026-10-12-image-i3-1-minimax-and-qwen.md)
("I3-1": the Bailian row, DashScope errors, a 2xx carrying `code`) and
[`2026-10-13-image-i3-2-gemini.md`](2026-10-13-image-i3-2-gemini.md) ("I3-2": Google's error envelope, `blockReason`,
policy finish reasons).

**Pre-approval (lv, 2026-10-13).** As for the I3 records and V3-1 to V3-3: a difference from the native arm that follows
an established precedent is accepted without a new question and is listed in §9; only new decisions are in §12.

## 0. What exists and what this step adds

| Needed by V4 | State at `40db8c8` |
|---|---|
| The `url` artifact form and gate ①'s `artifact_fetch` (refused without `synthesize`) | S-S-1 (`SpeechArtifactFormV1::Url`, `crates/south-contracts/src/speech.rs:555`), S-S-2 (`ArtifactFetchRequiresSynthesize`) |
| The `inline` form with `container: {"wav_pcm_s16le": …}` | S-S-1 (`SpeechContainerV1`, `speech.rs:469-530`); the transform `wav_pcm_s16le_v1` (`crates/south-contracts/src/media/transform.rs:257`) with host-obligation vectors (`crates/south-contracts/tests/vectors/speech-v1.json`, `wav_container`, including `mono-24000-the-native-vertex-shape`) |
| Speech media types with parameters (`audio/L16; rate=24000`) | `is_speech_media_type` (S-S-1; lv 2026-10-14, question 3) |
| The SSML text body | `MediaRequestDescriptorV1` `text` body, media type closed to `application/ssml+xml` (`crates/south-contracts/src/media/descriptor.rs:17`), a `contracts.media` 1 word; `TextPostRequestV1` (`crates/south-contracts/src/lib.rs:2006`, bounded by `MAX_TEXT_REQUEST_BODY_BYTES` = 1 MiB, `:195`) and `execute_text_binary_call_v1` (`crates/south-core/src/lib.rs:992`), HTTP contract 12 |
| `ArtifactUrlV1` and the safe fetch pure functions | `crates/south-contracts/src/media/egress.rs:14-104` (https only, `:54-56`) |
| `config_schema`, credential sections and exported attributes in the speech world | S-S-2 (the value channel); credential sections are validated in every world; the speech suite runs `credential_recipe_checks_v1` (`crates/south-component-conformance/src/speech_suite.rs:333`) and admits a `bearer` descriptor on a minted slot through `admit_media_descriptor_auth` |
| Suite rows `ssml_escaping`, `second_hop_artifact`, `empty_audio_is_unknown`, `metering_sample` | S-S-3 (`speech_suite.rs`, `ssml_check.rs`) |
| Vertex helpers | `vertex_location.rs` (`region`, `project`, `base_of`, `publisher_model_path`; I3-3) |
| Google error envelope, `blockReason`, policy finish reasons | `gemini_image.rs` (`error_envelope` `:522`, `POLICY_FINISH_REASONS` `:48`), crate-private |
| DashScope error reader | `reference_qwen_image.rs:92` (`dashscope_error`), private to the image reference |
| `speech-azure` (`transcribe` only, family `azure-speech`) | Accepted in V3-3, **not yet implemented** |
| The host's generic speech executor | **Missing** (V3-1 §7; `git grep component_speech` on `98de0f64` finds only a comment, `server:…/engine/south_component.rs:122`). §7 lists what V4 adds |

What V4 needs that does not exist: an answer to the Bailian audio URL being `http://` while `ArtifactUrlV1` admits
`https` only (§2.5, SV-Q10), and a reading of the `ssml_escaping` row that a voice-mapping, character-dropping SSML
component can pass (§4.4, SV-Q11). Everything else is a package.

## 1. What V4 delivers

| Item | Where | Change |
|---|---|---|
| `speech-qwen` 1.0.0, new | `components/speech-qwen/`, `reference_qwen_speech.rs`, `fixtures-speech-qwen/` | Family `bailian-qwen-speech` (proposed, §5 decision 1); `synthesize` and `artifact_fetch`; §2 |
| `speech-vertex` 1.0.0, new | `components/speech-vertex/`, `reference_vertex_speech.rs`, `fixtures-speech-vertex/` | Family `vertex-ai-speech` (proposed); `synthesize`; config keys and credential section copied from `image-vertex`; §3 |
| `speech-azure` gains `synthesize` | `components/speech-azure/`, `reference_azure_speech.rs`, `fixtures-speech-azure/` | Family `azure-speech` (V3-3); per SV-Q1 (b) a later version of the same package; §4 |
| The `url` artifact's insecure-scheme upgrade (SV-Q10) | `south-contracts::speech`, the suite's `second_hop_artifact` row | An optional member of the `url` form and one pure function (§2.5) |
| The `ssml_escaping` row (SV-Q11) | `speech_suite.rs`, `ssml_check.rs` | The requirement read against what an SSML document can carry, and the reader held to XML 1.0's `Char` production (§4.4) |
| Shared modules | `bailian_image.rs`, `gemini_image.rs`, `speech_shared.rs` | `dashscope_error` moves from `reference_qwen_image.rs` into `bailian_image.rs`; `gemini_image::error_envelope` and the policy table become crate-visible; `speech_shared.rs` gains a `url` success helper and a `body` success with a declared media type. Behaviour-neutral for `image-qwen` and `image-gemini` / `image-vertex` (their packs pass unchanged) |
| Q47 | §10 | |

**Packages, per SV-Q1** (lv 2026-10-14: one package per family; ASR or TTS for a family that already has a package is a
later version of that package). Bailian and Vertex have no speech package yet, so each gets a new one; Azure already has
`speech-azure` from V3-3, so it gains `synthesize`. Neither Bailian nor Vertex has a native ASR arm
(`AsrUpstream::resolve`, `server:…/handler/audio.rs:91-101`); a later Qwen ASR or Vertex ASR would be a later version of
`speech-qwen` or `speech-vertex`.

**Order of implementation.** The record (§13) lists Bailian, Vertex and Azure without a dependency between them. Proposed:
`speech-vertex` first (no contract question; reuses I3-3's pieces), then `speech-azure` (after V3-3 has built the
package), then `speech-qwen` (after SV-Q10 is ruled, since its only artifact depends on it).

**Common to the three** (speech world `speech-adapter-v1`; V3-1 §1 applies unless stated here):

- **Manifest**: secret `provider_api_key`; `contracts` `{media: 1}` until S-S-5 adds `speech: 1` (V3-1 Implementation,
  "Points that contradict the record"); `south_runtime` 0.64.0 (the first runtime with the speech world, its value channel
  and gate ①'s `artifact_fetch` rule; none of the three uses media 2 or 3, a declared query parameter or a manifest field
  newer than 0.64.0; `speech-azure` keeps what V3-3 gives it).
- **`model-capabilities`**: every configured model of the bound row, `operations` including `synthesize`,
  `metering_forms` including `characters`; which rows are routed to the package, and which surface a row serves, is the
  host's route plus its `tts` / `asr` modality check (V3-1 §7 item 1, V3-3 §6 item 1), not a model list in the wasm
  (record §4; I3-3 V7 is the lesson: the package declares every model the host passes in).
- **`input`, facts, refusal texts, refusal order**: V3-1 §1 and interpretations 4, 5, 7 (a missing, non-string, empty or
  elided `input` refused before dispatch; `operation` `synthesize`; `metering_forms` = `context.metering_required` when it
  is `[characters]`, else `capability` "this model cannot report {form}"; no `reservation`).
- **`characters`**: `count` = the scalar count of `input` (`tts.rs:296`); `upper` = `count` for Vertex and Azure (native
  reserves by count, `tts.rs:807-809`), the weighted count for Bailian (§2.3).
- **Non-2xx**: V3-1's `non_2xx` (status table, `rejected` for a 4xx other than 408, else `unknown`, a 3xx `unknown`),
  each section's `provider_message` source, capped at 256 characters.
- **Every 2xx failure parks**: a 2xx `rejected`, `unknown` or `charged_failure` is parked and answered 502 by the host
  (V3-1 §7 item 10, I3-1 IM-Q6). Native parks the same cases and answers 500 (`AppError::Internal` inside the sealed
  block, `tts.rs:1004-1183`) or, for Azure's empty body, 502 (`tts.rs:1156-1171`).

## 2. `speech-qwen` (record §12 row 7)

Reproduces `TtsUpstream::Bailian` (selected by provider type `bailian` and an upstream id containing `tts`,
`tts.rs:116-138`; built at `tts.rs:526-547`; tables at `tts_providers.rs:638-704`; response and second hop at
`tts.rs:1059-1082` and `tts_providers.rs:722-804`).

### 2.1 Request

- **Path and auth**: `POST api/v1/services/aigc/multimodal-generation/generation` under the row endpoint (native
  `build_upstream_url` passes a path starting `/api/` through unchanged for Bailian, `upstream.rs:455-467`; the row
  endpoint is the DashScope origin, the same base `image-qwen` uses), `bearer` on slot `provider_api_key`
  (`upstream.rs:133-136`). No `X-DashScope-Async` (synchronous, as `image-qwen`; the name is host-managed,
  `request_extras.rs:45-46`).
- **Body**, and nothing else from the client (`build_qwen_tts_body`, `tts_providers.rs:684-707`):
  ```json
  {"model": <upstream id>, "input": {"text": <input>, "voice": <voice>, "language_type"?: …, "instructions"?: …}}
  ```
- **Voice** (`resolve_qwen_voice` / `map_openai_voice_to_qwen`, `tts_providers.rs:659-675`): `voice` absent, not a string
  or `""` → `Cherry` (no trimming: `" "` passes as `" "`); otherwise lower-cased (Unicode `to_lowercase`, V3-1
  interpretation 5) and matched: `alloy`, `echo`, `fable`, `onyx`, `nova`, `shimmer`, `ash`, `sage`, `coral`, `ballad`,
  `verse` → `Cherry`; anything else verbatim (a Qwen voice id such as `Ethan`). An elided `voice` is forwarded as an
  `as_is` reference (V3-1 interpretation 4).
- **`language_type`**: the client's when it is a string (`""` included), forwarded as sent (elided → `as_is` reference);
  any other type or absent → omitted (`tts.rs:534`, `tts_providers.rs:694-696`).
- **`instructions`**: the client's when it is a string (`""` included) **and** the upstream id contains `instruct`
  (case-sensitive, as native `upstream_model.contains("instruct")`, `tts_providers.rs:697-702`); else omitted. Reading
  the model id is the I3-4 / V3-1 Groq / V3-2 MiMo precedent (§9 P9).
- **Format** (`qwen_tts_audio_format`, `tts_providers.rs:646-652`), the client `response_format` lower-cased: absent,
  `wav` or `mp3` → accepted (the upstream only returns wav; mp3 is served as wav, as MiMo); any other string →
  `invalid_request` "Bailian Qwen3-TTS cannot emit response_format='{f}'. Supported: wav (mp3 is served as wav)." (native's
  text, quoting the lower-cased value); a non-string is absent (`as_str` → `None`, `tts.rs:527`); an elided one is refused
  with "'response_format' is longer than the gateway accepts" (V3-1 interpretation 4). Nothing is sent upstream for the
  format.
- **Immutable paths**: `input.text`, `model` (= native, `tts.rs:66`). `input.voice` is not immutable (native's table omits
  it; V3-2 decision 5).

### 2.2 Response

`response_body_form` `json`, no elision paths (the response carries a URL, not audio). On a 2xx, in order:

1. **Body**: not a JSON object → `unknown` "a 2xx without a JSON object body" (V3-2 interpretation 6; native 500 parked,
   `tts.rs:1060-1062`).
2. **DashScope `code`**: a non-empty string `code` member → `unknown` with a 502 `provider_protocol_error` envelope whose
   `provider_message` is `code: message` (I3-1 decision 8; native does not detect it, and only fails later on the missing
   URL).
3. **Characters** (record §9, SV-Q7): `usage.characters` a JSON integer above 0 → `characters {value, source: upstream}`;
   absent or `null` → `characters {value: count, source: request}` (native `tts.rs:1063-1068`); `0`, or present but not a
   non-negative integer → `unknown` (SV-Q7's rule, V3-2 interpretation 5's "malformed").
4. **Audio URL**: `output.audio.url` not a string → `unknown` "a 2xx without audio" (`empty_audio_is_unknown`; native 500
   parked, `qwen_tts_audio_url`, `tts_providers.rs:722-734`); otherwise `succeeded` with
   `{"form": "url", "pointer": "/output/audio/url", "media_type": "audio/wav", "upgrade_insecure": true}` (the last member
   per SV-Q10, §2.5).

`output.finish_reason`, `output.audio.id`, `output.audio.expires_at`, `request_id` and the rest of the envelope are
ignored, as native. Whether the URL string passes `ArtifactUrlV1` is not decided by the component: the pointer names
where the URL is, and the host (and the suite's `second_hop_artifact` row) judges the string (record §6, §10).

**Error body** (non-2xx): the DashScope `{"code", "message"}` reader of I3-1 decision 8 (`code` containing
`DataInspection` → `content_policy`, otherwise by status; `provider_message` `code: message`), moved into
`bailian_image.rs` so `image-qwen` and `speech-qwen` read one copy (V3-2's `minimax_shared.rs` precedent); else the body
text. Whether Qwen-TTS errors carry that shape is **unverified** (the image arm's is).

### 2.3 Facts and reservation

`characters {count, upper}` with `upper` = the weighted count of `input` (each scalar 1 when ASCII, else 2; native
`weighted_upper_chars`, `tts.rs:300-303`, reserved for Bailian at `tts.rs:805-806`). The host reserves
`min(bytes(input), upper)`, which is `upper`, the native amount. That DashScope counts a non-ASCII character as 2 is the
native comment's claim (`tts.rs:297-299`, `:1052-1058`), **unverified**. A reported count above `upper` but within
`bytes(input)` is `succeeded`; the host's existing funds guard sends it to review (V3-2 §4 item 3; the native comment at
`media_arms.rs:399`).

`output_media_type` `audio/wav`. `state` `{"characters": <count>}` (V3-2 interpretation 1; the format is always wav).

### 2.4 The second hop (record §10)

The component only names the pointer; the host fetches through its single safe fetch executor (image record §11;
`safe_fetch.rs:1-21`): https only, no userinfo, DNS resolved and pinned, any forbidden address refuses the fetch, ports
443 / 8443 only, no redirects, no system proxy, only an `accept` header (the declared media type), a total deadline and a
byte limit (`min(host limit, MAX_BINARY_RESPONSE_BODY_BYTES)`), an empty body is a failure, the result's media type is the
declared one. Native fetches with `guarded_asset_client` (https only, pinned, no redirect; `tts_providers.rs:756-804`),
a timeout of `upstream_non_streaming_timeout`, a 64 MiB cap through `read_json_body_capped`, and refuses an empty body;
it labels the bytes `audio/wav` whatever the fetched `content-type` (`tts.rs:1079`). The fetch happens after the first
hop's characters are read and before settlement (`tts.rs:1063-1080`): any failure is `delivery_unknown` (the sealed
block), which is the record's verify-then-settle.

### 2.5 The audio URL is `http://` (SV-Q10)

Native upgrades the URL before fetching: in strict mode a leading `http://` becomes `https://` (`guarded_fetch_url`,
`tts_providers.rs:736-746`), because "the audio address Bailian returns is an `http://` OSS presigned link" and the
https-only guard refused every fetch from 2026-09-30 until that fix (03 #96, found by the host's E2E journey J16 against
the real upstream; the comment's claim, not a capture of mine, **unverified** by me). OSS serves https for the same object,
and a presigned signature does not cover the scheme.

The `url` artifact cannot express this today: its pointer lands on the upstream's own string (record §6: "a JSON Pointer
into the **unelided** upstream response view"), that string fails `ArtifactUrlV1::parse` (`NotHttps`), the host's
executor never fetches it (`server:…/handler/images/component/deliver.rs` tests
`a_url_to_a_forbidden_or_plain_http_destination_is_never_fetched`), and the suite's `second_hop_artifact` row fails a
fixture that carries the real shape. Every Bailian call would be `delivery_unknown`. Options are in SV-Q10; the
recommendation, which the rest of this record assumes:

- `SpeechArtifactFormV1::Url` gains an optional member `upgrade_insecure` (boolean, default `false`; on the wire only
  when `true`; refused on the other forms, as every form-foreign member is).
- South supplies one pure function in `south-contracts`, `upgrade_insecure_artifact_url_v1(&str) -> Cow<str>`: when the
  string starts with `http://` (scheme compared ASCII case-insensitively), that prefix becomes `https://`; anything else is
  returned unchanged. Nothing else is rewritten (host, port, path, query and fragment stay byte for byte).
- When the member is `true`, a host applies the function to the string at the pointer **before** `ArtifactUrlV1::parse`,
  and the safe fetch executor then runs with no rule changed: the fetch is still https-only, a `http://host:80/…` still
  fails the port rule, userinfo and forbidden addresses still refuse. The URL still comes from the wire; the component
  still chooses only where it is.
- The suite's `second_hop_artifact` row applies the same function before its grammar check, and gains vectors
  (`speech-v1.json`, a new `artifact_url_upgrade` section: `http` upgraded, `HTTP` upgraded, `https` unchanged,
  `ftp` / relative / `http:` without `//` unchanged and then refused by the grammar, an upgraded loopback still refused).
- The member is part of `contracts.speech` 1, which S-S-5 records for the first time (no host admits a speech package
  before then; every speech package so far declares `{media: …}` only), so no contract number moves.

### 2.6 Manifest

The V3-2 manifests' shape (`components/speech-minimax/manifest.json` on #224): `name` `speech-qwen`, `version` `1.0.0`,
`api_version` `speech-adapter-v1`, `providers` `["bailian-qwen-speech"]`, `capabilities` `["synthesize",
"artifact_fetch"]`, `auth_arms` `["bearer"]`, `permissions.secrets` `["provider_api_key"]`, `conformance` naming
`south.speech-component.v1` and `fixtures-speech-qwen/`, `compatibility.south_runtime` 0.64.0 and `contracts`
`{media: 1}`. No `config_schema`, no `query_parameters`. `model-capabilities`: `[synthesize]`, `[characters]`.

## 3. `speech-vertex` (record §12 row 8)

Reproduces `TtsUpstream::VertexGemini` (selected by provider type `vertex_ai` and an upstream id containing `tts`,
`tts.rs:103-111`; built at `tts.rs:411-438`; URL `tts.rs:696-709` through `build_vertex_media_url`, `vertex.rs:207-237`;
auth `tts.rs:736-748`; response `tts.rs:1032-1050`; tables `tts_providers.rs:851-978`).

### 3.1 Location, project, base URL, credentials (I3-3 §4, unchanged)

Copied from `image-vertex` (and so from `embeddings-vertex`), through the shared `vertex_location.rs`:

- **`config_schema`** for family `vertex-ai-speech`: `region` (`aws_region` syntax, required) and `project`
  (`gcp_project_id`, optional), byte-identical keys, syntaxes, requiredness and descriptions to `image-vertex`'s, so one
  provider row may route models to `embeddings-vertex`, `image-vertex` and `speech-vertex` (the host's R20 requires every
  routed package to accept the row's `config_values` whole).
- **Project**: the `project` key when set, else the credential's exported `project_id`; native order is the same
  (`providers.group_id`, else the credential `extras.projectId`, `vertex.rs:213-227`).
- **Region and base URL**: the package reads `declared.region`; the binding's `base_url` is the row endpoint, which must be
  that region's origin (`{region}-aiplatform.googleapis.com`, `aiplatform.googleapis.com` for `global`); a Vertex origin of
  another region is refused (`base_of`); a non-Vertex base (a proxy, a test server) is used as given. Native ignores the
  row endpoint and derives the host from `providers.region` (`vertex.rs:46-87`, `:112-126`).
- **Path**: `v1/projects/{project}/locations/{region}/publishers/google/models/{model}:generateContent`
  (`publisher_model_path`, each segment percent-encoded), the native shape (`vertex.rs:233-236`; the host test's path,
  `media_arms.rs:420-422`).
- **`credentials`**: byte for byte `image-vertex`'s section (fields `service_account` (secret, JSON) and `project_id`,
  the `service-account-json` import, slot `provider_api_key` minted by `vertex_sa`: RS256 `jwt_sign`, scope
  `https://www.googleapis.com/auth/cloud-platform`, then `oauth2_token`; `project_id` exported), so one credential's
  `recipe_fields` serve all three Vertex packages. Auth `{"arm": "bearer", "slot": "provider_api_key"}`; native presents
  the same `Authorization: Bearer <access token>` (`apply_vertex_auth`, `upstream.rs:312-314`, minted by
  `get_valid_vertex_token_for`, `tts.rs:737-741`) and no other header.
- **Missing values**: a missing `region`, or neither `project` nor an exported `project_id`, is a 500 naming the key,
  before any request refusal (`embeddings-vertex` / `image-vertex`); native answers 400 before admission
  (`required_region`, `vertex.rs:46-59`; the project text, `vertex.rs:218-226`; both run at `tts.rs:699-709`, before
  `admit_text_dispatch` at `:813`). §9 P12.

### 3.2 Request

- **Body**, and nothing else from the client (`tts.rs:423-433`):
  ```json
  {"contents": [{"role": "user", "parts": [{"text": <input>}]}],
   "generationConfig": {"responseModalities": ["AUDIO"],
     "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": <voice>}}}}}
  ```
  No `model` member (the model is in the path), no `languageCode`, no `x-goog-user-project`.
- **Voice** (`resolve_vertex_voice`, `tts_providers.rs:942-950`): the client `voice` when a string, else `""`; that value
  trimmed and lower-cased is matched: `""`, `alloy`, `echo`, `fable`, `onyx`, `nova`, `shimmer`, `ash`, `sage` → `Kore`;
  anything else is sent **as the client wrote it** (untrimmed, case kept). `coral`, `ballad` and `verse` are therefore sent
  as voice names (native defect §8 item 2, reproduced). An elided `voice` is forwarded as an `as_is` reference.
- **Format** (`vertex_tts_audio_format`, `tts_providers.rs:917-937`), the client `response_format` lower-cased (Unicode):
  absent or `wav` → *wav*; `pcm` → *pcm*; `mp3`, `opus`, `aac`, `flac` → *wav* (Q7, §3.4); any other string →
  `invalid_request` "Unknown response_format='{f}'. Gemini TTS on Vertex emits 24kHz mono PCM; supported: wav, pcm
  (mp3/opus/aac/flac degrade to wav)." (native's text); a non-string is absent; an elided one is refused (V3-1
  interpretation 4). Nothing is sent upstream for the format: the upstream always returns 24 kHz mono 16-bit PCM
  (`tts_providers.rs:851-857`, `:875`).
- **Immutable paths**: `contents`, `generationConfig.responseModalities` (= native, `tts.rs:63`). The voice path is not
  immutable (native's table omits it).

### 3.3 Response

`response_body_form` `json`, response elision path `/candidates/*/content/parts/*/inlineData/data`. Non-2xx: Google's
`{"error": {"code", "message", "status"}}` → `provider_message` = `error.message` (`gemini_image::error_envelope`, I3-2,
made crate-visible), else the body text. On a 2xx (`decode_vertex_tts_audio`, `tts_providers.rs:885-915`):

1. Body not a JSON object → `unknown` (native 500 parked, `tts.rs:1035-1037`).
2. **The audio part**, as native finds it: the first part of `candidates[0].content.parts[]` that has an
   `inlineData.data` member (native's `find_map` stops there even when the value is not a string). That value not a string
   (or no such part, or no `candidates[0]`) → step 3; a placeholder of 0 bytes or `""` → `unknown` (native "decoded to zero
   bytes", 500 parked); otherwise `succeeded` with
   - *wav*: `{"form": "inline", "pointer": "/candidates/0/content/parts/{p}/inlineData/data", "encoding": "base64",
     "media_type": "audio/wav", "container": {"wav_pcm_s16le": {"sample_rate": 24000, "channels": 1}}}`;
   - *pcm*: the same without `container`, `media_type` `"audio/L16; rate=24000"` (native's label, `tts_providers.rs:863-872`;
     lv's 2026-10-14 ruling admitted the parameter so this exact type can be declared);

   and metering `characters {value: count, source: request}` (native bills the local count; Vertex reports no character
   usage, `media_arms.rs:494`).
3. **No audio**: `promptFeedback.blockReason` non-empty → `rejected` with a `content_policy` envelope; a candidate
   `finishReason` in the I3-2 policy table → `unknown` with a `content_policy` envelope; otherwise `unknown` "a 2xx without
   audio" (I3-2 GI-Q5; native answers 500 for all three, parked).

`inlineData.mimeType`, `usageMetadata` (`promptTokenCount`, `candidatesTokenCount`; Google prices Gemini TTS by tokens),
`thought` and `trafficType` are not read, as native. `state` `{"format": "wav" | "pcm", "characters": <count>}`.
`output_media_type` = the artifact's media type. `model-capabilities`: `[synthesize]`, `[characters]`.

### 3.4 Q7: the silent downgrade, now and after the dual run

lv ruled Q7 on 2026-09-30: copy native's downgrade of mp3, opus, aac and flac to wav for the dual run, refuse them in
`prepare` afterwards. Applied here:

- **`speech-vertex` 1.0.0 declares the downgrade.** `mp3`, `opus`, `aac` and `flac` prepare a *wav* call:
  `output_media_type` `audio/wav`, the `container` artifact, the client receives WAV with `Content-Type: audio/wav` and
  200, exactly as native (`vertex_tts_tests.rs:28-37`). The pack pins the four values as successes and names Q7 in its
  README. This is the version the host dual-runs and cuts over to.
- **`speech-vertex` 1.1.0 refuses them.** Once the host records the Vertex TTS dual run as accepted, south ships a minor
  in which those four values are `invalid_request` before dispatch, "Gemini TTS on Vertex cannot emit
  response_format='{f}'. Supported: wav, pcm." (proposed, MiMo's form), and the "Unknown …" text drops its parenthesis. A
  behaviour change of the package is a minor (the `provider-kiro` 1.1.0 precedent); the 1.0.0 pack cases for the four
  values become refusal cases. The host moves its pin as a separate step of its cutover manual, and from then the refusal
  is a registered intentional difference from the (by then deleted) native arm. The native comment's reason for the
  downgrade (OpenAI SDKs default to mp3, `tts_providers.rs:924-927`) is the cost lv accepted; whether an SDK sends
  `response_format` when the caller omits it is **unverified** (an omitted format is *wav* in both versions).
- What "after the dual run" means is the host's acceptance, not a south date; south does not ship 1.1.0 in the same
  release as 1.0.0.

### 3.5 VI-Q1 and BYOK

VI-Q1 (the host re-reads a minted slot before every round after the first) does not arise: a speech call has one upstream
round, and the Bailian-style second hop carries no credential. The executor resolves the minted slot once per call through
the shared `component_credentials::slot_value`, as the image executor does. A BYOK credential is refused before admission
for a family whose package declares a credentials section (D9; I3-3 V1); native serves BYOK Vertex TTS through
`pick_credentials_byok` (`tts.rs:681-686`). §9 P11.

## 4. `speech-azure` gains `synthesize` (record §12 row 9)

Reproduces `TtsUpstream::AzureSpeech` (selected by provider type `azure_speech`, `tts.rs:146-147`; built at
`tts.rs:586-614`; content type `tts.rs:723-733`; the empty-body gate `tts.rs:1148-1173`; tables
`tts_providers.rs:82-257`).

### 4.1 Manifest and capabilities

`capabilities` `["transcribe", "synthesize"]` (V3-3 gave `transcribe`; SV-Q1 (b)), family `azure-speech`, `auth_arms`
`["header_secret"]`, secret `provider_api_key`, `contracts` `{media: 1}` (the `text` body is a media 1 word), no
`config_schema`, no `query_parameters`. `model-capabilities`: every configured model declares `[transcribe, synthesize]`
and `metering_forms` `[characters, seconds]`; the host's modality check decides which surface a row serves, and one
binding serves a row that has both MAI-Voice-2 and MAI-Transcribe models (V3-3 §6 item 12; native documents one host for
both, `server:gateway/src/infra/config/providers.rs:161-167`).

### 4.2 Request (D3a)

- **Path and auth**: `POST cognitiveservices/v1` under the row endpoint (`https://<resource>.cognitiveservices.azure.com`;
  native `{endpoint}/cognitiveservices/v1`, the default arm of `build_upstream_url`, `upstream.rs:425-430` and the
  catch-all), no query, auth `{"arm": "header_secret", "header": "ocp-apim-subscription-key", "slot": "provider_api_key"}`
  (native `upstream.rs:197-201`).
- **Body**: `{"text": {"media_type": "application/ssml+xml", "text": <ssml>}}`, the media descriptor's `text` kind; the
  host sends it as a `TextPostRequestV1` through `execute_text_binary_call_v1`, whose transport renders exactly one
  `content-type: application/ssml+xml` (native sets it once, `tts.rs:720-733`; the host test pins exactly one,
  `media_arms.rs:194-201`).
- **Header**: `x-microsoft-outputformat: <token>` (ordinary, not reserved), from the format table below.
- **Format** (`azure_tts_output_format`, `tts_providers.rs:233-257`), the client `response_format` compared **exactly**
  (no lower-casing, no trimming, as native; §8 item 9): absent → mp3. A non-string is absent.

  | `response_format` | `X-Microsoft-OutputFormat` | delivered media type |
  |---|---|---|
  | `mp3` (or absent) | `audio-24khz-48kbitrate-mono-mp3` | `audio/mpeg` |
  | `opus` | `ogg-24khz-16bit-mono-opus` | `audio/ogg` |
  | `wav` | `riff-24khz-16bit-mono-pcm` | `audio/wav` |
  | `pcm` | `raw-24khz-16bit-mono-pcm` | `audio/pcm` |

  Anything else → `invalid_request` "response_format '{f}' is not supported for MAI-Voice-2 (Azure Speech); use mp3, opus,
  wav, or pcm" (native's text). An elided `response_format` is refused (V3-1 interpretation 4).
- **Voice** (`tts.rs:592-597`, `map_openai_voice_to_mai`, `tts_providers.rs:93-127`): the client `voice` when a string,
  trimmed; absent, not a string or empty after trimming → `alloy`. Then:
  1. a value containing `:` is the full voice id, used as is;
  2. else the **case-sensitive** table `alloy` → `June`, `echo` → `Grant`, `fable` → `Jasper`, `onyx` → `Reed`, `nova` →
     `Iris`, `shimmer` → `Joy`, `ash` → `Grant`, `ballad` → `Jasper`, `coral` → `Iris`, `sage` → `June`, `verse` → `Reed`;
     any other value as is;
  3. a name with a locale prefix (`has_locale_prefix`, `tts_providers.rs:135-148`: a 2-3 letter lower-case language, a
     non-empty region of upper-case letters and digits, a non-empty rest, separated by `-`) becomes `{name}:{upstream id}`;
     any other name `en-US-{name}:{upstream id}`.

  An elided `voice` (above 1 MiB) cannot be a roster name and would make the SSML exceed the text body bound; it is refused
  with "'voice' is longer than the gateway accepts".
- **SSML** (`build_mai_ssml`, `tts_providers.rs:195-202`), exactly:
  ```
  <speak version='1.0' xml:lang='{lang}'><voice name='{voice}'>{text}</voice></speak>
  ```
  `{voice}` the resolved voice id, `{lang}` its locale (`voice_locale`, `tts_providers.rs:210-224`: the part before the
  first `:`, split on `-`; when the first two segments are a non-empty ASCII-alphabetic language and a non-empty ASCII
  alphanumeric region, `{lang}-{region}`, else `en-US`), `{text}` the client `input`. All three are escaped by native's
  `xml_escape` (`tts_providers.rs:163-182`): `&` → `&amp;`, `<` → `&lt;`, `>` → `&gt;`, `"` → `&quot;`, `'` → `&apos;`,
  and the characters XML 1.0 forbids even as references (U+0000 to U+0008, U+000B, U+000C, U+000E to U+001F, U+FFFE,
  U+FFFF) are **dropped**; tab, LF and CR are kept as they are. No XML declaration, no `xmlns` (§8 item 8, reproduced: the
  dual run compares SSML as strings, record §13).
- **Escaping lives in the component** (record §5): the reference holds the escaper and the builder; the suite judges the
  result (§4.4).
- **Bound**: an SSML document above `MAX_TEXT_REQUEST_BODY_BYTES` (1 MiB) cannot be a descriptor; `prepare` refuses it
  before dispatch with "'input' is longer than the gateway accepts" (V3-1 P4's generic text). Escaping can grow the text
  up to six times (`&` → `&amp;` is five bytes, `'` → `&apos;` six), so an `input` well below the 1 MiB elision threshold can
  reach it; native sends any length (§9 P1).
- **Immutable body paths**: `null` (record §7, S-S-1 interpretation 8: the host must not inject body extras into SSML;
  native refuses them at save, `body_unsupported`, `tts.rs:80-84`, pinned by
  `server:gateway/tests/model_request_extras_media.rs:553-563`). Header extras still apply (§7 item 5).
- **Facts**: `characters {count, upper: count}` (native reserves the local count, `tts.rs:807-809`); `output_media_type`
  from the table. `state` `{"format": <the table row's word>, "characters": <count>}`.

### 4.3 Response

`response_body_form` `binary`. On a 2xx: an empty body → `unknown` (record §12, `empty_audio_is_unknown`; native 502 and
parked, `tts.rs:1156-1171`); otherwise `succeeded` with `{"form": "body", "media_type": <the table's type>}` and metering
`characters {value: count, source: request}`. The media type is the format table's, **not** the upstream `content-type`:
native relabels deliberately (`tts.rs:1148-1174`, "label the response by the format we requested"), and the record (§6)
names "Azure per its format table". SV-Q2's header echo is the rule for the V3-1 arms, whose native arms echo. A 2xx
whose body is not audio is delivered as native does (V3-1 decision 4). Non-2xx: V3-3's `speech-azure` error reader (the
OpenAI shape, **unverified** for Azure TTS), else the body text.

### 4.4 `ssml_escaping` and a mapping, dropping component (SV-Q11)

The S-S-3 row (`speech_suite.rs:639-674`) requires, for every SSML body, that the document be well-formed, that the
northbound `input` be some element's whole text **unchanged**, and that the northbound `voice` be some attribute's value
**unchanged**, and the pack must hold a case with `<`, `&`, `'` and `"` in both. Native's Azure arm, which the record asks
this package to reproduce, fails two of those on ordinary requests:

- the voice is mapped: `alloy` becomes `en-US-June:MAI-Voice-2`, and a name with special characters becomes
  `en-US-{name}:{model}`, so the client's `voice` is never an attribute value unless it already contains `:`;
- characters XML forbids are dropped from `input`, so an `input` carrying U+0007 is not the element's text.

The row was written before any real SSML package (S-S-3 interpretation 10). Options are in SV-Q11; the recommendation,
which §5 assumes:

- `input` must equal some element's text after the XML-forbidden characters (the list above) are removed from the
  northbound value. The well-formedness reader (`ssml_check.rs`) accepts those characters today, raw in character data
  and as numeric references (`:75-93`, `:203-209`); it is tightened to XML 1.0's `Char` production in both places, so a
  component that embeds them instead of dropping them fails.
- When the northbound `voice` contains any of `<`, `&`, `'`, `"`, `>`, it must appear **as a substring** of some
  attribute value; a voice without them is not checked (its escaping is the identity, and a mapping is the component's
  business, as `xml:lang` already is).
- The special-characters case requirement is unchanged (both `input` and `voice` carry the four characters), and the
  synthetic reference's pack passes unchanged (it embeds both verbatim).

## 5. Fixtures, tests, wiring; decisions where the record is silent

Per package, as V3-1 §12 and the V3-2 Implementation: a gate ② pack `speech-v1.<family>.<case>` transcribed from §7 item
11's sources and written by hand before the first run; a suite test `<vendor>_speech_suite` with named behaviour tests and
mutations; a sandbox parity test; a build script; the `ci.yml` / `release.yml` enumerations (new entries for
`speech-qwen` and `speech-vertex`); `shipped_packages_v1`; README and ARCHITECTURE notes. Non-ASCII inputs use Hangul and
Latin with diacritics, as V3-2 did (the language gate refuses CJK ideographs; a Hangul syllable is three UTF-8 bytes, as an
ideograph is). Required cases:

- **All three**: the `input` refusals; `undeclared_operation_refused` attempts (`speech-azure` also its ASR cases, which
  V3-3 owns); `metering_required` `[tokens]` → `capability`; 401, 403, 404, 408, 429, 500 and a 3xx; the
  `metering_sample` characters success.
- **Qwen**: the `media_arms.rs:350-411` exchange (`qwen3-tts-flash`, upstream 7 characters billed for an 11-character
  input, the URL fetched); each voice class (an OpenAI name in another case, a Qwen id, `""`, absent, non-string, elided);
  `language_type` string, non-string and absent; `instructions` on an `instruct` id and on a plain id; every
  `response_format` row including the refusal (`opus`, `PCM16`) and elided; extra client members dropped;
  `usage.characters` present (upstream success), absent and `null` (the request fallback the suite requires), `0`,
  negative, fractional, a string; a count above the weighted `upper` within `bytes(input)`; the weighted `upper` for
  ASCII, Hangul and mixed input; a 2xx carrying `code`; a non-object 2xx; `output.audio.url` missing and not a string; an
  `http://` URL (the real shape, SV-Q10) and an `https://` URL; the DashScope non-2xx with `DataInspection`.
- **Vertex**: the `media_arms.rs:413-512` exchange (path, bearer, body, WAV); `global` and a regional location; a proxy
  base; a Vertex base of another region refused; `project` override and exported `project_id`; missing `region` and
  missing project (pinned by name in the suite test, not in the pack: the `pre_dispatch_refusal` row admits only
  `invalid_request` / `capability`, I3-3 departure 1); each voice class (the eight folded names and `""`, `coral` sent
  through, a Gemini name with padding and case kept, non-string, elided); every `response_format` row including the four
  Q7 downgrades and an unknown value; the five `credential.*` cases copied from `fixtures-image-vertex/`; a 2xx with audio
  in `parts[0]` and in `parts[1]`, a first `inlineData.data` that is not a string, `""`, no candidates, `blockReason`, a
  policy `finishReason`; the Google error envelope.
- **Azure**: the `media_arms.rs:143-201` exchange (header, auth, one content type); each `response_format` row, `MP3`
  refused (exact match), elided; each voice rule (a table name, `Alloy` passed through, `en-GB-Sonia`, a full id
  `fr-FR-Vivienne:MAI-Voice-2` and its `xml:lang`, `zh-CN-liaoning-Xiaobei` style, padded, empty, non-string); the
  `azure_tts_tests.rs:100-137` escaping and control-character cases; `speech-v1.ssml.special-characters` (§4.4); an SSML
  document just above 1 MiB refused; an empty 2xx (`unknown`); a 2xx with and without an upstream `content-type`.

Proposed decisions (not questions unless lv objects):

1. **Names**: `speech-qwen` / `bailian-qwen-speech` (the `image-qwen` / `bailian-qwen-image` precedent: the product the row
   serves, on the platform's row; V3-2 decision 1 named MiMo by product as well) and `speech-vertex` /
   `vertex-ai-speech` (I3-3 decision 1: each world names its own Vertex family). `speech-azure` keeps its V3-3 name and
   family. Alternative for Bailian: `speech-bailian` / `bailian-speech`, which would also fit a later non-Qwen DashScope
   speech model; not proposed, since no such native arm exists.
2. **Shared modules**: `bailian_image.rs` takes `dashscope_error`; `gemini_image.rs` exposes `error_envelope` and the
   policy table crate-wide; `vertex_location.rs` is used unchanged; `speech_shared.rs` gains `url_outcome` and a `body`
   success with a declared media type. No module is copied.
3. **Azure voice and format are not normalized** beyond native (exact format match, case-sensitive table): the dual run
   compares SSML and the header as strings, and the cost of a mismatch is an upstream 400 that P29 releases.
4. **Vertex reads neither `inlineData.mimeType` nor `usageMetadata`**: the container is fixed at 24000 Hz mono, as native;
   reading the rate from the mime type would be a new behaviour with no capture behind it.
5. **`upgrade_insecure` only on speech's `url` form** (SV-Q10): the image world's `url` artifacts are untouched; if a
   capture shows an image upstream returning `http://` URLs, the same member can follow in a media contract.

## 6. Shared rules restated for V4 (record §10, §11)

- **Verify then settle**: `url` (fetched, non-empty, within the limit), `inline` (decodes under its encoding to a non-empty
  result), `container` (PCM non-empty and a whole number of `2 × channels` frames, so an odd byte count is refused), `body`
  (non-empty). Any failure is `delivery_unknown`, no settlement.
- **Suite rows** that apply: `second_hop_artifact` (Qwen, with SV-Q10's upgrade), `ssml_escaping` (Azure, as SV-Q11
  reads it), `empty_audio_is_unknown` per body form (`json` for Qwen and Vertex, `binary` for Azure), `metering_sample`
  (Qwen both character sources; Vertex and Azure `request`), `reference_integrity` (Vertex `inline` lands on an elided
  string; Qwen's `url` pointer lands on an unelided string), `pre_dispatch_refusal`, `auth_errors_are_not_retriable`,
  `terminal_only_from_the_wire`, the credential rows (Vertex).

## 7. What the host must do before a V4 cutover (in addition to V3-1 §7, V3-2 §4, V3-3 §6)

Every item of the earlier lists applies (route and modality, pricing form from the package, admission, facts check,
reservation, request extras, dispatch, view, outcome and funds, verify-then-settle, host checks, settlement, delivery, Q10,
dual run). V4 adds, generic over packages and naming no provider:

1. **`url` artifacts through the safe fetch executor.** For a speech `url` artifact: read the string at the pointer in
   the unelided upstream body, apply `upgrade_insecure_artifact_url_v1` when the artifact says so (SV-Q10), parse with
   `ArtifactUrlV1`, fetch with `safe_fetch` (`declared_media_type` = the artifact's media type, `host_byte_limit` the
   speech response limit, a total timeout of `upstream_non_streaming_timeout` as native), all inside verify-then-settle
   (any failure `delivery_unknown`, no settlement), and deliver the bytes under the declared media type. The executor and
   its test injection exist (`safe_fetch.rs`, `:359`; the image executor's `deliver.rs:120`); the speech executor calls
   them. Gate ③: the host's `south.safe-fetch.v1` adaptation already covers the rules; the upgrade adds the new vectors.
2. **The WAV container.** Apply `container` with south's `wav_pcm_s16le_v1` (or `MediaTransformV1::apply`), not a host
   copy, so the bytes equal `speech-v1.json`'s `wav_container` vectors by construction; refuse what it refuses
   (`delivery_unknown`); never settle the header-only output of empty PCM (`deliverable: false`). Native's
   `wrap_pcm_in_wav` (`tts_providers.rs:956-978`; the record's `:940-966` has drifted) writes the same 44-byte header for
   24000 Hz mono (read, not run; the vector is named after the native shape) but does not refuse an odd byte count (§8
   item 3).
3. **`inline` artifacts** as V3-2 §4 item 2, with the decoded bytes accounted against the response-body memory budget, then
   the container.
4. **`TextPostRequestV1` execution.** For a descriptor with a `text` body: build `TextPostRequestV1::try_new` from the path,
   the descriptor's headers (no `content-type`: the contract renders it and the host must not add one, the multipart D3
   rule), the `TextBodyV1` and the auth; attach the query when present; call `execute_text_binary_call_v1`. No body extras
   (the call's `immutable_body_paths` is `null`). The host has no caller of either today (`git grep` on `98de0f64`).
5. **Header extras on a descriptor header.** Native refuses a header extra named `X-Microsoft-OutputFormat` at save
   (`HOST_MANAGED_HEADERS`, `request_extras.rs:34-48`, which lists that vendor header and `x-dashscope-async` by name;
   pinned at `model_request_extras_media.rs:567-578`), because overriding it would desynchronize the audio from its
   label. The image executor merges header extras over component-declared headers with `override` winning
   (`server:…/handler/images/component/mod.rs:1712-1738`). Recommended for the speech executor (a host decision, listed
   here rather than as a south question): a header extra never replaces a header the prepared descriptor carries
   (the header counterpart of immutable body paths), enforced per call when the descriptor is known. The admin save
   check for a routed row has no per-call declaration to read (V3-1 §7 item 7's open host item); with the per-call rule
   a saved extra of that name is skipped rather than applied, and the two vendor names can leave
   `HOST_MANAGED_HEADERS` when their native arms are deleted (rule 6, J1).
6. **Minted slots and the value channel for speech routes.** Resolve a `minted` slot with `component_credentials::slot_value`,
   compute exported attributes and pass `declared` from the row's `config_values` plus those attributes, as the image
   executor does (`server:…/handler/images/component/mod.rs:853`, `:1515-1530`, `:1867`). VI-Q1's per-round re-read is not needed (one
   round, §3.5); the D9 BYOK guard applies to speech routes too.
7. **Recipe fields on a native `vertex_ai` row for speech routes.** The admin path that appends a package's credential
   fields to a native row's credential looks at embeddings and image routes
   (`server:gateway/src/modules/admin_ui/handler/credentials/declared.rs:11-12`, `:283-301`); speech routes must join, and
   the recipe package listing (`server:gateway/src/modules/admin_ui/handler/credential_recipes.rs:24-33`) must list speech
   recipe packages, so `speech-vertex`'s digest can get its endpoint confirmation (R23; a confirmation is per package
   digest, I3-3 §6 item 3). R20 must count speech routes when it checks a row's `config_values`.
8. **Row preparation for Vertex** (I3-3 §6 item 4): endpoint = the region's origin, `config_values.region` =
   `providers.region`, `config_values.project` = `providers.group_id` when set; the binding `base_url = row_endpoint`.
9. **Bindings**: own binding keys on shared rows (the P13 precedent): the Bailian row also serves chat, image and task
   routes; the `vertex_ai` row chat, embeddings and image; the `azure_speech` row TTS and ASR through one `speech-azure`
   binding (V3-3 §6 item 12).
10. **Q10** (record §16): include the Bailian, Vertex and Azure TTS rows in the read-only listing of rows with only a token
    price (all three arms are characters-only, `tts.rs:49-55`; the one known Bailian instance was repriced, record §16
    note).
11. **Dual run per arm** (record §13): JSON body by value (Qwen, Vertex), SSML by string (Azure), headers (Azure's
    `x-microsoft-outputformat`), path, query; audio bytes and `Content-Type` byte for byte (Vertex WAV header included);
    reservation (Qwen weighted), frozen characters and amount (Qwen upstream-sourced). Qwen's second hop: the URL fetched
    (after the upgrade) and the bytes delivered are compared; the second hop's request headers differ by construction
    (the safe fetch sends only `accept`, §9 P3). The native test `media_arms.rs:350` fetches an `http://127.0.0.1` URL
    with `asset_fetch_unsafe_allow_private_destinations`, which the safe fetch executor does not honour
    (`safe_fetch.rs:18-21`): the host's dual run uses its safe fetch test injection with an `https` URL, plus an `http`
    URL to exercise the upgrade. Fixture sources: `media_arms.rs:143-201` (Azure), `:347-411` (Qwen), `:413-512`
    (Vertex), `:655-760` (the Qwen second hop's guard and redirects), `:1598-1700` (token-only Qwen rows, Q10);
    `azure_tts_tests.rs:7-175`; `vertex_tts_tests.rs:8-145`; `mimo_tts_tests.rs:261-450` (Qwen formats, voices, bodies,
    URL extraction); `model_request_extras_media.rs:553-590` (Azure extras).

## 8. Native defects found on the way (read, not run)

1. **Vertex `pcm` is labelled `audio/L16; rate=24000`** (`tts_providers.rs:863-872`) for little-endian samples, while RFC
   2586 defines L16 as network byte order; the host's own MiniMax comment avoids `audio/L16` for that reason
   (`tts_providers.rs:834-836`, and MiMo's at `:467-468`). Reproduced: lv admitted the parameterized type on 2026-10-14 so the package can declare
   it exactly as native delivers it.
2. **Vertex folds only eight OpenAI voice names** (`tts_providers.rs:944-946`): `coral`, `ballad` and `verse` are sent as
   Gemini voice names, which the upstream refuses (400, released by P29: a wasted call, not a funds defect); a padded
   Gemini name is sent untrimmed. Every sibling table folds eleven. Reproduced (V3-2 decision 3's precedent for wasted
   calls).
3. **Vertex wraps an odd byte count** into a WAV whose data is not whole frames (`wrap_pcm_in_wav` has no check), and
   `data_len as u32` truncates above 4 GiB (unreachable). Fixed by the host's south transform (verify-then-settle).
4. **Vertex ignores `promptFeedback` and `finishReason`**: a blocked prompt or a policy stop is a 500 with funds parked.
   The component path keeps the funds parked and answers 502 (§3.3 step 3).
5. **Vertex ignores `inlineData.mimeType`**: the WAV header always says 24000 Hz mono; if the upstream changed its rate,
   the header would be wrong (**unverified** that it can).
6. **Bailian `usage.characters: 0` settles $0** for delivered audio, and a malformed count falls back silently to the
   scalar count (`as_u64`, `tts.rs:1063-1068`); `chars as u32` truncates (unreachable). SV-Q7's precedent applies.
7. **Bailian does not detect a 2xx carrying a DashScope `code`** and answers 500 when the URL is then missing (I3-1 §6
   item 4, the image arm's twin).
8. **Azure SSML carries no `xmlns`** (`tts_providers.rs:195-202`); W3C SSML 1.0 requires the namespace on `<speak>`, and
   whether Azure accepts a document without it is **unverified** (the host test runs against a mock). Reproduced: the dual
   run compares SSML as strings, and a fix belongs in a later minor with a capture.
9. **Azure `response_format` and the voice table are case-sensitive**: `MP3` is refused, `Alloy` is sent as
   `en-US-Alloy:MAI-Voice-2`; every sibling arm lower-cases. Reproduced (§5 decision 3).
10. **Every native V4 arm delivers an unfollowed 3xx** and a non-2xx success status as received (V3-1 §9 item 6,
    `tts.rs:1261`); the component path makes a 3xx `unknown` and answers 200.

Per lv's I3-2 ruling, native-arm defects are registered here and leave with the arm. Items 3, 4, 6, 7 and 10 are fixed by
the component path; 1, 2, 8 and 9 are reproduced for the dual run; 5 is kept (§5 decision 4).

## 9. Differences that follow precedent (accepted under lv's 2026-10-13 pre-approval)

- **P1 Admission timing and generic texts** (V3-1 P1, P4): a missing, empty or elided `input`, an elided `voice` or
  `response_format`, and an SSML document above the text body bound are refused before admission; native sends `""`.
- **P2 Empty audio is `unknown`** (record §10, V3-1 P2): Vertex `""`, Qwen without a URL, Azure empty body; funds parked as
  native; the client status is 502 instead of native's 500 (V3-2 P3).
- **P3 The second hop runs on the safe fetch executor** (record §10, image record §11, P22 C6): `accept` only, ports 443 /
  8443, no private-destination escape hatch; native's guarded client has the same https, pinning and no-redirect rules
  but its own header set and honours the escape hatch.
- **P4 Upstream characters of 0 or malformed are `unknown`** (SV-Q7) for Qwen.
- **P5 A 2xx carrying a DashScope `code` is `unknown`** (I3-1 decision 8).
- **P6 Google `blockReason` is `rejected`, a policy `finishReason` `unknown`** (I3-2 GI-Q5): funds parked either way, as
  native.
- **P7 Non-2xx table, a 3xx `unknown`, the capped error message** (V3-1 P5, P6, P7); funds by the P29 allow-list, as native
  speech already applies.
- **P8 Delivered media types are declared by the component** (record §6): Qwen `audio/wav` and Vertex's table, as native;
  Azure its format table, as native (not SV-Q2's echo, §4.3).
- **P9 Model classes read from the upstream id** (I3-4, V3-1 Groq, V3-2 MiMo): Qwen's `instruct` substring.
- **P10 The arm is chosen by the route**, not by `contains("tts")` on the model id or by provider type (record §12): a
  non-TTS Vertex or Bailian model is refused at route save by the `tts` modality check instead of native's 400 at request
  time (`tts.rs:339-358` for Vertex) or native's fallback to the OpenAI-compatible arm.
- **P11 Vertex BYOK refused before admission** (I3-3 V1, D9).
- **P12 Vertex configuration errors** (missing region or project, a region-mismatched base) are a 500 naming the key before
  admission (I3-3 V2, V3); native answers 400 before admission. The row endpoint must be the region's origin (I3-3 V2).
- **P13 Vertex path segments percent-encoded** (I3-3 V4); identical for every `[A-Za-z0-9._-]` value.
- **P14 The recipe executor mints the Vertex token** (I3-3 V6), sharing the stored token with the other Vertex packages.
- **P15 The WAV container refuses PCM that is not whole frames** (record §10); native wraps it.
- **P16 Q7** (§3.4): 1.0.0 downgrades as native; 1.1.0 refuses after the dual run, as ruled.
- **P17 Status of a 2xx delivery is 200** (V3-1 §7 item 14).

## 10. Versioning (Q47)

No code is written by this record; the implementation follows the V3-1 and V3-2 pattern.

- **`south-contracts`**: SV-Q10's `upgrade_insecure` member and `upgrade_insecure_artifact_url_v1`, with vectors and the
  `media_parsers` fuzz target extended to the member. It moved in this unreleased cycle (0.55.0 → 0.56.0 with V3-1) and
  does not move again unless a release intervenes (S-S-3's rule). No contract number moves: the member is part of
  `contracts.speech` 1, first recorded at S-S-5.
- **`south-component-conformance`**: three references, the shared-module moves, the suite's `second_hop_artifact` upgrade
  and the SV-Q11 reading; it moved in this cycle (0.63.0 → 0.64.0) and likewise does not move again unless a release
  intervenes, in which case every package published by that release takes a patch.
- **`south-provider-api`**: unchanged.
- **Packages**: `speech-qwen` and `speech-vertex` start at 1.0.0, `contracts` `{media: 1}`, `south_runtime` 0.64.0.
  `speech-azure` gains `synthesize` at 1.0.0 if V3-3 and V4 both land before the release that first publishes it (S-S-5),
  else at 1.1.0 (V3-3 §10's rule). `speech-vertex` 1.1.0 (Q7) follows in a later release. `speech-qwen`'s `url`
  artifact carries SV-Q10's member, which a 0.64.0 `south-contracts` would refuse; that does not move its
  `south_runtime` (the 0.64.0 gate ① admits the manifest, which is what the declaration states), and no host admits a
  speech package before S-S-5 records `contracts.speech` 1 with the member in it. All three gain `speech: 1` at
  S-S-5 with the other speech packages. `image-qwen`, `image-gemini` and `image-vertex` change no behaviour (the shared
  readers move); if a release intervenes, they take the same patch as every other package.

Exact numbers are fixed at implementation.

## 11. Measurements to make while writing the fixtures (record §18 S-S-4)

Recorded in each package's fixture README, by the host team with real keys (the agent writing this record has none). The
packages ship without waiting for them; a measurement that contradicts a package rule is a later minor.

1. Qwen-TTS: one capture of a success (the URL's scheme, host, port and length; `usage.characters` for an ASCII, a Hangul
   and a mixed input, to confirm the weighting); one error body; whether any 2xx carries `code`.
2. Vertex Gemini TTS: one capture per voice class (a folded name, `coral`); `inlineData.mimeType` and its rate; a blocked
   prompt and a policy stop, if reproducible; the error envelope.
3. Azure Speech TTS: a capture with the native SSML (no `xmlns`) and one with it; the `content-type` returned per format;
   an error body (status and shape); whether a `User-Agent` is required (the host's shared client sends its own; the
   speech world sends none).

## 12. Open questions for lv

**Ruled by lv on 2026-10-15: SV-Q10 and SV-Q11 as recommended** (an optional `upgrade_insecure` member on speech's
`url` form with the pure function `upgrade_insecure_artifact_url_v1`, inside `contracts.speech` 1 before S-S-5; the
`ssml_escaping` row compares `input` after removing XML-forbidden characters, requires the voice only as a substring and
only when it holds an XML special character, and `ssml_check.rs` refuses forbidden characters raw or as numeric
references). The items below keep their text; each recommendation is the ruling.

- **SV-Q10 The Bailian audio URL is `http://`.** The `url` artifact points at the upstream's own string, which
  `ArtifactUrlV1` refuses, so every Bailian call would be `delivery_unknown` (§2.5). Options: (a) an optional
  `upgrade_insecure` member on speech's `url` form, executed by the host with a south pure function before the unchanged
  safe fetch rules, and applied by the suite's `second_hop_artifact` row; (b) a new artifact form carrying a URL the
  component writes (it can read the short string and rewrite the scheme), which gives up the record's provenance rule that
  the fetched URL comes from the wire; (c) the host upgrades every `http://` artifact URL in every world, which changes
  the safe fetch rule set the image record fixed; (d) keep strict and leave Bailian TTS native. **Recommended: (a)**: the
  URL stays the upstream's, the fetch stays https-only with every other rule unchanged, the transform is a closed public
  rule (a scheme upgrade, as browsers' "upgrade insecure requests" does) that a package must ask for, and it is speech-only
  until another world needs it (§5 decision 5). It lands inside `contracts.speech` 1 before S-S-5 records it.
- **SV-Q11 `ssml_escaping` against a component that maps the voice and drops characters.** The row requires the client's
  `voice` as an attribute value and `input` as an element's text, both unchanged; native's Azure arm maps every roster
  voice and drops XML-forbidden characters (§4.4), so the package fails the row on ordinary cases. Options: (a) read the
  row as §4.4 recommends: `input` compared after removing the characters XML 1.0 forbids, the voice required as a
  substring of an attribute value only when it contains an XML special character; (b) the package stops mapping voices and
  dropping characters, breaking the dual run and native behaviour (an `alloy` client would get an upstream 400); (c) keep
  the row and exempt the package by name. **Recommended: (a)**: it keeps what the row exists for (no injection, nothing
  client-controlled lost when it needs escaping), it is a suite reading with no contract change, and the synthetic
  reference passes unchanged. With the reader tightened to XML 1.0's `Char` production, dropping is the only passing
  behaviour for those characters (an Azure document carrying one is refused upstream, `azure_tts_tests.rs:109-120`).

Not questions (precedent or ruled): Q7's two versions (§3.4); the parameterized `audio/L16; rate=24000` (lv 2026-10-14);
package-per-family (SV-Q1); upstream character rules (SV-Q7); configuration errors as 500 (I3-3); the header-extra rule of
§7 item 5 is the host's to decide.

## 13. What is unverified

- Bailian: the `http://` scheme, host, port and length of the audio URL (the host comment and its E2E, not a capture of
  mine); that DashScope counts a non-ASCII character as 2; the Qwen-TTS error body shape; whether a 2xx can carry `code`.
- Vertex: the fixed 24 kHz mono output and the `inlineData.mimeType` value (the host test fixture is hand-written);
  `promptFeedback` and policy finish reasons on TTS; the error envelope; which voice names Gemini refuses; whether OpenAI
  SDKs send `response_format: mp3` when the caller omits it (Q7's cost).
- Azure: acceptance of SSML without `xmlns`; the TTS error body shape; the returned `content-type`; whether a
  `User-Agent` is required.
- Host: that native's and south's WAV headers are equal for even PCM (read, not run; the south vector is named after the
  native shape); how a routed row's admin save check reads the SSML body rule and descriptor headers (§7 item 5, the open
  V3-1 §7 item 7).

## Implementation (2026-10-15)

What landed on branch `speech-v4` (on `origin/main` `33bd0e9`: V3-1 #221, V3-2 #224, V3-3 #226 and this record #225):

- **`speech-qwen` 1.0.0** (new, family `bailian-qwen-speech`, `synthesize` and `artifact_fetch`) and **`speech-vertex`
  1.0.0** (new, family `vertex-ai-speech`, `synthesize`): component crates, references `reference_qwen_speech.rs` and
  `reference_vertex_speech.rs`, packs `fixtures-speech-qwen/` and `fixtures-speech-vertex/`, suite tests
  `qwen_speech_suite` and `vertex_speech_suite`, sandbox parity tests `qwen_speech_sandbox_parity` and
  `vertex_speech_sandbox_parity` (the latter also pins the four configuration refusals through the guest), build scripts,
  `ci.yml` / `release.yml` entries, README and ARCHITECTURE entries.
- **`speech-azure` 1.0.0** gains `synthesize` (SV-Q1 (b); unpublished, so no version move, §10): `prepare_synthesis` in
  `reference_azure_speech.rs` with native's voice mapping, locale, escaper and SSML template as public functions
  (`voice_id`, `xml_escape`, `ssml`), the output-format table and the declared-type `body` success; the manifest lists
  `synthesize`; 62 `tts-` cases join the pack.
- **SV-Q10** (`south-contracts`, inside `contracts.speech` 1): `SpeechArtifactFormV1::Url` gains `upgrade_insecure: bool`
  (wire member written only when `true`, refused on the other forms, `false` read as the default); the pure function
  `speech::upgrade_insecure_artifact_url_v1(&str) -> Cow<str>`; unit tests; the vector section `artifact_url_upgrade`
  in `tests/vectors/speech-v1.json` (14 cases: `http`, `HTTP` and `hTtP` upgraded with query, fragment and an explicit
  port 80 kept; `https` unchanged; `ftp`, a relative path, `http:` without `//` and a leading space unchanged and then
  refused by the grammar; an upgraded loopback, `localhost`, metadata address and userinfo still refused) with
  `speech_contract_v1::artifact_url_upgrade_vectors`; the `media_parsers` fuzz target checks that the upgrade changes
  only the scheme and that what the grammar then admits is https. The suite's `second_hop_artifact` row applies the
  function before `ArtifactUrlV1::parse` when the artifact asks for it.
- **SV-Q11** (suite): `ssml_escaping` compares `input` less the characters XML 1.0 forbids with some element's text,
  and, when the `voice` holds `<`, `&`, `'`, `"` or `>`, requires it (likewise less the forbidden characters) as a
  substring of some attribute value; the special-characters requirement is unchanged. `ssml_check.rs` gains `is_xml_char`
  and `without_forbidden` and refuses a forbidden character anywhere in the document and as a decimal or hexadecimal
  reference (unit test `characters_xml_forbids_are_refused_raw_and_as_references`). The synthetic reference's pack passes
  unchanged: no synthetic fixture changed.
- **Shared modules** (§5 decision 2): `dashscope_error` moves from `reference_qwen_image.rs` into `bailian_image.rs`
  unchanged; `gemini_image.rs` exposes `error_envelope`, `policy_envelope` and `POLICY_FINISH_REASONS` crate-wide;
  `vertex_location.rs` is used unchanged; `speech_shared.rs` gains `url_outcome`, `declared_body_outcome` and
  `non_2xx_with` (a family reader's envelope with the body-text fallback). `image-qwen`, `image-gemini` and `image-vertex`
  keep their behaviour: their packs, suite tests and sandbox parity tests pass with no fixture changed.
- **Tests**: `shipped_packages_v1` lists 37 official components and gains
  `the_v4_speech_packages_declare_their_values_and_runtime`; the V3-3 test reads `speech-azure`'s two operations.

| Package | Family | Operations | Auth | Pack | Suite test |
|---|---|---|---|---|---|
| `speech-qwen` | `bailian-qwen-speech` | `synthesize` (+ `artifact_fetch`) | `bearer` | 76 (2 capabilities, 40 prepare, 34 response) | 10 tests |
| `speech-vertex` | `vertex-ai-speech` | `synthesize` | `bearer` (minted by `vertex_sa`) | 75 + 5 credential (2 capabilities, 48 prepare, 25 response) | 9 tests |
| `speech-azure` | `azure-speech` | `transcribe`, `synthesize` | `header_secret` `ocp-apim-subscription-key` | 119 (62 added: 1 capabilities, 46 prepare, 15 response; 1 removed; 2 capability expectations updated) | 12 tests |

**Fixtures.** Every expectation was written by hand before the first run, from §2 to §4 and the sources §7 item 11
cites (`media_arms.rs:143-201`, `:347-411`, `:413-512`; `azure_tts_tests.rs`; `vertex_tts_tests.rs`;
`mimo_tts_tests.rs:261-450`; the native functions of `tts.rs` and `tts_providers.rs` at `origin/dev-v2` `25824d1f`, whose
speech files are byte-identical to the record's `98de0f64`); each pack's README names them. Non-Latin inputs are Hangul and
Latin with diacritics. The Azure SSML strings are spelled out literally in the expectations. The 1 MiB case carries an
input of 174,751 bytes whose escaped document is 1,048,577 bytes; the case exactly at the bound (whose expectation would
hold a 1 MiB string) is pinned by name in `azure_speech_suite::the_ssml_is_bounded_by_the_text_body_limit` instead.

**`south_runtime` choices.** All three declare 0.64.0, the first runtime with the speech world, its value channel, its
minted-slot admission and gate ①'s `artifact_fetch` rule; none uses media 2 or 3, a declared query parameter or a
manifest field newer than 0.64.0 (`speech-vertex`'s `config_schema` and `credentials` are admitted in the speech world
since S-S-2). `check-declared-runtime.sh --build` loads every package under the runtime it declares, and the published
0.64.0 runtime (a detached checkout of tag `v0.64.0`) admits all three (Gate results). As §10 says, `speech-qwen`'s
`upgrade_insecure` member would be refused by a 0.64.0 `south-contracts` when a host parses the outcome; no host admits a
speech package before S-S-5 records `contracts.speech` 1 with the member in it, so the declaration (what gate ① admits)
does not move.

**Versions (§10, Q47).** None move. `git diff v0.64.0 HEAD` shows V3-1 already moved `south-contracts` (0.55.0 →
0.56.0), `south-provider-api` (0.57.0 → 0.58.0) and `south-component-conformance` (0.63.0 → 0.64.0) in this unreleased
cycle and patched every package published with 0.64.0 (for example `image-qwen` 1.0.7 → 1.0.8, `image-vertex` 1.0.1 →
1.0.2, `task-wan-image-v2` 0.35.22 → 0.35.23), so the contract member, the suite changes, the moved readers and the two
references ride those versions and no package is patched again. `south-provider-api` is unchanged.

### Interpretations

1. **The Qwen 2xx `code` predicate** is the record's "a non-empty string `code`" (§2.2 step 2). `image-qwen`'s own
   predicate, which is not shared, also counts a non-string `code` and ignores a blank one; it is unchanged (open
   question 1).
2. **A Qwen `output.audio.url` of `""`** is `unknown` "a 2xx without audio", like a missing or non-string URL: an empty
   string names no audio, and the suite's `empty_audio_is_unknown` reads it so (`response.url-empty`). The record lists
   only "not a string"; native would fetch `""` and fail with 500.
3. **Qwen `instructions` elided** on an `instruct` model is forwarded as an `as_is` reference (V3-1 interpretation 4),
   as `language_type` is; on another model it is dropped like any `instructions`.
4. **Qwen refusal order**: the operation and metering forms, a JSON body, `input`, then `response_format`; `voice`,
   `language_type` and `instructions` never refuse.
5. **Vertex refusal order**: the location (a 500 naming the key) first, then the operation and metering forms, the JSON
   body, `input` and `response_format`, as `image-vertex` reads its location before the shared `prepare`.
6. **Vertex `""` and padded formats**: the format is lower-cased, not trimmed, as native; `""` and `" wav"` are refused
   with the unknown-format text, naming the lower-cased value.
7. **Vertex audio part**: native's `find_map` is reproduced exactly: the first part of `candidates[0]` that has an
   `inlineData.data` member decides; a string or a host placeholder of at least one byte is audio, a 0-byte one or `""`
   is "a 2xx without audio", and any other JSON type falls to step 3 (block reason, policy finish, else no audio). Every
   candidate's `finishReason` is read for the policy table, as `gemini_image` reads it; `NO_IMAGE`, an image-only reason
   in the shared table, can therefore mark a speech answer too (open question 2).
8. **Azure `state`** carries `"op": "synthesize"` beside the record's `format` and `characters`, because one package
   parses both operations and the V3-3 transcription state already has a `format` member.
9. **Azure refusal order**: the operation and metering forms, a JSON body, `input`, `response_format`, an elided
   `voice`, and last the 1 MiB bound on the built document.
10. **Azure capabilities**: `asr_shared::capabilities` with the synthesis form gives every model `[synthesize,
    transcribe]` and `[characters, seconds]`; the record writes `[transcribe, synthesize]`, the same set (a manifest's
    capabilities and a declaration's operations are sets). The manifest lists `["transcribe", "synthesize"]` as the record
    does.
11. **The `x-microsoft-outputformat` header** is written lower-case, as every descriptor header is; native writes
    `X-Microsoft-OutputFormat` (HTTP names are case-insensitive).
12. **SV-Q11 and the voice**: the voice is compared less the forbidden characters too (the record states the removal for
    `input`; without it a voice holding both a special and a forbidden character could never pass). The row's own text
    now reports which comparison failed.
13. **An explicit `"upgrade_insecure": false`** on a `url` artifact is read as the default and not written back, so the
    member's canonical wire form is "present only when `true`".

### Points that contradict the record (flagged, not silently chosen)

- **§5 names `reference_integrity` for Qwen's `url` pointer** ("lands on an unelided string"). The suite's
  `reference_integrity` row has no `url` branch; the pointer's string is judged by `second_hop_artifact`, which reads the
  unelided body and so covers the same property. The row was not extended.
- **§1's table puts the pure function in `south-contracts::speech`; §2.5 says "in `south-contracts`".** It is
  `south_contracts::speech::upgrade_insecure_artifact_url_v1`, beside the member it serves (the record's §5 decision 5
  keeps it speech-only).

### Open questions

1. Should `image-qwen`'s broader 2xx `code` predicate (interpretation 1) and the speech one be unified in
   `bailian_image.rs`? Moving `image-qwen` to the record's narrower rule would change its behaviour (a non-string `code`
   would no longer be `unknown`), so it is left for lv.
2. `POLICY_FINISH_REASONS` is the image table and holds `NO_IMAGE` and three `IMAGE_*` words; whether Gemini TTS reports
   an audio counterpart is unverified (§13). Shared as the record says; a speech-specific table would be a later minor.
3. For the release (S-S-5): add `"speech": 1` to the three manifests with the other speech packages, and record the
   `upgrade_insecure` member in `contracts.speech` 1; no runtime moves. `speech-vertex` 1.1.0 (Q7's refusal) follows the
   host's acceptance of the Vertex TTS dual run, in a later release.
4. The host items of §7 (and V3-1 §7, V3-2 §4, V3-3 §6) remain prerequisites of any cutover; §11's measurements are
   listed in each pack's README.

### Gate results

Run on 2026-10-15 against the final tree (macOS), mirroring `.github/workflows/ci.yml`, with `set -o pipefail`:

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `PROPTEST_CASES=32 cargo nextest run --workspace --all-features` | 2,126 passed, 4 skipped, 0 failed (the sandbox parity tests included) |
| `cargo test --workspace --doc --all-features` | 3 passed |
| `cargo test --workspace --no-default-features` | 1,960 passed, 2 ignored, 0 failed |
| `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --all-features` | clean |
| `cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked` | passed |
| `scripts/check-boundaries.sh --self-test`, `scripts/check-boundaries.sh` | passed |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | 72 passed |
| `scripts/check-language.sh` (files, and its self-test) and `--commits` | passed |
| `cargo machete` (workspace and `fuzz/`) | no unused dependency |
| every `scripts/build-*-component.sh` | 37 built |
| `scripts/check-declared-runtime.sh --build` | every package loads under the runtime it declares (the nine speech packages under this tree, 0.64.0) |

`speech-qwen`, `speech-vertex` and `speech-azure` were also loaded by the published 0.64.0 runtime itself (a detached
checkout of tag `v0.64.0`, `declared_runtime_v1`): all three are admitted.

**First run.** Every pack expectation was written by hand before the first compile. Clippy asked for four changes (two
doc paragraphs, a `contains`, a simplified boolean); the Qwen, Vertex and Azure packs then passed on their first run with
no expectation changed. Two test-side corrections, not expectation changes: the Azure `content_type` field of a response
case does not reach the view's headers, so the cases that pin the relabelling also carry `headers` (the mutation that
echoes the upstream's type stayed green until then); and `declared_keys` includes the exported `project_id`, which the
`shipped_packages_v1` assertion first omitted.

**Mutations** (each applied to the source, the named test binaries run, the file restored from a copy):

| Mutation | Turned red |
|---|---|
| SV-Q10: the upgrade rewrites any `scheme://` prefix (accepting `ftp` and others) | `speech::tests::only_an_insecure_prefix_is_upgraded`, `speech_contract_v1::artifact_url_upgrade_vectors` |
| SV-Q10: the upgrade skipped | the same two, `qwen_speech_suite::the_audio_is_an_upgradable_url_artifact`, `::the_reference_passes_its_own_pack` (`second_hop_artifact` on `response.native`) |
| SV-Q11: `input` compared without removing the forbidden characters | `azure_speech_suite::the_reference_passes_its_own_pack` (`synthetic_speech_suite` stays green) |
| SV-Q11: the voice required as an exact attribute value | `azure_speech_suite::the_reference_passes_its_own_pack` (`synthetic_speech_suite` stays green) |
| Vertex Q7: `mp3` / `opus` / `aac` / `flac` no longer served as WAV | `vertex_speech_suite::q7_transcode_only_formats_degrade_to_wav`, `::the_container_follows_the_format`, `::the_reference_passes_its_own_pack` |
| Bailian: a 2xx `code` no longer `unknown` | `qwen_speech_suite::a_2xx_carrying_a_code_is_unknown`, `::the_reference_passes_its_own_pack` |
| Azure: the 1 MiB SSML refusal removed | `azure_speech_suite::the_ssml_is_bounded_by_the_text_body_limit`, `::the_reference_passes_its_own_pack` |

The suite tests also carry wrapper mutations that must turn named cases red (`every_qwen_mutation_turns_the_pack_red`:
no upgrade, a 2xx `code` ignored, SV-Q7 dropped; `every_vertex_mutation_turns_the_pack_red`: Q7 refused early, no
container, a blocked prompt as `unknown`; `every_azure_synthesis_mutation_turns_the_pack_red`: no SSML bound, an
unescaped voice, forbidden characters as references, the upstream `content-type` echoed), and the V3-1 break module
turns its row red on all three packs.

## Revision note

- 2026-10-15: first draft (proposed).
- 2026-10-15: lv accepted the record and ruled SV-Q10 and SV-Q11 as recommended (§12); status accepted.
- 2026-10-15: Implementation section added (what landed, thirteen interpretations, two points that contradict the record:
  `reference_integrity` has no `url` branch and the function's module; two open questions; gate results and mutations);
  status implemented on branch `speech-v4`. No design decision of this record changed.
- 2026-10-15: speech step S-S-5 records `contracts.speech` 1 in `compatibility.json` and adds `"speech": 1` to this
  batch's manifests (speech record, S-S-5 implementation); status declared in `contracts.speech` 1, to be released in
  0.65.0. No design decision of this record changed.
