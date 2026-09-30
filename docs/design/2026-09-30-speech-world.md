# Speech World: TTS and ASR (speech-adapter-v1)

Status: **proposed — drafted for review by the host team (token-station-server P21/P22/P23), not accepted**

Date: 2026-09-30

Rulings: on 2026-09-30 the host owner (lv) ruled on Q3, Q4, Q5, Q6, Q7 and Q8 (§16), each as recommended. Q3 and Q8
also need the south maintainers. Q10 and Q11 depend on other host plans and stay open.


Baseline: south `origin/main` = `3135e36` (v0.42.0). Server line numbers come from the local checkout `8b2a1976`; P23
was written at `d672b945`, so its line numbers have drifted — re-verify in place before citing.

Citation convention: south file names as in `2026-09-30-image-world.md`. Server file names are relative to
`gateway/src/modules/`: `audio.rs` is `inference/handler/audio.rs`; `tts.rs`, `tts_providers.rs` and `multipart.rs`
are in `inference/handler/audio/`; `elevenlabs.rs` and `south_binary.rs` are in `inference/handler/`; `upstream.rs`,
`south_adapter.rs`, `south_switch.rs` and `request_extras.rs` are in `inference/engine/`; `media.rs` is in
`inference/engine/token_counter/`; `pricing_state.rs` is in `catalog/`; `settlements.rs` is in `billing/repo/`;
`webhook_sender.rs` is in `tasks/`; `usage_types.rs` is in `crates/gateway-provider-protocol/src/`.

Predecessors:
`2026-09-30-image-world.md` (**this record's premise**: its §6 media vocabulary — request view, elision rules, request
descriptor, closed transforms, response view — and its §11 safe fetch executor are reused unchanged; this record only
writes the differences on the speech surface), `2026-09-09-multipart-request-body.md` (0.25.0),
`2026-09-09-buffered-binary-response.md` (0.26.0), `2026-09-18-task-adapter-world.md`,
`2026-09-27-task-contract-v6-facts.md`, `2026-09-30-host-zero-vendor-boundary.md` (its §3 credential recipe for
Vertex, §4.2 descriptor auth admission, §6.3 host checks and undetectable zone).

