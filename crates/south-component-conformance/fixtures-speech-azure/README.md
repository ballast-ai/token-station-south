# `fixtures-speech-azure`

Gate ② pack (`south.speech-component.v1`) of `speech-azure` (family `azure-speech`), speech world batch V3-3
(`docs/design/2026-10-14-speech-v3-3-asr.md` §5, §10). The manifest that names this pack is
`components/speech-azure/manifest.json`.

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
