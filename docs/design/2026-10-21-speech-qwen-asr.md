# `speech-qwen` 1.1.0: Qwen-ASR transcription

Status: proposed; implemented on branch `feature/speech-qwen-asr` (Implementation, below). The interpretations in §7
were made for the owner (lv) and may be overturned.

Date: 2026-10-21

Baseline: south `origin/main` = `0f1ebf8f` (release 0.75.0). Upstream statements come from Alibaba Cloud Model Studio's
"Qwen-ASR API reference" (`https://www.alibabacloud.com/help/en/model-studio/qwen-asr-api-reference`, DashScope
synchronous section), read on 2026-10-21. Nothing here was captured from live traffic; §6 lists what still has to be
measured.

Parent records: [`2026-09-30-speech-world.md`](2026-09-30-speech-world.md) (the speech world: operations, facts,
metering, delivery), [`2026-10-14-speech-v3-3-asr.md`](2026-10-14-speech-v3-3-asr.md) ("V3-3": the shared recognition
pieces in `asr_shared`, the duration value rule, the client formats) and
[`2026-10-15-speech-v4-tts.md`](2026-10-15-speech-v4-tts.md) ("V4": `speech-qwen` 1.0.0, the `DashScope` readers, and
`speech-azure` gaining a second operation in one package, which this change follows).

## 1. Scope

`speech-qwen` (family `bailian-qwen-speech`) has served Qwen-TTS (`synthesize`) since 1.0.0. 1.1.0 adds
`transcribe` for Qwen-ASR (`qwen3-asr-flash`) so a host's `/v1/audio/transcriptions` (multipart, the `OpenAI` audio
shape) can reach it. Synthesis is unchanged: every synthesis fixture is byte-identical and its state keeps its shape.
`translate` is not declared. The change is a package behaviour change only: no contract, WIT or runtime change.

## 2. Upstream

- `POST api/v1/services/aigc/multimodal-generation/generation` under the row endpoint (Singapore:
  `https://{WorkspaceId}.ap-southeast-1.maas.aliyuncs.com`; the older `dashscope-intl.aliyuncs.com` /
  `dashscope.aliyuncs.com` origins are documented as still working), `Authorization: Bearer`, the same path and auth as
  Qwen-TTS.
- Body: `{"model", "input": {"messages": [{"role": "system", "content": [{"text": <context>}]}?, {"role": "user",
  "content": [{"audio": <URL or data URL>}]}]}, "parameters": {"asr_options": {"language"?, "enable_itn"?}}}`. The
  system message "provides context for speech recognition, such as background text and entity glossaries" and "does not
  support setting model role or other traditional system prompts"; it must come first. `enable_itn` defaults to `false`.
- Audio by data URL: `data:<mediatype>;base64,<data>`, the media type "varies by audio format, for example: WAV:
  audio/wav MP3: audio/mpeg". "Keep the source file small enough that the encoded result still meets the input audio
  size limit (10 MB)". Recordings "up to 5 minutes long".
- Answer (non-streaming): `output.choices[0].message.content[0].text` (the transcript) with
  `message.annotations[{type: "audio_info", language, emotion}]`; `usage` has `input_tokens_details`,
  `output_tokens_details` and `seconds` ("The audio duration … in seconds", an integer); `request_id`. Qwen-ASR is
  priced per second.
- Errors: the `DashScope` `{"code", "message", "request_id"}` body, as for Qwen-TTS.
- Streaming exists (`X-DashScope-SSE`, `incremental_output`); the synchronous answer carries no timestamps.

## 3. Design

### 3.1 Request

`prepare` with `operation = transcribe` reads the host's multipart view (`asr_shared::request_parts`: a body that is
not multipart, an elided text part, no file part named `file`, or `stream` = `true` / `1` is refused before dispatch,
as for every recognition package). The upstream request is a JSON body built with the synthesis path's
`speech_shared::descriptor`:

```json
{"model": "<upstream id>",
 "input": {"messages": [
   {"role": "system", "content": [{"text": "<prompt>"}]},
   {"role": "user", "content": [{"audio": {"$south.ref": {"blob": "<file blob>", "transform": "data_url", "media_type": "<type>"}}}]}]},
 "parameters": {"asr_options": {"language": "<language>", "enable_itn": true}}}
```

