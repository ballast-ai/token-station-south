# Qwen-Audio Flash, Gemini 3.8 TTS, and served-model exclusions

Status: **implemented** on branch `release-0.87-round` (issue #357; lv, 2026-11-24: do the two TTS models and fold #357
into the same full package round). Not released.

Date: 2026-11-24

Baseline: south `origin/main` = `4a712dfe` (release 0.86.0).

## 1. Why one round

`qwen-audio-3.1-tts-flash` and Gemini 3.8 TTS live in reference implementations inside `south-component-conformance`,
which every component links. Changing it re-identifies every package (boundary record §13.12, §16 Q47). Issue #357
had already parked two changes that re-identify every package: an exclusion form for `served_models`, and `url` 2.5.9
in the component lockfiles. All four ship in 0.87.0.

## 2. `served_models.exclude_suffixes` (`south-provider-api` 0.66.0)

A family declaring `prefixes` may also declare `exclude_suffixes`. An id one of the prefixes matches, but that ends
with one of these suffixes, is not served. The list grammar is the one `ids` and `prefixes` use: non-empty, sorted,
distinct, at most 256 entries of 1 to 128 visible ASCII bytes. It is only allowed with `prefixes`. `ids` and `any` do
not take it, and gate ① refuses a manifest that uses it with either.

The motivating case is `speech-qwen`'s `bailian-qwen-speech` family. It declares `qwen-tts`, `qwen3-asr-` and
`qwen3-tts-`, and Model Studio names its WebSocket-only Realtime models under the same prefixes (`*-realtime`). The
family now declares `exclude_suffixes: ["-realtime"]`, so a host's route-time check refuses a Realtime model instead
of letting it fail upstream.

Older runtimes read the manifest with unknown fields refused. A package that declares the field must therefore
declare `south_runtime` 0.87.0, and `speech-qwen` does.

## 3. `speech-qwen` 1.4.0: `qwen-audio-3.1-tts-flash`

Model Studio's Qwen-Audio-TTS HTTP API page states that `qwen-audio-3.1-tts-flash` is billed by input and output
tokens, and that `qwen-audio-3.1-tts-plus`, `qwen-audio-3.0-tts-plus` and `qwen-audio-3.0-tts-flash` are billed by
characters. The flash model uses the same `SpeechSynthesizer` endpoint and body as the rest of the family.

- **Served**: the id joins `bailian-speech-synthesizer`'s `ids`. This reverses SV-Q13, which left it out because the
  family had no token metering.
- **Metering per model**: `model-capabilities` declares `tokens` for flash and `characters` for every other model of
  the family. A host that prices flash by characters, or another model by tokens, is refused before admission.
- **Tokens**: read from `usage.input_tokens` / `usage.output_tokens`. Each must be a positive integer, and absent,
  `null`, zero or a non-integer makes the answer `unknown`. The page documents these fields for 3.1 plus and flash,
  but its examples show only `characters`, so the field names are unverified until the check after cutover.
- **Unchanged**: the character-billed models' prepared calls and states are byte-identical to 1.3.1, so their
  fixtures are unchanged.

## 4. `speech-vertex` 1.3.0: Gemini 3.8 TTS

Source: Google's "Generate speech with Gemini TTS" guide and the two model pages (`gemini-3.8-flash-tts`,
`gemini-3.8-flash-lite-tts`). Neither model has a preview or dated id.

- **Served**: both ids join `vertex-ai-speech`'s `prefixes`.
- **Voice**: 3.8 takes `generationConfig.speechConfig.voiceConfig.voice`. The pages never mention
  `prebuiltVoiceConfig.voiceName`. The 2.5 folding of `OpenAI` names to `Kore` is kept.
- **Raw PCM**: a 3.8 unary answer is a whole WAV file by default. The request therefore asks for
  `generationConfig.responseFormat: [{"audio": {"mimeType": "AUDIO_L16"}}]`, and the delivery is the 2.5 one: raw
  24 kHz PCM, wrapped once in a WAV container for `wav` and passed through for `pcm`.
  `generationConfig.responseFormat` is an immutable path on 3.8 calls.
- **Defensive reading**: an answer whose `inlineData.mimeType` still says WAV is delivered as that WAV for `wav`,
  never wrapped twice. For `pcm` it is `unknown`.
- **Metering**: 3.8 can report `characters` or `tokens`, and the host's pricing decides which. Tokens come from
  `usageMetadata.promptTokenCount` / `candidatesTokenCount`; each must be a positive integer, otherwise the answer is
  `unknown`. Google's pricing pages could not be read when this was written, so the billing unit is the operator's to
  confirm. 2.5 is unchanged: characters only, the same request, state and fixtures.
- **Locations**: the 3.8 pages list `global`, `us` and `eu`. `vertex_location` maps `us` and `eu` to
  `https://aiplatform.{us,eu}.rep.googleapis.com`. This applies to every Vertex reference (`embeddings-vertex`,
  `image-vertex`, `speech-vertex`, `provider-vertex`). A base URL on those hosts must match the location, as the
  regional hosts must.
- **Unverified**: nothing about 3.8 has been called. The usage field names come from the Gemini API, not from the 3.8
  pages.

## 5. `url` 2.5.9

Every component lockfile moves to `url` 2.5.9 (RUSTSEC-2026-0592). `fuzz/Cargo.lock` moved in #355.

## 6. Hosts

The server host re-pins to 0.87.0 and adopts nothing else for the speech changes: `served_models` is read through
`south-provider-api`, and per-model metering forms are the speech world's existing contract.

Two host follow-ups:
- The speech route preflight's Vertex host rule (`p23_speech_cutover_preflight.py`, and the image preflight's) must
  accept the `us` / `eu` multi-region hosts.
- The family catalog's descriptions need no change.

Operators price `qwen-audio-3.1-tts-flash` with token prices and Gemini 3.8 per the unit Google bills.
