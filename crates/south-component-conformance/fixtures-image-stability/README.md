# `fixtures-image-stability`

Gate ② pack (`south.image-component.v1`) of `image-stability`, image world step I3-4
(`docs/design/2026-10-13-image-i3-4-stability.md` §2 to §4, §8).

The inputs were written from the server repository's native Stability arm (`handler/images/stability.rs`), its unit
tests (`handler/images/tests/stability.rs`) and its integration tests (`tests/images_ideogram_stability.rs`), not from
captured traffic; the error bodies follow Stability's documented `{"id", "name", "errors"}` shape (unverified against a
capture). Source images are host placeholders as `elide_v1` builds them (the first 64 bytes as `head`, the decoded
length as `bytes`). The `prepare.sd3-large-turbo-image-url-to-image`, `prepare.core-with-a-source-image` and
`response.content-filtered` expectations were written by hand before the reference ran; the others were produced by the
reference and reviewed against the record row by row.

Rows: each of the five ids (path, `model` / `mode` parts) and an undeclared id; text-to-image and image-to-image on Ultra
and SD3 (`image_url` as a string and as `{"url"}`, `image` as a string and as `{"url"}`; `image/png`, `image/jpg`,
`image/jpeg`, `image/webp`); `size` to the nearest ratio, `auto`, empty, a native ratio, either case of `x`;
`aspect_ratio` over `size` and a `null` `aspect_ratio`; Ultra image-to-image ignoring `size` / `aspect_ratio`;
`output_format` png, jpeg, webp; dropped fields; `response_format` accepted forms; the 10 MiB payload-length edge. Every
refusal of §3: `prompt` (missing, `null`, not a string, blank, elided), elided `negative_prompt`, every `response_format`
refusal, `n` 2, a multipart request, a `tokens` requirement, the wrong family, both source fields, each wrong source
shape, a non-`data:` URL, no `;base64,`, no media type, an unsupported type, an empty payload, a payload over the limit,
Core with an image, each `mode` combination, `output_format`, `style_preset`, `seed`, `strength`, `cfg_scale` and the
SD3 image-to-image `size` / `aspect_ratio` refusal. Responses: `SUCCESS`, `CONTENT_FILTERED`, each output type, a missing,
empty or non-string image, a missing or unknown finish reason, a non-JSON 2xx, a JSON array, and 400, 401, 402,
403 `content_moderation`, 404 without a name, 413, 429, 500 and a 504 without a body.
