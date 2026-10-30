# Kling AI (video)

Upstream facts for [`task-kling-v2`](../../components/task-kling-v2/manifest.json). Moved from the
token-station-server host documentation on 2026-10-30. The
[Kling observe-context record](../design/2026-09-20-kling-task-observe-context.md) and the
task-kling-v2 fixtures are authoritative for package behaviour.

- **Auth** is a JWT signed from an AccessKey and SecretKey, not a static bearer.
- **Sources** (checked 2026-06-05): `kling.ai/dev/pricing` and the document-API reference. Both answer
  HTTP 446 to plain fetchers; use a browser user agent. The docs app loads per-endpoint specs from
  `s15-kling.klingai.com/kos/s101/nlav112918/api-doc/assets/api-*.js`, which can be read directly.
- **Hosts and ids.** Global `https://api-singapore.klingai.com`, China `https://api-beijing.klingai.com`
  (`api.klingai.com` is the legacy global host). Kling 3.0 is `kling-v3` (the older `kling-v3-0` no longer
  appears). Text-to-video ids: kling-v1, v1-6, v2-master, v2-1-master, v2-5-turbo, v2-6, v3; image-to-video
  adds v1-5 and v2-1; omni video: `kling-video-o1` (default) and `kling-v3-omni`; motion control:
  `kling-v2-6` (default) and `kling-v3`.
- **Endpoints** (create, then query on the same family; there is no generic task query):

  | Surface | Create | Query |
  | --- | --- | --- |
  | Text to video | `POST /v1/videos/text2video` | `GET /v1/videos/text2video/{id}` |
  | Image to video | `POST /v1/videos/image2video` | `GET /v1/videos/image2video/{id}` |
  | Omni video | `POST /v1/videos/omni-video` | `GET /v1/videos/omni-video/{id}` |
  | Motion control | `POST /v1/videos/motion-control` | `GET /v1/videos/motion-control/{id}` |

- **Billing evidence.** Every task query reports `data.final_unit_deduction` (a string such as `"4.5"`,
  present since 2026-01-23), which already includes mode, sound, voice control, multi-shot and
  reference-video surcharges. One unit is $0.14 on the standard card.
- **Per-second unit table** (kling.ai/dev/pricing, 2026-06-05):

  | Family | std | std + audio | pro | pro + audio | with reference video | 4k |
  | --- | --- | --- | --- | --- | --- | --- |
  | kling-v3 (text / image to video) | 0.6 | 0.9 | 0.8 | 1.2 | — | 3.0 (audio or not) |
  | kling-v3 motion control | 0.9 | — | 1.2 | — | — | — |
  | kling-v3-omni | 0.6 | 0.8 | 0.8 | 1.0 | std 0.9 / pro 1.2 (no audio) | 3.0 (no reference) |
  | kling-video-o1 | 0.6 | — | 0.8 | — | std 0.9 / pro 1.2 | — |

- **Parameters.** `mode` is lowercase `std` / `pro` / `4k` (the base v3 model refuses `4k` and `4K`);
  `sound` defaults to `off` and must be `off` whenever `video_list` is non-empty; multi-shot is
  `multi_shot: true` plus `shot_type` (`customize` needs `multi_prompt[]`: at most 6 shots, each prompt at
  most 512 characters, shot durations summing to the task `duration`, at most 15 s in all; the top-level
  `prompt` is then ignored). Image to video takes `image` and/or `image_tail` (an end frame alone is valid).
  Omni `video_list` holds at most one video with `refer_type` `feature` or `base` (default `base`) and
  `keep_original_sound`; 4k has no reference-video support. Motion control needs `image_url`, `video_url`
  (a 3–30 s reference clip) and `character_orientation` `image` or `video`; `prompt` is optional.
- **Duration** is a string `"3"`–`"15"`, default `"5"`, and comes back per video as a string in
  `task_result.videos[].duration`.
