# `fixtures-speech-openai-compatible`

Gate ② pack (`south.speech-component.v1`) of `speech-openai-compatible` (families `openai-compatible`,
`azure-openai-v1` and `azure-openai-dated`), speech world
batch V3-1 (`docs/design/2026-10-14-speech-v3-1-tts.md` §2, §8, §12, SV-Q1, SV-Q3). The manifest that names this pack
is `components/speech-openai-compatible/manifest.json`.

Every expectation was written by hand from the record and from token-station-server's native catch-all arm
(`TtsUpstream::OpenaiCompat`) before the reference first ran:

- Character-priced calls (`prepare.chars-*`): the client's JSON with `model` the upstream id, every other member as sent
  (numbers as `serde_json` renders them, an elided member as an `as_is` reference), posted to `v1/audio/speech` under an
  origin-only `base_url` and to `audio/speech` under a `base_url` with a path (the embeddings package's API-root rule).
  The predicted media type is `openai_tts_content_type` (`tts_providers.rs:72-81`) over the lower-cased client format;
  a client `stream_format` goes as sent (SV-Q3). The responses are the common binary table (record §1), the 404 and
  503 bodies of `gateway/tests/zero_usage_release_media.rs:353-487`, a 2xx typed `application/json` (decision 4), the
  buffered event-stream transcript a client `stream_format: "sse"` gets (SV-Q3) and a provider message capped at 256
  characters.
- Token-priced calls (`prepare.tokens-*`): `stream_format` is `"sse"` whatever the client sent, the response form is
  `sse` with `/*/data/audio` elided, and the state carries the client format. `response.sse-two-deltas` transcribes
  `openai_token_priced_tts_requests_sse_and_bills_the_exact_reported_tokens` (`gateway/tests/media_arms.rs:1054`): two
  deltas and `usage {5, 40}` give a two-item `segments` artifact typed `audio/wav` and `tokens {5, 40}`.
  `sse-usage-missing` transcribes `:1120`; `sse-input-missing`, `sse-output-missing` and `sse-output-null` transcribe
  the three cases of `:1170`. Native parks each of them; here they are `unknown` (record §2 outcomes). The other
  `sse-*` cases cover the rest of record §12: `event:` lines, a non-object data and `[DONE]` skipped, both counts 0, a
  negative and a fractional count (record §9 item 3, §10 P3), no deltas and 0-byte deltas (`unknown`, sidecar `audio`),
  two valid `done` events (the last valid one wins) and a later invalid one (skipped), and `input_token_details` closing
  and not closing (I3-2 GI-Q4).
- The generated 4,096-event case of record §8 is built by `tests/openai_compatible_speech_suite.rs`, not stored here.

## `MAX_SPEECH_SEGMENTS` (record §8, SV-Q6)

Not measured: the agent that wrote this pack has no upstream key. By lv's SV-Q6 ruling (2026-10-14) the provisional
bound of 65,536 ships, and the capture of record §8 (one per format, at least `mp3` and `pcm`, with `gpt-4o-mini-tts`,
`stream_format: "sse"` and the longest input the row accepts) is a host cutover prerequisite for token-priced rows
only. When it is made, record here: the delta count, the smallest, median and largest decoded delta, the total audio
bytes, the SSE body bytes, whether events carry an `event:` line, and the `usage` shape (with `input_token_details`);
commit one short real capture trimmed to its first deltas and its `speech.audio.done` event. If the measured count
× 4 is not below 65,536, the bound and the view budget are revisited before release.

The two computed bounds hold at the provisional value: the unit test
`max_speech_segments_fits_the_view_and_the_outcome` builds the view and the outcome of a 65,536-event stream and
checks both fit (the `parse-response` payload, 16 MiB; the outcome frame, 4 MiB), and that one more event is refused
by the artifact codec.

## Recognition (speech world batch V3-3)

`docs/design/2026-10-14-speech-v3-3-asr.md` §2, §10. The `asr-*` cases, the `capabilities.recognition-words` and
`capabilities.misspelt-dialect-word` cases, and the updated `capabilities.models` (every configured model now also
declares `transcribe` and `seconds`) were written by hand from the record and from the native fallback arm
`AsrUpstream::OpenaiCompat` (`handler/audio.rs:91-101`, `:226-235`, `:284-287`, `:559-564`) before the reference
first ran:

- The configuration carries `whisper-1` with `speech.translate` and `speech.verbose_json` (a verbose row) and
  `gpt-4o-transcribe` with neither (record §9 item 1's expectation, **unverified**). The host test bodies of
  `gateway/tests/media_arms.rs:203-215` and `:794-815` (a `model` part valued with the gateway name and a file part
  `a.mp3`) are the base request.
- Verbose rows: `response_format` set to `verbose_json` in place or appended; `response.asr-verbose-{json, text,
  verbose-json, srt, vtt}` render one answer in every client format with the same reported seconds
  (`client_format_rendering`); a missing or `null` duration is `unknown` (sidecar `seconds`), `0`, `-1` and a string
  are `unknown`, a missing transcript, a non-object body, missing or unusable `segments` are `unknown`; an empty
  segment list renders `WEBVTT` and a blank line.
- Other rows: the client's format forwarded; `json` answers passed through, billed by `duration`, else by
  `usage.seconds` when `usage.type` is `"duration"`, else `seconds: null` (sidecar `seconds`; `media_arms.rs:1370`);
  text answers passed through with the upstream's `content-type`, its essence, or the format's type (a label naming
  JSON is not echoed on a text document).
- Translation: `audio/translations` for a row carrying `speech.translate`; refused (`capability`) for a row without it,
  for a model without a row, and on a `base_url` with a path (`asr-base-url-with-path-translate`).
- Refusals before dispatch: not multipart, an elided text part, no file part `file` (a text part named `file` does not
  count), `stream` `true` / `1` / ` TRUE `, an unknown, padded or upper-case format, and metering other than
  `[seconds]`. `stream=false` is forwarded.
- Non-2xx: V3-1's table with the `OpenAI` error shape (400, 401, 403, 404, 408, 429, 500, 302).
- The elided-transcript cases (a `$south.ref` for `json`, `unknown` for `text`) need an answer above 1 MiB and are
  generated by `tests/openai_compatible_speech_suite.rs`, not stored here.

Measurements still to make (record §9 items 1-2): per model, the accepted formats and endpoints, whether a `json`
answer carries `usage` with `type: "duration"`, and whether `verbose_json` carries `duration` and `segments`.

## Azure OpenAI (`azure-openai-v1`, `azure-openai-dated`)

`docs/design/2026-10-15-speech-azure-openai.md` §6. The `azure-*` and `asr-azure-*` cases and
`capabilities.azure-dated-models` were written by hand from the record and from the native arms reached on an
`azure_openai` or `azure_foundry` row (`build_upstream_url`, `engine/upstream.rs:537-581`; `api-key`, `:190-196`) before
the reference first ran:

- Dated: `openai/deployments/{deployment}/audio/{speech,transcriptions,translations}` below the row endpoint (the
  classic `openai.azure.com` and the `services.ai.azure.com` hosts), `api-version` the config value or `2024-10-21`.
- v1: the same audio paths below the base `{endpoint}/openai/v1`.
- Both: `header_secret` `api-key`; bodies, parts, metering and readers as `openai-compatible`'s, since native runs the
  same arms. The deployment names (`tts-prod`, `whisper-prod`) are the operator's, as native sends the row's upstream
  model.
- `response.azure-dated-status-401` uses Azure's documented error shape `{"error": {"code": "401", "message"}}`, not a
  capture (record §5.2 U4).
- `capabilities.wrong-family` names `azure_openai`, the native provider type's word, now that `azure-openai-v1` is
  served.

The configuration errors (an undated `api_version`, a 500) and the empty deployment are pinned by
`tests/speech_azure_openai_v1.rs`, not stored here.
