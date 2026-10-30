# Ideogram

Upstream facts for [`image-ideogram`](../../components/image-ideogram/manifest.json). Moved from the
token-station-server host documentation on 2026-10-30. The
[image I3-5 record](../design/2026-10-13-image-i3-5-ideogram.md) is authoritative for what the package
sends and parses; where these notes disagree with it, the record wins.

- **Host and auth.** `https://api.ideogram.ai`, key in an `Api-Key` header (not `Authorization: Bearer`;
  header names are case-insensitive). There is no model-list endpoint to probe.
- **Ideogram 4.0** (launched 2026-06-03): `POST /v1/ideogram-v4/generate`, synchronous, with a
  `multipart/form-data` body even for text-only generation. Fields checked on 2026-06-04: `text_prompt` or
  a structured `json_prompt`, `resolution` (a 23-value 2K-class enum), `rendering_speed` (`FLASH`,
  `TURBO`, `DEFAULT`, `QUALITY`), `enable_copyright_detection`. Version 4 has no image count, seed, aspect
  ratio, style or negative prompt (those are version 3 fields), so one call yields one image. Edit and
  remix exist only on version 3 endpoints.
- **Response.** `{"created": "<ISO 8601>", "data": [{url, prompt, resolution, is_image_safe, seed}]}`.
  The URLs are pre-signed and short-lived, fetched without an auth header. Ideogram rewrites prompts
  server-side and returns the final text in `prompt`.
- **Safety.** An unsafe prompt is refused with HTTP 422. An unsafe generated image comes back in a 200
  with that entry's `url` empty or null and `is_image_safe` false.
- **Prices** (ideogram.ai/features/api-pricing, captured 2026-06-04; the figures are in CSV assets the
  page loads, not in its HTML): 4.0 Turbo $0.03, Default $0.06, Quality $0.10 per image. Version 3 tiers
  are $0.03 / $0.06 / $0.09 plus character-reference surcharges; upscale, describe and background removal
  are priced per input. Each `rendering_speed` is its own price, which is why each is served as its own
  model id.
- **Limits.** 10 concurrent in-flight requests on the default tier.
