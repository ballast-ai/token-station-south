# `fixtures-image-reve`

Gate ② pack (`south.image-component.v1`) of `image-reve`, image world step I3-6
(`docs/design/2026-10-13-image-i3-6-reve.md` §3 to §5, §12).

The inputs were written from the server repository's native Reve arm (`handler/reve.rs`: the generation and edit
bridges, their validators and body builders, `finish_reve_openai_surface`) and its unit tests, not from captured
traffic. Reve's success body is the shape those tests transcribe from Reve's documentation, which has not been checked
against a capture since Reve's 2026-08-14 API change (record §0.2); the error bodies (`error`, `message`, `detail`,
`error.message`) are unverified too. The endpoint pin (`endpoint`, RV-Q2 (c)) is written as the host's image-route
`request.set` would write it: a top-level member of the JSON view, a text part of the multipart view. Elided values are
host placeholders as `elide_v1` builds them. The `prepare.create-basic`, `prepare.edit-one-image`, `response.success`,
`response.violation-empty-image` and `render.success` expectations were written by hand from the native unit tests
before the reference ran; the others were produced by the reference and reviewed against the record row by row.

Rows: generation with the native unit tests' bodies (every pass-through field; explicit `aspect_ratio` and `version`
over `size` and the row), dropped fields, no auth slot, the wrong family, each metering requirement other than
`credits` and none, a request that is neither JSON nor multipart; the pin (absent, `null`, `create`, `edit`, `remix`,
unknown, upper case, a number, before `n`); `n` 2, `null`, `1.0`, `1.5`, `"1"`; `stream` `true`, `"true"`, `false`;
`output_format` `png`, `PNG`, `webp`, empty, a number, elided; every `prompt` refusal (missing, `null`, a number, blank,
elided) and a prompt sent untrimmed; the `size` table (each of the seven ratios, the tie `7x8`, the near tie `59x36`,
absent, `null`, a number, `auto`, `AUTO`, empty, `1024X1024`, `+5x5`, spaces around the `x`) and its refusals (`0x100`,
`100x0`, `huge`, `axb`, `10xb`, `-5x100`, `5x5x5`, elided); `aspect_ratio` winning over a bad `size`, unvalidated,
empty, not a string, elided; `version` explicit, empty, not a string, elided; the pass-through fields `null`, any JSON
value, elided; the refusal order. Edit and remix: one image, `image[]`, two images on an edit, none, every text field
(with the dropped `aspect_ratio`, `response_format`, `model`, `user`), a text part named `image`, an elided one; remix
with one, two, three mixed-name, six, seven and no images, and six 6 MiB images (prepared as usual: the host refuses the
expansion above 32 MiB before admission, record §6.3a); the pin missing, `create`, unknown, twice, as a file part;
`mask` as a file and as a text part; `n` 2, padded `1`, empty, `01`; `stream` `TRUE` and `1`; `output_format` padded
`PNG`, `webp`, empty; `prompt` missing, blank, a file part, elided; `size` tie, blank, `huge`, `0x100`;
`version` blank and padded; `test_time_scaling` `2.5`, `abc`, blank, `NaN` (sent as `null`), `postprocessing` an object,
not JSON, empty; extras on a remix; an elided `breadcrumb`; a `tokens` requirement; the refusal order. Responses: the
native success body; fractional, six-decimal, whole-float, exponent and zero meters; the violation flag missing, `null`
and a string; no metadata; a violation with an empty image and with an image; a violation without a meter and a success
without one (`missing` rows); a `null`, string, negative, seven-decimal and too-large meter; an empty, missing and
non-string image; a JSON array; a non-JSON 2xx; 400, 401, 402, 403, 408, 429, 500 and a 504 without a body; an edit
success and a remix violation. Renders: the success, a `null` violation flag, no metadata, an edit.
