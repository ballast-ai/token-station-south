# Speech World: TTS and ASR (speech-adapter-v1)

Status: **accepted 2026-10-08, with Q10 and Q11 open** (neither blocks the contract, §16; reconciled with south 0.50.0 in
§17; implementation outline in §18); proposed 2026-09-30 by the host team (token-station-server P21/P22/P23); steps S-S-1 to S-S-3
implemented and released in 0.64.0 without `contracts.speech` (S-S-4 packages and S-S-5 still to come)

Date: 2026-09-30

Revised: 2026-10-01 after independent review; 2026-10-08 rulings and 2026-10-09 reconciliation with 0.50.0 (see the
revision notes at the end).

Rulings: on 2026-09-30 the host owner (lv) ruled on Q3, Q4, Q5, Q6, Q7 and Q8 (§16), each as recommended. Q3 and Q8
also need the south maintainers. On 2026-10-01 lv ruled that the ASR missing-duration fix goes into the component
(§9; recorded under Q4), and that an ElevenLabs duration taken from the last timestamp is labeled estimated (Q12).
Q10 and Q11 depend on other host plans and stay open.
On 2026-10-08 lv, acting for the south maintainers, ruled as recommended on the south half of Q3 and Q8 and on Q1, Q2,
Q13 and Q14, and ruled Q9 the same way as image Q5 and embeddings E-Q5 (§16). Q10 and Q11 stay open; neither blocks
accepting this record, because neither changes anything the contract contains (§16 says what each one gates). No ruling
is reversed; §17 records one reconciliation finding for lv: the contract numbers in the Q1 ruling are stale (its
substance stands; the bump is 11 → 12).


Reconciliation baseline (2026-10-09): south `origin/main` = `6aa2813` = v0.50.0; §17 lists what moved since the
baseline below. Host plans were read at token-station-server `fbee0ed7`.

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
  vectors. *(Amended 2026-10-10: it lives in the host-only crate `south-host-grammars`, under the same fuzz
  obligation, and ships in 0.53.0 (Q-B6-6); no component runs it, and adding it to `south-contracts` would have
  re-identified every package. Boundary record §13.13.)* The rules follow the WHATWG event-stream format: LF, CR and CRLF all end a line; a leading BOM is dropped;
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
  `basis = last_timestamp`, which the ledger labels as estimated (Q12, ruled by lv on 2026-10-01; see **Labels**
  below). If the upstream response carries an explicit duration field (**to be measured**), that is used instead as
  `reported`. The component also renders the client's format from the words (json stays `{"text"}`,
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

**Labels.** `quantity_estimated = 1` when the host applied the 60-second fallback, and also when
`seconds.basis = last_timestamp` (Q12, ruled by lv on 2026-10-01): the end of the last timestamp undercounts trailing
silence, the reason Q4 gave against the same method for subtitles, so the ElevenLabs duration is no longer recorded
as a measurement. The amount is unchanged; only the ledger mark moves. `basis = reported` keeps
`quantity_estimated = 0`.

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
| ElevenLabs | multipart `/v1/speech-to-text`, `model → model_id`, `language → language_code`, word timestamps requested | `header_secret` `xi-api-key` | json `{"text"}`; text / srt / vtt rendered from words | `words[-1].end` (`last_timestamp`, labeled estimated per Q12), or an explicit duration if measured (`reported`); otherwise `null` (§9 case 2) | V3-3 |
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
- **Intentional difference** (on record, not a reconciliation failure): ElevenLabs ASR rows whose duration has
  `basis = last_timestamp` carry `quantity_estimated = 1` where the native path records 0; the duration and the
  amount are unchanged (Q12, ruled by lv on 2026-10-01, §9);
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
  `decode_sse_v1` with its golden vectors. *(Amended 2026-10-10: `decode_sse_v1` is not part of `contracts.media`; it
  ships in 0.53.0 from `south-host-grammars` (boundary record §13.13). Building the `sse` form's view still needs a
  `south-contracts` entry point that takes the events the host decoded, because `south-contracts` may not depend on
  that crate; the speech minor adds it.)* The speech minor therefore adds nothing to `contracts.media`, and speech
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
| Q1 | SSML: add `TextPostRequestV1` (in the image minor's single HTTP contract 10 bump, §14), or the host sends it itself (D3a) | Add it, in that bump | south maintainers — **ruled 2026-10-08** (below) |
| Q2 | `output_format` (D3b): declared by the package per the boundary record §10, or added to `QueryParameterV1` | The §10 declaration; the closed-set addition only as a stop-gap if V3-1's ElevenLabs cutover precedes §10 | south maintainers — **ruled 2026-10-08** (below) |
| Q3 | Decoding and container wrapping executed by host closed transforms, rather than P23 V1's "pure functions inside the component" (§6) | The host executes them | lv — **ruled by lv, 2026-09-30: as recommended** (P23 V1's text is amended accordingly); south maintainers (transform table, released complete in `contracts.media` v1, §14) — **ruled by lv for them, 2026-10-08: as recommended** (below) |
| Q4 | When the ASR client asks for srt / vtt: `seconds: null` with estimation, or the end time of the last subtitle (which slightly undercounts silence) | Estimate, visibly in the ledger | lv — **ruled by lv, 2026-09-30: as recommended**. **Superseded in scope by lv's ruling of 2026-10-01**: the component asks for a duration-bearing format and renders srt / vtt itself (§9), so srt / vtt no longer lead to `null`; the 2026-09-30 ruling (estimate, visibly) still governs the residual `null` cases listed in §9 |
| Q5 | "Character count" defined as the number of Unicode scalar values in the northbound `input`; the host checks `source = request` for exact equality | As stated | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q6 | Azure missing duration: keep `unknown` (502 today), or fold it into the generic estimation | Keep `unknown` | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q7 | Vertex silently downgrading mp3 and others to wav: copy it, or refuse in `prepare` | Copy it during the dual run; refuse afterwards | lv — **ruled by lv, 2026-09-30: as recommended** |
| Q8 | Two separate worlds for image and speech, or one synchronous media world (image record Q4) | Separate | lv — **ruled by lv, 2026-09-30: as recommended**; south maintainers — **ruled by lv for them, 2026-10-08: as recommended** (below) |
| Q9 | A second consumer of the metering vocabulary: the community host today has no multipart surface and no byte-returning surface (`2026-09-09-multipart-request-body.md:189`, `2026-09-09-buffered-binary-response.md:302`), so per `ARCHITECTURE.md:114-115` the admission condition is not met at present. The same question is open as the image record Q5, the boundary record Q9 and the embeddings record E-Q5 | Take P21 §7's "synchronous implementation recommended" as the written commitment; otherwise do not admit for now | lv + south maintainers — **ruled 2026-10-08** (below) |
| Q10 | Rows on non-OpenAI-compatible arms configured with only a token price: today they can be listed but every request is refused at admission with a "no price configured" 400 (no funds move); after migration the refusal comes from `prepare` and names the cause (§8) | Accept; list the affected rows with a read-only query before cutting over | lv — **open**; blocks nothing in the contract (below) |
| Q11 | If P25 chooses A for the native ElevenLabs route, does it reuse this world's components and immutability declaration directly | Reuse | lv (P25) — **open**; blocks nothing in the contract (below) |
| Q12 | ASR duration with `basis = last_timestamp` (ElevenLabs): record it as a measurement (as today) or set `quantity_estimated = 1` (it undercounts trailing silence, the reason Q4 gave against the same method for subtitles) | Label it estimated; the amount is unchanged, only the ledger mark moves | lv — **ruled by lv, 2026-10-01: as recommended** (label it estimated, `quantity_estimated = 1`; the amount is unchanged; §9) |
| Q13 | South supplies `decode_sse_v1` (rules in §6) as a pure function with golden vectors in `south-contracts`, which both hosts call | Supply it; place it in `south-contracts` under the fuzz obligation | south maintainers — **ruled 2026-10-08** (below) |
| Q14 | The speech dialect words `speech.translate` and `speech.verbose_json` (§4): defined by this world's contract, or free-form words each component documents | Defined by the contract (closed, two words), so catalog data can set them without knowing the component | south maintainers — **ruled 2026-10-08** (below) |

### Rulings of 2026-10-08

lv, the host owner, ruled on 2026-10-08 on the questions tagged for the south maintainers, acting for them, each as
recommended; the line after a ruling states the consequence. Q4 to Q7 and Q12 were ruled on 2026-09-30 and 2026-10-01
and are unchanged.

- **Q1** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. South adds `TextPostRequestV1`, with a
  binary execution entry point, in the single HTTP contract bump of the media minor. **The bump is 11 → 12, not 9 → 10**
  (§17.1; the contract numbers in the record were stale, the substance of the ruling stands).
- **Q2** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. `output_format` is a package-declared
  `query_parameters` entry per the boundary record §10; adding `OutputFormat` to `QueryParameterV1` remains only a
  stop-gap. The declaration route needs gate ① to admit `query_parameters` in this world (§17.3), which is a manifest
  change and not a contract change; with it, the stop-gap is not needed.
