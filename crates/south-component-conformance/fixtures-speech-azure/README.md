# `fixtures-speech-azure`

Gate ② pack (`south.speech-component.v1`) of `speech-azure` (family `azure-speech`), speech world batch V3-3
(`docs/design/2026-10-14-speech-v3-3-asr.md` §5, §10) for recognition and batch V4
(`docs/design/2026-10-15-speech-v4-tts.md` §4, §5) for SSML synthesis, whose cases carry a `tts-` prefix. The manifest
that names this pack is `components/speech-azure/manifest.json`.

Every expectation was written by hand from the record and from token-station-server's native arm
`AsrUpstream::AzureSpeech` (`handler/audio.rs:96-97`, `:281-283`, `:534-551`; `handler/audio/multipart.rs:291-446`) and
its tests before the reference first ran:

- `prepare.native` and `response.native` transcribe the host test
  `azure_speech_asr_transcribes_via_fast_transcription_and_bills_the_reported_duration` (`media_arms.rs:218`): `file`
  -> `audio`, the `model` part dropped, `definition` appended, `speechtotext/transcriptions:transcribe?api-version=
  2025-10-15` with `ocp-apim-subscription-key`; `durationMilliseconds: 30000` and one phrase give
  `{"text": "hi there", "duration": 30.0}` and 30 s billed.
- `prepare.every-dropped-name`: every name of native `AZURE_ASR_DROPPED_FIELDS` (a file part named `model` included),
  a client `definition` (§7 D9: dropped, the component writes that part) and the forwarded parts in order
  (`stream=false` included). `prepare.definition-escaping`: an upstream id with `"` and `\` gives native's
  `serde_json` bytes. `prepare.client-audio-part-kept`: a client file part `audio` is forwarded beside the renamed one
  (§8 P2).
- Client formats (native `azure_asr_response_format`, trimmed): absent, `""`, `json`, ` json `, `verbose_json`, `text`
  and ` text ` are served; `srt`, `vtt` and a padded ` srt` are refused with native's subtitle text (showing the
  trimmed value), `yaml` and `JSON` with native's unknown-format text. A `stream=true` part is refused before the format
  is read.
- Responses transcribe the Azure tests of `handler/audio/strip_multipart_tests.rs`: `durationMilliseconds: 2500` gives 2.5 s in every format
  (`json` `{"text", "duration"}`, `verbose_json` `{"task": "transcribe", "duration", "text"}`, `text` as
  `text/plain; charset=utf-8`); the `duration` fallback; both absent (`unknown`, sidecar `seconds`; native 502 and
  park), `0` (also with a `duration` beside it, as native), negative and string durations (`unknown`); the phrases
  joined by one space with non-string texts skipped; the top-level `text` when the join is empty; `""` when neither.
  Hangul stands in for a non-Latin script (the language gate refuses CJK ideographs in tracked files).
- Non-2xx: V3-1's table; the error body is read in the `OpenAI` shape `{"error": {"message"}}` (Azure Cognitive
  Services', **unverified**), else its text.

Measurements still to make (record §9 item 5): one capture of a fast-transcription answer (the `durationMilliseconds`,
`combinedPhrases` and `phrases` shapes) and one error body, and which of two `definition` parts Azure honours.

## Synthesis (batch V4)

Every `tts-` expectation was written by hand from the V4 record and from native's `TtsUpstream::AzureSpeech`
(`handler/audio/tts.rs:586-614`, `:720-733`, `:1148-1173`; `tts_providers.rs:82-257`) before the reference first ran:

- `prepare.tts-native` and `response.tts-native` transcribe the host test
  `azure_speech_tts_posts_ssml_with_the_subscription_key_and_bills_local_characters` (`media_arms.rs:143`): `POST
  cognitiveservices/v1` with `ocp-apim-subscription-key`, one `application/ssml+xml` text body, `x-microsoft-outputformat`
  `audio-24khz-48kbitrate-mono-mp3`, the SSML `<speak version='1.0' xml:lang='en-US'><voice
  name='en-US-June:MAI-Voice-2'>hello world</voice></speak>` (spelled out literally in every case), `MP3DATA` delivered
  as `audio/mpeg` and the local 11 characters billed. `immutable_body_paths` is `null` (no body extras into SSML).
- Formats (`azure_tts_output_format`, `azure_tts_tests.rs:137-158`), matched exactly: absent / non-string `mp3`, `opus`,
  `wav`, `pcm` with their header token; `MP3`, `flac`, ` wav` and `""` refused with native's text; an elided one refused.
  Responses are labelled by the table whatever the upstream's `content-type` (`opus` under `audio/mpeg`, `wav` with none,
  `pcm` under `application/octet-stream`, an HTML body under `mp3`).
- Voices (`map_openai_voice_to_mai`, `azure_tts_tests.rs:7-99`): roster names, `Alloy` passed through as
  `en-US-Alloy:MAI-Voice-2` (case-sensitive, record §8 item 9), a MAI short name, `en-GB-Sonia`, the full ids
  `fr-FR-Vivienne:MAI-Voice-2` (its `xml:lang` `fr-FR`) and `en-US-June:MAI-Voice-1`, `zh-CN-liaoning-Xiaobei`,
  `Mary-Jane-Extra`, padded, `""`, blank, absent and a non-string (`alloy`), a re-pointed `MAI-Voice-3` row; an elided
  voice refused.
- Escaping (`xml_escape`, `azure_tts_tests.rs:100-122`): the five entities; U+0007 / U+0000 and every other character
  XML 1.0 forbids dropped; tab, line feed and carriage return kept; `prepare.tts-ssml-special-characters` holds `<`, `&`,
  `'` and `"` in both the input and the voice (`speech-v1.ssml.special-characters`, SV-Q11); a short name with `&`.
- `prepare.tts-ssml-above-the-text-body-bound`: an input of 174,747 apostrophes and four letters (well below the
  elision threshold) whose escaped document is 1,048,577 bytes, one above the text body bound, refused before dispatch
  (record §4.2, P1).
- `response.tts-empty-body` (sidecar `audio`): an empty 2xx is `unknown`; non-2xx 400, 401, 403, 404, 408, 429, 500
  and 302 read the `OpenAI` shape's `message`, else the body text (**unverified** for Azure TTS).
- The V3-3 case `prepare.synthesize-refused` is gone: the package now synthesizes on every configured model (the host's
  modality check decides which surface a row serves). The two capability cases now list `synthesize` and `transcribe`
  with `characters` and `seconds`, as does the new `capabilities.voice-and-transcribe-models`.

Measurements still to make (record §11 item 3): a capture with native's SSML (no `xmlns`) and one with it; the
`content-type` returned per format; an error body; whether a `User-Agent` is required.
