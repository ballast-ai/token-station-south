# `fixtures-speech-xai`

Gate ② pack (`south.speech-component.v1`) of `speech-xai` (family `xai-speech`), speech world batch V3-1
(`docs/design/2026-10-14-speech-v3-1-tts.md` §3, §12). The manifest that names this pack is
`components/speech-xai/manifest.json`.

Every expectation was written by hand from the record and from token-station-server's native arm before the
reference first ran:

- `prepare.native-wav` and `response.audio` transcribe the host test `xai_tts_posts_v1_tts_with_the_mapped_voice_and_bills_local_characters`
  (`gateway/tests/media_arms.rs:818`): `{input: "hello world", voice: "alloy", response_format: "wav"}` posts
  `{"text", "voice_id": "eve", "language": "en", "output_format": {"codec": "wav", "sample_rate": 24000}}` to `v1/tts`,
  the `audio/wav` answer is relayed and 11 characters are billed from the request.
- `prepare.format-*` transcribe `handler/audio/xai_tts_format_tests.rs` and `map_openai_response_format_to_xai`
  (`tts_providers.rs:266-300`), refusal texts included (the refused value is the lower-cased one, as native).
- `prepare.voice-*` cover each class of `map_openai_voice_to_xai` (`tts_providers.rs:302-324`): an xAI name in another
  case, mapped OpenAI names, an unknown value and `""` verbatim, absent and non-string (`eve`), and an elided voice
  forwarded as an `as_is` reference.
- `prepare.output-format-*`: a client `output_format` is forwarded verbatim, `null` included, and `response_format` is
  then ignored (`tts.rs:457-458`; record §9 item 7, reproduced).
- `prepare.input-*` and `prepare.tokens-refused`: the common refusals (record §1, §12).
- Responses: the common binary table (record §1): audio typed by `content-type` (absent or invalid → `audio/mpeg`;
  parameters kept; a quoted parameter falls back to the essence), an empty 2xx (`unknown`, sidecar `audio`), and 401,
  403, 404, 408, 429, 500 and 302. xAI's error body `{"code", "error"}` gives `error` as the provider message
  (**unverified** against a capture); a JSON body of another shape gives its compact text.

## Recognition (speech world batch V3-3)

`docs/design/2026-10-14-speech-v3-3-asr.md` §3, §10. The `stt-*` cases and the updated `capabilities.models` (every
configured model now also declares `transcribe` and `seconds`; `speech.translate` on a row adds nothing, xAI does not
translate) were written by hand from the record and from the native arm `AsrUpstream::Xai` (`handler/audio.rs:94-95`,
`:203-208`, `:278`, `:559-564`) before the reference first ran:

- `prepare.stt-native` and `response.stt-json-duration` transcribe the host test
  `xai_stt_strips_the_model_field_posts_v1_stt_and_bills_the_reported_duration` (`media_arms.rs:871`): the `model`
  part removed, `v1/stt`, the answer passed through and 15 s billed; `response.stt-json-no-duration` transcribes
  `:1401` (`seconds: null`). Both answers are the host's hand-written ones, not captures (**unverified**). Every
  `model` part is removed, a value containing `name="model"` is not touched (§7 D1).
- `usage.seconds` is not read (`stt-json-usage-seconds-not-read`).
- Verbose rows use a configuration whose `grok-stt` row carries `speech.verbose_json`; whether `/v1/stt` accepts
  `response_format` and returns `segments` is **unverified** (record §9 item 3), and the capture decides whether any
  real row carries the word. `stt-verbose-*` render one answer in every client format.
- Translation is refused before dispatch (`capability`). The common refusals and the non-2xx table with the
  `{"code", "error"}` reader as for synthesis.
