# Alibaba Cloud Model Studio (Bailian / DashScope)

Upstream facts for the packages that serve Alibaba Cloud Model Studio: the Bailian chat families of
[`provider-openai-compatible`](../../components/provider-openai-compatible/manifest.json),
[`task-bailian-v2`](../../components/task-bailian-v2/manifest.json) (managed video),
[`task-wan-image-v2`](../../components/task-wan-image-v2/manifest.json) (Wan image),
[`image-qwen`](../../components/image-qwen/manifest.json) (Qwen-Image) and
[`speech-qwen`](../../components/speech-qwen/manifest.json) (Qwen TTS and ASR).

These notes were moved here from the token-station-server host documentation on 2026-10-30, under
the rule that a host keeps only generic mechanisms and vendor facts live with the packages. Each fact
carries the date it was last checked. Prices are list prices as published on that date; promotions
are not prices. The design records linked below are authoritative for what each package sends and
parses; where these notes and a record disagree, the record wins and these notes are wrong.

Related records:

- [Bailian managed video task component](../design/2026-09-20-bailian-video-task-component.md)
- [Bailian Wan media requests](../design/2026-10-21-bailian-wan-media.md) (`input.media[]`, Wan3.0, Wan 2.7)
- [Image I3-1: MiniMax and Qwen](../design/2026-10-12-image-i3-1-minimax-and-qwen.md)
- [Explicit cache reads](../design/2026-10-17-explicit-cache-reads.md)
- [Qwen ASR](../design/2026-10-21-speech-qwen-asr.md) and [speech V4 TTS](../design/2026-10-15-speech-v4-tts.md)

## 1. Deployments, keys and list prices

- **Two deployments with separate key namespaces.** Mainland China is `dashscope.aliyuncs.com`;
  International (Singapore) is `dashscope-intl.aliyuncs.com`. A mainland key answers 401 on the
  international endpoint, so each deployment is enumerated and billed with its own key.
- **Each deployment bills its own card.** Mainland bills the "Chinese mainland" CNY column of
  `help.aliyun.com/zh/model-studio/model-pricing`; International bills the USD card of
  `alibabacloud.com/help/en/model-studio/model-pricing`. The mainland page's Singapore / global CNY
  columns are not the international USD card. International runs about 1.4× mainland (qwen3.6-plus:
  ¥2 / ¥12 mainland, $0.5 / $3 international; checked 2026-07).
- **Qwen Cloud (`qwencloud.com`) is the same backend.** Its OpenAI-compatible base URL is
  `https://dashscope-intl.aliyuncs.com/compatible-mode/v1` (Anthropic-compatible
  `…/apps/anthropic`), Bearer auth. It issues its own keys (`home.qwencloud.com/api-keys`;
  pay-as-you-go `sk-…`, Coding Plan `sk-sp-…`) and bills DashScope pay-as-you-go. Its lower prices
  (qwen3.7-max $1.25 / $3.75 against the $2.50 / $7.50 list) are a limited-time 50% promotion shown
  next to the same list price; the key does not select a different rate card (checked 2026-07;
  sources `qwencloud.com/promo/discount-qwen`, `docs.qwencloud.com/developer-guides/getting-started/pricing`).
  A Qwen Cloud pay-as-you-go key works wherever an international DashScope key does.
- **glm-5.2 on the international deployment** is listed at both "Chinese mainland" and "Global"
  scope at $1.1 / $3.851 per million tokens; "Global" is what `dashscope-intl` bills. A figure taken
  from Zhipu's own console (¥10.492 / ¥32.974) is not this card. The mainland card lists ¥8 / ¥28 and
  an implicit-cache read at 25% of input; the international card omits the cache row.
- **Per-tier thinking prices.** Some tiered models charge a higher output rate in thinking mode, and
  that rate also varies by input tier: qwen-plus mainland ¥2 non-thinking / ¥8 thinking at ≤ 128K
  input, ¥20 / ¥24 at 128K–256K; international $1.2 / $4 at ≤ 256K. qwen-plus publishes no
  context-cache discount on either card.

## 2. Discovering models and prices

- `GET {endpoint}/compatible-mode/v1/models` with the deployment's key returns the chat model ids
  (about 220 on 2026-07). It does not list the asynchronous video and image models (Wan, HappyHorse,
  Qwen-Image on `/api/v1/...`) or some aliases (for example `ZHIPU/GLM-5.2`), which are still valid.
- The pricing pages are single-page applications: a fetcher that does not run scripts sees only the
  shell. The data is in the server-rendered HTML, so fetching the page with a browser user agent and
  stripping tags exposes each model row with its deployment scope and tiered per-million cells.

## 3. Account-level failures

- **An account in arrears rejects every DashScope call** — chat, image, speech and video — with HTTP
  400 and `code = "Arrearage"` ("Access denied, please make sure your account is in good standing")
  until the bill is paid. This is an account condition, not a request error; a live test suite should
  probe DashScope directly with a one-token completion and skip, loudly, on `Arrearage`.

## 4. Chat: Qwen3.7 and caching

