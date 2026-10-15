# `fixtures-image-xai`

Gate ② pack (`south.image-component.v1`) of `image-xai`, image world step I3-7
(`docs/design/2026-10-15-image-i3-7-xai.md` §2 to §4, §8).

The inputs were written from the server repository's native xAI arm (`handler/images/xai.rs`, its unit tests, the JSON
edit branch of `handler/images/handlers.rs` and the OpenAI-compatible pass-through of `handler/images/openai_compat.rs`),
not from captured traffic; the success and error bodies follow the shape those tests use and are unverified against a
capture. Source-image strings are host placeholders, as `elide_v1` builds them for the declared request elision paths.
The `prepare.generate-standard-defaults`, `prepare.generate-v2-auto`, `prepare.edit-image-url-array-to-images` and
`response.url-one` expectations were written by hand before the reference ran; the others were produced by the reference
and reviewed against the record row by row.

Rows: the six declared ids, an undeclared one and the wrong family; generation defaults, `size` and `resolution`
normalisation (precedence, spellings, empty, `null`, a bad `size` beside a valid `resolution`) and its refusals (an
unknown word, a non-string, an elided value); 2.0 quality pinned (`low`, an untrimmed `Medium`), `auto`, absent and
`null` (candidates with the generation default) and refused (`high`, a number, elided); `quality` passed through on
another id; pass-through of untouched fields with elided strings; every `n` case (`null`, whole float, 0, 11, a
fraction, a string, above `u32`) and the refusal order; edits with `image`, `image_url` as a string and an array,
`image_urls`, five images on 2.0 (candidates with the edit default), a pinned quality, every alias and count refusal,
four images on the standard id (the maximum is catalog data), 65 images, `size` dropped, `image` as a string and as an
array, a multipart edit, a `tokens` requirement and no auth slot. Responses: `url` and `b64_json` images, mixed, an entry
with both, quotes recognisable as a 2.0 tier and not, the tick conversions (one tick, zero, whole dollars, a float, a
string, a negative and an out-of-range number, absent), entries without an image, empty, missing or non-array `data`, a
JSON array, and 400, 401, 403, 429, 500 and a 503 without a body. Renders: `url`, `b64_json`, mixed, both, and an edit.
