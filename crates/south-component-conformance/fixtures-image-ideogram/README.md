# `fixtures-image-ideogram`

Gate ② pack (`south.image-component.v1`) of `image-ideogram`, image world step I3-5
(`docs/design/2026-10-13-image-i3-5-ideogram.md` §2 to §4, §8).

The inputs were written from the server repository's native Ideogram arm (`handler/images/ideogram.rs`), its unit
tests (`handler/images/tests/ideogram.rs`) and its integration tests (`tests/images_ideogram_stability.rs`), not from
captured traffic; the success body follows the shape those tests use, and the error bodies (`error`, `message`,
`detail`, `error.message`) are unverified against a capture. Elided values are host placeholders as `elide_v1` builds
them. The `prepare.turbo-url-generation`, `response.unsafe-mixed` and `render.b64-json` expectations were written by
hand before the reference ran; the others were produced by the reference and reviewed against the record row by row.

Rows: each of the three ids (`rendering_speed` TURBO, DEFAULT, QUALITY) and two undeclared ids (FLASH, v3, the second
also showing the upstream id is judged before `prompt`); the `size` table (absent, `null`, empty, `auto`, `AUTO`, a
member, a member with surrounding spaces, `1024x1024`, `1536x512`, `1000x1000`, `1792x1024`, `1024x1792`, `1536x1024`,
`+5x5`, spaces around the `x`) and its refusals (`1024X1024`, `1024`, `axb`, `0x1024`, `1024x0`, `-5x100`, `5x5x5`, a
number, an elided string); every `prompt` refusal (missing, `null`, a number, an array, blank, elided) and a prompt sent
untrimmed; `n` 2, `null`, `1.0` and `1.5`; every `response_format` acceptance (`url`, `b64_json`, `null`, `{"type":
"url"}`, `{"type": "b64_json"}`, `{}`, `{"type": 5}`) and refusal (`{"type": "png"}`, `"png"`, `5`, `["url"]`, an
elided string or `type`); the refusal order after the upstream id; dropped fields; a multipart request, a `tokens`
requirement, the wrong family and no auth slot. Responses: one safe entry, `b64_json` asked, a missing or non-string
`prompt`, no safety flag, unsafe entries beside a safe one (either order, with and without a URL), two safe entries, an
unsafe entry with a URL alone, the all-filtered answer, an empty, `null`, non-string or missing `url`, an empty, missing,
`null` or non-array `data`, a JSON array, a non-JSON 2xx, and 400, 401, 403, 422, 429, 500 and a 504 without a body.
Renders: `url`, `b64_json`, two entries, and a missing rewritten prompt.