The audio is a reference to the `file` part's blob with the `data_url` transform, a media contract 1 word, which the
host expands to `data:<type>;base64,<payload>` (`expand_json_template_v1`). The component never sees the bytes, only
the part's size. The system message and `parameters` appear only when they carry something. The immutable body paths
are `model` and `input` (dotted, the contract's grammar), so request extras may still set
`parameters.asr_options.*`. The state is `{"op": "transcribe", "format": <client format>}`; the synthesis state stays
`{"characters": n}`, and `parse_response` tells them apart by `op`.

### 3.2 Text parts

- `language`: sent as given in `asr_options.language` when non-empty.
- `prompt`: the system message's text when non-empty (an approximation, §7 item 3).
- `enable_itn` (not an `OpenAI` field): `true` / `false`, trimmed, any ASCII case, into `asr_options.enable_itn`;
  empty is not sent; any other value is refused.
- `model`, `temperature`, `response_format`, `stream` and every other part are not sent.

### 3.3 Audio media type

The part's own media type when it is `audio/*` or `video/*`, lower-cased. When the part has none or
`application/octet-stream`, the file name's extension, lower-cased: `aac` `audio/aac`, `amr` `audio/amr`, `flac`
`audio/flac`, `m4a` `audio/mp4`, `mp3` / `mpeg` / `mpga` `audio/mpeg`, `mp4` `video/mp4`, `ogg` `audio/ogg`, `opus`
`audio/opus`, `wav` `audio/wav`, `webm` `audio/webm`. Anything else (another media type, no file name, an unknown
extension) is refused before dispatch with 400.

### 3.4 Bounds

- The data URL may be at most 10,000,000 bytes: `len("data:<type>;base64,") + 4 * ceil(bytes / 3)`, refused before
  dispatch above it (§7 item 2). The expanded JSON body is far below the 32 MiB request bound.
- The facts reserve `max_seconds = 300` (the documented 5-minute cap); the host reserves the smaller of its own bound
  and this one.

### 3.5 Client formats

`response_format` compared exactly: absent or `json` is `{"text": …}`, `text` the plain transcript
(`text/plain; charset=utf-8`). `verbose_json`, `srt` and `vtt` are refused before dispatch: the synchronous answer
carries no segment or word timestamps to render them from. Any other value (including a padded one) is refused.

### 3.6 Response

The body form is `json`. In order:

1. A non-2xx reads the `DashScope` `{"code", "message"}` body with the reader `image-qwen` and Qwen-TTS share
   (`dashscope_error`), `rejected` for a 4xx other than 408, `unknown` otherwise.
2. A 2xx whose body is not a JSON object is `unknown`.
3. A 2xx carrying a non-empty string `code` is `unknown` with a 502 `provider_protocol_error` envelope (V4's rule).
4. Seconds: `usage.seconds` when it is a non-negative integer, `reported`. Absent, `null` or `usage` absent is `unknown`
   ("the upstream did not report a duration"); any other value (fractional, a whole float, negative, a string, an
   object) is `unknown` ("a malformed duration"). There is no fallback duration: the host parks the call for manual
   review rather than billing a guess (§7 item 1).
5. The transcript at `output.choices[0].message.content[0].text`: a string, `""` included, else `unknown` ("a 2xx
   without a transcript"). A transcript the host elided (above 1 MiB) is an `as_is` reference in `json` and `unknown`
   in `text`, the V3-3 rule.

## 4. Operations and capabilities

Following `speech-azure` (V4 §4), every configured model declares both operations, `synthesize` and `transcribe`, with
the forms `characters` and `seconds` (`asr_shared::capabilities`); which rows reach the package for which operation is
the host's route and its modality check, not a list of model ids in the package. `translate` is never declared, so a
`translate` call is refused with `capability`. A transcription requires the host to ask for `seconds` alone
(`asr_shared::recognition_forms`).

## 5. Versions and compatibility

- `speech-qwen` 1.0.10 → 1.1.0 (a minor: a new operation). The manifest adds the capability `transcribe`.
- `south_runtime` stays 0.65.0 and `contracts` stay `{media: 1, speech: 1}`: `data_url` is a media contract 1 word, the
  JSON body, `reservation.max_seconds` and the transcript deliveries are all in speech contract 1, and gate ① of
  0.65.0 admits `transcribe` (CONTRIBUTING: a package declares the oldest runtime it needs).
- The change edits `south-component-conformance` (the reference and its pack), one of the three crates every component
  links. CONTRIBUTING then asks for that crate's version bump and a version bump of every package. That bump is **not**
  made here: two other workstreams change the same crates in parallel and the integrator makes one combined bump. Until
  then these checks are expected to fail: `scripts/check_crate_versions.py --unreleased` (the crate changed without a
  version move), the release's digest-stability check (every package's `component.wasm` changes with the crate), and
  `shipped_packages_v1::kiro_images_retires_every_published_074_package_identity` (it pins `speech-qwen` at 1.0.10,
  the 0.75.0 round). The integration commit made the bump: `south-component-conformance` 0.74.0 → 0.75.0, every
  other package the Q47 patch (`shipped_packages_v1::bailian_retires_every_published_075_package_identity`).

## 6. Open points and measurements

1. **MB or MiB.** The documentation says "10 MB" for the encoded data. 10,000,000 bytes is taken as the bound (the
   smaller reading), counting the whole data URL including its prefix (the stricter reading). A capture near the bound
   would settle both.
2. **`usage.seconds`.** Documented as an integer audio duration. Unknown: whether it is always present on success, and
   how a clip shorter than a second is rounded (0 is accepted and billed as 0).
3. **`prompt` as context.** The system message is recognition context (glossaries, background text), not an
   instruction; the `OpenAI` `prompt` is close but not the same thing.
4. **No timestamps.** The synchronous API offers no segments, so `verbose_json`, `srt` and `vtt` cannot be served
   without inventing timings. A later version could use the asynchronous file transcription API if timestamps are
   wanted.
5. One captured error body, to confirm the `DashScope` shape on this model.

## 7. Interpretations for lv's review

1. Seconds come only from `usage.seconds` (exact, reported); missing or malformed is `unknown`, not a 60-second (or any
   other) fallback.
2. The size bound is 10,000,000 bytes over the whole data URL (§6 item 1).
3. `prompt` maps to the system message; `temperature` is ignored; `language` passes through as given (no validation of
   the language list).
4. `enable_itn` is read from a text part of that name (a non-`OpenAI` extension), strict `true` / `false`.
5. A part media type that is not `audio/*` or `video/*` (for example `image/png`) is refused rather than sent; the
   media type is lower-cased.
6. `json` and `text` only; `verbose_json`, `srt`, `vtt` and streaming refused with 400.
7. Every model declares both operations (the `speech-azure` precedent), not a split by model id.
8. The immutable paths are `model` and `input` (dotted form of `/model`, `/input`), leaving `parameters.asr_options.*`
   open for operator extras.

## 8. Host note

A host's recognition executor that only sends multipart bodies has to send this package's JSON body; that is a generic
host change (the body kind follows the descriptor) and is not part of this change. The package passes gate ② on its
own.

## Implementation

- `crates/south-component-conformance/src/reference_qwen_speech.rs`: `prepare` routes non-synthesis operations to the
  transcription path; `parse_response` routes on the state's `op`; capabilities via `asr_shared::capabilities`.
- `fixtures-speech-qwen/`: the `asr-*` cases, `translate-refused` (replacing `transcribe-refused`), the
  `tts-and-asr-models` capabilities case, and `capabilities.models` updated; README updated.
- `tests/qwen_speech_suite.rs`: the pack's rows now include the seconds metering and missing-duration rows; the V3-3
  recognition breaks; behaviour tests for the data URL expansion, text parts, media types, the bound, client formats,
  seconds and rendering; a guessed-duration mutation that turns the missing-duration cases red.
- `components/speech-qwen`: 1.1.0, capability `transcribe`.
