# `fixtures-speech-mimo`

Gate ② pack (`south.speech-component.v1`) of `speech-mimo` (family `mimo-speech`), speech world batch V3-2
(`docs/design/2026-10-14-speech-v3-2-tts.md` §2, §8). The manifest that names this pack is
`components/speech-mimo/manifest.json`.

Every expectation was written by hand from the record and from token-station-server's native arm before the
reference first ran:

- `prepare.native-mp3` and `response.audio` transcribe the host test
  `mimo_tts_speaks_chat_completions_with_an_audio_object_and_bills_local_characters` (`gateway/tests/media_arms.rs:923`):
  `{input: "hello world", voice: "alloy", response_format: "mp3"}` posts exactly
  `{"model": "mimo-v2.5-tts", "messages": [{"role": "assistant", "content": "hello world"}], "audio": {"format": "wav",
  "voice": "mimo_default"}, "stream": false}` to `v1/chat/completions`; the base64 audio of `choices[0]` is delivered as
  `audio/wav` and 11 characters are billed from the request, the chat `usage` taking no part.
- `prepare.format-*` transcribe `mimo_tts_audio_format` and its unit tests (`handler/audio/mimo_tts_tests.rs:9-60`,
  `tts_providers.rs:491-504`): absent, `wav` and `mp3` in any case are `wav` (`audio/wav`), `pcm` and `pcm16` are
  `pcm16` (`audio/pcm`), `opus` / `aac` / `flac` and any other string refused with the native texts naming the
  lower-cased value, a non-string absent, an elided one refused (V3-1 interpretation 4).
- `prepare.voice-*`, `prepare.clone-*`, `prepare.preset-*` and `prepare.design-*` cover `resolve_mimo_voice`, the model
  classes and `build_mimo_tts_body` (`mimo_tts_tests.rs:62-200`, `tts.rs:483-518`): an OpenAI name folds to
  `mimo_default` on the preset model only; a clone voice colliding with an OpenAI name stays verbatim; `""`, absent and
  non-string voices are `mimo_default`; `" "` is sent untrimmed; the user message is `instructions` when not blank
  (sent untrimmed), else on the design model the voice as sent, and is omitted when blank; a design model's `audio` has
  no `voice`; a design model with no description is dispatched (decision 3); elided values are `as_is` references.
- `prepare.extra-members-dropped`, `prepare.input-*`, `prepare.tokens-refused`: every other client member is dropped;
  the common refusals (V3-1 §1).
- Responses: audio in both formats, the first of two choices, an empty, missing, `null` or non-string audio string, an
  empty `choices`, `choices` that is not an array (an object keyed `"0"`: native's `get(0)` finds no choice), a JSON
  body that is not an object (`unknown`), a body that is not JSON (the host's `unknown`), and 400, 401, 403, 404, 408,
  429, 500 and 302. MiMo's error shape is taken to be OpenAI's `{"error": {"message"}}` (**unverified**); another JSON
  shape gives its compact text.
