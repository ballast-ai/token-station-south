# `fixtures-speech-synthetic`

Gate ② pack (`south.speech-component.v1`) of the speech suite's native reference, `reference_synthetic_speech`
(`SyntheticSpeechReferenceV1`), speech world step S-S-3 (`docs/design/2026-09-30-speech-world.md` §11, §18; the
record's "S-S-3 implementation" section says why the reference is synthetic). The manifest that names this pack is
`tests/guests/synthetic-speech/manifest.json`, the guest that runs the same reference in the sandbox.

**Nothing here is a transcription of a real upstream.** The family `synthetic-speech` and its API (`v1/speech`,
`v1/speech/inline`, `v1/speech/url`, `v1/speech/ssml`, `v1/audio/transcriptions`, `v1/audio/translations`) are a test
double whose models each take one shape the record describes, so that one pack exercises every row. The real packages
and their native-arm samples are step S-S-4. The expectations were written by hand from the reference's module
documentation and the record before the reference first ran, and the reference passed every one of them on that first
run, unchanged.

Models (from the id's prefix, a synthetic convenience no real package may use):

| Model | Shape |
|---|---|
| `tts-stream-1` | JSON request; `binary` body (characters from the request), or, when the host requires `tokens`, `stream_format: "sse"` with `segments` over the `speech.audio.delta` events and `tokens` from the `speech.audio.done` event |
| `tts-inline-1` | `inline` audio at `/audio/data`: `hex` mp3, `base64` PCM wrapped as WAV (`container`), or raw PCM as `audio/L16; rate=24000`; `characters.upper` counts non-ASCII twice; characters from `usage.characters`, else from the request |
| `tts-url-1` | `url` artifact at `/output/audio/url` (the second hop; the manifest declares `artifact_fetch`) |
| `tts-ssml-1` | SSML text body with the input and both attribute values escaped; `binary` response |
| `asr-verbose-1` | row words `speech.translate` and `speech.verbose_json`: always asks for `verbose_json` and renders every client format itself from one response; translation allowed |
| `asr-basic-1` | no dialect words: forwards `json` or `text`, bills `usage.seconds` when the JSON has it, `seconds: null` otherwise; translation refused |

Rows, by case:

- Capabilities: the six models (`capabilities.models`); another family refused; a misspelt `speech.` word refused.
- Prepare: each shape's request; refusals before admission (`stream-unsupported-format`, `stream-missing-input`,
  `stream-elided-input`, `unknown-model`, `inline-tokens-refused`, `url-mp3-refused`, `ssml-missing-voice`,
  `basic-srt-refused`, `basic-translate-refused`, `recognition-missing-file`). `ssml-escaping` carries `<`, `&`, `'`,
  `"` (and `>`) in both the input and the voice; `ssml-short-voice` takes the default `xml:lang`. The suite also asks
  each prepared case for every operation its model does not declare (`undeclared_operation_refused`).
- Responses: `binary` (`stream-binary`, `ssml`), `sse` (`sse-usage`), `json` inline and URL artifacts, and transcripts
  in each client format from one `verbose_json` answer (`verbose-*`, all billed 3.52 s). Sidecars (`*.meta.json`,
  `{"missing": …}`): the whole `usage`, each token count, the duration (`verbose-missing-duration` ends `unknown`,
  `basic-json-no-duration` succeeds with `seconds: null`) and no audio for each response body form
  (`stream-binary-empty`, `sse-no-deltas`, `inline-empty`, `url-missing`). Non-2xx: a 401, a 429, a 503 and a 400.
- Metering samples: characters from the request and from the upstream, seconds, tokens.
