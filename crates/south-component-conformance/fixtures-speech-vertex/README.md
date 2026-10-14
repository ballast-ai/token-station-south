# `fixtures-speech-vertex`

Gate ② pack (`south.speech-component.v1`) of `speech-vertex` (family `vertex-ai-speech`), speech world batch V4
(`docs/design/2026-10-15-speech-v4-tts.md` §3, §5), including the package's `credential.*` cases. The manifest that names
this pack is `components/speech-vertex/manifest.json`.

Every expectation was written by hand from the record and from token-station-server's native arm
`TtsUpstream::VertexGemini` (`handler/audio/tts.rs:411-438`, `:696-748`, `:1032-1050`; `tts_providers.rs:851-950`;
`engine/vertex.rs:207-237`), read at `origin/dev-v2`, before the reference first ran:

- `prepare.native` and `response.native` transcribe the host test
  `vertex_gemini_tts_generates_audio_with_the_sa_bearer_and_bills_local_characters` (`gateway/tests/media_arms.rs:416`):
  region `us-central1`, the credential's `test-project`, `gemini-2.5-flash-preview-tts`, `POST
  v1/projects/test-project/locations/us-central1/publishers/google/models/gemini-2.5-flash-preview-tts:generateContent`
  with the minted bearer and no other header; `{"contents": [{"role": "user", "parts": [{"text": "hello world"}]}],
  "generationConfig": {"responseModalities": ["AUDIO"], "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig":
  {"voiceName": "Kore"}}}}}`; the answer's base64 `PCM!` in `candidates[0].content.parts[0].inlineData.data` is an
  `inline` artifact delivered as `audio/wav` inside the 24000 Hz mono `wav_pcm_s16le` container; the local 11 characters
  are billed (Vertex reports none).
- Location rows (I3-3 §4, as `fixtures-image-vertex/`): `global` on the unprefixed origin, `europe-west4`, a proxy base
  used as given, the `project` override over the exported `project_id`, and the exported `project_id` alone.
- `prepare.voice-*` transcribe `resolve_vertex_voice` (`vertex_tts_tests.rs:136-145`): `""`, blank and the eight names
  `alloy`, `echo`, `fable`, `onyx`, `nova`, `shimmer`, `ash`, `sage` (trimmed, any case) fold to `Kore`; `coral`, `ballad`
  and `Verse` are sent as written (record §8 item 2, reproduced); a Gemini name verbatim, padding and case kept; a
  non-string is `Kore`; an elided voice an `as_is` reference.
- `prepare.format-*` transcribe `vertex_tts_audio_format` (`vertex_tts_tests.rs:8-45`): absent / `wav` (any case) is WAV,
  `pcm` (any case) raw PCM labelled `audio/L16; rate=24000`; **Q7 (1.0.0): `mp3`, `MP3`, `opus`, `aac` and `flac` are
  served as WAV**, as native does for the dual run; a later minor refuses them (record §3.4). Any other string, `""`
  included, is refused with native's text; a non-string is absent; an elided one is refused.
- Responses (record §3.3): audio in `parts[0]` and in `parts[1]`, `pcm`, a degraded `mp3`, Hangul; native's `find_map`
  stopping at a first `inlineData.data` that is not a string; `""`; text-only parts; no candidates; candidates absent;
  audio only in `candidates[1]` (native reads `candidates[0]` only); a non-policy finish reason (all `unknown`, sidecar
  `audio`); a blocked prompt (`rejected`, `content_policy`) and a `SAFETY` finish (`unknown`, `content_policy`, I3-2
  GI-Q5); audio beside a policy finish delivered; a non-object body and a body that is not JSON; 400 (an unknown voice),
  401, 403, 404, 408, 429, 500 and 302 in Google's `{"error": {"message"}}` envelope, else the body text.

Not in this pack: the configuration refusals (a missing `region`, a missing project, a Vertex AI base URL of another
region, the prefixed `global` host). They are 500s naming the value, as `image-vertex` answers (record §3.1, P12), and the
suite's `pre_dispatch_refusal` row admits only `invalid_request` and `capability` from `prepare`, so they are pinned by
name in `tests/vertex_speech_suite.rs` (`CONFIGURATION_REFUSALS`) and through the guest in
`tests/vertex_speech_sandbox_parity.rs`.

Credential cases: the five `credential.*` cases of `fixtures-image-vertex/`, copied byte for byte (the two packages declare
the same `credentials` section; no case names a family).

Measurements still to make (record §11 item 2): one capture per voice class (a folded name, `coral`);
`inlineData.mimeType` and its rate; a blocked prompt and a policy stop, if reproducible; the error envelope.
