# Stability AI (Stable Image)

Upstream facts for [`image-stability`](../../components/image-stability/manifest.json). Moved from the
token-station-server host documentation on 2026-10-30. The
[image I3-4 record](../design/2026-10-13-image-i3-4-stability.md) is authoritative for what the package
sends and parses; where these notes disagree with it, the record wins.

- **Host and auth.** `https://api.stability.ai`, `Authorization: Bearer <key>`. There is no
  OpenAI-compatible model list to probe.
- **Endpoints** (v2beta, multipart form bodies): `POST /v2beta/stable-image/generate/ultra`,
  `/generate/core` and `/generate/sd3`. With `Accept: application/json` a success carries a base64
  `image`, `seed` and `finish_reason` instead of raw image bytes; Stability returns no hosted URLs and one
  image per call.
- **Models.** Ultra and Core are selected by endpoint. The SD3 endpoint takes a `model` field:
  `sd3.5-large`, `sd3.5-large-turbo` or `sd3.5-medium`. `sd3.5-flash` is not in the live OpenAPI enum
  (checked 2026-06-12).
- **Fields.** `prompt` is required. Optional: `negative_prompt`, `seed`, `style_preset`, `output_format`
  (`png` default, `jpeg`, `webp`), `aspect_ratio` (`21:9`, `16:9`, `3:2`, `5:4`, `1:1`, `4:5`, `2:3`,
  `9:16`, `9:21`); SD3 also takes `cfg_scale`. Ultra and SD3 accept a source image with a numeric
  `strength` in 0..=1; SD3 then needs `mode=image-to-image` and takes no `aspect_ratio` (text-to-image is
  `mode=text-to-image`). Core is text-to-image only.
- **Content filtering.** `finish_reason = CONTENT_FILTERED` is documented as a delivered, blurred
  result, not a refusal.
- **Prices.** Credit based: 1 credit = $0.01 (platform pricing page). Successful generations cost Ultra 8
  credits, Core 3, SD3.5 Large 6.5, SD3.5 Large Turbo 4, SD3.5 Medium 3.5 (endpoint docs, captured
  2026-06-12).