Origin: token-station-server P23 (DV1 / DV2 / DV4 as recommended; DV3 — lv has endorsed "fix it at the root, in the
component"; DV5 is P21 DP1), P21 DP0 / DP1.

## 1. Problem

The server's speech surface has 9 TTS arms and 4 ASR arms, all translated by the host:

- **TTS**: `TtsUpstream` has 9 variants (`audio.rs:617-648`), and `resolve` matches in order (`tts.rs:71-140`): Vertex
  with `tts` in the model name, `name == "xai"`, `name == "xiaomi"`, Bailian with `tts`, MiniMax starting with
  `speech-`, AzureSpeech, `name == "groq"` with `orpheus`, ElevenLabs starting with `eleven_`, and everything else
  falls to OpenAI-compatible. Three of these discriminate by **provider row name** (P23-F2, a J1 red item).
- **ASR**: `AsrUpstream` has 4 variants (`audio.rs:50`); `resolve` checks ElevenLabs, `name == "xai"`, AzureSpeech,
  and otherwise OpenAI-compatible (`audio.rs:88-98`).
- Voice mapping, format tables, SSML assembly, hex / base64 decoding, adding WAV headers, SSE concatenation,
  second-hop audio fetching and duration extraction are all written in the host (about 41 per-provider functions in
  `tts_providers.rs`).

All south can offer today is transport: binary responses for TTS (`BufferedBinaryResponseV1`, `lib.rs:2331`) and
multipart requests for ASR (`MultipartPostRequestV1`, `lib.rs:1585`), and both switches are off by default
(`south_switch.rs:225,231`) — without a database row, **not a single speech arm goes through south**. The header
names for ElevenLabs and Azure Speech have long been in the closed set (`xi-api-key`, `ocp-apim-subscription-key`,
`lib.rs:884-894`), yet the host's `south_auth_for` excludes them by type (`south_adapter.rs:198`) — an authentication
decision the host makes by provider type, exactly what P21 J2b ① is meant to remove (on the south side, by the
descriptor auth admission of the boundary record §4).

The reasons the existing worlds cannot hold this are the same as in the image record §1 (the provider world only does
chat, task-world artifacts can only be URLs, kernel descriptors can only carry JSON bodies, response bodies can only be
strings), and the speech surface adds two more: **SSML is an `application/ssml+xml` text body**, which neither the
kernel descriptor nor south's three existing request shapes can express; and **ElevenLabs's `output_format` travels
as a query parameter** (`tts.rs:616-618`), which is not in the closed `QueryParameterV1` set (`lib.rs:1064-1090`), so
the host's south binary plan falls back to the old path when it meets it (`south_binary.rs:208-212`).

## 2. Scope

**In scope**: every execution arm of `/v1/audio/speech`, `/v1/audio/transcriptions`, `/v1/audio/translations` and
their `/openai/` aliases.

**Out of scope**: the native `/elevenlabs/v1/text-to-speech/{voice_id}` route (P25; it shares the immutability table
`tts_body_rules` with the unified surface, and this record ensures that the table's semantics remain usable by it
once they are declared by the component); client-side streaming TTS (DV4); realtime speech; Vertex service-account
minting itself (P21 S3; specified as credential recipe v1 in the boundary record §3, phase B4 there — this world only
consumes the minted slot, §3).

## 3. D1 — A new world, a separate package, no `host` import (DV1)

**Package**: `token-station:speech-adapter@1.0.0`, world `speech-adapter-v1`, suite `south.speech-component.v1` — the
same `token-station:*-adapter@1.0.0` / `*-adapter-v1` naming as `image-adapter-v1` and `embeddings-adapter-v1`. The
reasoning is the same as the image record's D2 (§4 there): adapter@2 is not touched (adding an export is a major); the
task world cannot hold audio bytes; and the package version does not become entangled with the image world's. The
alternative "one synchronous media world holding both image and speech" and its trade-offs are in the image record
§16. lv ruled on 2026-09-30 for separate worlds (Q8 here, Q4 there); the south maintainers' ruling is still needed.

**No `host` import**: there is no signing consumer (precedent: `task-adapter-v2.wit:63-66`). Host-import linking and
the import scan follow the image record §4.

**Auth arms**: `bearer`, `header_secret` (`xi-api-key` and `ocp-apim-subscription-key` are already sanctioned
headers). Vertex needs no new arm: the component declares the service-account credential recipe in its manifest, the
host executes it and presents the minted slot as `bearer` (boundary record §3.3; phase B4 there, which unlocks P21 S3
and P23 Vertex TTS). This world does not admit the `oauth` arm, which the boundary record proposes to deprecate (§3.7
there). The auth form is declared by the manifest and executed by the host as declared (descriptor auth admission,
boundary record §4.2); **the host must no longer decide by provider type whether a provider may go through a
component** (the `south_adapter.rs:198` branch is deleted with the migration).

## 4. D2 — Operation words and the root fix for "transcribes but does not translate" (F3)

Manifest capability words: `synthesize`, `transcribe`, `translate`, `artifact_fetch`.

- At least one of `synthesize` or `transcribe` must be declared; `translate` requires `transcribe` to be declared as
  well.
- `artifact_fetch` is optional and means the same as in the task world: the component may produce URL artifacts that
  the host has to fetch (precedent: `manifest.rs:109-121`), so the host knows at load time whether to wire up the
  second hop.
- Which operations each model supports is declared per model by `model-capabilities` (among OpenAI-compatible
  upstreams, only some models support translations).

Host obligation: if the requested operation is not declared by both **the component** and **the model**, 400 before
admission — no `prepare` call, no funds action. This is the general form of the P23-F3 fix (`audio.rs:84-86`,
`:177-181`, written today as "only the OpenAI-compatible arm supports translation"): the host no longer knows who can
translate.

The conformance suite cannot verify "is the translation output English" — that is semantics, not shape. P23 V1's
"verify with non-English audio that the output is English" stays in host acceptance (§11), and must not be replaced by
agreement with old results: the old results are themselves wrong on the ElevenLabs, xAI and Azure arms.

## 5. D3 — Request descriptors: JSON / SSML / multipart

The image record §6.3's `MediaRequestDescriptorV1` is reused; the speech surface uses three body kinds, plus one gap in
the transport contract:

| Body | Used by | Existing transport shape |
|---|---|---|
| `json {template}` | every TTS arm except Azure | `JsonPostRequestV1`, binary responses via `execute_binary_call_v1` |
| `text {media_type, text}`, `media_type` closed to `application/ssml+xml` | Azure TTS | **none**: south has only JSON POST, GET and multipart POST |
| `multipart {parts}` | the four ASR arms | `MultipartPostRequestV1` (the response is UTF-8, which is enough) |

**D3a — SSML text body**. Two options:
- **A — add a transport shape** (recommended): the HTTP contract adds `TextPostRequestV1` (a bounded UTF-8 body, media
  type from a closed set, the host must not add its own `content-type` — the same rule as multipart's D3), together
  with an execution entry point for binary responses. It is an additive change, like the 0.24.0 / 0.25.0 / 0.26.0
  precedents.
- **B — the host sends it itself**: the component still describes the SSML, and the host sends it without going
  through south-core. The contract is untouched, but the host writes a separate sending path for one body kind, and
  endpoint binding, header rules and auth assembly all have to be done again.

**D3b — the `output_format` query parameter**. ElevenLabs TTS must carry `?output_format=mp3_44100_128`
(`tts.rs:616-618`). Recommended: add `OutputFormat` to `QueryParameterV1` (wire name `output_format`, grammar
`[a-z0-9_]{1,32}`), added — like `GroupId` and `TaskId` — only because it has a consumer. D3a and D3b together take
the HTTP contract from 9 to 10. If the boundary record §10 (closed mechanisms, manifest-declared instances) lands
first, `output_format` is instead declared by the component as a `query_parameters` entry with a value grammar from
that record's closed set, and D3b needs no contract change.

**SSML escaping lives in the component**: the Azure arm today assembles `<speak …><voice name='…'>{text}</voice></speak>`
in the host and does the XML escaping (`tts_providers.rs:161-200`). The component can read the input text (it is far
below the image record §6.2's 1 MiB fallback threshold), so escaping moves with the component, and the conformance
suite must include fixtures containing `<`, `&` and quotes.

**ASR multipart**: the host splits the client body into an ordered part list for the component (image record §6.1),
the component outputs the upstream part list, and the host encodes it generically. Today the four arms respectively
do byte replacement of the first occurrence (OpenAI-compatible swaps the model; ElevenLabs changes `name="model"` to
`name="model_id"` and `name="language"` to `name="language_code"`, `multipart.rs:13-22`, `:81-94`), field deletion
(xAI, `multipart.rs:28-75`), and full reconstruction (Azure renames `file → audio` and appends a `definition` JSON
part, `multipart.rs:291-328`). "Replace only the first occurrence" byte replacement carries its own risk of collateral
damage; with part lists that class of problem disappears, but the dual run can only compare decoded part lists (image
record §14).

## 6. D4 — Bytes in and out: declared by the component, executed by the host as closed transforms

P23 V1 says "bytes in, bytes out; base64 or hex decoding and adding the WAV header are pure functions inside the
component". **This record proposes instead that the component declares and the host executes**, because of the
runtime limits (single payload 16 MiB, memory 64 MiB, 2 seconds, `runtime.rs:26-34`) and because ordinary calls are
serialized through one instance (`component.rs:136`):

- MiniMax returns audio as hex (`tts_providers.rs:1106-1147`), doubling its size: a 10-minute 128 kbps mp3 is about
  9.6 MB, about 19.2 MB as hex — already past 16 MiB;
- Vertex returns base64 PCM (24 kHz, mono, 16-bit, `tts_providers.rs:860`): 48 KB per second, 14.4 MB for 5 minutes,
  about 19.2 MB as base64;
- OpenAI's token-priced SSE path splits the audio into many base64 deltas (`tts_providers.rs:7-65`), with a total of
  the same order of magnitude.

Reading these into the sandbox and emitting them again means either relaxing the limits (memory doubles with the
number of concurrent instances) or failing at random around the critical length. The component does not need to
**read** the audio here; it only needs to say **where** the audio is and **which public standard encodes it**.

**Artifact form `SpeechArtifactV1`** (TTS):

| Form | Description | Used by |
|---|---|---|
| `body {media_type}` | the whole response body is the audio (`binary` framing) | OpenAI-compatible (non-token), xAI, Groq, ElevenLabs, Azure |
| `inline {pointer, encoding, media_type}`, `encoding` is `base64` / `hex` | a string (already elided) in the response JSON | MiMo (base64), MiniMax (hex), Vertex (base64) |
| `segments {items: [{pointer, encoding}], media_type}` | several fragments concatenated in order | OpenAI token-priced SSE (`sse` framing) |
| `url {url, media_type}` | a URL from the upstream, fetched by the host through the safe fetch executor | Bailian Qwen-TTS |

Any form may additionally carry `container: {"wav_pcm_s16le": {"sample_rate": …, "channels": …}}`, and the host adds a
44-byte header per the RIFF specification (today `tts_providers.rs:937-963`: 24000 Hz / mono / 16-bit). The transform
words join the image record §6.4's closed table: `from_hex`, `concat`, `wav_pcm_s16le`. `media_type` is declared by
the component (today it is scattered across the arms: Azure per its format table, Vertex `audio/wav` or
`audio/L16; rate=24000`, Bailian always `audio/wav`).

**SSE response framing**: the component declares `sse` in `prepare`, and the host decodes the buffered SSE into an
event list `[{event, data: <elided JSON view>}]` for the component. This is the media-world response framing word of
the image record §6.5, which the host implements once for both media worlds. It is not the provider world's stream
framing (`stream_framing`, boundary record §5.2), which declines `sse` because provider-world components receive raw
chunks and split SSE themselves; here the body stays out of the sandbox, so the host does the splitting.

**ASR delivery** is a document, not bytes: `delivery: {"json": <template>} | {"text": "…", "media_type"}`. The
component renders the format the client asked for itself — today Azure is rendered by the host as json / verbose_json
/ text, with srt / vtt refused (`multipart.rs:356-402`), and ElevenLabs is rewritten down to just `{"text"}`
(`multipart.rs:456-476`); all of that moves with the component.

## 7. D5 — Function set

```wit
// Speech synthesis and recognition. All exports are pure translation.
// The host owns bytes, credentials, HTTP, clock, pricing, persistence and delivery.
package token-station:speech-adapter@1.0.0;

interface speech-adapter {
    record adapter-metadata { name: string, version: string, api-version: string }
    enum health-status { ready, degraded, unavailable }
    record adapter-health { status: health-status, detail: option<string> }

    type json = string;

    metadata: func() -> adapter-metadata;
    healthcheck: func() -> adapter-health;

    // ProviderConfig -> list<SpeechModelCapabilitiesV1>: operations, voices/formats, metering forms.
    model-capabilities: func(provider-config: json) -> result<json, json>;

    // ProviderConfig, MediaRequestViewV1, SpeechCallContextV1 -> PreparedSpeechCallV1.
    // Err(ErrorEnvelope) is a pre-dispatch refusal (unsupported format, voice, operation).
    prepare: func(provider-config: json, request: json, context: json) -> result<json, json>;

    // Prepared state, MediaResponseViewV1 -> SpeechOutcomeV1 (delivery + metering, or a failure kind).
    parse-response: func(state: json, response: json) -> result<json, json>;
}

// No host import: this world has no signing consumer.
world speech-adapter-v1 {
    export speech-adapter;
}
```

One function fewer than the image world — no `render`: speech has no multi-round aggregation, ASR's client format can
be rendered at parse time, and no host clock is needed. Second-hop audio needs no second function: the fetched bytes
are delivered as-is with the media type the component declared. `SpeechOutcomeV1` uses the image record §9.2's four
outcomes (`succeeded`, `rejected`, `charged_failure`, `unknown`). MiniMax T2A errors arrive as HTTP 200 +
`base_resp.status_code` (`tts_providers.rs:1106-1147`), so the outcome decision must be made in the component.

**`PreparedSpeechCallV1`** carries: the request descriptor; the pre-dispatch facts (§8); the response framing and
elision paths; `state` (≤ 8 KiB); and the immutability declaration for request extras — JSON bodies use
`immutable_body_paths` (task contract 6 §4 semantics, replacing `tts_body_rules`, `tts.rs:12-69`), SSML bodies give
`null` (the host must not inject — today's Azure `body_unsupported`), and multipart bodies use
`immutable_form_fields` (replacing ASR's `owned_form_fields`, `audio.rs:71-78`). If P25 chooses branch A for the
native ElevenLabs route, it can read the same declaration directly.

## 8. D6 — Pre-dispatch facts

| Fact | Form | Which host provider logic it replaces |
|---|---|---|
| `operation` | `synthesize` / `transcribe` / `translate` | `supports_translation` (`audio.rs:84-86`) |
| `characters` | `{count, upper?}`: `count` is the number of Unicode scalar values in the northbound `input`; `upper` is an upper bound under the upstream's counting rule (non-ASCII counting as 2 and the like) | `weighted_upper_chars` and the `bailian_tts \|\| minimax_audio_format.is_some()` branch (`tts.rs:268-271`, `:760-777`) |
| `metering_forms` | the forms that must be reported on success: `characters` / `seconds` / `tokens` | each arm deciding for itself whether to read upstream usage |
| `reservation` | `max_output_tokens?` (null = the host uses its own default, today `default_max_output_tokens` 4096), `max_seconds?` | the output bound for token-priced TTS |
| `output_media_type` | the delivered media type (TTS) | each arm's format table |

An unsupported format, voice or language makes `prepare` return `invalid_request` directly, still before admission
(today these 400s all happen before admission, in the format tables of `tts_providers.rs`). `tts_max_input_chars`
(`tts.rs:279-286`) is catalog data and stays in the host.

**The three reservation algorithms are unchanged (C4.2a / C4.2b / C4.3, required by P23 §5)**; only the branch
conditions become generic facts:
- Per character: `calculate_tts_cost(characters.upper ?? characters.count)` — the upper bound when there is one
  (today's weighted characters for Bailian and MiniMax), otherwise the count;
- Per token: input estimated from `characters.count`, output from `reservation.max_output_tokens ?? the host
  default`;
- ASR: the host-configured `asr_max_billable_seconds` (`audio.rs:335-336`); the component may give a tighter
  `max_seconds`, and the host takes the smaller.

**Pricing form** as in the image record §9.3: the host keeps the single decision (the speech surface already has one,
`tts_uses_token_pricing`, `pricing_state.rs:104-108`) and passes it to `prepare` as `context.metering_required`. The
OpenAI-compatible component uses it to decide whether to ask the upstream for `stream_format: "sse"` — today the host
does this by rewriting the request body according to the pricing form (`tts.rs:628-637`); after migration the host
only says "I need token evidence", and how to get it is the component's business. A component that cannot report the
required form refuses in `prepare`. One configuration is affected: a row on a non-OpenAI-compatible arm configured
with only a token price (character price 0) passes the listing check (`tts_uses_token_pricing` looks only at the
model row, not at the arm), but those arms compute the reservation upper bound from the character price
(`tts.rs:760-773`), get 0, and the host refuses every request at admission with a "no price configured" 400 — the
row can be listed but never served, and no funds move (host P21 §9, P23-F7; verified 2026-09-30). After migration it
is still a refusal before admission, now raised by `prepare` and naming the real cause.

## 9. D7 — Metering units and host generic checks

`SpeechMeteringV1` (all nullable; **null and 0 are different facts**):

| Field | Meaning | Source today |
|---|---|---|
| `characters {value, source}` | `source = upstream`: billed characters the upstream reports; `source = request`: the upstream bills by input characters but reports no number | Bailian `usage.characters` (`tts.rs:981-987`), MiniMax `extra_info.usage_characters` (`tts.rs:1017-1023`); for the other arms the host counts locally with `chars().count()` (`tts.rs:264`) |
| `seconds {value}` | ASR audio duration | `duration` (OpenAI-compatible, xAI, `audio.rs:514-519`), `words[-1].end` (ElevenLabs), `durationMilliseconds` (Azure, `multipart.rs:418-446`) |
| `tokens {input, output, input_text?, input_audio?}` | upstream token counts | OpenAI TTS SSE `speech.audio.done.usage` (`tts_providers.rs:37-46`); ASR token counts as side information (`SecondsWithTokens`, `usage_types.rs:129-221`) |

All of these units are already in ARCHITECTURE.md's admission list ("tokens, seconds, images, characters, milliunits
an upstream reported", `ARCHITECTURE.md:110`). P23 §1.5's statement that "no seconds-based usage unit can be found in
the south contract" is **inaccurate**: the task world already has `TaskUsageFactsV2.seconds` (`task_v2.rs:100-105`).
What is missing is characters, and a set of fact types belonging to the speech world.

**Handling missing evidence** (the division DV3 settled):

- `tokens` declared in `metering_forms` but no `usage` in the SSE → outcome `unknown`, **not 0**. Today, when
  `speech.audio.done` lacks the `usage` object, the host takes 0 for both counts (`tts_providers.rs:37-46`:
  `unwrap_or(0)`), and token-priced rows settled at $0 — a defect found in this survey, of the same kind as
  P22-F2; fixed in the host on 2026-09-30 (P21 §9, P23-F6: missing usage now ends in `delivery_unknown`).
- `seconds` missing: the component decides per dialect. If the upstream **may legitimately not report** it for this
  response format (e.g. srt / vtt), report `seconds: null`, and the host applies the generic rule of a 60-second
  fallback and marks `quantity_estimated = 1` (`audio.rs:535-540`, DV3's interim scheme); if the dialect **always
  reports** it and did not this time (Azure), report `unknown`, consistent with today's Azure 502
  (`audio.rs:489-506`). Each component's choice is pinned by conformance fixtures.
- `characters.source = upstream` with no number: fall back to `source = request`, consistent with today's "override
  the local count only when the upstream reports one".

**F1's root fix is in the component** (DV3): the component decides what to ask the upstream for in order to get a
duration. OpenAI-compatible: when the client asks for `json` / `text` or does not specify, ask the upstream for
`verbose_json` and render the client's format from it; when the client asks for `verbose_json`, pass it through; for
`srt` / `vtt`, pass through the upstream's native format with `seconds: null` (whether to use the last subtitle's end
time instead: see Q4). If the upstream json response carries its own usage seconds (whisper's
`usage.type = "duration"` as OpenAI documents it, **to be measured**), use it directly. ElevenLabs: guarantee that
word-level timestamps are requested (`timestamps_granularity` is already a field the host treats as owned,
`audio.rs:71-78`), and read the end time of the last word.

**Host generic checks** (dialect-independent; any violation is routed to manual review):
1. When `characters.source = request`, `value` must **equal** the host's own count of the northbound `input` — the
   northbound `input` is an OpenAI speech protocol field and part of the product surface, so counting it is not
   provider logic;
2. When `characters.source = upstream`, `value ≤ prepare.characters.upper ?? count × 2`;
3. `seconds ≤` the seconds bound used for the reservation (today there is no clamp at all; review is triggered only
   when the amount goes out of bounds at settlement, see the comment at `audio.rs:579-582`);
4. `tokens.output ≤` the output bound of `reservation`; when the buckets and the total both appear, they are equal;
5. Settlement ≤ reservation (existing).

These out-of-bound and internal-consistency checks can only find values outside the bounds, not under-reporting
within them (accepted by DP1; the same undetectable zone as the boundary record §6.3).

## 10. D8 — Second-hop audio (DV2)

Bailian Qwen-TTS returns `output.audio.url`, and the host fetches it on a second hop (`tts.rs:977-1000`,
`tts_providers.rs:742-789`). After migration the component only produces a `url {url, media_type: "audio/wav"}`
artifact and declares `artifact_fetch` in its manifest; execution falls entirely on the image record §11's **single**
safe fetch executor, with not one rule changed: https only, refuse userinfo, resolve DNS and pin the address, refuse if
any address falls in a forbidden range, no redirects, system proxy disabled, no credential headers, a total timeout
and a byte limit, an empty body is a failure. The fixes for P23-F4 and server E-1 (`guarded_asset_client`,
`webhook_sender.rs:491-513`) are the current implementation of exactly these rules.

**Delivery order**: verify-then-settle — settle only after the fetch succeeds and is verified (non-empty, within the
limit); failure → `delivery_unknown`. Bailian already uses this order today (a decode or fetch failure goes to
`delivery_unknown`), the same as the image record §10.2's DI6 = A; no ledger state needs to change.

## 11. D9 — Conformance suite `south.speech-component.v1`

Fixture families: `capabilities`, `prepare`, `response`. Carried over: coverage, fixture equality, determinism,
unknown-field tolerance, `terminal_only_from_the_wire`, `endpoint_confinement`, `reference_integrity` (defined in the
image record §12.1); plus required rows for the speech surface:

| Check | Requirement |
|---|---|
| `metering_sample` | every declared form (`characters` counted separately for each of its two sources, `seconds`, `tokens`) has at least one `succeeded` fixture giving the exact value |
| `missing_meter_is_not_zero` | a component declaring `tokens` has at least one 2xx fixture missing `usage`, with outcome `unknown` |
| `absent_duration_is_null_or_unknown` | a component declaring `seconds` has at least one fixture missing the duration, whose result can only be `seconds: null` or `unknown` — never 0 or any fallback number |
| `undeclared_operation_refused` | for a model that does not declare `translate`, `prepare(translate)` returns an error and produces no descriptor |
| `ssml_escaping` (components that emit SSML) | with input containing `<`, `&`, `'`, `"`, the SSML is valid and the content unchanged |
| `second_hop_artifact` (components that declare `artifact_fetch`) | at least one `url` artifact fixture, and the URL passes the `ArtifactUrlV1` grammar |
| `empty_audio_is_unknown` | 2xx but no audio (empty body, empty `data.audio`, no deltas) → `unknown` |

The metering samples are transcribed from the server's native arms (the P13 S8 pattern); they are exactly the "usage
samples" P21 DP1 requires.

**Host obligations** (gate ③): all of the image record §12.3 applies, plus: the WAV container encoding is byte-for-byte
equal to the vectors south provides; SSE decoding is deterministic; undeclared operations are refused before
admission; the estimate mark is set when `seconds: null`; the translation capability is accepted with non-English
audio (semantics, not in the component suite).

## 12. Mapping of the existing execution arms

**TTS** (today's selection → after migration, always selected by the model row's component route; the host no longer
looks at row names or model names):

| Execution arm | Upstream request | Auth | Artifact / transform | Metering | Batch |
|---|---|---|---|---|---|
| OpenAI-compatible | JSON `/v1/audio/speech`, only `model` changed; `stream_format: "sse"` added when token evidence is needed | `bearer` | `body`; `segments` (base64) for SSE | `characters(request)` or `tokens` | V3-1 |
| xAI | JSON `/v1/tts`, `{text, voice_id, language, output_format?}`; format table (`tts_providers.rs:264-300`) | `bearer` | `body` | `characters(request)` | V3-1 |
| Groq Orpheus | JSON `/v1/audio/speech`, voices mapped to personas (`tts_providers.rs:332-366`) | `bearer` | `body` | `characters(request)` | V3-1 |
| ElevenLabs | JSON `/v1/text-to-speech/{voice_id}` + `output_format` query (D3b) | `header_secret` `xi-api-key` | `body` | `characters(request)` | V3-1 (depends on D3b) |
| MiMo | JSON `/v1/chat/completions` + `audio` object | `bearer` | `inline base64` (`choices[0].message.audio.data`) | `characters(request)` | V3-2 |
| MiniMax T2A | JSON `/v1/t2a_v2`, `GroupId` query (already sanctioned) | `bearer` | `inline hex` (`data.audio`) | `characters(upstream)`, weighted upper bound | V3-2 |
| Bailian Qwen-TTS | JSON multimodal-generation | `bearer` | `url` + second hop (§10) | `characters(upstream)`, weighted upper bound | V4 |
| Vertex Gemini TTS | JSON `:generateContent`, AUDIO modality; project / region in the URL, taken from an exported credential attribute or non-secret config (boundary record §3.3, §7.3) | `bearer`; the slot is `minted` by the service-account credential recipe (boundary record §3.3) | `inline base64` + `wav_pcm_s16le{24000, 1}`, or raw PCM | `characters(request)` | V4 |
| Azure Speech | **SSML** (D3a) + `X-Microsoft-OutputFormat` header | `header_secret` `ocp-apim-subscription-key` | `body`; empty 2xx → `unknown` | `characters(request)` | V4 (depends on D3a) |

**ASR**:

| Execution arm | Upstream request | Auth | Delivery | Duration source | Batch |
|---|---|---|---|---|---|
| OpenAI-compatible | multipart part list, each part `as_is`, only `model` changed; whether to ask for `verbose_json` decided per §9 | `bearer` | the component renders the client format | `duration` / usage seconds | V3-3 |
| xAI | multipart `/v1/stt`, `model` removed | `bearer` | pass-through | `duration` | V3-3 |
| ElevenLabs | multipart `/v1/speech-to-text`, `model → model_id`, `language → language_code` | `header_secret` `xi-api-key` | `{"text"}` | `words[-1].end` | V3-3 |
| Azure fast transcription | multipart `file → audio` + `definition` part; `api-version` query (already sanctioned) | `header_secret` `ocp-apim-subscription-key` | json / verbose_json / text; srt and vtt refused in `prepare` | `durationMilliseconds`; missing → `unknown` | V3-3 |

Vertex today silently downgrades mp3 / opus / aac / flac to wav (`tts_providers.rs:902-919`): the client asks for mp3,
gets wav, status 200. For dual-run agreement the component copies this; after the dual run it refuses the
unsupported formats in `prepare` instead (ruled by lv on 2026-09-30, Q7).

## 13. Migration order and dual-run acceptance

**Prerequisite (V0, host)**: done as of 2026-09-30. The $0 settlement found in this record — §9's "SSE missing
`usage` counts as 0" — is fixed (P23-F6); §8's "a non-compatible arm configured with only a token price" turned out
on verification to be refused at admission, not settled at $0, and is left to this migration (P23-F7). The other V0
items (the F1 interim scheme, F3, F4, handler-level cases) have landed.

**Order** (P23 V3–V4; on the south side, in the same batch as the image world's first minor or right after it):
1. **V3-1, the binary-returning arms**: xAI, Groq, OpenAI-compatible (including the SSE path), ElevenLabs (waits for
   D3b);
2. **V3-2, JSON-wrapped audio**: MiMo, MiniMax;
3. **V3-3, the four ASR arms**: need the host part parser and encoder (image record §6.1, §6.7);
4. **V4**: Bailian (second hop), Vertex (credential recipe v1 — boundary record §3, phase B4 there, i.e. P21 S3 — plus
   the WAV container), Azure SSML (D3a).

**Dual-run reconciliation** (per arm, both billing forms):
- Upstream request: JSON body equal by value; SSML equal as strings; multipart equal as decoded part lists; request
  headers equal;
- Client response: bytes and `Content-Type` equal **byte for byte** (audio); ASR document JSON equal by value, text
  equal as strings;
- Reservation bound, frozen usage kinds and values, and amounts equal;
- **Not accepted by dual run**: translation (the old results are wrong on three arms, §4); ASR duration — once the
  component asks for `verbose_json` per §9, the upstream request body **necessarily differs** from the old path, so
  acceptance becomes "the duration comes from the upstream's report, `quantity_estimated = 0`", while client response
  equality is kept;
- In addition, run J2 end to end with a synthetic provider: a speech component the host has never seen, which the
  host can use to synthesize and transcribe with zero changes (the speech-world counterpart of the boundary record's
  unseen-provider guest (T21), §12 there).

## 14. Versioning

A South **minor**, sharing `contracts.media` with the image world:

- A new WIT `crates/south-provider-api/wit/speech-adapter.wit`; `manifest.rs` gains `SPEECH_*` constants and
  `SPEECH_WORLD_SCHEMA` (capability words in §4; `validate_role` adds "at least one synthesis or transcription word;
  `translate` implies `transcribe`"); `KNOWN_WORLDS` gains a row.
- The runtime gains one `bindgen!` module and one `InstanceKind` variant; the limits are unchanged.
- `south-contracts`: a `speech` module (facts, outcomes, artifacts); the `media` module gains three transform words
  (§6).
- If D3a / D3b are adopted: `HTTP_CONTRACT_VERSION` 9 → 10 (`TextPostRequestV1` and its binary execution entry point,
  `QueryParameterV1::OutputFormat`), with matching new provider-suite rows; whether they form their own suite or join
  an existing one, following the 0.25.0 / 0.26.0 precedents, is for the maintainers to decide.
- `compatibility.json`: `contracts.speech: 1`, `conformance.speech_component_v1_suite_id`, a host verification block
  (`not_verified` at first release).
- If this lands together with the compatibility range of the boundary record §8, speech packages declare
  `contracts: {"media": 1, "speech": 1}` (image record §15, embeddings record §12).

## 15. Rejected alternatives

- **Add functions to provider-adapter-v2** (rejected by DV1): speech input and output are not chat IR, and adding an
  export is a major.
- **Reuse the task world** (rejected by DV1): artifacts can only be URLs; it cannot hold hex / base64 / binary audio.
- **Decoding in the component** (P23 V1's original text): see the numbers in §6; instead the component declares and
  the host executes closed public-standard transforms.
- **The host keeps rewriting the OpenAI TTS request body by pricing form**: the host would be deciding the upstream
  request shape on the component's behalf, violating DP0; replaced by `metering_required` (§8).
- **ASR without a duration is always `delivery_unknown`** (DV3 option C): transcriptions in the default format would
  all return 502.
- **An `oauth` auth arm for Vertex, added in a later minor** (this record's earlier draft): superseded by the boundary
  record §3 — the component declares a credential recipe, the host executes it and presents the minted slot as
  `bearer`. The boundary record proposes deprecating the `oauth` arm (§3.7 there), so this world never admits it.
- **Client-side streaming TTS** (DV4): not included. Adding a streaming export later is a new version of
  `speech-adapter`; if by then a single media world has been formed per the image record §16, that upgrade would drag
  along every image component — a point Q8 has to weigh as well.

## 16. Open questions

| # | Question | Recommendation | Decided by |
|---|---|---|---|
| Q1 | SSML: add `TextPostRequestV1` (HTTP contract 10), or the host sends it itself (D3a) | Add it | south maintainers |
| Q2 | Add the sanctioned query parameter `output_format` (D3b) | Add it; if the boundary record §10 lands first, declare it in the component's manifest instead | south maintainers |
| Q3 | Decoding and container wrapping executed by host closed transforms, rather than P23 V1's "pure functions inside the component" (§6) | The host executes them | lv — **ruled by lv, 2026-09-30: as recommended** (P23 V1's text is amended accordingly); south maintainers (transform table) — open |
| Q4 | When the ASR client asks for srt / vtt: `seconds: null` with estimation, or the end time of the last subtitle (which slightly undercounts silence) | Estimate, visibly in the ledger | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q5 | "Character count" defined as the number of Unicode scalar values in the northbound `input`; the host checks `source = request` for exact equality | As stated | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q6 | Azure missing duration: keep `unknown` (502 today), or fold it into the generic estimation | Keep `unknown` | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q7 | Vertex silently downgrading mp3 and others to wav: copy it, or refuse in `prepare` | Copy it during the dual run; refuse afterwards | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q8 | Two separate worlds for image and speech, or one synchronous media world (image record Q4) | Separate | lv — **ruled by lv, 2026-09-30: as recommended**; south maintainers — open |
| Q9 | A second consumer of the metering vocabulary: the community host today has no multipart surface and no byte-returning surface (`2026-09-09-multipart-request-body.md:189`, `2026-09-09-buffered-binary-response.md:302`), so per `ARCHITECTURE.md:114-115` the admission condition is not met at present. The same question is open as the image record Q5, the boundary record Q9 and the embeddings record E-Q5 | Take P21 §7's "synchronous implementation recommended" as the written commitment; otherwise do not admit for now | lv + south maintainers |
| Q10 | Rows on non-OpenAI-compatible arms configured with only a token price: today they can be listed but every request is refused at admission with a "no price configured" 400 (no funds move); after migration the refusal comes from `prepare` and names the cause (§8) | Accept; list the affected rows with a read-only query before cutting over | lv |
| Q11 | If P25 chooses A for the native ElevenLabs route, does it reuse this world's components and immutability declaration directly | Reuse | lv (P25) |
