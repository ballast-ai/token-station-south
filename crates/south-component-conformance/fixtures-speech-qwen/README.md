# `fixtures-speech-qwen`

Gate ② pack (`south.speech-component.v1`) of `speech-qwen` (family `bailian-qwen-speech`), speech world batch V4
(`docs/design/2026-10-15-speech-v4-tts.md` §2, §5). The manifest that names this pack is
`components/speech-qwen/manifest.json`.

Every expectation was written by hand from the record and from token-station-server's native arm `TtsUpstream::Bailian`
(`handler/audio/tts.rs:526-547`, `:1059-1082`; `tts_providers.rs:638-746`), read at `origin/dev-v2`, before the reference
first ran:

- `prepare.native` and `response.native` transcribe the host test
  `bailian_qwen3_tts_fetches_the_audio_url_and_bills_the_upstream_characters` (`gateway/tests/media_arms.rs:350`): `POST
  api/v1/services/aigc/multimodal-generation/generation` with `bearer`, `{"model": "qwen3-tts-flash", "input": {"text":
  "hello world", "voice": "Cherry"}}`; the answer's `output.audio.url` is a `url` artifact delivered as `audio/wav` and
  the upstream's 7 characters are billed, not the local 11. The host test serves the audio from `http://127.0.0.1` with
  its private-destination escape hatch; the pack carries the real shape instead, an `http://` OSS presigned link
  (`tts_providers.rs:736-746`, the comment of 03 #96), so the artifact asks for SV-Q10's `upgrade_insecure` and the
  suite's `second_hop_artifact` row judges the upgraded `https://` URL. `response.https-url` is an `https://` link.
- `prepare.voice-*`, `prepare.format-*`, `prepare.language-type-*` and `prepare.instructions-*` transcribe
  `resolve_qwen_voice`, `qwen_tts_audio_format` and `build_qwen_tts_body` with their unit tests
  (`handler/audio/mimo_tts_tests.rs:261-296`, `:396-428`): the eleven `OpenAI` names fold to `Cherry` in any case, `""`
  / absent / a non-string give `Cherry`, `" "` is sent as is, a Qwen id verbatim; `wav` / `mp3` / absent are served (as
  WAV) and every other string is refused with native's text naming the lower-cased value; `language_type` forwarded
  when a string; `instructions` only on an upstream id containing `instruct`. Elided values follow V3-1 interpretation
  4 (`voice`, `language_type` forwarded as `as_is` references; `response_format` and `input` refused).
- `prepare.input-hangul`, `prepare.input-mixed`, `prepare.input-accented`: the weighted `upper` (ASCII 1, any other scalar
  2; `tts.rs:297-303`). Hangul stands in for Chinese, which the language gate refuses in tracked files.
- Responses (record §2.2): `usage.characters` positive (upstream), absent / `null` / `usage` absent (the request's
  count), above the weighted `upper` but within the input's bytes (succeeded), and `0`, negative, fractional, `7.0`, a
  string and an object (`unknown`, SV-Q7), judged before the URL; a 2xx carrying a non-empty string `code` (`unknown`
  with a 502 envelope, I3-1 decision 8; the body is the one `mimo_tts_tests.rs:438-449` uses), with and without a URL,
  and a `DataInspection` code; an empty `code` ignored; a non-object body and a body that is not JSON; the URL missing,
  `output` missing, a non-string URL and `""` (`unknown`, sidecar `audio`); 401, 403, 404, 408, 429, 500 and 302, plus
  400 with a `DataInspection` code (`content_policy`), a message alone and another shape, read with the `DashScope`
  `{"code", "message"}` reader `image-qwen` uses, else the body text.

Transcription (1.1.0, `docs/design/2026-10-21-speech-qwen-asr.md`) adds the `asr-*` cases, the
`translate-refused` case (which replaces 1.0.x's `transcribe-refused`) and the `tts-and-asr-models` capabilities
case; `capabilities.models` now lists `transcribe` and `seconds` for every model. Their expectations were written by
hand from Alibaba Cloud Model Studio's Qwen-ASR API reference (the DashScope synchronous call, read 2026-10-21) before
the reference first ran, and from the record's decisions:

- `prepare.asr-native` and `response.asr-native`: the documented request (`{"model", "input": {"messages": [{"role":
  "user", "content": [{"audio": …}]}]}}`) with the audio as a `data_url` reference to the `file` part's blob, and the
  documented non-streaming answer (`output.choices[0].message.content[0].text`, `usage.seconds` 1, the `audio_info`
  annotation), delivered as `{"text"}` and billed 1 reported second. The server-side `seconds` is the documented integer.
- `prepare.asr-language-prompt-itn`, `asr-itn-false`, `asr-language-and-prompt-empty`, `asr-itn-invalid-refused`:
  `language` and `enable_itn` into `parameters.asr_options`, `prompt` as the system message's text, `temperature`
  dropped, empty values not sent.
- `prepare.asr-media-type-*`, `asr-no-filename-refused`: the part's audio / video type (lower-cased), else the file
  name's extension; anything else refused.
- `prepare.asr-largest-accepted` / `asr-too-large-refused`: the data URL bound of 10,000,000 bytes (`data:audio/wav;
  base64,` is 22 bytes, so 7,499,982 bytes fit and 7,499,983 do not).
- `prepare.asr-format-*`: `json` and `text` served; `verbose_json`, `srt`, `vtt` refused (no timestamps), an unknown or
  padded value refused. `asr-stream-refused`, `asr-missing-file`, `asr-not-multipart`, `asr-elided-text-part` are the
  shared recognition refusals; `translate-refused` and `asr-tokens-refused` the operation and metering refusals.
- Responses: `usage.seconds` 0 and positive integers are reported; absent, `null` and `usage` absent are `unknown`
  (sidecar `seconds`, no fallback); fractional, whole float, negative, a string and an object are `unknown` (malformed);
  a missing or non-string transcript, an empty `choices` and a body that is not an object are `unknown`; an empty
  transcript is delivered; a 2xx with `code`, and 400 / 401 / 429 / 500, read as for synthesis.

Measurements still to make (record §11 item 1): one capture of a success (the URL's scheme, host, port and length;
`usage.characters` for an ASCII, a Hangul and a mixed input, to confirm the weighting), one error body, and whether any
2xx carries `code`.

Transcription measurements still to make (ASR record §6): one captured success (is `usage.seconds` always present, and how
is a sub-second clip rounded), one captured error body, the exact size bound (MB or MiB, and whether it counts the
`data:` prefix), and the effect of a system message built from `prompt`.
