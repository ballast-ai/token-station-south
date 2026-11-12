# Speech: the Model Studio SpeechSynthesizer family and Model Studio-hosted MiniMax TTS

Status: proposed. The decisions in §5 are not ruled yet.

Date: 2026-11-10

Issue: #337. Baseline: south `origin/main` = `a9795fb9`. The host requirement is token-station-server
TTS component family extension requirement (`docs/plans/`, dated 2026-11-10) §7, at `origin/dev-v2` = `056910b0d`.

Parent records: [`2026-09-30-speech-world.md`](2026-09-30-speech-world.md) ("the record"),
[`2026-10-15-speech-v4-tts.md`](2026-10-15-speech-v4-tts.md) ("V4": `speech-qwen`) and
[`2026-10-14-speech-v3-2-tts.md`](2026-10-14-speech-v3-2-tts.md) ("V3-2": `speech-minimax`).

Unlike V3 and V4, neither family has a native arm to transcribe. The upstream facts come from Model Studio's published
API pages, read on 2026-11-10 (§6). No traffic has been captured. Every statement about the upstream is
**unverified** until the check after cutover. §6 lists those checks.

## 0. Why

The host catalog has eight Model Studio TTS rows. `speech-qwen` 1.2.2 serves three of them: the Qwen3-TTS ids, through
multimodal generation. This record covers three of the other five:

| Upstream id | Upstream API | Served by today |
|---|---|---|
| `qwen-audio-3.1-tts-plus` | HTTP SpeechSynthesizer | nothing (`speech-qwen` prefixes are `qwen-tts`, `qwen3-tts-`, `qwen3-asr-`) |
| `cosyvoice-v3.5-plus` | HTTP SpeechSynthesizer | nothing |
| `MiniMax/speech-2.8-hd` | multimodal generation, MiniMax's body under `input` | nothing (`speech-minimax` sends MiniMax's own `v1/t2a_v2`) |

Out of scope:

- `sambert-zhide-v1`. It is WebSocket-only, and the speech world describes one HTTP exchange. The host is delisting it.
- Token-billed `qwen-audio-3.1-tts-flash` (§5 SV-Q13).

## 1. What this delivers

| Package | Family, new | Serves |
|---|---|---|
| `speech-qwen` 1.3.0 | `bailian-speech-synthesizer` | §2 |
| `speech-minimax` 1.2.0 | `bailian-minimax-speech` | §3 |

Placement follows each package's existing code, not the vendor name (§5 SV-Q12):

- `speech-qwen` already holds the DashScope error reader, the `code`-in-a-2xx rule and the `http://` audio URL upgrade
  that the SpeechSynthesizer family needs.
- `speech-minimax` already holds MiniMax's voice, format, speed and `base_resp` rules, which the hosted body repeats.

Each existing family is unchanged. No contract, world or host change is needed:

- both families are `bearer` on the configured slot and use `base_url = "row_endpoint"`;
- both meter `characters`;
- both deliver through the existing `url` and `inline` forms.

## 2. `bailian-speech-synthesizer`

### 2.1 Served models

`ids` (not prefixes), the character-billed ids on the CosyVoice and Qwen-Audio-TTS pages:

- `qwen-audio-3.1-tts-plus`, `qwen-audio-3.0-tts-plus`, `qwen-audio-3.0-tts-flash`
- `cosyvoice-v3.5-plus`, `cosyvoice-v3.5-flash`, `cosyvoice-v3-plus`, `cosyvoice-v3-flash`, `cosyvoice-v2`

### 2.2 Request

- `POST api/v1/services/audio/tts/SpeechSynthesizer` under the row endpoint, `bearer`, no `X-DashScope-SSE` (the
  non-streaming answer).
- Body: `{"model", "input": {"text", "voice", "format", "rate"?, "instruction"?}}`. Every other client member is dropped.

Mapping from the client body:

- **Voice**: required. Absent, not a string, `""`, or one of the eleven OpenAI names (any case) is refused before
  admission (§5 SV-Q14). Any other string is sent verbatim.
- **`response_format` → `format`**:
  - lower-cased;
  - absent or `mp3` → `mp3` (`audio/mpeg`); `wav` → `audio/wav`; `pcm` → `audio/pcm`; `opus` → `audio/opus`;
  - `aac`, `flac` and any other string are refused before admission, naming the four.
  - The page's `sample_rate` default (22050) is kept by not sending it.
- **`speed` → `rate`**: a JSON number, sent as a float. The page's range [0.5, 2.0] is narrower than OpenAI's
  [0.25, 4.0]. Out-of-range values are sent, and the upstream's 4xx is released by the P29 allow-list (V3-2's
  forward-as-given rule for MiniMax `speed`).
- **`instructions` → `instruction`**: the client's string, when present.

Immutable paths: `model`, `input.text`, `input.format` (the declared media type follows it), `input.voice`.

### 2.3 Facts and reservation

Characters: `count` is the scalar count; `upper` is the weighted count (ASCII 1, other scalars 2), as V4 §2.3 for
Qwen-TTS. The page bills "effective characters". Whether its weighting matches is `character_weighting_observed`.

### 2.4 Response

`json`, nothing elided. On a 2xx, in order:

1. A body that is not a JSON object is `unknown`.
2. A non-empty string `code` is `unknown` with a 502 `provider_protocol_error` (V4 P5).
3. `usage.characters`:
   - a positive integer is the upstream's count;
   - absent or `null`, the request's count;
   - `0` or anything else is `unknown` (SV-Q7).
4. `output.audio.url` that is not a non-empty string is `unknown`.
5. Otherwise a `url` artifact at `/output/audio/url` with the media type from §2.2 and `upgrade_insecure` (SV-Q10; the
   scheme is `audio_url_shape_observed`).