- **Q3** — Ruled, south half (lv for the south maintainers, 2026-10-08): as recommended. The transform table and
  container wrapping are executed by the host as closed transforms and released complete in `contracts.media` v1.
  Together with lv's ruling of 2026-09-30 both halves are decided.
- **Q8** — Ruled, south half (lv for the south maintainers, 2026-10-08): as recommended. Separate worlds for image and
  speech; with the ruling of 2026-09-30 both halves are decided.
- **Q9** — Ruled (lv, 2026-10-08), the same ruling as image Q5 and embeddings E-Q5 (it is the same question): the
  release record of the first media release states in writing that a synchronous implementation in the community host is
  recommended (P21 §7), and the media vocabulary capabilities are `not_verified` for the community host in
  `compatibility.json` until that host lands it.
- **Q13** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. South supplies `decode_sse_v1` with golden
  vectors in `south-contracts`, under the fuzz obligation; both hosts call it. It ships with the image minor (S-I-1).
  - Note (2026-10-10): lv ruled Q-B6-6 on 2026-10-09 (release it separately and early, so it left S-I-1) and on
    2026-10-10 placed it in the new host-only crate `south-host-grammars` rather than `south-contracts`: an unused
    function added to `south-contracts` changed 3 of 19 `component.wasm`, and the Q47 bump it requires changed all 19.
    Both hosts still call the same function, under the same fuzz obligation; it ships in 0.53.0. Boundary record
    §13.13.
- **Q14** — Ruled (lv for the south maintainers, 2026-10-08): as recommended. The speech dialect words `speech.translate`
  and `speech.verbose_json` are defined by the contract, closed to those two, so catalog data can set them without
  knowing a component. See §17.5 on where such data can come from today.

**Q10 and Q11 stay open, and neither blocks accepting this record.** They depend on other host plans (read at
token-station-server `fbee0ed7`) and decide nothing the south contract contains:

- **Q10 (lv)** concerns the host's cutover, not the contract. After migration a row on a non-OpenAI-compatible arm that
  has only a token price is refused by `prepare` naming the cause (§8), which the contract already says. What Q10 gates
  is the host's pre-cutover step: list the affected rows with a read-only query before each such TTS arm is cut over
  (V3 and V4). Its premise is partly out of date: per the host's gap ledger (#91, 2026-10-08), the host now derives its
  listing check from the selected arm, so such a row is judged "no price configured" when listed and can no longer be
  listed and then refused at admission; the one known instance (a Bailian row) was repriced, and what remains undone is
  an active scan for such rows already listed. §8's sentence "can be listed" describes the state before that fix.
  The contract needs nothing beyond the `prepare` refusal.
- **Q11 (lv, in host plan P25)** applies only if P25 chooses branch A (a generic native passthrough contract) for the
  native ElevenLabs route. The host plans recommend branch B (retire it) for ElevenLabs and have not decided; nothing in
  them selects A. The immutability declaration of §7 (`immutable_body_paths`, `immutable_form_fields`) is already what
  an A-branch design would read, and the record already says so. No field specific to a native route is to be added
  to this contract; a real branch-A design would be its own record.

## 17. Reconciliation with 0.50.0 (2026-10-09)

This record was written at v0.42.0; the image record's §18 lists what 0.43.0 to 0.50.0 changed and its section numbers
are cited here. Everything below was read from `origin/main` at `6aa2813` (v0.50.0). Nothing of this world exists in
the code yet (no `TextPostRequestV1`, `decode_sse_v1`, `from_hex`, `concat`, `wav_pcm_s16le`, `speech` module or WIT).

### 17.1 HTTP contract number

| Record assumes | Fact on main | Effect |
|---|---|---|
| The single bump is HTTP contract 9 → 10, released in the image minor (§5, §13, §14, Q1) | `HTTP_CONTRACT_VERSION` is 11 (`crates/south-contracts/src/lib.rs:106`): 10 is B7a (0.43.0), 11 is SF26 (0.47.0) (image record §18.1) | The bump is 11 → 12, in the media minor, carrying `execute_multipart_binary_call_v1` and `TextPostRequestV1` with its binary entry point. "Contract 10" for these shapes reads "contract 12" throughout this record |
| The ASR arms use `execute_multipart_binary_call_v1` (§5) | The twin is a thin entry point over the shared `execute_buffered`; the reqwest binary transport already carries any prepared request (image record §18.1). No raw twin of any binary entry point exists (0.26.0 D7) | none for speech; the raw-twin question is the image record's S-I-2 |
| `TextPostRequestV1` adds a request-body shape (§5, D3a) | The prepared request has a JSON and a multipart body arm (`south-core/src/lib.rs`, `PreparedHttpRequestV1`); the reqwest transport renders the multipart media type itself (`ARCHITECTURE.md:31`) | `TextPostRequestV1` is the larger half of the bump: contract type, a text body arm, the rendering of the `content-type` from the closed media type (`application/ssml+xml` only), a binary execution entry point, testkit runners and provider-suite rows |
| The `OutputFormat` stop-gap (D3b) would join the bump | Not needed if Q2's declaration route is open (§17.3) | If it were needed it would join the same 11 → 12 bump |

### 17.2 `contracts.media`, `contracts.speech` and ordering

`compatibility.json` has no `media` or `speech` key yet (image record §18.2 lists the keys). Speech needs from the image
world's work, and cannot ship before them: S-I-1 (the `media` module with the complete transform table including
`from_hex`, `concat` and `wav_pcm_s16le`, the `text` body and `sse` form, the elider, parser and encoder,
`decode_sse_v1` (amended 2026-10-10: released separately in 0.53.0 from `south-host-grammars`, boundary record
§13.13), the safe fetch pure functions), S-I-2 (HTTP contract 12) and the host suite `south.safe-fetch.v1` of
S-I-5. The image minor releases `contracts.media` v1 complete, so speech adds nothing to it (§14); speech packages declare
`{"media": 1, "speech": 1}`. The record allows the speech world to ship in the image minor or after it; the steps of §18
are written to let either happen.

### 17.3 Value channel, instance declarations and Vertex

| Record assumes | Fact | Effect |
|---|---|---|
| Vertex TTS takes project and region from "an exported credential attribute or non-secret config" (§12) | Gate ① admits `config_schema` and exported credential attributes only in the provider and embeddings worlds (`south-provider-api/src/manifest.rs:867`, `values.rs:72`); the embeddings widening of 0.48.0 is the model (embeddings record §16) | Same widening for this world (image record §18.3); a Vertex TTS package then copies the `embeddings-vertex` configuration: keys `region` (`aws_region` syntax) and `project` (`gcp_project_id`), `base_url` the location's API origin, the component appends `/v1/projects/{project}/locations/{region}/publishers/google/models/{model}:generateContent` |
| Credential recipe v1 (B4) is needed first (§12, §13 V4) | B4 shipped in 0.43.0; the recipe `vertex_sa` of `components/embeddings-vertex/manifest.json` (RS256 `jwt_sign`, then `oauth2_token`, scope `cloud-platform`, exports `project_id`) is reusable as is for Vertex TTS (image record §18.4); the package ships its own `credential.*` fixtures | The south-side prerequisite of V4 Vertex is met except the gate ① widening |
| D3b: ElevenLabs `output_format` is declared as a `query_parameters` entry (boundary record §10) | `validate_instances` (`south-provider-api/src/instances.rs:244-254`) refuses `query_parameters`, `quota_headers` and `user_agent` outside the provider world, so the recommended route is closed in this world today. Everything else it needs exists: `QueryParameterV1::Declared` (`lib.rs:1202`, HTTP contract 10) and a `Token` value syntax (`declared.rs:76`) that admits `mp3_44100_128` (unreserved bytes include `_`) | Gate ① admits `query_parameters` in the speech world — that one declaration only; `quota_headers` and `user_agent` stay refused until a consumer exists. Then `OutputFormat` is not added. Azure's `api-version` and MiniMax's `GroupId` are sanctioned names and need nothing |
| The two header names are in the compiled-in set; "once the boundary record §10 lands, secret header names are declared by the package" (§3) | §10 landed in 0.43.0: the closed set is five names including `xi-api-key` and `ocp-apim-subscription-key` (`lib.rs:993`), and a package may declare others in `secret_headers` (auth contract 5, `DeclaredHeaderSecret`) | The descriptor auth twin admits both (image record §18.4); no list in this world |

### 17.4 Descriptor, runtime and gate ① shape

