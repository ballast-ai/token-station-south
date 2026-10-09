# Frozen samples: Gemini native embeddings

Gate ② samples for `embeddings-gemini` (family `gemini`), judged by
`south.embeddings-component.v1` (`docs/design/2026-09-30-embeddings-contract.md` §10).

Expectations are hand-transcribed from token-station-server's native Gemini embeddings arm:
`proxy_gemini_embeddings` in `gateway/src/modules/inference/handler/embeddings.rs` (a string goes
to `…/v1beta/models/{model}:embedContent`, an array to `:batchEmbedContents`; the estimate is
`(utf8_len + 3) / 4` per text input; the immutable fields are `content`, `requests` and `model`),
`openai_embeddings_to_gemini` in `crates/gateway-provider-protocol/src/translate_gemini.rs` (the
`models/{model}` field, `content.parts[].text`, `outputDimensionality` per item, token ids
refused), the provider world's Gemini reference (`x-goog-api-key`, the error-status mapping),
plus synthesised boundary negatives. **Not captured live traffic, and expectations are never
regenerated from the implementation under test.**

Gemini reports no token counts (DE3 unmeasured), so every 2xx is `not_reported` and settles by the
fallback estimate the request carries; `max_input_tokens` equals it, so the reservation is
today's. `response.missing-usage` therefore expects `not_reported`, and no response case carries a
`usage_pointer`: there is no usage object to delete.

Estimates by hand: "The quick brown fox" is 19 bytes, (19 + 3) / 4 = 5; "alpha", "beta",
"gamma δ" are 5, 4 and 8 bytes (δ is two), 2 + 1 + 2 = 5; "dimensions sample" is 17 bytes, 5;
"a" and "b" are 1 each; "hello" is 5 bytes, 2; "abcd" is 1.

Intentional differences from the native arm (record §11), pinned here:
`response.batch-missing-embeddings` and `response.batch-empty-vector` are refused by the host's
extraction (`vector_not_found`, `invalid_vector`), where the native arm filled in an empty vector
and answered 200. `request.extra-fields` shows the unmodelled fields, `user` and
`encoding_format` ignored, as the native arm ignores them.

## Media inputs (embeddings contract 2, record §17)

The `request.media*` and `response.media*` cases use the multimodal model `gemini-embedding-2-preview`
(the model the native arm's own refusal message points to). Each media input becomes one request object
with one part, `{"inline_data": {"mime_type": <media type>, "data": <the client's string>}}`, exactly as
`embedding_input_to_gemini_part` in the native `translate_gemini.rs` builds it; text and media mix
freely in a batch, and `outputDimensionality` goes on every item.

Estimates by hand, from `estimate_media_tokens` and the text rule: an image or a PDF is 258, `audio/*` is
512, `video/*` is 1024, and "describe" is 8 bytes, (8 + 3) / 4 = 2. The mixed batch is therefore
2 + 258 + 512 + 1024 + 258 = 2054, and a single image is 258. `max_input_tokens` equals the estimate.

- `request.media`: one image, the row a package declaring `media` ships by name.
- `request.media-batch`: text, image, audio with a parameter, video and PDF in one array, with `dimensions`.
- `request.media-empty-payload`: `data:image/png;base64,` is a media input with an empty `data`, as it is
  natively; the upstream refuses it, the component does not.
- `request.media-verbatim-payload`: the payload is everything after the FIRST `;base64,`, passed through
  undecoded and unchanged, odd characters and a second marker included.
- `request.media-text-only-model`: `gemini-embedding-001` is text-only; the native arm refuses media for
  that one model by name, and so does the component, with a capability error. A known hard-code carried
  over from the native arm (record §17.5).
- `response.media`, `response.media-batch`: answers to the single and the batch request; the count and the
  vector length (`dimensions` 4) are checked as for text.

Intentional difference (record §17.9): a `data:…;base64,…` string whose media type breaks the §3 grammar
is text under contract 2, and natively was media.