A non-2xx reads the DashScope `{"code", "message"}` answer, as `bailian-qwen-speech` does.

`usage.input_tokens` and `output_tokens`, which the page lists for `qwen-audio-3.1-*`, are not read.

## 3. `bailian-minimax-speech`

### 3.1 Served models

`prefixes`: `MiniMax/speech-`.

### 3.2 Request

- `POST api/v1/services/aigc/multimodal-generation/generation` under the row endpoint, `bearer`, no `X-DashScope-SSE`,
  no `?GroupId=` (the family has no `config_schema`).
- Body: `{"model", "input": {"text", "output_format": "hex", "voice_setting", "audio_setting"}}`. Every other client
  member is dropped.
- `voice_setting` and `audio_setting` are built by V3-2's rules, unchanged:
  - voice default `Wise_Woman`, with the OpenAI names folded to it;
  - formats `mp3` / `wav` / `flac` / `pcm`, and the media types V3-2 declares;
  - `speed` copied as a float.
- `stream` is not sent. The page streams only on `X-DashScope-SSE`.

Immutable paths: `model`, `input.text`, `input.output_format`, `input.audio_setting.format`,
`input.voice_setting.voice_id`.

### 3.3 Response

`json` with `/output/data/audio` elided. On a 2xx, in order:

1. A body that is not a JSON object is `unknown`.
2. A non-empty string `code` is `unknown`, 502 (V4 P5).
3. `output.base_resp` is read by `minimax_shared`'s table, as `speech-minimax` reads the top-level `base_resp`.
4. The hex audio at `output.data.audio` (missing, not a string or empty is `unknown`).
5. Characters:
   - `usage.characters` a positive integer is the upstream's count;
   - else `output.extra_info.usage_characters` by the same rule;
   - else the request's count;
   - a `0` or malformed value in the first one present is `unknown`.

A non-2xx reads the DashScope `{"code", "message"}` answer.

## 4. Fixtures and tests

- Conformance fixtures, one directory per family:
  - `fixtures-speech-bailian-synthesizer/` and `fixtures-speech-bailian-minimax/`;
  - the prepare cases above, refusals included;
  - every response branch in §2.4 and §3.3;
  - the cases are written from the pages, not from traffic.
- Reference implementations sit beside the existing ones and reuse `speech_shared`, `bailian_image::dashscope_error`
  and `minimax_shared`.
- Suite rows: as for each sibling family.
- The host gains no dual-run leg: there is no native arm. Host tests are the registry leg (load, exact selection,
  `served_models`) and the routing leg.

## 5. Decisions

Proposed. Defaults apply unless lv objects.

- **SV-Q12 Placement.** Both families are added to existing packages (§1), not to a new `speech-bailian` package. One
  binding serves one family, so the Model Studio row needs one binding per family whichever package holds them.
- **SV-Q13 Ids.**
  - The SpeechSynthesizer family lists only the character-billed ids.
  - `qwen-audio-3.1-tts-flash` bills tokens. It waits until a row sells it and its token usage is captured.
- **SV-Q14 No default voice for SpeechSynthesizer.**
  - The pages give no default, and voices differ by series.
  - Mapping OpenAI names to a guessed voice would bill a call whose audio the client did not ask for.
  - The client must name a voice. V4 and V3-2 have defaults only because native had them.
- **SV-Q15 Cloned-voice unlock fee is not metered.**
  - The MiniMax page charges a one-off unlock on a cloned voice's first use and reports it as `usage.count`.
  - The component cannot tell a cloned voice from a system one before the call, and the world has no metering form for a
    flat fee.
  - The package therefore ignores `usage.count`. The host would under-bill once per cloned voice.
  - The alternative is to refuse a 2xx that carries `usage.count`. That parks funds on a call that delivered audio, so it
    is not recommended.

## 6. Upstream facts to check after cutover

Sources:

- [Non-realtime TTS guide](https://help.aliyun.com/zh/model-studio/non-realtime-tts-user-guide)
- [Qwen-Audio-TTS HTTP](https://help.aliyun.com/zh/model-studio/qwen-audio-tts-http-api)
- [CosyVoice HTTP](https://help.aliyun.com/zh/model-studio/cosyvoice-tts-http-api)
- [MiniMax synchronous synthesis](https://help.aliyun.com/zh/model-studio/minimax-synchronous-speech-synthesis-api)

These go into `docs/upstreams/alibaba-model-studio.md` with the implementation.

| Item | Why it matters |
|---|---|
| `endpoint_host_observed` | The pages show only the workspace host `{WorkspaceId}.cn-beijing.maas.aliyuncs.com`. Whether the generic DashScope origin of the existing rows also serves these paths decides whether ops needs new rows. The package is host-agnostic either way. |
| `character_weighting_observed` | The reservation `upper` assumes ASCII 1, other 2. |
| `audio_url_shape_observed` | SpeechSynthesizer audio URL scheme and port, for the safe fetch. |
| `opus_media_type_observed` | Whether `opus` is an Ogg container (`audio/ogg`) rather than `audio/opus`. |
| `error_body_shape_observed` | The pages give no error schema for either API. |
| `minimax_base_resp_location_observed` | `base_resp` under `output`, as the page shows, or at the top level as on MiniMax's own API. |
| `singapore_availability_observed` | The guide lists CosyVoice and MiniMax in Beijing only. Qwen-Audio-TTS names a Singapore host. |

## 7. Host work after release

- Repin.
- Add both families to the host speech family table (the speech family reference under `docs/runbook/`) and the preflight `FAMILY_TABLE`.
- Ops then binds and routes the rows (one binding per family on each Model Studio row), smoke-tests each region, and
  delists `sambert-zhide-v1`.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
