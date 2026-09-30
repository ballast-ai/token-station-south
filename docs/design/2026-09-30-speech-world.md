# Speech World: TTS and ASR (speech-adapter-v1)

Status: **proposed — drafted for review by the host team (token-station-server P21/P22/P23), not accepted**

Date: 2026-09-30

Revised: 2026-10-01 after independent review (see the revision note at the end).

Rulings: on 2026-09-30 the host owner (lv) ruled on Q3, Q4, Q5, Q6, Q7 and Q8 (§16), each as recommended. Q3 and Q8
also need the south maintainers. On 2026-10-01 lv ruled that the ASR missing-duration fix goes into the component
(§9; recorded under Q4). Q10 and Q11 depend on other host plans and stay open.


Baseline: south `origin/main` = `3135e36` (v0.42.0). Server line numbers come from host `a82c852b`; P23 was written at
`d672b945`, so its line numbers have drifted — re-verify in place before citing.

Citation convention: south file names as in `2026-09-30-image-world.md`. Server file names are relative to
`gateway/src/modules/`: `audio.rs` is `inference/handler/audio.rs`; `tts.rs`, `tts_providers.rs` and `multipart.rs`
are in `inference/handler/audio/`; `elevenlabs.rs` and `south_binary.rs` are in `inference/handler/`; `upstream.rs`,
`south_adapter.rs`, `south_switch.rs` and `request_extras.rs` are in `inference/engine/`; `media.rs` is in
`inference/engine/token_counter/`; `pricing_state.rs` is in `catalog/`; `settlements.rs` is in `billing/repo/`;
`webhook_sender.rs` is in `tasks/`; `usage_types.rs` is in `crates/gateway-provider-protocol/src/`.