The `WorldSchemaV1` row, `validate_role` branch ("at least one of `synthesize`, `transcribe`; `translate` implies
`transcribe`") and runtime world are as the image record's §18.6 describes for an image row: `SPEECH_WORLD_SCHEMA`,
capabilities `synthesize`, `transcribe`, `translate`, `artifact_fetch` and auth arms `bearer`, `header_secret`. The
`artifact_fetch` precedent is now at `manifest.rs:230-242` (was `:109-121`). The speech world is the fourth world without
a host import; the enumerated exclusions should be world properties by then. The runtime limits and the single-instance
lock are as in the image record §18.6.

### 17.5 The model catalog and the speech dialect words

The 0.50.0 catalog (`south.model-catalog.v1`) carries image and video capabilities only, in the host's vocabulary, which
South does not interpret; text was ruled out of it, and a `modality` field was not adopted (boundary record §13.11). It
therefore has no place for `speech.translate` or `speech.verbose_json`. Today those words reach a component only through
`ProviderConfig.models[].supported_parameters`, that is, from the operator's or the host catalog's model rows
(`capability.rs:59` in the pinned kernel). §4's phrase "that the catalog or the operator sets" has only the second path
available: carrying the words in a South catalog would be a new catalog schema and a change to the host's capability
vocabulary and loader, and is not part of this record. The Q14 ruling (two closed words defined by the contract) stands
unchanged, and so does §4's rule that the host refuses an operation not declared by both the component and the model.

### 17.6 Citations that drifted

| Cited | Now |
|---|---|
| `lib.rs:1585`, `:2331` (`MultipartPostRequestV1`, `BufferedBinaryResponseV1`) | `lib.rs:1729`, `:2519` |
| `lib.rs:884-894`, `:1064-1090` (`SecretHeaderV1`, `QueryParameterV1`) | `lib.rs:993`, `:1179-1204` |
| `manifest.rs:109-121` | `manifest.rs:230-242` |
| `component.rs:136` | `component.rs:165` |
| `ARCHITECTURE.md:110`, `:114-115` | `ARCHITECTURE.md:136`, `:140-141` |
| image record `lib.rs` / `south-core` / `component.rs` / `loader.rs` citations (the speech record cites them through the image record's convention) | image record §18.7 |

Unchanged: `runtime.rs:26-34`, `task_v2.rs:100-105`, `task-adapter-v2.wit:63-66`. Server citations were not re-verified;
two host facts moved and are noted in the rulings above (Q10's premise) and in §18 (the host's generic executor).

## 18. Implementation steps, at outline level

Speech comes after the image world's first steps and reuses them; it adds its own contract module, a world, a suite and
packages. Steps are in dependency order; "host prerequisite" is work outside south that the *host cutover* waits for.

| Step | Work | Depends on | Acceptance |
|---|---|---|---|
| **S-S-0** Shared with the image world | S-I-1 (`media` module including the speech words, `decode_sse_v1` — since 2026-10-10 released on its own in 0.53.0 from `south-host-grammars`, boundary record §13.13 —, safe fetch functions), S-I-2 (HTTP contract 12 with `TextPostRequestV1`), the `south.safe-fetch.v1` host suite of S-I-5. Nothing in speech is built twice | image record §19 | Those steps' own acceptance; the WAV container encoder and the hex and concat transforms have golden vectors even though no image component uses them |
| **S-S-1** `speech` contract module | Facts (`characters {count, upper?}`, `metering_forms`, `reservation`, `output_media_type`), `SpeechMeteringV1` (characters with `source`, seconds with `basis`, tokens), outcomes, `SpeechArtifactV1` (`body`, `inline`, `segments`, `url`, `container`), the ASR delivery forms, the two dialect words, `immutable_form_fields`, `PreparedSpeechCallV1` | S-S-0 | Types and codecs with tests in `crates/south-contracts`; `south-contracts` version bump (Q47) |
| **S-S-2** WIT, manifest, gate ①, runtime | `wit/speech-adapter.wit` (§7), `SPEECH_*` constants and schema row, `validate_role` branch; gate ① admits `config_schema`, credential attributes and `query_parameters` in this world (§17.3); runtime `bindgen!` module and instance kind; `UndeclaredValuesIgnored` row | S-S-1, image S-I-3 and S-I-4 | Refusal tests for unknown words, `host_values`, `endpoint`, `quota_headers`, `user_agent`; the embeddings-style runtime tests |
| **S-S-3** Suite `south.speech-component.v1` and references | Codecs, trait, ABI and sandbox adapters, the required rows of §11 (`ssml_escaping`, `client_format_rendering`, `absent_duration_is_null_or_unknown`, `undeclared_operation_refused`, `second_hop_artifact`, `empty_audio_is_unknown`, the metering rows) as additive `CheckV1` variants; the host-obligation vectors for the WAV container and `decode_sse_v1` | S-S-2, image S-I-5 | A native reference passes every row and each row has a failing mutation; `south-component-conformance` version bump |
| **S-S-4** Packages, by the record's batches (§13) | V3-1: xAI, Groq, OpenAI-compatible with SSE, then ElevenLabs (needs the `query_parameters` declaration); V3-2: MiMo, MiniMax; V3-3: the four ASR arms (need S-I-2's multipart binary twin and the parser and encoder); V4: Bailian (second hop), Vertex (the recipe of §17.3 and the WAV container), Azure SSML (needs `TextPostRequestV1`). Fixtures are transcribed from the server's native arms; the ASR duration cases marked "to be measured" in §9 (whisper-style usage seconds, an explicit ElevenLabs duration field, which models accept `verbose_json`) are measured while the fixtures are written and recorded in the package's fixture README | S-S-3 | Each package passes the suite natively and in the sandbox with identical ABI answers; **host prerequisites for every cutover**: the host's generic media executor (not started), the host's existing handling of `seconds: null` (the 60-second fallback marked estimated, kept as is), and the Q10 listing query for non-OpenAI TTS rows |
| **S-S-5** Release | As the image record's S-I-8: crate and package version bumps (Q47), `compatibility.json` (`contracts.speech`, the suite fields, the speech entry in `media_component_capabilities`, `not_verified`), the written Q9 commitment, release record, README and ARCHITECTURE entries, digest-stability and declared-runtime checks, release index | S-S-4 (or its first batch) | The release checks of 0.47.0 and 0.48.0 pass |

Host prerequisites outside south, in addition to the image record's list: the host's WAV container encoding and SSE
decoding equal to south's vectors (in practice: the host calls the south functions), the three reservation algorithms
keeping today's amounts with generic branch conditions (§8), and the translation acceptance with non-English audio, which
stays a host test (§4).

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
- Rulings of 2026-10-01:
  - Q12 ruled by lv: an ASR duration with `basis = last_timestamp` (ElevenLabs) is labeled estimated
    (`quantity_estimated = 1`); the amount is unchanged. §9 (ElevenLabs bullet, **Labels**) and the §12 ASR table
    state it.
  - §13 dual-run reconciliation records the resulting intentional difference: ElevenLabs rows carry
    `quantity_estimated = 1` where the native path records 0, the amount unchanged (Q12).

## Revision note (2026-10-09)

- Header: `Status` is accepted with Q10 and Q11 open; `Rulings` and `Revised` updated; a reconciliation baseline
  paragraph added.
- §16: rulings of 2026-10-08 recorded under Q1, Q2, Q3, Q8, Q9, Q13 and Q14 with their consequences; Q10 and Q11 stay
  open with what each blocks (nothing in the contract; the host's cutover for Q10, a branch-A design for Q11). No design
  decision changed.
- §17 (new): reconciliation with 0.50.0. The HTTP contract bump is 11 → 12, not 9 → 10 (the substance of the Q1 ruling
  stands); gate ① must admit `config_schema`, credential attributes and `query_parameters` in this world; Vertex TTS
  reuses the `embeddings-vertex` recipe; the 0.50.0 catalog carries no speech data; a table of drifted citations.
- §18 (new): outline of the implementation steps S-S-0 to S-S-5 and the prerequisites outside south.
- 2026-10-10, lv ruling (boundary record §13.13): `decode_sse_v1` lives in the host-only crate `south-host-grammars` and
  ships in 0.53.0, early and alone (Q-B6-6). §6, §14, §17, §18 S-S-0 and the Q13 ruling gain notes; the `sse` view
  needs a `south-contracts` entry point taking already-decoded events, added with the speech minor.

## S-S-1 implementation (2026-10-14)

Built on branch `speech-s-s-1` from `origin/main` `479f380` (v0.63.0), for review. The image world's `image.rs` is the
model throughout (naming, strict decoding of what a component returns, tolerant decoding of what the host sends, one
error enum, compact source-text codecs); where this record says nothing, the image convention is followed and the
choice is listed below.

### What landed

`crates/south-contracts/src/speech.rs`, public as `south_contracts::speech`, with unit tests in the module:

| Type or function | Record |
|---|---|
| `SPEECH_CONTRACT_VERSION` (1) | §14 |
| `SpeechOperationV1` (`synthesize`, `transcribe`, `translate`) | §4, §8 `operation` |
| `SpeechDialectWordV1`, `SPEECH_DIALECT_WORDS`, `speech_dialect_words_v1` | §4, Q14 |
| `SpeechMeteringFormV1` (`characters`, `seconds`, `tokens`) | §8 `metering_forms`, §9 |
| `SpeechFactsV1`, with `SpeechCharactersV1 {count, upper?}` and `SpeechReservationV1 {max_output_tokens?, max_seconds?}` | §8 |
| `SpeechMeteringV1`, with `SpeechCharacterMeterV1 {value, source}`, `SpeechSecondsMeterV1 {value, basis}`, `SpeechTokensV1 {input, output, input_text?, input_audio?}` | §9 |
| `check_required_metering_v1` | §9 "Handling missing evidence" |
| `SpeechArtifactV1` (`body`, `inline`, `segments`, `url`, `container`), `SpeechEncodingV1`, `SpeechContainerV1`, `SpeechSegmentV1`, `fits` | §6, §10 |
| `TranscriptDeliveryV1` (`json`, `text`, `passthrough`), `parse_transcript_delivery_v1`, `fill_transcript_template_v1` | §6 "ASR delivery" |
| `validate_immutable_form_fields_v1`, `form_field_is_immutable` | §7 |
| `SpeechModelCapabilitiesV1`, `parse_speech_model_capabilities_v1` | §7 `model-capabilities` |
| `SpeechCallContextV1` | §7 `prepare`, §8 "Pricing form" |
| `PreparedSpeechCallV1`, `parse_prepared_speech_call_v1`, `to_json` | §7 |
| `is_speech_media_type` | §6 `media_type` |

Reused from `contracts.media` without change: `MediaRequestDescriptorV1` (the `json`, `text` and `multipart` bodies),
`ResponseBodyFormV1` (including `sse`), `PathPatternV1`, `MediaLimitsV1` (`state_bytes`, `elision_paths`,
`inline_string_bytes`), the transforms `from_base64`, `from_hex` and `wav_pcm_s16le` with their bounds, `BlobSetV1`, and
the template walk and expansion of the descriptor (now re-exported inside the crate, `pub(crate)`, so the transcript
template is checked and filled by the same code; no behaviour change). `JsonPointerV1` and
`validate_immutable_body_paths` are the existing crate-root items. The fuzz target `media_parsers` decodes every new
codec and checks three invariants: an artifact re-encodes to itself, a prepared call and a transcript delivery
round-trip through `to_json`, and a checked transcript template fills once its blobs exist.

### Interpretations where the record is silent or ambiguous

1. **Outcomes.** §18 lists outcomes in S-S-1. `SpeechOutcomeV1`'s failure arms carry an `ErrorEnvelope`, which
   `south-contracts` does not depend on; the image world's outcome is in the conformance crate for that reason
   (`component_image.rs`, although image §19 also listed it in S-I-1). S-S-1 lands every type the outcome carries
   (artifact, transcript delivery, metering) and the required-metering rule; the enum itself (`succeeded`, `rejected`,
   `charged_failure`, `unknown`, §7) lands with the component trait in S-S-3. **This departs from the §18 row wording.**
2. **Facts per operation.** Synthesis must carry `characters` and `output_media_type` and may tighten
   `max_output_tokens`; recognition carries neither and may tighten `max_seconds`. `metering_forms` is non-empty, has
   no repeats, and names only forms the operation can report: `characters` for synthesis, `seconds` for recognition,
   `tokens` for both (gpt-4o-transcribe style ASR is token-priced). Anything else is refused.
3. **`reservation`.** Both bounds are integers of at least 1; `max_seconds` is whole seconds, as the host's
   `asr_max_billable_seconds` is. `characters.upper` is not required to be at least `count`: an upstream that bills
   fewer characters than the scalar count may say so, and the reservation is `min(bytes(input), upper ?? count)` either
   way (§8).
4. **Required metering** (`check_required_metering_v1`). `characters` must be present when declared (an upstream that
   reports no number is `source = request`, §9); `tokens` must carry both `input` and `output` (§9: a missing count is
   `unknown`, never 0); `seconds` may be `null` (the host's 60-second estimate, §9). The host's own "not both zero" rule
   for token-priced TTS (`a82c852b`) and the §9 host checks stay in the host. `seconds.value` is a finite non-negative
   number, as `TaskUsageFactsV2.seconds` is.
5. **Artifact wire.** `{"form": "body" | "inline" | "segments" | "url", …, "media_type", "container"?}`, the image
   world's `form` tag. `media_type` is required on every form, `url` included (§6 writes `url {pointer, media_type}`;
   the image world's is optional). A member that belongs to another form is refused. `container` is
   `{"wav_pcm_s16le": {"sample_rate", "channels"}}`, the same bounds as the transform. `segments` holds 1 to 65,536
   items (`MAX_SPEECH_SEGMENTS`) and no pointer twice.
6. **`fits`.** `body` needs the `binary` body form; `inline`, `segments` and `url` point into a JSON tree, so they need
   `json` or `sse`; a `text` response carries no audio. The record names `binary` for `body` and `sse` for OpenAI's
   `segments`; allowing `inline` / `url` on `sse` and `segments` on `json` is my reading. Whether `output_media_type`
   must equal the artifact's `media_type` is left to the host (no check here).
7. **Transcript delivery wire, read literally from §6**: `{"json": <template>}`, `{"text": "…", "media_type": "…"}` (two
   keys) and `{"passthrough": {"media_type": "…"}}`. A template may hold `$south.ref` nodes with transform `as_is` and
   no `media_type`; any other reserved key or transform is refused. As in the image world's render check, no string or
   key outside a reference, and no `text` document, may exceed the fallback threshold (1 MiB), so bytes cannot leave
   the sandbox through a transcript. `fill_transcript_template_v1` is not bounded by the 32 MiB JSON request bound
   (§5: a transcript between 32 and 64 MiB is delivered).
8. **Immutability.** The key the body kind uses must be present and may be `null` (task contract 6 §4 makes the key
   required); the other key must be absent. A JSON body uses `immutable_body_paths`; a text (SSML) body uses
   `immutable_body_paths` and it must be `null`; a multipart body uses `immutable_form_fields`. Form-field names are
   `[A-Za-z0-9_-]{1,256}`, at most 64, no repeats, and a name also covers its array form (`model` covers `model[]`), as
   a multipart immutable path does in the image world. The image world's `immutable_body_paths` defaults to `[]` when
   absent; speech follows contract 6 instead, because §7 cites its `null` semantics.
9. **Body kinds.** A descriptor with an `empty` body is refused (§5 lists three kinds). The body kind is not tied to
   the operation, so a future JSON-bodied recognition upstream needs no contract change.
10. **Dialect words.** Entries without the `speech.` prefix are other vocabularies and are skipped; an entry under the
    prefix that is not one of the two words is refused, so a misspelt catalog word is reported rather than silently
    turning a capability off.
11. **`model-capabilities`.** `{model, operations, metering_forms}` only: each form must be admitted by some declared
    operation. "Voices/formats" in the §7 WIT comment are not carried (open question 1). `translate` without
    `transcribe` is not refused per model; §4's rule is a manifest rule (S-S-2).
12. **`SpeechCallContextV1`** is `{upstream_model, operation, metering_required}`, tolerant of unknown fields like
    `ImageCallContextV1`. `operation` is new: the host knows it from the route (`/v1/audio/transcriptions` vs
    `/translations`), and the component needs it for the `undeclared_operation_refused` row.
13. **No `SPEECH_CONTRACT_VERSIONS`.** The image module has only `IMAGE_CONTRACT_VERSION`; a list is added when a
    second speech contract exists.
14. **`compatibility.json`**: no entry. The image world recorded `contracts.image` only at its release step S-I-8 (image
    record, S-I-3 and S-I-4 implementation note); `contracts.speech` follows at S-S-5.

### Points that contradict the record (flagged, not silently chosen)

- **Media types with parameters.** §6 has Vertex deliver `audio/L16; rate=24000`, but `contracts.media`'s
  `is_media_type` admits no parameters. `is_speech_media_type` admits `;name=value` parameters of `tchar`s (one optional
  space after each `;`, no quoted values, 128 bytes in all) for every speech media type; the image grammar is
  unchanged. This keeps the record's example expressible; if lv prefers the parameterless grammar, raw PCM must be
  delivered wrapped (`audio/wav`) or as `audio/L16` without a rate.
- **The `sse` response view is not built in S-S-1.** §14 (amended 2026-10-10) says the speech minor adds a
  `south-contracts` entry point taking the events the host decoded; §18 assigns it to no step. Two texts disagree on its
  shape: §6 elides the event list **as one document whose root is the array** (pointers `/*/data/delta`), while
  `media::elide_from` documents itself as "one document per event" for the `sse` form. The pointer form of `segments`
  depends on the choice, so it is left for lv (open question 2); `build_media_response_view_v1` still refuses `sse`.

### Open questions for lv

1. Should `SpeechModelCapabilitiesV1` carry voices and formats (§7's WIT comment), or are they catalog data like the
   image world's ranges and defaults (image R-1), so that the declaration stays `{model, operations, metering_forms}`?
2. The `sse` view: one elided array (§6) or one elided document per event (`elide_from`'s note), and in which step (S-S-1
   follow-up or S-S-3)?
3. Media type parameters for speech (above): keep `is_speech_media_type`, or the parameterless media grammar?
4. `MAX_SPEECH_SEGMENTS` = 65,536 is a guess sized to the 16 MiB payload; the OpenAI SSE delta count for a long text
   has not been measured.

### Resolutions (2026-10-14)

- **Question 1, capabilities:** catalog data, by the image world's precedent (image R-1, lv 2026-10-09: the catalog is
  authoritative for per-role limits and default words). The declaration stays `{model, operations, metering_forms}`;
  voices and formats are not carried. Settled by precedent, without a new ruling.
- **Question 2, the `sse` view (lv, 2026-10-14):** §6 is normative. The host's decoded event list is elided as **one
  document whose root is the array** `[{event, data}]`, and `segments` pointers address it (`/*/data/delta`). The note on
  `media::elide_from` that calls the `sse` form "one document per event" is corrected when the view is built. It is built
  in S-S-3, with the suite that exercises it, and must exist before the first package that declares `response_body_form:
  sse` (the V3-1 batch).
- **Question 3, media type parameters (lv, 2026-10-14):** keep `is_speech_media_type`. Speech media types admit
  `;name=value` parameters, so Vertex can declare `audio/L16; rate=24000` exactly as the native arm delivers it; the image
  and media grammar is unchanged.
- **Question 4, `MAX_SPEECH_SEGMENTS`:** 65,536 stays, as a provisional bound. The V3-1 fixtures measure the OpenAI SSE
  delta count of a long text and record it in the package's fixture README; the bound moves before release if the
  measurement needs it.

### Versions (Q47)

`south-contracts` 0.54.0 → 0.55.0. `south-component-conformance` 0.62.0 → 0.63.0: the reference `VERSION` constants
in its `src/` move with the package patches, and `scripts/check_crate_versions.py` requires a guest-linked crate whose
sources changed to move. `south-provider-api` (0.56.0) does not change. Every package published with 0.63.0 takes a
patch with unchanged behaviour and keeps its `south_runtime` and its `contracts`
(`s_s_1_retires_every_published_063_package_identity`); `i3_3_retires_every_published_062_package_identity` no longer
pins the identities it left.

## S-S-2 implementation (2026-10-14)

Built on branch `speech-s-s-2` from `origin/main` `f1d6ecb` (S-S-1), for review. The image world's S-I-3 and S-I-4
(0.52.0, image record "Implementation note: S-I-3 and S-I-4") are the model throughout; where this record is silent the
image convention is followed and the choice is listed below.

### What landed

| Piece | Where | Record |
|---|---|---|
| `wit/speech-adapter.wit`: package `token-station:speech-adapter@1.0.0`, world `speech-adapter-v1`, exports `metadata`, `healthcheck`, `model-capabilities`, `prepare`, `parse-response`, no import; `SPEECH_ADAPTER_WIT` embeds it | `crates/south-provider-api/` | §3, §7 |
| `SPEECH_WIT_PACKAGE`, `SPEECH_WORLD`, `SPEECH_BEHAVIOR_SUITE`, `SPEECH_CAPABILITIES` (`synthesize`, `transcribe`, `translate`, `artifact_fetch`), `SPEECH_OPERATION_CAPABILITIES`, `SPEECH_WORLD_SCHEMA` (auth arms `bearer`, `header_secret`; `HostImportV1::Pure`; value channel) and its `KNOWN_WORLDS` row | `manifest.rs` | §3, §4, §17.4 |
| The `validate_role` branch: at least one of `synthesize` / `transcribe` (`ManifestErrorV1::SpeechOperationRequired`), `translate` only with `transcribe` (`ManifestErrorV1::TranslateRequiresTranscribe`), a provider family required | `manifest.rs` | §4, §17.4 |
| Gate ① admits `config_schema` and exported credential attributes (the value channel, under the provider world's key rules) and `query_parameters` (under the boundary record §10 rules, through `InstanceDeclarationsV1::QueryParameters`); `quota_headers`, `user_agent`, `host_values`, `endpoint` and every other provider-world declaration stay refused | `manifest.rs`, `instances.rs`, `values.rs` | §17.3 |
| Runtime: `bindings::speech` (`bindgen!`), `InstanceKind::Speech` with its accessor and the named world-mismatch errors in every other accessor, `call_speech_model_capabilities`, `call_speech_prepare`, `call_speech_parse_response`, the identity probe; the linker offers no `host` and the import scan is the pure worlds' | `crates/south-provider-runtime/src/` | §3, §17.4 |
| Gate ②'s `UndeclaredValuesIgnored` row for the worlds whose only value source is `ProviderConfig.declared` | `crates/south-component-conformance/src/undeclared_values.rs` | §17.3, §18 |
| Test guest `tests/guests/test-speech` and `tests/speech_world_v1.rs` | `crates/south-provider-runtime/` | §18 acceptance |

Tests. Gate ① (`provider_api_v2.rs`, `component_values_v1.rs`): every admitted combination of the operation words under
either arm; refusal of no operation word, of `artifact_fetch` alone, of `translate` without `transcribe`; unknown words
(including `tts`, `asr`, another world's words and the dialect word `speech.translate`); the arms `oauth`, `host_signed`
and `bearer_and_header_secret`; `host_values`, `endpoint`, `signing`, `stream_framing`, `usage_evidence`,
`request_facts`, `immutable_body_paths`, `quota_headers`, `user_agent`, the task-v2 bucket declarations and
`min_poll_interval_seconds`; `query_parameters` admitted and held to the provider world's name and syntax rules, and
still refused in the image, embeddings and task worlds; the value channel admitted and held to the provider key rules;
the WIT's package, world, function set, parameter names and `result` returns; every world's posture. Runtime
(`speech_world_v1.rs`, the embeddings and image cases ported): the package loads and answers its three calls; every
`token-station:` import and every `host` interface is refused before instantiation; the other worlds' faces, the image
face included, are refused by name on a speech component and the speech face on theirs; speech bytes cannot claim
another world or identity, and provider and image bytes cannot claim the speech world; both handshakes and gate ① run
before the bytes are opened; input, output and guest-error bounds; the default 16 MiB payload bound in every argument
position; a panic, an allocation past the limit and a hang are clean errors the component survives; determinism across
calls and instances; coexistence with the image, embeddings and provider worlds in one runtime.

### Interpretations where the record is silent or ambiguous

1. **A world property, not a world name check.** §17.3 admits `query_parameters` and keeps `quota_headers` and
   `user_agent` refused, but `WorldSchemaV1::instance_declarations` was one flag for all three. It becomes a
   three-valued `InstanceDeclarationsV1` (`None`: task, embeddings, image; `QueryParameters`: speech; `All`: provider)
   rather than a second flag: a fourth `bool` in the struct is refused by the workspace lints
   (`clippy::struct_excessive_bools`), and no world admits `quota_headers` or `user_agent` without `query_parameters`.
   Every existing world's behaviour is unchanged, its refusal order included (`query_parameters`, `quota_headers`,
   `user_agent`). A world refuses the first declaration it does not admit before any declaration is checked, so a
   valid query declaration never hides a refused `user_agent`. Changing the field's type breaks a host that reads it
   or builds a `WorldSchemaV1` literal, as the image world's four new fields did; none in this repository does.
2. **Two error variants, in this order.** "At least one of `synthesize` / `transcribe`" is checked before "`translate`
   implies `transcribe`", so `["translate"]` alone is `SpeechOperationRequired` and `["synthesize", "translate"]` is
   `TranslateRequiresTranscribe`. Both are new `ManifestErrorV1` variants, breaking for a host that matches the enum
   exhaustively, as 0.52.0's `ImageOperationRequired` was; the release record (S-S-5) lists them.
3. **`artifact_fetch` needs `synthesize`** (resolved 2026-10-14, open question 2 below): first drafted with no rule of
   its own; gate ① now refuses it without `synthesize` (`ArtifactFetchRequiresSynthesize`).
4. **`SPEECH_OPERATION_CAPABILITIES`** is a public constant (the two words of which one is required), the counterpart
   of `TASK_REQUIRED_CAPABILITIES`; the image world needs none because both its words are operations.
5. **The WIT comment on `model-capabilities`** names the operations and the metering forms only: §7's "voices/formats"
   is dropped, per the S-S-1 resolution of question 1. The function set, parameter names and the `prepare` refusal
   comment are §7's.
6. **Runtime names and limits.** The calls are `call_speech_*` because the provider and image worlds export the same
   WIT names (the image world's convention). The limits are the defaults (payload 16 MiB, memory 64 MiB, 2 s), not the
   embeddings media memory (192 MiB): §6 keeps audio out of the sandbox. Calls go through the single shared instance
   as for every other world.
7. **The `UndeclaredValuesIgnored` row without a speech suite.** The suite is S-S-3, yet the §18 row puts this check
   in S-S-2. The embeddings and image suites each carried a copy of the same check; S-S-2 moves it into one crate
   function that takes a fixture input, its first answer, the manifest and the call, injects an undeclared key into
   `/provider_config/declared` and requires the same answer. The embeddings and image suites now call it (the image
   row's failure detail gains the embeddings row's wording; nothing else changes), and S-S-3's speech suite calls it on
   its `prepare` cases. Its unit tests run a speech-shaped `prepare` input against a Vertex-style speech manifest.
8. **The test guest** `tests/guests/test-speech` is built by `scripts/prebuild-components.sh`, cached by CI, and
   `speech_world_v1` is in the nextest setup-script filter, as for `test-image`. Its manifest declares no south contract:
   `compatibility.json` records `contracts.speech` only at S-S-5, and no package is published before then.
9. **The provider world's `immutable_body_paths` stays refused** in this world: speech's immutability is declared per
   call in `PreparedSpeechCallV1` (§7, S-S-1), not per family in the manifest.

### Points that contradict the record (flagged, not silently chosen)

- **A declared query parameter cannot yet travel on a media descriptor.** §17.3 says that, once gate ① admits
  `query_parameters`, "everything else it needs exists" (`QueryParameterV1::Declared` and the `token` syntax). That is
  true of the provider world, but `MediaRequestDescriptorV1::parse` (`south-contracts/src/media/descriptor.rs`) admits
  only the fixed `QueryParameterV1::ALL` and refuses any other name as `UnsanctionedQuery`, and it is not given the
  package's declarations. So an `ElevenLabs` speech package may now declare `output_format`, but a descriptor carrying
  `?output_format=mp3_44100_128` would still be refused. S-S-2 lands the gate ① half as §18 specifies and does not
  change the media descriptor; the other half is open question 1.

### Open questions for lv

1. **The media descriptor's declared query parameters.** Which step adds them (a parse that takes the package's
   `query_parameters` and builds `QueryParameterV1::Declared`, the host passing the same declarations), and does it move
   a contract number? The wire shape (`query: [{name, value}]`) is unchanged; what widens is what a host must accept,
   which suggests `contracts.media` 3 (the 0.59.0 precedent for `media` 2) rather than no bump. It must land before the
   `ElevenLabs` package, the last of the V3-1 batch (S-S-4); §18 assigns it to no step. Recommendation: S-S-3, with the
   other contract work this world still needs (the `sse` view), as `contracts.media` 3.
2. **`artifact_fetch` without `synthesize`.** URL artifacts are synthesis output (§6); recognition delivers documents.
   S-S-2 admits `artifact_fetch` with `transcribe` alone, since §4 states no rule. Should gate ① refuse it there, as it
   refuses `translate` without `transcribe`? Recommendation: refuse it, as a third rule with its own variant, since a
   word nothing honours is a promise (the reason the task worlds refuse credential attributes).

### Resolutions (2026-10-14)

- **Question 1, declared query parameters on a media descriptor (lv, 2026-10-14, as recommended but in another step):**
  the media descriptor accepts a query parameter the package declares (`QueryParameterV1::Declared`, values under the
  existing grammar) as `contracts.media` 3, additive. It lands with the ElevenLabs package in S-S-4 (V3-1), the first
  package that needs it, not in S-S-3; the earlier V3-1 packages do not depend on it.
- **Question 2, `artifact_fetch` without `synthesize`:** refused, as recommended. It follows the repo's rule that a
  declaration nothing honours is refused (the task worlds refuse credential attributes for that reason): URL artifacts
  are synthesis output (§6). Gate ① returns `ManifestErrorV1::ArtifactFetchRequiresSynthesize` (a third variant; a host
  matching the enum exhaustively adds the arm); `artifact_fetch_without_synthesize_is_refused` pins it, and removing the
  rule turns that test red.

## S-S-3 implementation (2026-10-14)

Built on branch `speech-s-s-3` from `origin/main` `33eb77b` (S-S-2), for review. The image world's suite step (S-I-5 and
S-I-6, released in 0.52.0) is the model throughout: the trait and outcome beside it in the conformance crate, the ABI
shim, the sandbox seam, the fixture loader, the suite, a native reference with its pack, a mutation test per row and a
sandbox parity test. Where this record is silent the image convention is followed and the choice is listed below.

### What landed

| Piece | Where | Record |
|---|---|---|
| The `sse` response view: `build_media_sse_response_view_v1(round, events, …)` over the events the host decoded (`SseEventFieldsV1 {event, data}`), the body variant `MediaResponseBodyV1::Sse` (`{"sse": [...]}`), the event list elided as **one** document whose root is the array; the notes on `ResponseBodyFormV1::Sse` and `media::elide_from` corrected | `south-contracts/src/media/response.rs`, `elide.rs`, `mod.rs` | §6; S-S-1 resolution of question 2 |
| Host-obligation vectors: the WAV container (five outputs, one of them the header-only output a host never settles, and six refusals) and the `sse` view (four bodies with their `decode_sse_v1` events and views, two refusals) | `south-contracts/tests/vectors/speech-v1.json`, `tests/speech_contract_v1.rs`; the `decode_sse_v1` step in `south-component-conformance/tests/speech_host_obligations_v1.rs` | §11 host obligations |
| `SpeechComponentV1` (`metadata`, `model_capabilities`, `prepare`, `parse_response`), `SpeechOutcomeV1` (`succeeded`, `rejected`, `charged_failure`, `unknown`) and `SpeechDeliveryV1` (`Audio(SpeechArtifactV1)`, `Transcript(TranscriptDeliveryV1)`) | `component_speech.rs` | §7; S-S-1 interpretation 1 |
| The strict outcome codec (`parse_speech_outcome_json`, `speech_outcome_json`) | `speech_json.rs` | §6, §7, §9 |
| The guest shim (`abi_speech`) and the host seam over the runtime (`SandboxedSpeechComponentV1`, feature `sandbox`) | `abi_speech.rs`, `sandbox_speech.rs` | §7 |
| `speech-v1.<family>.<case>` packs (`capabilities`, `prepare`, `response`) with the response sidecar `{"missing": …}` | `speech_fixture.rs` | §11 |
| `south.speech-component.v1`: `run_speech_component_suite_v1` | `speech_suite.rs` | §11 |
| Six additive `CheckV1` variants: `AbsentDurationIsNullOrUnknown`, `ClientFormatRendering`, `UndeclaredOperationRefused`, `SsmlEscaping`, `SecondHopArtifact`, `EmptyAudioIsUnknown` | `report.rs` | §11 |
| A strict, minimal XML well-formedness reader for `ssml_escaping` | `ssml_check.rs` (crate-private) | §5, §11 |
| The native reference `SyntheticSpeechReferenceV1` (family `synthetic-speech`) and its pack | `reference_synthetic_speech.rs`, `fixtures-speech-synthetic/` | §18 S-S-3 acceptance |
| The reference as a sandbox guest, its manifest, and its parity test | `tests/guests/synthetic-speech/`, `tests/synthetic_speech_sandbox_parity.rs` | §18 S-S-3 |
| The suite run, the row-by-row mutations and the required-case checks | `tests/synthetic_speech_suite.rs` | §18 S-S-3 acceptance |
| The `sse` view in the `media_parsers` fuzz target | `fuzz/fuzz_targets/media_parsers.rs` | §6 (fuzz obligation) |

Rows of `south.speech-component.v1`. Carried over from the image world (image record §12.1), unchanged in meaning:
coverage, fixture match, determinism, unknown-field tolerance (context and request view), undeclared values ignored (the
shared S-S-2 row), descriptor auth within the manifest (`admit_media_descriptor_auth`), endpoint confinement, terminal
only from the wire, auth errors not retriable, pre-dispatch refusal, reference integrity, `metering_sample` and
`missing_meter_is_not_zero`. New: the six variants above. Every row is listed below with its test and its mutation.

| Row (`CheckV1`) | Requirement as implemented | Mutation that fails it (`synthetic_speech_suite.rs`) |
|---|---|---|
| `SsmlEscaping` | every SSML body is well-formed XML, carries `input` as some element's whole text and `voice` as some attribute's value, unchanged; the pack has an SSML case with `<`, `&`, `'`, `"` in both (`speech-v1.ssml.special-characters`) | `UnescapedSsml`; dropping `prepare.ssml-escaping` fails the pack row |
| `ClientFormatRendering` | for a model whose row carries `speech.verbose_json`: one upstream response rendered in every client format the pack's prepare cases accept, all with the same non-null `seconds`; no upstream response settled with different `seconds` in two formats; each delivery is the kind of document its format names | `SubtitlesWithoutDuration`; dropping `response.verbose-vtt` fails the pack row |
| `AbsentDurationIsNullOrUnknown` | a package declaring `seconds` has a `{"missing": "seconds"}` case (`speech-v1.absent.seconds`), and every such case ends `seconds: null` or `unknown` | `FallbackDuration`; dropping both duration cases fails the pack row |
| `UndeclaredOperationRefused` | each prepared call names the operation it was asked for; each successful prepare case, asked for every operation its model does not declare, is refused with `invalid_request` or `capability`; at least one such attempt (`speech-v1.undeclared-operation`) | `TranscribeForTranslate`; a pack of capabilities only fails the pack row |
| `SecondHopArtifact` | every `url` artifact's pointer lands on a string of the unelided upstream response that passes `ArtifactUrlV1`, and only a package declaring `artifact_fetch` delivers one; a package declaring it has a `url` success (`speech-v1.second-hop`) | `UrlAtTheWrongPointer`; dropping `response.url` fails the pack row |
| `EmptyAudioIsUnknown` | every `{"missing": "audio"}` case ends `unknown`, and for every response body form a successful synthesis uses there is such a case (`speech-v1.empty-audio.<form>`) | `SucceedWithoutAudio`; dropping `response.sse-no-deltas` fails `…empty-audio.sse` |
| `MeteringSample` | for each declared form a success with an exact value (`characters`, `seconds`, `tokens`); a package that reports upstream characters also shows the `request` fallback (`…characters.request`); the facts echo the required forms | `AlwaysUpstreamCharacters`; dropping the request-count successes fails `…characters.request` |
| `MissingMeterIsNotZero` | a package declaring `tokens` has cases missing the whole `usage`, `input` and `output` (`speech-v1.missing.tokens*`), each `unknown`; every success carries every required fact (`check_required_metering_v1`) | `ZeroForMissingTokens`; dropping `response.sse-missing-input-tokens` fails `…missing.tokens.input` |
| `ReferenceIntegrity` | no `$south.` key in any output but a transcript's `$south.ref`, which names a blob of the view; no output string over the threshold; descriptor references name view blobs; an artifact fits the declared body form; `inline` lands on a string, `body` on an opaque body, every `segments` pointer on an **elided** string (§6) | `UnelidedSegments`, `BlobInTranscript` |
| `PreDispatchRefusal` | every `prepare` refusal is `invalid_request` or `capability`; the pack has one | `InternalRefusal` |
| `AuthErrorsAreNotRetriable` | a 401 or 403 maps to a non-retriable error; the pack has one | `RetriableAuth` |
| `TerminalOnlyFromTheWire` | a non-2xx is never `succeeded`; the pack has one | `SucceedOnError` |
| coverage, fixture match, determinism, unknown-field tolerance, undeclared values ignored, descriptor auth, endpoint confinement | as in the image suite | carried over unchanged; their image and embeddings mutations stand |

### The reference

The acceptance asks for "a native reference" that passes every row. Image S-I-5 and S-I-6 used a real family's reference
(`image-azure`, transcribed from the server's native arm). No single real speech family exercises every conditional row:
SSML is Azure's, `verbose_json` rendering OpenAI-compatible's and xAI's, the second hop Bailian's, the token-priced event
stream OpenAI's, hex audio MiniMax's; and the real packages, transcribed from the native arms, are S-S-4. So the
reference is **synthetic**, in the manner of the `t21-unseen-*` guests: one family, `synthetic-speech`, with a made-up API
whose models each take one of the record's shapes, chosen by the model id's prefix (`tts-stream`, `tts-inline`,
`tts-url`, `tts-ssml`, `asr`). Picking behaviour from a model id is what a real package must not do (§4); it is confined
to this test double, documented in its module and in the pack README, and no host routes to the family. One reference
covers every row, so no second reference was needed. The same code runs natively and as the guest
`tests/guests/synthetic-speech` (built by `prebuild-components.sh`, listed in the CI cache, its parity test in the
nextest setup filter through `~sandbox_parity`); the guest is not an official component, is not counted by
`shipped_packages_v1`, and records no gate ② report.

### Interpretations where the record is silent or ambiguous

1. **The `sse` entry point.** `south-contracts` cannot decode SSE (§14 as amended), so the view is built by a new
   function that takes the decoded events, `build_media_sse_response_view_v1`; `build_media_response_view_v1` keeps its
   signature and still refuses the `sse` form (`SseUnavailable`, its message now naming the events), since it has no
   events to build from. Each event is `{"event", "data"}`; `data` is the event's data as a JSON value when it is one
   JSON document and as a string otherwise (§6); the list is elided once, so a declared `/*/data/delta` matches every
   event and blob ids run across the list. A non-2xx body ignores the events and follows the existing non-2xx rule. A
   `$south.` key or a repeated key in any event's data makes the round `unknown`, as in a JSON body; a data nested past
   the parser's depth bound does too (it is JSON, so it is not carried as a string).
2. **The body key.** The record names the list but not its key in the view. It is `{"sse": [...]}`, the form's own
   word, as `json`, `text` and `opaque` are; `inline` and `segments` pointers are relative to that list. This adds
   `MediaResponseBodyV1::Sse`, a breaking change for a host that matches the enum exhaustively (no code in this repository
   does).
3. **Image keeps refusing `sse`.** The image suite and an image host call `build_media_response_view_v1`, which still
   refuses the form, so nothing in the image world changes. `PreparedImageCallV1` still admits `response_body_form: sse`
   at parse, so a host would dispatch such a call and judge its 2xx `unknown`, as before this step (open question 2).
4. **The outcome's delivery.** `succeeded` carries one `SpeechDeliveryV1`: synthesis delivers one artifact (§6), so
   there is no list; recognition delivers a transcript document. Wire: `"artifact": …` or `"transcript": …`, exactly one.
   The frame is bounded at 4 MiB (the image world's rendered-frame bound: a transcript template's strings are each at
   most 1 MiB) and an `unknown` reason at 1 KiB, as in the image world.
5. **The metering rows** of the §18 row (`metering_sample`, `missing_meter_is_not_zero`) already exist as `CheckV1`
   variants since the image world; the speech suite reuses them with the speech requirements, rather than adding twins.
   The six new variants are the speech-only rows.
6. **`metering_sample`.** "`characters` counted separately for each of its two sources": a package that reports
   `source = upstream` must also show the `request` fallback (§9: an upstream that reports no number is `request`); a
   package that only counts the request needs only that. "`seconds` for each basis the component uses": every basis
   seen in a success is by construction sampled, so the row asks for one success with a value. Exactness is
   `fixture_match`'s.
7. **`missing_meter_is_not_zero`** requires the whole `usage`, `input` and `output`. `input_text` and `input_audio` are
   side information (§9) and nullable, so a missing one is not `unknown`.
8. **`client_format_rendering`** applies when a prepare case routes to a model whose row carries `speech.verbose_json`.
   The accepted formats are those of such recognition cases that `prepare` did not refuse; "one duration-bearing upstream
   response" is response cases whose fixture `response` is identical. The suite reads the northbound `response_format`
   part (absent is `json`), a field of the `OpenAI` audio protocol the host serves, never a provider's. A delivery's kind
   is also checked: `json` and `verbose_json` are a template or a JSON pass-through, `text`, `srt` and `vtt` text.
9. **`undeclared_operation_refused`** is run as a suite mutation on every successful prepare case, for every operation
   the routed model does not declare in `model_capabilities` (synthesis models are asked to transcribe and translate,
   recognition models without `speech.translate` to synthesize and translate), and the prepared facts must name the
   operation asked for. It needs the prepare cases' provider configs to list their models.
10. **`ssml_escaping`** checks every SSML body, not only the special-character case. Well-formedness is judged by a
    hand-written reader in the conformance crate (an XML dependency would enter every component lockfile under Q47): it
    admits an XML declaration, elements, quoted attributes, character data, comments, the five predefined entities and
    numeric references, and refuses everything else (DOCTYPE, CDATA, undefined entities, raw `<` or `&`, repeated
    attributes). The `xml:lang` derived from the voice is not compared (it is the component's derivation), only escaped.
11. **`second_hop_artifact`** is checked on every `url` artifact of any package, against the unelided upstream body the
    fixture gives; a `url` artifact from a package without `artifact_fetch` fails it, the gate ③ counterpart of S-S-2's
    gate ① rule.
12. **`empty_audio_is_unknown`** is required per response body form, because §11's examples are one per form (an empty
    `binary` body, an empty inline string, no deltas); a form is required when a successful synthesis in the pack uses
    it.
13. **Fixture bodies.** A response case gives `body` (JSON), `body_text` (how an `sse` body is written) or
    `body_base64`. The conformance crate is linked into every component and may not link `south-host-grammars`, so the
    suite splits an `sse` body with this crate's `sse_split`, which `openai_responses_vocabulary_v1` holds equal to
    `decode_sse_v1` by property test; `speech_host_obligations_v1` checks the two agree on the speech vectors too.
14. **The sidecar** is `{"missing": "<fact>"}` only, the fact one of `tokens`, `tokens.input`, `tokens.output`,
    `seconds`, `audio`; the image world's `absent` (evidence facts) has no speech counterpart.
15. **The host-obligation vectors** live in `south-contracts` (`speech-v1.json`, beside `media-v1.json`), and the step
    that needs `decode_sse_v1` is checked where that crate is a dev-dependency, so `south-host-grammars` does not change
    and its versioning rule (boundary record §13.13) is not engaged. The WAV vectors mark the header-only output of
    empty PCM `deliverable: false`: the transform produces it, and §10 forbids settling it.
16. **The guest's manifest** declares `contracts: {"media": 1}`, as the test guests declare none for speech:
    `compatibility.json` records `contracts.speech` at S-S-5, so a host range today has no `speech` key to admit.

### Points that contradict the record (flagged, not silently chosen)

- **"The required rows … as additive `CheckV1` variants"** (§18 S-S-3) includes "the metering rows", which are not new
  variants: they exist since the image world and are reused (interpretation 5).
- **"A native reference"** (§18 acceptance) is read as the image world's real-family reference; the reference here is
  synthetic (above), because the real packages are S-S-4 and none of them alone would exercise every row.

### Open questions for lv

1. **The `sse` view's body key and enum variant**: `{"sse": [...]}` with a new `MediaResponseBodyV1::Sse` (breaking for
   an exhaustive match), or the list under the existing `{"json": …}` key (no new variant, but a component could not tell
   the forms apart from the view alone)? Recommendation: keep `sse`.
2. **Image and `sse`**: should `parse_prepared_image_call_v1` refuse `response_body_form: sse` before dispatch, rather
   than the host judging every 2xx `unknown` afterwards? Not changed here (least invasive; no image package declares it).
   Recommendation: refuse it at parse in the next image change.
3. **`metering_sample` for characters** (interpretation 6): confirm that a package reporting upstream characters must
   also show the request fallback.
4. **A synthetic reference chosen by model prefix** (above): acceptable for the suite's own reference, with S-S-4's
   packages each passing the suite on transcribed samples?

### Versions (Q47)

`south-contracts` (the `sse` view) and `south-component-conformance` (the suite) both change; both already moved in this
unreleased cycle (S-S-1: 0.55.0 and 0.63.0; `git diff v0.63.0 HEAD` shows the moves) and no release has carried them
since, so neither moves again. `south-provider-api` and `south-host-grammars` do not change. The packages took this
release's patch in S-S-1 and take no second one. No `compatibility.json` entry (S-S-5).

### Resolutions (2026-10-14, S-S-3)

None of the four S-S-3 questions needed a new ruling; each is settled by precedent or by the fail-closed discipline:

- **Question 1, the `sse` body key:** `{"sse": [...]}` with `MediaResponseBodyV1::Sse` stays. It is an implementation
  choice the record left open; a separate variant keeps a component from reading an event list as a JSON body.
- **Question 2, image `response_body_form: sse`:** the image world will refuse it in `parse_prepared_image_call_v1`,
  before dispatch, in a separate image change. Today a host would dispatch such a call and then judge every 2xx
  `unknown`, parking the funds: a defect, even though no image package declares `sse`. It is fixed outside this speech
  step so that the speech change stays speech-only.
- **Question 3, `metering_sample` and the two character sources:** kept as implemented, following §11's "counted
  separately for each of its two sources". The rule is provisional and is re-checked against the real packages of
  S-S-4.
- **Question 4, a synthetic reference:** accepted. The `t21-unseen-*` guests are the precedent for a test double that
  no host routes to. Choosing the shape by model-id prefix is confined to it and documented as test-double-only.

## Revision note (2026-10-14)

- S-S-1 implementation section added: what landed, fourteen interpretations, two points that contradict the record
  (media type parameters; the `sse` view's shape and step), four open questions for lv, and the Q47 versions. No design
  decision changed.
- 2026-10-14: the four S-S-1 open questions resolved (Resolutions): 1 and 4 by precedent and as provisional; 2 (`sse` view
  as one array document per §6, built in S-S-3) and 3 (speech media type parameters kept) ruled by lv as recommended.
- S-S-2 implementation section added: what landed (the WIT, the `SPEECH_*` constants and schema row, the `validate_role`
  branch, gate ① admitting the value channel and `query_parameters`, the runtime world, the shared
  `UndeclaredValuesIgnored` row), nine interpretations, one point that contradicts §17.3 (a declared query parameter
  cannot yet travel on a media descriptor) and two open questions for lv. Under Q47 `south-provider-api` moves 0.56.0 →
  0.57.0; the packages already took this release's patch in S-S-1. No design decision changed.
- 2026-10-14: the two S-S-2 open questions resolved (Resolutions): declared query parameters on a media descriptor as
  `contracts.media` 3 with the ElevenLabs package (S-S-4); `artifact_fetch` without `synthesize` refused.
- S-S-3 implementation section added: what landed (the `sse` response view as one elided array document, the
  host-obligation vectors for the WAV container and the `sse` view, the speech trait, outcome, codec, ABI and sandbox
  seams, the fixture loader, `south.speech-component.v1` with six additive `CheckV1` variants, and the synthetic native
  reference with its pack, guest and mutation tests), sixteen interpretations, two points that contradict the §18 row's
  wording and four open questions for lv. No crate version moves (Q47: both changed crates already moved in this cycle).
  No design decision changed.
- 2026-10-14: the four S-S-3 open questions settled without a new ruling (Resolutions, S-S-3); the image world's
  refusal of `response_body_form: sse` before dispatch is a separate image change.
- 2026-10-14: the S-S-4 batch V3-1 design (OpenAI-compatible, xAI, Groq and ElevenLabs TTS, and `contracts.media` 3) is
  proposed in [`2026-10-14-speech-v3-1-tts.md`](2026-10-14-speech-v3-1-tts.md). No design decision of this record changed.
- 2026-10-14: batch V3-1 implemented on branch `speech-v3-1` ([V3-1 record](2026-10-14-speech-v3-1-tts.md),
  Implementation): `speech-openai-compatible`, `speech-xai`, `speech-groq` and `speech-elevenlabs` 1.0.0 and
  `contracts.media` 3. Until S-S-5 records `contracts.speech`, the packages declare `contracts` `{media: 1}` (or 3)
  only, as the synthetic guest does (S-S-3 interpretation 16). No design decision of this record changed.
- 2026-10-14: the S-S-4 batch V3-2 design (MiMo and MiniMax TTS) is proposed in
  [`2026-10-14-speech-v3-2-tts.md`](2026-10-14-speech-v3-2-tts.md). No design decision of this record changed.
- 2026-10-14: the S-S-4 batch V3-3 design (OpenAI-compatible, xAI, ElevenLabs and Azure ASR; `transcribe` added to three
  V3-1 packages per SV-Q1 (b) and a new `speech-azure`) is proposed in [`2026-10-14-speech-v3-3-asr.md`](2026-10-14-speech-v3-3-asr.md).
  No design decision of this record changed.
- 2026-10-14: batch V3-2 implemented on branch `speech-v3-2` ([V3-2 record](2026-10-14-speech-v3-2-tts.md),
  Implementation): `speech-mimo` and `speech-minimax` 1.0.0 (`contracts` `{media: 1}` until S-S-5), with the `MiniMax`
  `base_resp` table shared with `image-minimax`; SV-Q7 as ruled. No design decision of this record changed.
- 2026-10-15: the S-S-4 batch V4 design (Bailian Qwen-TTS as a new `speech-qwen`, Vertex Gemini TTS as a new
  `speech-vertex`, Azure SSML TTS added to `speech-azure` per SV-Q1 (b); Q7 restated as two package versions; SV-Q10 on the
  `http://` audio URL and SV-Q11 on `ssml_escaping`) is proposed in [`2026-10-15-speech-v4-tts.md`](2026-10-15-speech-v4-tts.md).
  No design decision of this record changed.
- 2026-10-15: batch V3-3 implemented on branch `speech-v3-3` ([V3-3 record](2026-10-14-speech-v3-3-asr.md),
  Implementation): `transcribe` added at 1.0.0 to `speech-openai-compatible` (with `translate`), `speech-xai` and
  `speech-elevenlabs`, and `speech-azure` 1.0.0 (`transcribe` only); SV-Q8 and SV-Q9 as ruled. No design decision of
  this record changed.
- 2026-10-15: batch V4 implemented on branch `speech-v4` ([V4 record](2026-10-15-speech-v4-tts.md), Implementation):
  `speech-qwen` and `speech-vertex` 1.0.0, SSML synthesis added to `speech-azure` 1.0.0; SV-Q10 (`upgrade_insecure` on the
  `url` form, inside `contracts.speech` 1) and SV-Q11 (`ssml_escaping` reading, `ssml_check.rs` held to XML 1.0 `Char`) as
  ruled. No design decision of this record changed.
