# Frozen samples: OpenAI-compatible embeddings

Gate ② samples for `embeddings-openai-compatible` (families `openai-compatible` and
`azure-openai-v1`), judged by `south.embeddings-component.v1`
(`docs/design/2026-09-30-embeddings-contract.md` §10).

Expectations are hand-transcribed from token-station-server's native OpenAI-compatible
embeddings arm (`gateway/src/modules/inference/handler/embeddings.rs`: the client body is
forwarded with only `model` replaced, the upstream bytes are returned unchanged, settlement reads
`usage.prompt_tokens` and refuses a 2xx without it, the immutable fields are `input` and `model`),
the provider world's `azure-openai-v1` reference (GA v1 surface, `api-key` header) and its error
wording, plus synthesised boundary negatives. **Not captured live traffic, and expectations are
never regenerated from the implementation under test.**

Every request case runs through the component; a response case names the request case it
answers, and the suite extracts and erases the vectors with that request's locator before the
component sees the skeleton, then applies the host's consistency checks — so
`host_refusal` expectations (`vector_count_mismatch`, `dimensions_mismatch`, `invalid_index`) pin
what a host decides, not only the component.

Notes on individual cases:

- `request.batch-text` and `request.dimensions` send `encoding_format` and `user` explicitly;
  `request.single-text` sends neither, and the body carries neither, as the client sent it.
- `request.refused-capability` mixes text and token-id inputs, which no northbound body parses to
  and the OpenAI shape cannot carry.
- `request.origin-only-endpoint` pins the API root rule shared with the provider world's
  `resolve`: an origin-only endpoint gains `/v1`.
- `response.base64-without-index` omits `index` on every item (body order holds) and carries
  base64 vectors (1.0, 2.0 and 3.0, 4.0 as little-endian f32).
- `response.mixed-index` carries `index` on one item only, which the host refuses.
- `error.timeout` (408) is `unknown`; every other 4xx is `rejected`, a 5xx is `unknown`.
