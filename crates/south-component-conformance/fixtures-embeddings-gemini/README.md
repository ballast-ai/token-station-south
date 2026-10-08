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
