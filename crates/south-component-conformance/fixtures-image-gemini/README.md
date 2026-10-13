# `fixtures-image-gemini`

Gate ② pack (`south.image-component.v1`) of `image-gemini`, image world step I3-2
(`docs/design/2026-10-13-image-i3-2-gemini.md` §2 to §4, §9).

The inputs were written from the record's account of the host's native Gemini image arm (the server repository's
`handler/images/gemini.rs`, `GEMINI_IMAGE_OWNED` in `images/execute.rs`, and the Gemini cases of its image tests), not
from captured traffic. The `prepare.generate-image-url-png`, `prepare.generate-resolution-images-form` and
`response.prompt-blocked` expectations were written by hand before the reference ran; the others were produced by the
reference and reviewed against the record row by row.

Rows: every pre-dispatch refusal of generation (prompt missing, not a string, elided; `n` zero, eleven, fractional, a
string; `image_url` as an `http` URL, without `;base64,`, without a media type, an array, a number, an unelided URL with
bad base64; `aspect_ratio` and `resolution` of the wrong type; a resolution outside the tier grammar on a per-image
call; a `credits` requirement) and of edit (`mask`, `stream` true or `1`, `n` zero, eleven, not a number, missing
prompt, no `image` file part, an elided text field); `image_url` absent, `null`, a PNG and a JPEG data URL (sent as a
`data_url_payload` reference) and an unelided data URL; `resolution` lower-case, with `tokens`, `images` and both
required (the tier word only when `images` is, GI-Q3); `aspect_ratio`; `n` 1, 2.0, 3 and 10 as `repeat` (GI-Q2);
dropped fields; no auth slot; an edit with PNG, JPEG, `application/octet-stream` and unlabelled image parts (GI-Q6),
two image parts, every text field, `n` empty and ten. Responses: usage with and without a closing modality split, text
output tokens, cached and thinking tokens (GI-Q4), a `thought` image (GI-Q7), JPEG and untyped outputs, two
candidates, an empty image part, each declared bucket missing (`missing` rows), no usage under `tokens` and under
`images` (`absent` row for `images_reported`), negative, fractional, integral-float and malformed detail counts, a
blocked prompt, an `IMAGE_SAFETY` and a `NO_IMAGE` stop, a text-only answer, no candidates (GI-Q5), a non-JSON and an
array 2xx, and 400, 403, 404, 429, 500, 503 and a 504 without a body; an edit answer; three rounds rendered together.
