# MiniMax

Upstream facts for the video families of [`task-minimax-v2`](../../components/task-minimax-v2/manifest.json).
Moved from the token-station-server host documentation on 2026-10-30. The MiniMax design records
([v1 task component](../design/2026-09-20-minimax-v1-task-component.md),
[H3 estimate facts](../design/2026-09-20-minimax-h3-estimate-facts.md),
[artifact fetch](../design/2026-09-20-minimax-artifact-fetch-capability.md),
[immutable paths](../design/2026-10-14-task-minimax-immutable-paths.md)) are authoritative for what the
package sends and parses; where these notes disagree with them, the records win. MiniMax image and
speech facts are in the image I3-1 and speech V3-2 records.

## 1. Platforms

Keys are platform-scoped: a key from one platform is refused (code 2049) on the other platform's host.
The international platform is `api.minimax.io` (card in USD); China is `api.minimaxi.com` for the v1
API and `api.minimax.cn` for the v2 API (card in CNY). The two cards are separate tables, not an FX
conversion of each other: the USD/CNY ratio varies per tier (0.14–0.17 on the Hailuo card).

## 1a. Chat and speech billing facts

- The M2 chat family publishes cache read and write prices, but the chat usage object reports no cache buckets
  (probed 2026-06-11 on the China platform: `prompt_tokens` / `completion_tokens` only, with the same 2.6K-token
  prefix sent twice).
- Speech synthesis reports `extra_info.usage_characters`, priced per character. Rates come from the official
  China pay-as-you-go card (`platform.minimaxi.com/docs/guides/pricing-paygo`) in CNY.

## 2. Hailuo (v1 API)

- **Flow.** `POST /v1/video_generation` (text and image to video) returns `{task_id, base_resp}`;
  `GET /v1/query/video_generation?task_id=<id>` reports `status` in `Preparing`, `Queueing`,
  `Processing`, `Success`, `Fail`, with a top-level `file_id` (and `video_width` / `video_height`) on
  success; `GET /v1/files/retrieve?file_id=<id>` returns `file.download_url`. The query endpoint is
  tightly rate-limited; poll no faster than about every 12 s.
- **Errors arrive as HTTP 200.** The real outcome is `base_resp.status_code` (0 is success). Codes seen:
  1004 and 2049 (authentication / wrong platform), 2013 and 1026 (invalid request), 1002 (rate limit),
  1008 and 2153 (unavailable). A non-zero `base_resp` on a query is a query-side error such as a rate
  limit, not a task failure.
- **Models** (checked 2026-06-01 against the international schema): `MiniMax-Hailuo-2.3`,
  `MiniMax-Hailuo-2.3-Fast` (image to video), `MiniMax-Hailuo-02`. `last_frame_image`,
  `subject_reference` and `S2V-01` are not in the live international `/v1/video_generation` schema. The
  older 01 series (`T2V-01`, `I2V-01`, …) has no published price.
- **Fields.** `prompt` (≤ 2000 characters), `first_frame_image` (a public URL or a `data:` URI),
  `resolution` (`512P` / `768P` / `1080P`, default `768P`), `duration` (6 or 10, default 6),
  `prompt_optimizer`, `fast_pretreatment`. A prompt or an image is required.
- **Prices are per clip, not per second.** Each (model, resolution, duration) clip has its own price, and
  the ratios are neither per-model multiples nor linear in duration (Hailuo-2.3 1080P/6s is 1.75× its
  768P/6s).

  China card (platform.minimaxi.com/docs/guides/pricing-paygo, checked 2026-06-11; `—` not offered):

  | Model | 512P/6s | 768P/6s | 1080P/6s | 512P/10s | 768P/10s |
  |---|---|---|---|---|---|
  | `MiniMax-Hailuo-2.3` | — | ¥2.00 | ¥3.50 | — | ¥4.00 |
  | `MiniMax-Hailuo-2.3-Fast` | — | ¥1.35 | ¥2.31 | — | ¥2.25 |
  | `MiniMax-Hailuo-02` | ¥0.60 | ¥2.00 | ¥3.50 | ¥1.00 | ¥4.00 |

  International card (platform.minimax.io, checked 2026-09-29): Hailuo-2.3 768P/6s $0.28, 768P/10s
  $0.56, 1080P/6s $0.49; 2.3-Fast $0.19 / $0.32 / $0.33; Hailuo-02 the 2.3 prices plus 512P/6s $0.10 and
  512P/10s $0.15.

## 3. H3 (v2 API)

`MiniMax-H3` (released 2026-07-31) and `MiniMax-H3-Max`. Sources checked 2026-09-11:
`platform.minimax.io/docs/api-reference/video-generation-v2-create.md`, `…/video-generation-v2-query.md`,
`…/guides/video-generation.md`, `…/guides/pricing-paygo` (China: same paths on platform.minimaxi.com).

| | v1 (Hailuo) | v2 (H3) |
| --- | --- | --- |
| Submit | `POST /v1/video_generation`, flat fields | `POST /v2/video_generation`, `content` array |
| Errors | HTTP 200 + `base_resp.status_code` | real HTTP status + `{"error": {type, message}}` |
| Poll | `GET /v1/query/video_generation?task_id=` | `GET /v2/query/video_generation/{task_id}` |
| Status words | `Queueing` / `Processing` / `Success` / `Fail` | `queued` / `running` / `succeeded` / `failed` / `cancelled` |
| Artifact | `file_id` → `GET /v1/files/retrieve` | `task.content.url` directly |
| China host | `api.minimaxi.com` | `api.minimax.cn` |

- **Content items.** A text item; `first_frame`, `last_frame` (needs a first frame), `reference_image`
  (up to 9). Frame mode and reference mode are mutually exclusive; `MiniMax-H3-Max` takes no references.
  `duration` in whole seconds (default 5; H3 4–15, H3-Max 5–15); `resolution` `768P` / `2K` (H3) or
  `480P` / `768P` (H3-Max), default `768P`; `ratio`.
- **`ratio` is required for text-only requests although the parameter table lists it as optional**
  (found by a live call on 2026-09-17). A prompt-only request is refused with 400 unless `ratio` is set
  to one of `16:9`, `4:3`, `1:1`, `3:4`, `9:16`, `21:9`; `adaptive` is refused in that case too.
- **Input video and audio** (`video`, `reference_video`, `audio`, …) are billed per input second at the
  output rate, a length known only after the upstream decodes it.
- **Task errors.** `task.error.code` is a string code (1026 seen for an invalid request).
- **Limits.** 300 requests per minute and 30 in-flight tasks; the guide recommends polling every 10 s.
- **Prices** (per output second; checked 2026-09-14 on the raw bytes of the pricing pages):

  | | H3 768P | H3 2K | H3-Max 480P | H3-Max 768P | H3 input images |
  |---|---|---|---|---|---|
  | China | ¥0.50/s | ¥0.80/s | ¥0.33/s | ¥0.50/s | first 5 free, then ¥0.20 each |
  | International | $0.08/s | $0.13/s | $0.05/s | $0.08/s | first 5 free, then $0.04 each |

  H3-Max input images are not billed on either card (only the output video is).
