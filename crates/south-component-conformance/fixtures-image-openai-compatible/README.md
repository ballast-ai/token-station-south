# `fixtures-image-openai-compatible`

Gate ② pack (`south.image-component.v1`) of `image-openai-compatible`, the host's OpenAI-compatible image fallback as a
package (`docs/design/2026-10-16-image-openai-compatible.md` §2 to §5, §7; step 1: the `openai-compatible`,
`azure-openai-v1` and `azure-openai-dated` families).

The inputs were written from the server repository's native fallback arm (the strict `n` and the generic multipart edit
pass-through of `handler/images/handlers.rs`, `handler/images/openai_compat.rs`, the owned sets of
`handler/images/execute.rs`, the size hint of `engine/token_counter/media.rs`, `ImageTokenUsage::from_openai_usage` and
the `AzureOpenai` arm of `engine/upstream.rs`), not from captured traffic; the success and error bodies follow the
OpenAI images shape and are unverified against a capture (record U1 to U6). The expectations were written from the
record by an independent transcription, before the reference ran; the reference matched every row on its first run.

Rows: capabilities for each family (every configured id declared), no models and the wrong family; the URL roots of
record §3 (GLM, BytePlus and DeepInfra endpoints with a path, an OpenAI origin), the two D13 shapes (an `openai` row
whose endpoint has a path, a GLM row given an origin) and a trailing slash; `azure-openai-v1` and `azure-openai-dated`
with the default and a declared `api-version`; a row without an auth slot; the body passed through with `model`
replaced, unknown fields kept and elided strings (a reference image data URL under `/image` and `/image/*`, an oversized
prompt, a nested value) sent `as_is`; every `n` case (`null`, a whole float, 10, 0, 11, a fraction, a string, a
negative, above `u32`); the size-hint table (`1K`, `2k`, `×`, `X` with spaces, the `image_size` fallback, `size` beside
`image_size`, a non-string or `null` `size` with no fallback, `auto`, `0x0`, three parts); a `credits` requirement, a
body that is not an object, a request that is neither JSON nor multipart and the wrong family; multipart edits (parts
in order with `model` replaced or appended, `image`, `image[]` and `mask` counted, `n` trimmed, `stream=false`
forwarded) and their refusals (`n` not a number, empty, 0, 11, `n` before `stream`, `stream` `TRUE` and `1`, an elided
text part), on both Azure families too. Responses: `b64_json`, `url`, mixed, both in one entry, an empty `b64_json`
beside a `url`, `output_format` `webp` / `jpeg` / `png`, an edit with two images, an Azure dated row; D1 / D2 (empty,
missing or non-array `data`, an entry without an image, an empty `url`, a non-object entry, a JSON array, a non-JSON
2xx); token rows (details, top-level input only for D11, top-level cached, details cached winning, output from its
details, integral floats for D9, a negative, a fraction and a string for D10, output missing and no usage for D12, the
Seedream shape for OCI-Q4, an edit); images rows with well-formed and malformed usage; and 400 (nested, string `error`),
404 (`message`), 422 (`detail`), 401, 403, 429, 408, 500 and a 503 without a body. Renders: `b64_json`, `url`, mixed,
both in one entry, a token row with its usage, an edit and an `output_format`.