Predecessors:
`2026-09-30-image-world.md` (**this record's premise**: its §6 media vocabulary — request view, elision rules, request
descriptor, closed transforms, response view and `response_body_form` — and its §11 safe fetch executor are reused
unchanged; this record only writes the differences on the speech surface), `2026-09-09-multipart-request-body.md`
(0.25.0), `2026-09-09-buffered-binary-response.md` (0.26.0), `2026-09-18-task-adapter-world.md`,
`2026-09-27-task-contract-v6-facts.md`, `2026-09-30-host-zero-vendor-boundary.md` (its §3 credential recipe for
Vertex, §4.2 descriptor auth admission, §6.3 host-computed bounds and undetectable zone, §7.4 dialect words, §10
instance declarations).

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
  second-hop audio fetching and duration extraction are all written in the host (about 40 per-provider functions in
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

**Auth arms**: `bearer`, `header_secret`. The two header names the speech surface uses (`xi-api-key`,
`ocp-apim-subscription-key`) are in today's compiled-in set; once the boundary record §10 lands, secret header names
are declared by the package instead, and this world needs no list of its own. Vertex needs no new arm: the component
declares the service-account credential recipe in its manifest, the host executes it and presents the minted slot as
`bearer` (boundary record §3.3; phase B4 there, which unlocks P21 S3 and P23 Vertex TTS). This world does not admit
the `oauth` arm, which the boundary record proposes to deprecate (§3.8 there). The auth form is declared by the
manifest and executed by the host as declared (descriptor auth admission, boundary record §4.2, applied to the media
descriptor of the image record §6.3); **the host must no longer decide by provider type whether a provider may go
through a component** (the `south_adapter.rs:198` branch is deleted with the migration).

## 4. D2 — Operation words and the root fix for "transcribes but does not translate" (F3)

Manifest capability words: `synthesize`, `transcribe`, `translate`, `artifact_fetch`.

- At least one of `synthesize` or `transcribe` must be declared; `translate` requires `transcribe` to be declared as
  well.
- `artifact_fetch` is optional and means the same as in the task world: the component may produce URL artifacts that
  the host has to fetch (precedent: `manifest.rs:109-121`), so the host knows at load time whether to wire up the
  second hop.

**Per-model operations come from dialect words, not from a catalog inside the component.** A component cannot know
which of an operator's OpenAI-compatible models can translate or return a duration-bearing format (among
OpenAI-compatible upstreams only some models do either). The boundary record already rules that model catalogs are
data outside the package (§7.5 there; lv's Q7 ruling) and that per-model request differences are dialect words
declared on the model row and interpreted by the component (§7.4 there). So this world defines speech dialect words
that the catalog or the operator sets in `ProviderConfig.models[].supported_parameters`:

| Word | Meaning |
|---|---|
| `speech.translate` | the model supports translation to English |
| `speech.verbose_json` | the model can return a transcription format that carries the audio duration (§9) |

`model-capabilities` returns, per model, the operations the component will perform: the intersection of what the
dialect can do and the words on the row. The component does not compile a model list into its wasm (the boundary
record §15 rejects that).

Host obligation: if the requested operation is not declared by both **the component** and **the model** (as returned
by `model-capabilities`), 400 before admission — no `prepare` call, no funds action. This is the general form of the
P23-F3 fix (`audio.rs:84-86`, `:177-181`, written today as "only the OpenAI-compatible arm supports translation"): the
host no longer knows who can translate.

The conformance suite cannot verify "is the translation output English" — that is semantics, not shape. P23 V1's
"verify with non-English audio that the output is English" stays in host acceptance (§11), and must not be replaced by
agreement with old results: the old results are themselves wrong on the ElevenLabs, xAI and Azure arms.

## 5. D3 — Request descriptors: JSON / SSML / multipart

The image record §6.3's `MediaRequestDescriptorV1` is reused; the speech surface uses three body kinds, plus one gap in
the transport contract. All three body kinds are part of `contracts.media` v1 (§14): the `text` kind is released with
the image world's first minor, not added later.

| Body | Used by | Existing transport shape |
|---|---|---|
| `json {template}` | every TTS arm except Azure | `JsonPostRequestV1`, binary responses via `execute_binary_call_v1` |
| `text {media_type, text}`, `media_type` closed to `application/ssml+xml` | Azure TTS | **none**: south has only JSON POST, GET and multipart POST |
| `multipart {parts}` | the four ASR arms | `MultipartPostRequestV1`, sent with `execute_multipart_binary_call_v1`, the multipart binary twin that HTTP contract 10 adds (image record §6.3a) |

Every call in this world reads its response as bytes (image record §6.3a rule 1): the host builds the response view
itself (§6) and decodes UTF-8 / JSON outside the sandbox, so the UTF-8 entry points are not used even for ASR, whose
answer is a text document. A transcript between the UTF-8 limit (32 MiB) and the binary limit (64 MiB) is thereby
delivered instead of refused after dispatch.

**D3a — SSML text body**. Two options:
- **A — add a transport shape** (recommended): the HTTP contract adds `TextPostRequestV1` (a bounded UTF-8 body, media
  type from a closed set, the contract sets `content-type` from that media type and the host must not add its own —
  the same rule as multipart's D3), together with an execution entry point for binary responses. It is an additive
  change, like the 0.24.0 / 0.25.0 / 0.26.0 precedents.
- **B — the host sends it itself**: the component still describes the SSML, and the host sends it without going
  through south-core. The contract is untouched, but the host writes a separate sending path for one body kind, and
  endpoint binding, header rules and auth assembly all have to be done again.

**D3b — the `output_format` query parameter**. ElevenLabs TTS must carry `?output_format=mp3_44100_128`
(`tts.rs:616-618`). Recommended: the package declares `output_format` as a `query_parameters` entry per the boundary
record §10 (a value grammar from that record's closed set, e.g. `token`), which follows lv's Q1 ruling there (a new
query name must not require re-pinning the host) and needs no HTTP contract change. **Stop-gap only**, if the ElevenLabs
cutover (V3-1) must happen before §10 lands: add `OutputFormat` to `QueryParameterV1` (wire name `output_format`,
grammar `[a-z0-9_]{1,32}`), to be subsumed by the §10 declaration afterwards. The ElevenLabs arm is ordered last in
V3-1 so that the stop-gap is needed only if §10 slips.

**SSML escaping lives in the component**: the Azure arm today assembles `<speak …><voice
name='…'>{text}</voice></speak>` in the host and does the XML escaping (`tts_providers.rs:163-231`: `xml_escape`,
`build_mai_ssml`, `voice_locale`). The component can read the input text (it is far below the image record §6.2's 1 MiB
fallback threshold), so escaping moves with the component. Both contexts are client-controlled and both must be escaped:
the text content and the attribute values (the voice name and the `xml:lang` derived from it come from the client's
`voice`). The conformance suite must include fixtures containing `<`, `&`, `'` and `"` in the input **and** in the
voice.

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

- MiniMax returns audio as hex (`tts_providers.rs:1108-1149`), doubling its size: a 10-minute 128 kbps mp3 is about
  9.6 MB, about 19.2 MB as hex — already past 16 MiB;
- Vertex returns base64 PCM (24 kHz, mono, 16-bit, `tts_providers.rs:862`): 48 KB per second, 14.4 MB for 5 minutes,
  about 19.2 MB as base64;
- OpenAI's token-priced SSE path splits the audio into many base64 deltas (`tts_providers.rs:7-67`), with a total of
  the same order of magnitude.

Reading these into the sandbox and emitting them again means either relaxing the limits (memory doubles with the
number of concurrent instances) or failing at random around the critical length. The component does not need to
**read** the audio here; it only needs to say **where** the audio is and **which public standard encodes it**.

**Artifact form `SpeechArtifactV1`** (TTS):

| Form | Description | Used by |
|---|---|---|
| `body {media_type}` | the whole response body is the audio (`binary` body form) | OpenAI-compatible (non-token), xAI, Groq, ElevenLabs, Azure |
| `inline {pointer, encoding, media_type}`, `encoding` is `base64` / `hex` | a string (already elided) in the response JSON | MiMo (base64), MiniMax (hex), Vertex (base64) |
| `segments {items: [{pointer, encoding}], media_type}` | several fragments, each decoded on its own, then the decoded bytes concatenated in order | OpenAI token-priced SSE (`sse` body form) |
| `url {pointer, media_type}` | a JSON Pointer into the **unelided** upstream response view, landing on a string that is an absolute https URL (≤ 8 KiB), fetched by the host through the safe fetch executor — the same form as the image record §10.1 | Bailian Qwen-TTS |

Any form may additionally carry `container: {"wav_pcm_s16le": {"sample_rate": …, "channels": …}}`, and the host adds a
44-byte header per the RIFF specification (today `tts_providers.rs:940-966`: 24000 Hz / mono / 16-bit). The transform
words `from_hex`, `concat` and `wav_pcm_s16le` (parameter names `sample_rate`, `channels`) belong to the closed table
of `contracts.media` v1 (image record §6.4), released together with the image words. `media_type` is declared by the
component (today it is scattered across the arms: Azure per its format table, Vertex `audio/wav` or
`audio/L16; rate=24000`, Bailian always `audio/wav`).

**The SSE response body form**: the component declares `response_body_form: sse` in `prepare`, and the host decodes
the buffered SSE body into an event list `[{event, data: <elided JSON view>}]` for the component. This is the media
vocabulary's `response_body_form` of the image record §6.5, extended with `sse`; it is not the provider world's
`stream_framing` (boundary record §5.2), which declines `sse` because provider-world components receive raw chunks and
split SSE themselves; here the body stays out of the sandbox, so the host does the splitting. Two hosts must produce
the same list, or one of them sees the usage event and the other does not. So:

- South supplies the decoder as a pure function, `decode_sse_v1(bytes) -> Result<Vec<SseEventV1>, SseErrorV1>`, in
  `south-contracts` (the SSE sibling of the eventstream deframer, boundary record §5.2; new grammars live there under a
  fuzz obligation, `south-core/src/raw.rs:11-12`), with golden
  vectors. The rules follow the WHATWG event-stream format: LF, CR and CRLF all end a line; a leading BOM is dropped;
  comment lines are dropped; several `data:` lines join with LF; an event without `event:` is named `message`;
  `id:` and `retry:` are ignored. One deliberate departure: a final event not followed by a blank line is **still
  dispatched** — the standard drops it because a live stream might continue, but a buffered body has a definite end,
  and dropping it would lose a terminal usage event on an upstream that omits the last blank line. A `data` that is
  not JSON is carried as a string.
- Elision (image record §6.2) runs over the event list as one JSON document whose root is the array: pointers take
  the form `/*/data/delta` (the `*` segment matches any event index). The fallback threshold does not catch small
  deltas, so a component that reads SSE audio **must** declare the audio paths; the conformance row
  `reference_integrity` fails a component whose `segments` pointers land on a non-elided string.

**ASR delivery** is a document, not bytes: `delivery: {"json": <template>} | {"text": "…", "media_type"} |
{"passthrough": {"media_type"}}`. The component renders the format the client asked for itself (§9) — today Azure is
rendered by the host as json / verbose_json / text, with srt / vtt refused (`multipart.rs:356-402`), and ElevenLabs is
rewritten down to just `{"text"}` (`multipart.rs:456-476`); all of that moves with the component. A `json` template
may contain `{"$south.ref": {"blob": "<id>", "transform": "as_is"}}` nodes for strings the host elided from the
response view (a transcript `text` above the 1 MiB threshold), and `passthrough` returns the upstream body unchanged
when the component needs no change to it; either way a large transcript never has to pass through the sandbox.

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

    // ProviderConfig -> list<SpeechModelCapabilitiesV1>: operations (from dialect words), voices/formats,
    // metering forms.
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

One function fewer than the image world — no `render`: speech has no multi-round aggregation, ASR's client format can be
rendered at parse time, and no host clock is needed. Second-hop audio needs no second function: the fetched bytes are
delivered as-is with the media type the component declared. `SpeechOutcomeV1` uses the image record §9.2's four outcomes
(`succeeded`, `rejected`, `charged_failure`, `unknown`); the boundary record's rule that `rejected` releases the
reservation after the dual run (§6.4 there; lv's ruling on image record Q7) applies here unchanged. MiniMax T2A errors
arrive as HTTP 200 + `base_resp.status_code` (`tts_providers.rs:1108-1149`), so the outcome decision must be made in the
component.

**`PreparedSpeechCallV1`** carries: the request descriptor; the pre-dispatch facts (§8); the `response_body_form` and
elision paths; `state` (≤ 8 KiB); and the immutability declaration for request extras — JSON bodies use
`immutable_body_paths` (task contract 6 §4 semantics, replacing `tts_body_rules`, `tts.rs:12-69`), SSML bodies give
`null` (the host must not inject — today's Azure `body_unsupported`), and multipart bodies use
`immutable_form_fields` (a new list of form-field names with the same `null` / `[]` / non-empty semantics, replacing
ASR's `owned_form_fields`, `audio.rs:71-78`). If P25 chooses branch A for the native ElevenLabs route, it can read the
same declaration directly.

## 8. D6 — Pre-dispatch facts

| Fact | Form | Which host provider logic it replaces |
|---|---|---|
| `operation` | `synthesize` / `transcribe` / `translate` | `supports_translation` (`audio.rs:84-86`) |
| `characters` | `{count, upper?}`: `count` is the number of Unicode scalar values in the northbound `input`; `upper` is a tighter bound under the upstream's counting rule (non-ASCII counting as 2 and the like) | `weighted_upper_chars` and the `bailian_tts \|\| minimax_audio_format.is_some()` branch (`tts.rs:268-271`, `:760-777`) |
| `metering_forms` | the forms that must be reported on success: `characters` / `seconds` / `tokens` | each arm deciding for itself whether to read upstream usage |
| `reservation` | `max_output_tokens?`, `max_seconds?` — both may only tighten the host's bounds | the output bound for token-priced TTS |
| `output_media_type` | the delivered media type (TTS) | each arm's format table |

An unsupported format, voice or language makes `prepare` return `invalid_request` directly, still before admission
(today these 400s all happen before admission, in the format tables of `tts_providers.rs`). `tts_max_input_chars`
(`tts.rs:279-286`) is catalog data and stays in the host.

**Bounds are the host's (boundary record §6.3).** This is the speech world's instance of that rule. Every bound the
host checks is computed by the host from the northbound request or its own configuration, with no provider
knowledge; the speech surface has no media input part, so no per-media-part allowance applies (an ASR upload is
bounded in seconds, not bytes). A component value may only lower what is **reserved**: the reservation uses
`min(host bound, component value)`. **Every check compares against the host bound only**, never against the
component's value, so a component number is never both the reservation and the thing it is checked against. The
host bounds on the speech surface:

| Quantity | Host bound (from the northbound request or host configuration) | Component may tighten with |
|---|---|---|
| characters billed | the UTF-8 byte length of `input` (every non-ASCII scalar is at least 2 bytes, so this is at or above any "non-ASCII counts as 2" rule) | `characters.upper` |
| input tokens (token-priced TTS) | the host's own scalar count of `input` (a token is at least one character) | — |
| output tokens (token-priced TTS) | `default_max_output_tokens` (host configuration, today 4096) | `reservation.max_output_tokens` |
| ASR seconds | `asr_max_billable_seconds` (`audio.rs:335-336`) | `reservation.max_seconds` |

**The three reservation algorithms keep today's amounts (C4.2a / C4.2b / C4.3, required by P23 §5)**; only the branch
conditions become generic facts:
- Per character: `calculate_tts_cost(min(bytes(input), characters.upper ?? characters.count))`. A component whose
  upstream bills by scalar count gives `upper = count`, and one that applies a weighting rule gives the weighted count,
  so the reserved amount equals today's for every arm (Bailian and MiniMax weighted, the rest by count);
- Per token: input bounded by the host's scalar count, output by `min(default_max_output_tokens,
  reservation.max_output_tokens)`;
- ASR: `min(asr_max_billable_seconds, reservation.max_seconds)`.

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
| `seconds {value, basis}` | ASR audio duration; `basis = reported` when the upstream states the duration, `basis = last_timestamp` when it is the end time of the last timestamped unit (a lower bound: trailing silence is not counted) | `duration` (OpenAI-compatible, xAI, `audio.rs:514-519`), `durationMilliseconds` (Azure, `multipart.rs:418-446`) — both `reported`; `words[-1].end` (ElevenLabs, `multipart.rs:456-476`) — `last_timestamp` |
| `tokens {input, output, input_text?, input_audio?}` | upstream token counts | OpenAI TTS SSE `speech.audio.done.usage` (`tts_providers.rs:37-48`); ASR token counts as side information (`SecondsWithTokens`, `usage_types.rs:129-221`) |

All of these units are already in ARCHITECTURE.md's admission list ("tokens, seconds, images, characters, milliunits
an upstream reported", `ARCHITECTURE.md:110`). P23 §1.5's statement that "no seconds-based usage unit can be found in
the south contract" is **inaccurate**: the task world already has `TaskUsageFactsV2.seconds` (`task_v2.rs:100-105`).
What is missing is characters, and a set of fact types belonging to the speech world.

**Handling missing evidence** (the division DV3 settled):

- `tokens` declared in `metering_forms` but no `usage` in the SSE, **or `usage` missing either count** → outcome
  `unknown`, **not 0**. Before 2026-09-30 the host took 0 for each missing count, so token-priced rows settled at $0 or
  without the (expensive) output tokens; the host fixed both on 2026-09-30 / 2026-10-01 (P23-F6, completed in
  `a82c852b`: both counts must be present and not both zero, `tts_providers.rs:37-48`). The component rule matches the
  host as it now stands, so the dual run pins the fixed behaviour.
- `seconds` missing: see the ASR duration rule below. The component returns `seconds: null` only in the cases listed
  there; the host then applies the generic 60-second fallback and marks `quantity_estimated = 1` (`audio.rs:535-540`,
  DV3's interim scheme). If the dialect **always reports** a duration and did not this time (Azure), the outcome is
  `unknown`, consistent with today's Azure 502 (`audio.rs:489-506`). Each component's choice is pinned by conformance
  fixtures.
- `characters.source = upstream` with no number: fall back to `source = request`, consistent with today's "override
  the local count only when the upstream reports one".

**ASR duration: the root fix is in the component** (DV3; lv's ruling of 2026-10-01). The component asks the upstream
for the richest format that carries a duration and renders the client's format itself from that response:

- **OpenAI-compatible and xAI** (both accept `response_format`; the host comment `audio.rs:511-514` notes that only
  `verbose_json` carries `duration`): on a model whose row declares `speech.verbose_json` (§4), the component always
  asks for `verbose_json` and renders what the client asked for — `json` (`{"text"}`), `text`, `verbose_json`
  (pass-through), `srt` and `vtt` (from the `segments` start / end / text, which are public formats). The client's
  format choice no longer changes what is billed. If the upstream `json` carries its own usage seconds (whisper's
  `usage.type = "duration"` as OpenAI documents it, **to be measured**), that counts as `reported` too.
- **ElevenLabs**: the component guarantees that word-level timestamps are requested (`timestamps_granularity` is
  already a field the host treats as owned, `audio.rs:71-78`) and reports the end of the last word as
  `basis = last_timestamp`. If the upstream response carries an explicit duration field (**to be measured**), that is
  used instead as `reported`. The component also renders the client's format from the words (json stays `{"text"}`,
  equal to today; text / srt / vtt are new, §13).
- **Azure fast transcription**: always reports `durationMilliseconds`; missing → `unknown` (Q6). srt / vtt stay
  refused in `prepare`, as today (`multipart.rs:356-402`).

`seconds: null` remains only where **no** format of that upstream can carry a duration:
1. an OpenAI-compatible or xAI model whose row does not declare `speech.verbose_json` (some models accept only `json`
   / `text`, **to be measured per model**) and whose response carries no usage seconds;
2. an ElevenLabs response with no timestamped word (e.g. audio without speech) and no explicit duration field.

In these cases the ruling on Q4 applies: the 60-second estimate, visibly labeled in the ledger. The difference from
today is that the case is chosen by the model row, not by the client's `response_format`: a client can no longer turn
an hour of audio into a 60-second charge by asking for `srt` or `vtt`. Measuring the duration from the audio file
itself in the host was considered and not adopted: it would need a container parser per audio format (mp3, m4a, ogg,
webm, flac, wav), which is far more than a closed public-standard transform, for a residual case.

**Host generic checks** (dialect-independent; any violation is routed to manual review, with the funds outcome of the
boundary record §6.3; every bound here is the **host** bound of §8, not the reservation):
1. When `characters.source = request`, `value` must **equal** the host's own count of the northbound `input` — the
   northbound `input` is an OpenAI speech protocol field and part of the product surface, so counting it is not
   provider logic;
2. When `characters.source = upstream`, `value ≤` the host characters bound (the UTF-8 byte length of `input`);
3. `seconds ≤ asr_max_billable_seconds` (today there is no clamp at all; review is triggered only when the amount
   goes out of bounds at settlement, see the comment at `audio.rs:579-582`);
4. `tokens.input ≤` the host's scalar count and `tokens.output ≤ default_max_output_tokens`; when `input_text` /
   `input_audio` appear, their sum equals `input`;
5. Settlement ≤ reservation (existing). This is the one comparison with the tightened number, and it is the existing
   funds guard, not a check of the component: a settlement above a tightened reservation but within the host bound
   goes to manual review, as it does today when upstream-reported characters exceed the weighted reservation.

These out-of-bound and internal-consistency checks can only find values outside the bounds, not under-reporting
within them (accepted by DP1; the same undetectable zone as the boundary record §6.3).

**Labels.** `quantity_estimated = 1` when the host applied the 60-second fallback. Whether `basis = last_timestamp`
should also set it is open (Q12): today the ElevenLabs duration is recorded as a measurement, while the same method
(the end of the last timestamp) was judged an undercounting estimate when Q4 weighed it for subtitles.

## 10. D8 — Second-hop audio (DV2) and verify-before-settle

Bailian Qwen-TTS returns `output.audio.url`, and the host fetches it on a second hop (`tts.rs:977-1000`,
`tts_providers.rs:744-791`). After migration the component only produces a `url {pointer: "/output/audio/url",
media_type: "audio/wav"}` artifact and declares `artifact_fetch` in its manifest; execution falls entirely on the image
record §11's **single** safe fetch executor, with not one rule changed: https only, refuse userinfo, resolve DNS and pin
the address, refuse if any address falls in a forbidden range, no redirects, system proxy disabled, no credential
headers, a total timeout and a byte limit, an empty body is a failure. The fixes for P23-F4 and server E-1
(`guarded_asset_client`, `webhook_sender.rs:491-513`) are the current implementation of exactly these rules.

**Delivery order: verify-then-settle, for every artifact form.** Before settlement the host must have the final bytes
and have verified them:
- `url`: the fetch succeeded, non-empty and within the limit;
- `inline` / `segments`: every referenced string decodes under its declared encoding and the result is non-empty;
- `container`: the header was added to a non-empty PCM body whose length is a multiple of `2 × channels`;
- `body`: non-empty.

Any failure → `delivery_unknown`, no settlement. Today the host already decodes every form inside its sealed block
before the single `finalize` (`tts.rs:918-1100`, `:1164`), the same order as the image record §10.2's DI6 = A; no
ledger state needs to change.

## 11. D9 — Conformance suite `south.speech-component.v1`

Fixture families: `capabilities`, `prepare`, `response`. Carried over: coverage, fixture equality, determinism,
unknown-field tolerance, `terminal_only_from_the_wire`, `endpoint_confinement`, `reference_integrity` (defined in the
image record §12.1); plus required rows for the speech surface:

| Check | Requirement |
|---|---|
| `metering_sample` | every declared form (`characters` counted separately for each of its two sources, `seconds` for each `basis` the component uses, `tokens`) has at least one `succeeded` fixture giving the exact value |
| `missing_meter_is_not_zero` | a component declaring `tokens` has, **for each count it reads**, at least one 2xx fixture missing that count (and one missing `usage` entirely), each with outcome `unknown` |
| `absent_duration_is_null_or_unknown` | a component declaring `seconds` has at least one fixture missing the duration, whose result can only be `seconds: null` or `unknown` — never 0 or any fallback number |
| `client_format_rendering` | a component that declares `speech.verbose_json` handling has, from one duration-bearing upstream response, fixtures rendering every client format it accepts (json, text, verbose_json, srt, vtt), each with the same `seconds` |
| `undeclared_operation_refused` | for a model that does not declare `translate`, `prepare(translate)` returns an error and produces no descriptor |
| `ssml_escaping` (components that emit SSML) | with `<`, `&`, `'`, `"` in the input and in the voice, the SSML is well-formed, the text content is unchanged, and the attribute values are unchanged |
| `second_hop_artifact` (components that declare `artifact_fetch`) | at least one `url` artifact fixture whose pointer lands on a string that passes the `ArtifactUrlV1` grammar |
| `empty_audio_is_unknown` | 2xx but no audio (empty body, empty `data.audio`, no deltas) → `unknown` |

The metering samples are transcribed from the server's native arms (the P13 S8 pattern); they are exactly the "usage
samples" P21 DP1 requires.

**Host obligations** (gate ③): all of the image record §12.3 applies, plus: the host's SSE decoding equals
`decode_sse_v1` on south's golden vectors (in practice: the host calls it); the WAV container encoding is byte-for-byte
equal to the vectors south provides; the reservation is the minimum of the host bound and the component value, and
every check of §9 uses the host bound alone (fixtures: a component declaring `characters.upper` or `max_output_tokens`
above the host bound is reserved at the host bound; one declaring them below it is reserved at its value and still
checked against the host bound); undeclared operations are refused before admission; the estimate mark is set
when `seconds: null`; verify-then-settle (§10) for each artifact form; the translation capability is accepted with
non-English audio (semantics, not in the component suite).

## 12. Mapping of the existing execution arms

**TTS** (today's selection → after migration, always selected by the model row's component route; the host no longer
looks at row names or model names):

| Execution arm | Upstream request | Auth | Artifact / transform | Metering | Batch |
|---|---|---|---|---|---|
| OpenAI-compatible | JSON `/v1/audio/speech`, only `model` changed; `stream_format: "sse"` added when token evidence is needed | `bearer` | `body`; `segments` (base64) for SSE | `characters(request)` or `tokens` | V3-1 |
| xAI | JSON `/v1/tts`, `{text, voice_id, language, output_format?}`; format table (`tts_providers.rs:266-306`) | `bearer` | `body` | `characters(request)` | V3-1 |
| Groq Orpheus | JSON `/v1/audio/speech`, voices mapped to personas (`tts_providers.rs:334-368`) | `bearer` | `body` | `characters(request)` | V3-1 |
| ElevenLabs | JSON `/v1/text-to-speech/{voice_id}` + `output_format` query (D3b) | `header_secret` `xi-api-key` | `body` | `characters(request)` | V3-1 (last; depends on D3b) |
| MiMo | JSON `/v1/chat/completions` + `audio` object | `bearer` | `inline base64` (`choices[0].message.audio.data`) | `characters(request)` | V3-2 |
| MiniMax T2A | JSON `/v1/t2a_v2`, `GroupId` query (already sanctioned) | `bearer` | `inline hex` (`data.audio`) | `characters(upstream)`, weighted `upper` | V3-2 |
| Bailian Qwen-TTS | JSON multimodal-generation | `bearer` | `url` + second hop (§10) | `characters(upstream)`, weighted `upper` | V4 |
| Vertex Gemini TTS | JSON `:generateContent`, AUDIO modality; project / region in the URL, taken from an exported credential attribute or non-secret config (boundary record §3.3, §7.3) | `bearer`; the slot is `minted` by the service-account credential recipe (boundary record §3.3) | `inline base64` + `wav_pcm_s16le{sample_rate: 24000, channels: 1}`, or raw PCM | `characters(request)` | V4 |
| Azure Speech | **SSML** (D3a) + `X-Microsoft-OutputFormat` header | `header_secret` `ocp-apim-subscription-key` | `body`; empty 2xx → `unknown` | `characters(request)` | V4 (depends on D3a) |

**ASR**:

| Execution arm | Upstream request | Auth | Delivery | Duration source | Batch |
|---|---|---|---|---|---|
| OpenAI-compatible | multipart part list, each part `as_is`, only `model` changed; `response_format=verbose_json` when the row declares `speech.verbose_json` (§9) | `bearer` | the component renders json / text / verbose_json / srt / vtt | `duration` or usage seconds (`reported`); otherwise `null` (§9 case 1) | V3-3 |
| xAI | multipart `/v1/stt`, `model` removed; `response_format=verbose_json` as above | `bearer` | the component renders the client format | `duration` (`reported`); otherwise `null` (§9 case 1) | V3-3 |
| ElevenLabs | multipart `/v1/speech-to-text`, `model → model_id`, `language → language_code`, word timestamps requested | `header_secret` `xi-api-key` | json `{"text"}`; text / srt / vtt rendered from words | `words[-1].end` (`last_timestamp`), or an explicit duration if measured; otherwise `null` (§9 case 2) | V3-3 |
| Azure fast transcription | multipart `file → audio` + `definition` part; `api-version` query (already sanctioned) | `header_secret` `ocp-apim-subscription-key` | json / verbose_json / text; srt and vtt refused in `prepare` | `durationMilliseconds` (`reported`); missing → `unknown` | V3-3 |

Vertex today silently downgrades mp3 / opus / aac / flac to wav (`tts_providers.rs:904-921`): the client asks for mp3,
gets wav, status 200. For dual-run agreement the component copies this; after the dual run it refuses the
unsupported formats in `prepare` instead (ruled by lv on 2026-09-30, Q7).

## 13. Migration order and dual-run acceptance

**Prerequisite (V0, host)**: done as of 2026-10-01. The $0 settlement found in this record — §9's "SSE missing
`usage` counts as 0" — is fixed (P23-F6), including a `usage` that carries only one of the two counts (`a82c852b`);
§8's "a non-compatible arm configured with only a token price" turned out on verification to be refused at admission,
not settled at $0, and is left to this migration (P23-F7). The other V0 items (the F1 interim scheme, F3, F4,
handler-level cases) have landed.

**Order** (P23 V3–V4; on the south side, in the same batch as the image world's first minor or right after it — the
`contracts.media` v1 vocabulary is complete in the image minor either way, §14):
1. **V3-1, the binary-returning arms**: xAI, Groq, OpenAI-compatible (including the SSE path), ElevenLabs last (it
   needs `output_format`, D3b);
2. **V3-2, JSON-wrapped audio**: MiMo, MiniMax;
3. **V3-3, the four ASR arms**: need the host part parser and encoder (image record §6.1, §6.7) and HTTP contract
   10's multipart binary twin (§5, §14);
4. **V4**: Bailian (second hop), Vertex (credential recipe v1 — boundary record §3, phase B4 there, i.e. P21 S3 — plus
   the WAV container), Azure SSML (D3a).

**Dual-run reconciliation** (per arm, both billing forms):
- Upstream request: JSON body equal by value; SSML equal as strings; multipart equal as decoded part lists; request
  headers equal;
- Client response: bytes and `Content-Type` equal **byte for byte** (audio); ASR document JSON equal by value, text
  equal as strings;
- Reservation bound, frozen usage kinds and values, and amounts equal;
- **Not accepted by dual run**: translation (the old results are wrong on three arms, §4); ASR duration and client
  formats — once the component asks for `verbose_json` per §9, the upstream request body **necessarily differs** from
  the old path, and srt / vtt are rendered by the component instead of passed through. Acceptance becomes: the
  duration comes from the upstream's report with `quantity_estimated = 0`; json and text responses equal by value;
  srt / vtt equal by cue (index, start, end, text), not by bytes; ElevenLabs text / srt / vtt are new behaviour and
  are accepted against fixtures, not against the old path;
- In addition, run J2 end to end with a synthetic provider: a speech component the host has never seen, which the
  host can use to synthesize and transcribe with zero changes (the speech-world counterpart of the boundary record's
  unseen-provider guest (T21), §12 there).

## 14. Versioning

A South **minor**, sharing `contracts.media` with the image world:

- A new WIT `crates/south-provider-api/wit/speech-adapter.wit`; `manifest.rs` gains `SPEECH_*` constants and
  `SPEECH_WORLD_SCHEMA` (capability words in §4; `validate_role` adds "at least one synthesis or transcription word;
  `translate` implies `transcribe`"); `KNOWN_WORLDS` gains a row.
- The runtime gains one `bindgen!` module and one `InstanceKind` variant; the limits are unchanged.
- `south-contracts`: a `speech` module (facts, outcomes, artifacts, `immutable_form_fields`, the speech dialect words
  of §4). **`contracts.media` v1 is released once, complete**: besides the image words it already carries `from_hex`,
  `concat`, `wav_pcm_s16le{sample_rate, channels}`, the `sse` response body form, the `text` request body kind and
  `decode_sse_v1` with its golden vectors. The speech minor therefore adds nothing to `contracts.media`, and speech
  packages declare `media: 1` whether they ship in the image minor or after it.
- **HTTP contract 10 is one bump, released in the image world's minor**, carrying both
  `execute_multipart_binary_call_v1` (image record §6.3a, Q14 there; the ASR arms need it, §5) and this record's D3a
  `TextPostRequestV1` with its binary execution entry point (Q1). This record commits D3a to that bump rather than a
  later one, so the speech minor itself changes no transport contract. D3b needs no contract change when the boundary
  record §10 declaration is used; if the stop-gap `QueryParameterV1::OutputFormat` is needed, it joins the same bump.
  Matching new provider-suite rows; whether they form their own suite or join an existing one, following the
  0.25.0 / 0.26.0 precedents, is for the maintainers to decide.
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
- **Component-supplied bounds used as they are** (this record's first draft): a component could raise the ceiling it
  is checked against; replaced by host bounds that a component may only tighten (§8).
- **Passing the client's srt / vtt request through to the upstream** (this record's first draft): the upstream's
  subtitle formats carry no duration, so every such request fell to the 60-second estimate, chosen by the client;
  replaced by the component rendering subtitles from `verbose_json` (§9).
- **The host measures ASR duration from the audio file**: needs a parser per container format; not adopted (§9).
- **An `oauth` auth arm for Vertex, added in a later minor** (this record's earlier draft): superseded by the boundary
  record §3 — the component declares a credential recipe, the host executes it and presents the minted slot as
  `bearer`. The boundary record proposes deprecating the `oauth` arm (§3.8 there), so this world never admits it.
- **Client-side streaming TTS** (DV4): not included. Adding a streaming export later is a new version of
  `speech-adapter`; if by then a single media world has been formed per the image record §16, that upgrade would drag
  along every image component — a point Q8 has to weigh as well.

## 16. Open questions

| # | Question | Recommendation | Decided by |
|---|---|---|---|
| Q1 | SSML: add `TextPostRequestV1` (in the image minor's single HTTP contract 10 bump, §14), or the host sends it itself (D3a) | Add it, in that bump | south maintainers |
| Q2 | `output_format` (D3b): declared by the package per the boundary record §10, or added to `QueryParameterV1` | The §10 declaration; the closed-set addition only as a stop-gap if V3-1's ElevenLabs cutover precedes §10 | south maintainers |
| Q3 | Decoding and container wrapping executed by host closed transforms, rather than P23 V1's "pure functions inside the component" (§6) | The host executes them | lv — **ruled by lv, 2026-09-30: as recommended** (P23 V1's text is amended accordingly); south maintainers (transform table, released complete in `contracts.media` v1, §14) — open |
| Q4 | When the ASR client asks for srt / vtt: `seconds: null` with estimation, or the end time of the last subtitle (which slightly undercounts silence) | Estimate, visibly in the ledger | lv — **ruled by lv, 2026-09-30: as recommended**. **Superseded in scope by lv's ruling of 2026-10-01**: the component asks for a duration-bearing format and renders srt / vtt itself (§9), so srt / vtt no longer lead to `null`; the 2026-09-30 ruling (estimate, visibly) still governs the residual `null` cases listed in §9 |
| Q5 | "Character count" defined as the number of Unicode scalar values in the northbound `input`; the host checks `source = request` for exact equality | As stated | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q6 | Azure missing duration: keep `unknown` (502 today), or fold it into the generic estimation | Keep `unknown` | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q7 | Vertex silently downgrading mp3 and others to wav: copy it, or refuse in `prepare` | Copy it during the dual run; refuse afterwards | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q8 | Two separate worlds for image and speech, or one synchronous media world (image record Q4) | Separate | lv — **ruled by lv, 2026-09-30: as recommended**; south maintainers — open |
| Q9 | A second consumer of the metering vocabulary: the community host today has no multipart surface and no byte-returning surface (`2026-09-09-multipart-request-body.md:189`, `2026-09-09-buffered-binary-response.md:302`), so per `ARCHITECTURE.md:114-115` the admission condition is not met at present. The same question is open as the image record Q5, the boundary record Q9 and the embeddings record E-Q5 | Take P21 §7's "synchronous implementation recommended" as the written commitment; otherwise do not admit for now | lv + south maintainers |
| Q10 | Rows on non-OpenAI-compatible arms configured with only a token price: today they can be listed but every request is refused at admission with a "no price configured" 400 (no funds move); after migration the refusal comes from `prepare` and names the cause (§8) | Accept; list the affected rows with a read-only query before cutting over | lv |
| Q11 | If P25 chooses A for the native ElevenLabs route, does it reuse this world's components and immutability declaration directly | Reuse | lv (P25) |
| Q12 | ASR duration with `basis = last_timestamp` (ElevenLabs): record it as a measurement (as today) or set `quantity_estimated = 1` (it undercounts trailing silence, the reason Q4 gave against the same method for subtitles) | Label it estimated; the amount is unchanged, only the ledger mark moves | lv |
| Q13 | South supplies `decode_sse_v1` (rules in §6) as a pure function with golden vectors in `south-contracts`, which both hosts call | Supply it; place it in `south-contracts` under the fuzz obligation | south maintainers |
| Q14 | The speech dialect words `speech.translate` and `speech.verbose_json` (§4): defined by this world's contract, or free-form words each component documents | Defined by the contract (closed, two words), so catalog data can set them without knowing the component | south maintainers |

## Revision note (2026-10-01)

- Header: host baseline moved to `a82c852b`; `tts_providers.rs` citations updated to it; the 2026-10-01 ruling noted.
- §3: header names follow the boundary record §10 once it lands; descriptor auth admission applies to the media
  descriptor.
- §4: per-model operations come from dialect words (`speech.translate`, `speech.verbose_json`) read by the component,
  not from a model list inside the component (new Q14).
- §5: the `text` body kind is part of `contracts.media` v1; D3b now recommends the boundary record §10 declaration
  with the closed-set addition as a stop-gap only; SSML escaping covers attribute values as well as text.
- §6: framing renamed `response_body_form`; `sample_rate` parameter name; `segments` decode each fragment then
  concatenate; SSE decoding specified and supplied by south as `decode_sse_v1` (new Q13); elision pointers over the
  event list defined; ASR delivery gains `passthrough` and blob references.
- §7: `rejected` follows the boundary record's rule (§6.4 there); `immutable_form_fields` defined.
- §8: bounds are host-computed from the northbound request, and a component value may only tighten them; reservation
  amounts unchanged.
- §9: a single missing token count is `unknown` (host fixed in `a82c852b`); ASR duration root fix in the component per
  lv's 2026-10-01 ruling, with subtitles rendered from `verbose_json` and the residual `null` cases listed; `seconds`
  gains `basis` (new Q12); checks rewritten against the host bounds.
- §10: verify-then-settle extended to every artifact form.
- §11: per-count missing-usage rows, a client-format rendering row, attribute escaping, and host obligations for SSE
  decoding and bound clamping.
- §12: ASR table rewritten for the duration fix; the ElevenLabs TTS arm is ordered last in V3-1.
- §13: V0 updated; subtitle acceptance by cue instead of bytes.
- §14: `contracts.media` v1 is released complete with the image minor; D3b needs no contract change on the §10 route.
- §15: three rejected alternatives added (component bounds as-is, subtitle pass-through, host-side duration parsing).
- Round 2 (2026-10-01), §3 / §15: `oauth` deprecation cited as the boundary record §3.8.
- Round 2, §5: every call reads its response as bytes; the ASR arms use the multipart binary twin
  `execute_multipart_binary_call_v1` (image record §6.3a).
- Round 2, §6 / §10 / §11: `url` artifacts are `url {pointer, media_type}`, a pointer into the upstream response.
- Round 2, §6 / §7: `decode_sse_v1` cited to the boundary record §5.2; `rejected` to its §6.4.
- Round 2, §8 / §9 / §11: reserve at the minimum; every check compares against the host bound only; the speech
  instance of the bound rule has no media-part allowance.
- Round 2, §13 / §14 / Q1: `TextPostRequestV1` committed to the single HTTP contract 10 bump in the image minor.
