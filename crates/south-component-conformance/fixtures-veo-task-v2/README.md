# Frozen samples: Google Veo video (the Gemini API line)

Expectations are hand-transcribed from token-station-server's native Veo arm
(`handler/video/gemini.rs` — `build_veo_submit_body`, `build_veo_instance`,
`encode_veo_image_part`; `durable.rs::parse_veo_create`, which feeds a receipt
straight through only when it carries no LRO name *and* is genuinely terminal;
`observe.rs::normalize_veo` for its three fallback keys and the RAI filter; and
the failure wording of `VideoTaskAdapter for Veo`). **Expectations are never
regenerated from the implementation under test.** Not captured live traffic, and
the base64 payloads are placeholder strings.

Where contract 6 lands in this family: the credential travels in the
`x-goog-api-key` header; every artifact is `fetch_with_credential: true` (the
rendered body still carries the upstream URI, so the host must rewrite it to its
own proxy path); `sampleCount` is `requested_outputs` and the number of delivered
samples is `usage.outputs`; a receipt is already terminal, so
`accepted-terminal`.

Differences from the native arm: an input image must already be a `data:` URI
(the host pre-fetches it), and a plain URL is refused with a 400 that says where
to go instead; a non-positive-integer duration is a 400, where the native arm
silently dropped it and reserved against the default 8 seconds; no declared
seconds are reported when the request carries no duration; and a Vertex operation
name (`projects/…`) is refused — that line needs service-account minting and a
region-derived endpoint, which are outside this component.