- `qwen3.7-plus` (image and video input, reasoning, tool calls) and `qwen3.7-max` (text only)
  launched about 2026-06-02, both with a 1M context, on `/compatible-mode/v1/chat/completions`.
  Mainland list prices: Plus $1.20 / $4.80, Max $2.50 / $7.50 per million tokens; implicit-cache reads
  at 0.2× input.
- The automatic prefix cache is reported as `usage.prompt_tokens_details.cached_tokens`, a subset of
  `prompt_tokens`. The console also lists explicit cache creation and read prices (1.25× and 0.1× of
  input); explicit caching triggers only on a `cache_control` marker, and a response from an explicit
  cache carries the `prompt_tokens_details.cache_creation_input_tokens` key even when its value is 0.
  How the package reports the two kinds of read is in the
  [explicit cache record](../design/2026-10-17-explicit-cache-reads.md).

## 5. Managed video

Submission is `POST /api/v1/services/aigc/video-generation/video-synthesis` with
`X-DashScope-Async: enable`, polled at `GET /api/v1/tasks/{task_id}` until `output.task_status` is
terminal. Input shapes per model are in the two video records; the facts below are the ones those
records do not repeat.

- **HappyHorse** (`happyhorse-1.0-t2v` / `-i2v`, launched 2026-04-27; `happyhorse-1.1-*` with the same
  wire). Image-to-video needs `input.media = [{"type": "first_frame", "url": …}]`. A flat
  `input.img_url` is accepted at submission and the task then fails mid-render with
  `InvalidParameter: Field required: input.media`, so the error does not surface at the call (seen
  2026-06-15). Every media URL must be downloadable by DashScope: a `data:` URI or an unreachable host
  fails the task with `Failed to download …`; same-cloud OSS is safest. The model generates
  synchronised audio from the prompt in the same call. Mainland list prices: 1.0 at ¥0.9/s (720P) and
  ¥1.6/s (1080P); the 1.1 prices recorded on 2026-06-23 ($0.14/s 720P, $0.18/s 1080P) came from an
  operator, not the public page. The upstream default resolution is 1080P.
- **Wan 2.7 video** (`wan2.7-t2v`, `wan2.7-i2v`, `wan2.7-r2v`, `wan2.7-videoedit`; GA 2026-04-15; dated
  snapshots such as `wan2.7-t2v-2026-04-25`). Clips up to 15 s, aspect ratios 16:9 (default), 9:16, 1:1,
  4:3, 3:4. All four share one mainland card: ¥0.6/s at 720P and ¥1.0/s at 1080P (checked 2026-05-21).
  `wan2.7-i2v` and `wan2.7-r2v` always produce a video with audio at that price; only
  `wan2.6-i2v-flash` offers an audio-on / audio-off split.
- **First-and-last-frame (kf2v)** (`wan2.2-kf2v-flash`; `wanx2.1-kf2v-plus` mainland /
  `wan2.1-kf2v-plus` international). Differs from every other Bailian video model: submission goes to
  `POST /api/v1/services/aigc/image2video/video-synthesis` (same async header and poll);
  `input.first_frame_url` is required and `input.last_frame_url` optional, with no `img_url` or `media`;
  the duration is fixed at 5 s (the reference says the value is fixed and cannot be changed). `parameters.resolution` is
  `480P` / `720P` / `1080P` (default 720P); `usage.video_duration` is the billed length. List prices
  (checked 2026-09-11; reference
  `help.aliyun.com/zh/model-studio/image-to-video-by-first-and-last-frame-api-reference`):

  | Model | Deployment | 480P | 720P | 1080P |
  | --- | --- | --- | --- | --- |
  | `wan2.2-kf2v-flash` | mainland (Beijing) | ¥0.10/s | ¥0.20/s | ¥0.48/s |
  | `wan2.2-kf2v-flash` | international (Singapore) | $0.015/s | $0.036/s | $0.07/s |
  | `wanx2.1-kf2v-plus` / `wan2.1-kf2v-plus` | mainland / international | — | ¥0.70/s / $0.10/s | — |

## 6. Wan 2.7 image

- `wan2.7-image` (up to 2K) and `wan2.7-image-pro` (4K, thinking-mode rendering, up to 3 source
  images) went GA on 2026-04-15. One endpoint serves generation and editing:
  `POST /api/v1/services/aigc/image-generation/generation` with `X-DashScope-Async: enable`. The body
  is message based: each `input.messages[].content[]` interleaves `{"image": url}` and
  `{"text": prompt}` parts, the source image first when editing; `parameters.bbox_list` marks edit
  regions. The task is polled at `/api/v1/tasks/{task_id}` (`PENDING` → `RUNNING` → `SUCCEEDED` /
  `FAILED` / `CANCELED`); results are in `output.choices[].message.content[].image`, and
  `usage.image_count` is the billed count.
- As of 2026-05-21 the public pricing page listed no Wan 2.7 per-image price (only Wan 2.6 at
  $0.028671 per image, mainland). Any Wan 2.7 image price in use before it is published is an
  operator estimate, not a list price.
