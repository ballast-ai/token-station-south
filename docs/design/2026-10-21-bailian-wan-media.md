# Bailian Wan media requests: `task-bailian-v2` 0.32.0 learns `input.media[]`

Status: proposed 2026-10-21, awaiting lv. Implemented on branch `feature/bailian-wan-media`.

Date: 2026-10-21

Predecessor: [the Bailian video task component](2026-09-20-bailian-video-task-component.md) ("the Bailian
record"), whose input shapes this record extends.

## 1. Why

Alibaba Cloud Model Studio's newer Wan models take every input in one `input.media[]` array of
`{"type", "url"}` items instead of the older `input.img_url` / `first_frame_url` fields. The package
chose a shape from `upstream_model_family` (falling back to the model id): kf2v first and last frame
fields, HappyHorse `media[]`, otherwise `input.img_url` when an image was present, otherwise plain
text-to-video. That sent `input.img_url` to `wan2.7-i2v`, which the reference says takes only
`input.media[]` (a frozen sample pinned the wrong shape), and it offered no way to send the reference
videos, reference audio or video inputs of wan3.0, wan2.7-r2v and wan2.7-videoedit. It also turned a
smart `duration: -1` into a 5 s estimate and reported no resolution when the caller named none, so the
host reserved too little: the upstream bills its `1080P` default tier and, for video inputs, input
seconds as well as output seconds.

## 2. Sources (international site, read 2026-10-21)

- Wan3.0 video generation: https://www.alibabacloud.com/help/en/model-studio/wan3-video-generation-api-reference
- Wan 2.7 image-to-video: https://www.alibabacloud.com/help/en/model-studio/image-to-video-general-api-reference
- Wan 2.7 reference-to-video: https://www.alibabacloud.com/help/en/model-studio/wan-video-to-video-api-reference
- Wan 2.7 video editing: https://www.alibabacloud.com/help/en/model-studio/wan-video-editing-api-reference
- Pricing (billing formulas): https://www.alibabacloud.com/help/en/model-studio/model-pricing

All four post to `api/v1/services/aigc/video-generation/video-synthesis` with `X-DashScope-Async: enable`
and are polled at `api/v1/tasks/{id}`, as today.

| Model | Media combinations and counts | Parameters | Billed `usage.duration` |
|---|---|---|---|
| wan3.0-video, wan3.0-video-prime | none (text only); `first_frame`; `first_frame` + `last_frame`; any mix of `reference_image` ≤ 10, `reference_video` ≤ 5 (≤ 15 s in all), `reference_audio` ≤ 5 (≤ 15 s), optionally with one of `file` / `link`. Frames exclude every reference type. Prompt or media required. | `resolution` 480P/720P/1080P (default 1080P); `ratio` (default `adaptive`); `duration` default 5, 2–30 without video, input + output ≤ 30 with video, `-1` smart; `audio` (default true, price unchanged); `seed`, `prompt_extend`, `watermark` | input video seconds + output seconds |
| wan2.7-i2v | `first_frame` (+ `driving_audio`); `first_frame` + `last_frame` (+ `driving_audio`); `first_clip` (2–10 s) (+ `last_frame`). Each type at most once. Prompt optional. | `resolution` 720P/1080P (default 1080P); `duration` 2–15 (default 5); `input.negative_prompt`; `seed`, `prompt_extend`, `watermark` | output only; for a continuation `duration` is the total output, billed in full |
| wan2.7-r2v | `reference_image` + `reference_video` 1–5 in all, plus `first_frame` ≤ 1. Prompt required. | `resolution` 720P/1080P (default 1080P); `ratio` (default 16:9); `duration` 2–10 with a video, 2–15 without (default 5); `input.negative_prompt`; `seed`, `prompt_extend`, `watermark` | input video (at most 5 s counted) + output |
| wan2.7-videoedit | one `video` (2–10 s) + `reference_image` ≤ 4. Prompt optional. | `resolution` 720P/1080P (default 1080P); `ratio`; `duration` 0 (= input length, default) or 2–10; `audio_setting` `auto`/`origin`; `input.negative_prompt`; `seed`, `prompt_extend`, `watermark` | input + output |

## 3. Shape selection

The prefix of `upstream_model_family` (or the model id when the host gives none) selects the shape:
`wan3.` → wan3 media; `wan2.7-i2v` → image-to-video media; `wan2.7-r2v` → reference-to-video media;
`wan2.7-videoedit` → edit media. Everything else, `wan2.7-t2v`, wan2.6 and older, kf2v and HappyHorse
included, keeps its existing shape byte for byte: no earlier frozen sample changes except the corrected
`wan-image` expectation, and a new `wan26-image` sample pins `input.img_url` for wan2.6-i2v.

## 4. Northbound mapping

The host forwards the caller's `/v1/video/generations` body; the names below are the ones the host's
capability pre-check and the playground already use, plus three new list keys.

| Northbound key | wan3 | wan2.7-i2v | wan2.7-r2v | wan2.7-videoedit |
|---|---|---|---|---|
| `image` / `image_url` / `first_frame_image` | `first_frame` | `first_frame` | `first_frame` | refused |
| `last_frame` / `last_frame_url` | `last_frame` | `last_frame` | refused | refused |
| `reference_images[]` | `reference_image` | refused | `reference_image` | `reference_image` |
| `reference_videos[]` (new) | `reference_video` | refused | `reference_video` | refused |
| `reference_audios[]` (new) | `reference_audio` | refused | refused | refused |
| `video` | `reference_video` (before `reference_videos[]`) | `first_clip` | refused | `video` |
| `audio_url` | refused | `driving_audio` | refused | refused |
| `generate_audio` (boolean) | `parameters.audio` | refused | refused | refused |
| `aspect_ratio`, else `ratio` | `parameters.ratio` | refused | `parameters.ratio` | `parameters.ratio` |
| `negative_prompt` | refused | `input.negative_prompt` | `input.negative_prompt` | `input.negative_prompt` |
| `audio_setting` | refused | refused | refused | `parameters.audio_setting` |
| `file`, `link`, `size` | refused | refused | refused | refused |
| `seed`, `prompt_extend`, `watermark` | passed through | passed through | passed through | passed through |

`media[]` lists frames first, then the clip or video, then reference images, videos and audio, each in
the caller's order (the prompt's "Image 1" / "Video 1" count within a type). A key a shape does not take
is refused rather than dropped, so a caller never pays for a request whose input silently vanished. The
same role under two aliases, an empty or non-string URL, and a list that is not an array of strings are
refused. Refusals are request-shaped (`invalid_request`, 400) before dispatch, and their messages never
echo a URL.

