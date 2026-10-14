# `fixtures-speech-elevenlabs`

Gate ② pack (`south.speech-component.v1`) of `speech-elevenlabs` (family `elevenlabs-speech`), speech world batch
V3-1 (`docs/design/2026-10-14-speech-v3-1-tts.md` §5, §6, §12, SV-Q4). The manifest that names this pack is
`components/speech-elevenlabs/manifest.json`; it declares the query parameter `output_format` (`token`) and
`contracts.media` 3.

Every expectation was written by hand from the record and from token-station-server's native arm before the
reference first ran:

- `prepare.native-default-mp3` and `response.audio` transcribe the host test
  `elevenlabs_tts_on_the_openai_surface_puts_the_voice_in_the_path_and_bills_local_characters`
  (`gateway/tests/media_arms.rs:1463`): `alloy` is Aria, `POST v1/text-to-speech/9BWtsMINqrJLrRacOk9x` with the query
  `output_format=mp3_44100_128`, the secret in `xi-api-key`, body `{"text", "model_id"}`; the `audio/mpeg` answer is
  relayed and 11 characters are billed. Every prepared case carries the declared query, so the suite's host parse with
  the manifest's declarations is exercised on each.
- `prepare.format-*` transcribe `elevenlabs_output_format` (`tts_providers.rs:430-442`): `mp3` in any case or a
  non-string is `mp3_44100_128`; any other string is refused with the native text naming the lower-cased value.
- `prepare.voice-*` cover `resolve_elevenlabs_voice` / `map_voice_name_to_elevenlabs_id` (`tts_providers.rs:370-428`):
  an OpenAI name, a default-voice name in another case, a raw 20-character id (trimmed), blank, absent and non-string
  (the default voice), and SV-Q4's grammar: a value with `/`, `?`, a space, `.` or `..`, non-ASCII or above 128 bytes
  is refused before dispatch (record §9 item 1); 128 unreserved bytes are admitted.
  `prepare.format-and-voice-refused` keeps native's order (format first).
- `prepare.input-*`, `prepare.tokens-refused` and the responses: the common rows (record §1, §12). Error bodies
  (**unverified** against captures): `detail.message`, a string `detail`, or a validation list's first `msg`; a JSON
  body of another shape gives its compact text.

## Recognition (speech world batch V3-3)

`docs/design/2026-10-14-speech-v3-3-asr.md` §4, §10, SV-Q8, SV-Q9. The `stt-*` cases and the updated
`capabilities.models` (every configured model now also declares `transcribe` and `seconds`) were written by hand from
the record and from the native arm `AsrUpstream::ElevenLabs` (`handler/audio.rs:92-93`, `:277`;
`handler/audio/multipart.rs:13-22`, `:456-476`) before the reference first ran:

- `prepare.stt-native` and `response.stt-native` transcribe the host test
  `elevenlabs_asr_renames_fields_posts_speech_to_text_and_bills_the_last_word_end` (`media_arms.rs:1228`): `model` ->
  `model_id` with the upstream id, `language` -> `language_code`, `v1/speech-to-text` with `xi-api-key`, `{"text"}`
  delivered and the last word's end (7.5 s) billed, now with `basis = last_timestamp` (Q12; native recorded it as
  exact). `timestamps_granularity` is set to `word`, replacing a client `none` in place (§7 D2).
- The same answer rendered in every client format (`stt-json`, `stt-text`, `stt-srt`, `stt-vtt`, `stt-verbose-json`):
  its two words span 7.5 s, so the subtitles have two cues (SV-Q8's 7-second bound).
- SV-Q8 cue grouping (constants in `asr_shared.rs`: a cue closes after `.`, `!`, `?` or their full-width forms, before
  a word that starts 1 s or more after the previous one ends, names another `speaker_id`, or would take the cue past
  7 s or 84 characters; `spacing` text joins words inside a cue; `audio_event` entries are skipped):
  `stt-cues-srt` / `-vtt` (punctuation, an audio event, a 1.2 s gap, a speaker change), `stt-cues-84-characters` (ten
  9-character words: eight fit, the ninth would make 89 characters), `stt-hangul-vtt` (an ideographic full stop; Hangul
  stands in for a non-Latin script because the language gate refuses CJK ideographs and punctuation in tracked files,
  so the full stop is written as a JSON escape).
- SV-Q9 `verbose_json`: `{"task": "transcribe", "language": <language_code>, "duration": <last end>, "text", "words":
  [{"word", "start", "end"}]}`, `language` omitted when the answer names none, `duration` omitted when the seconds are
  `null`, `words` empty when the answer has no list.
- Duration (record §4): `words` empty, absent or not a list, and a last element without `end` or with `end: null`,
  give `seconds: null` (sidecar `seconds`; the last three transcribe `media_arms.rs:1424-1460`); `0`, `-1` and a string
  are `unknown`; the last element counts whatever its type (`stt-last-element-spacing`).
- `srt` / `vtt` with no `words` list render an empty document; a list that is not an array, or a word without a usable
  start, end or text, is `unknown`. A missing transcript is `unknown` for `json`, `text` and `verbose_json`, not for the
  subtitles, which are built from the words.
- Translation is refused before dispatch. The common refusals and the non-2xx table with ElevenLabs' error readers as
  for synthesis.

Measurements still to make (record §9 item 4): the default of `timestamps_granularity`, whether an unknown
`response_format` part is ignored, whether any answer carries an explicit duration, and a capture of a silent clip.
