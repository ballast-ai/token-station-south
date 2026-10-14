# `fixtures-speech-groq`

Gate ② pack (`south.speech-component.v1`) of `speech-groq` (family `groq-speech`), speech world batch V3-1
(`docs/design/2026-10-14-speech-v3-1-tts.md` §4, §12). The manifest that names this pack is
`components/speech-groq/manifest.json`.

Every expectation was written by hand from the record and from token-station-server's native arm before the
reference first ran:

- `prepare.native-english` and `response.audio` transcribe the host test
  `groq_orpheus_tts_maps_the_voice_to_a_persona_and_bills_local_characters` (`gateway/tests/media_arms.rs:979`): the
  OpenAI body with `model` the upstream id and `alloy` mapped to the English persona `autumn`, posted to
  `v1/audio/speech`; the `audio/wav` answer is relayed and 11 characters are billed.
- `prepare.voice-*` and `prepare.arabic-*` cover each class of `resolve_orpheus_voice` / `map_openai_voice_to_orpheus`
  (`tts_providers.rs:334-368`): a persona in another case, mapped OpenAI names, a trimmed value, an unknown value kept
  as trimmed with its case, blank, absent and non-string (the default persona), and the Arabic set chosen by an
  upstream id containing `arabic`.
- `prepare.members-forwarded`: every other member goes as sent, `stream_format` and `response_format` included (native
  has no format handling on this arm); `prepare.elided-member-forwarded` writes an elided member back as an `as_is`
  reference.
- `prepare.input-*`, `prepare.tokens-refused` and the responses: the common rows (record §1, §12). Groq's error body is
  the OpenAI shape (**unverified**); a JSON body of another shape gives its compact text.