`resolution` is normalized with the existing hint rules (`720p` → `720P`), must be a tier the model
offers, and is sent in its canonical form. When the caller names none, the component writes `1080P`
into `parameters.resolution` and reports `1080P` in the estimate, so the host prices the tier the
upstream bills instead of falling back to its base tier.

`duration` must be a whole number of seconds (a number or a numeric string) within the model's range;
`-1` is accepted for wan3 only, `0` for videoedit only. It is sent as a JSON number.

## 5. Estimates

The estimate's requested seconds is the upper bound of the `usage.duration` the upstream bills
(`d` = the requested duration):

| Shape | Requested seconds |
|---|---|
| wan3 without input video | `d` (absent → 5, `-1` → 30) |
| wan3 with input video | `min(30, 15 + d)` (absent or `-1` → 30) |
| wan2.7-i2v from a frame | `d` (absent → 5) |
| wan2.7-i2v continuation | `max(d, 10)` (absent → 10) |
| wan2.7-r2v with a reference video | `d + 5` (absent → 10) |
| wan2.7-r2v with images only | `d` (absent → 5) |
| wan2.7-videoedit | `10 + d` (absent or 0 → 20) |

The input image count is first frame + last frame + reference images. Settlement is unchanged: it
already reads `usage.duration` first, which these references define as the billed total; the
`wan3-reference-video` observation sample pins that (`duration` 13 = input 8 + output 5).

## 6. Model catalog

`catalogs/model-catalog.json` gains entries (exact id and dated `-` prefix) for wan3.0-video (covering
`wan3.0-video-prime`), wan2.7-i2v, wan2.7-r2v and wan2.7-videoedit, in the host's existing vocabulary:
duration ranges (wan3 with `auto`), resolution options with default `1080P`, aspect ratio options,
media roles (first / last frame and reference images, wan3 grouping frames apart from references) and
wan3's `generate_audio` feature. Rules the vocabulary cannot express (video counts, r2v's shorter range
with a video, i2v's "last frame needs a first frame or clip") are enforced by the component.

## 7. Package identity

`task-bailian-v2` 0.31.33 → 0.32.0 (minor: new accepted inputs and a changed wire shape for
wan2.7-i2v). `south_runtime` stays 0.57.0 and the contracts stay `task` 7: nothing here needs a newer
runtime. The change edits `south-component-conformance` (source and fixtures), which every component
links; per CONTRIBUTING that crate's version and every package's version must move, which is left to
the release integration combining this with parallel workstreams. Until then
`scripts/check_crate_versions.py`, the digest-stability check and the 0.75.0 round assertion in
`shipped_packages_v1` are expected to fail. The integration commit made the bump: `south-component-conformance`
0.74.0 → 0.75.0, every other package the Q47 patch
(`shipped_packages_v1::bailian_retires_every_published_075_package_identity`).

## 8. Open points

1. **`file` / `link` (wan3).** Refused for now: no northbound key exists, and the reference requires
   `prompt_extend: true` with them. Adding them needs a host-side input name and lv's decision.
2. **Continuation billing (wan2.7-i2v `first_clip`).** The reference says `duration` is the total output
   and is billed in full; whether a `duration` shorter than the clip bills the clip's length is not
   documented. The estimate assumes it may (`max(d, 10)`); unverified on a live key.
3. **`reference_voice` (wan2.7-r2v).** Each reference may carry a voice URL; the northbound lists carry
   plain URL strings, so the component cannot express it yet.
4. **wan3 input video length.** The component cannot see input video lengths, so it accepts `duration`
   2–30 with video and leaves the "input + output ≤ 30" rule to the upstream; the estimate stays the
   safe upper bound.
5. **wan2.6-i2v-flash audio vs silent pricing.** That model's price differs by audio on/off; the host's
   per-second price has no audio dimension, so this package cannot report it. Unchanged here.
6. **Host follow-up.** The host's role table knows only first frame, last frame and reference image;
   `reference_videos`, `reference_audios`, `video` and `audio_url` are not pre-checked by the host and
   are validated only here. The playground has no inputs for them yet.
