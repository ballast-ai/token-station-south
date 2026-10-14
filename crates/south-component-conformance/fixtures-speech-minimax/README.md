# `fixtures-speech-minimax`

Gate ② pack (`south.speech-component.v1`) of `speech-minimax` (family `minimax-speech`), speech world batch V3-2
(`docs/design/2026-10-14-speech-v3-2-tts.md` §3, §8). The manifest that names this pack is
`components/speech-minimax/manifest.json`.

Every expectation was written by hand from the record and from token-station-server's native arm before the
reference first ran:

- `prepare.native-mp3` and `response.audio-upstream-characters` transcribe the host test
  `minimax_tts_posts_t2a_v2_and_bills_the_upstream_usage_characters` (`gateway/tests/media_arms.rs:293`): `POST
  v1/t2a_v2`, `{"model", "text", "stream": false, "output_format": "hex", "voice_setting": {"voice_id": "Wise_Woman"},
  "audio_setting": {"format": "mp3"}}`; the hex `data.audio` is delivered as `audio/mpeg` and the upstream's 9
  characters are billed, not the local 11. `prepare.group-id-sent` and `response.audio-group-id` transcribe
  `minimax_tts_with_a_group_id_puts_the_group_id_query_on_the_wire` (`:519`): the query is exactly `GroupId=1900001234`
  when the config key `group_id` is set. The BYOK tests (`gateway/tests/byok_media_ownership.rs:749-870`) use the same
  body and the bearer form.
- `prepare.settings-*` and `prepare.speed-*` transcribe `build_minimax_tts_body` and its unit tests
  (`mimo_tts_tests.rs:504-614`): the client's `voice_setting` / `audio_setting` objects are forwarded with `voice_id` and
  `format` overwritten, other types (an elided string included) ignored; a `null` `voice_setting.speed` is removed; the
  top-level `speed`, when a JSON number and `voice_setting` has none, is sent as a float (`1` as `1.0`); an explicit
  `voice_setting.speed` wins as sent; a non-number top-level `speed` is dropped.
- `prepare.voice-*` and `prepare.format-*` transcribe `resolve_minimax_voice` and `minimax_tts_audio_format`
  (`mimo_tts_tests.rs:451-502`), refusal texts naming the lower-cased value.
- `prepare.input-hangul`, `prepare.input-mixed`, `prepare.input-accented`: the weighted `upper` (ASCII 1, any other scalar
  2; `tts.rs:297-303`).
- Responses (record §3, `decode_minimax_tts_audio`, `mimo_tts_tests.rs:617-660`): audio in all four formats;
  `base_resp` for each table code, an unnamed code, a string and a fractional code, absent and `null`;
  `usage_characters` positive (upstream), absent / `null` / `extra_info` `null` (the request's count), above the
  weighted `upper` but within the input's bytes (succeeded), and `0`, negative, fractional, `9.0`, a string and an
  object (`unknown`, SV-Q7); empty, missing, `null` and non-string audio, `data` not an object, a non-object body and a
  body that is not JSON; 401, 403, 404, 408, 429, 500 and 302, whose provider message is `base_resp.status_msg`, else
  the body text.
