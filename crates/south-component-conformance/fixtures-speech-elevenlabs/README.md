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
