# Frozen samples: Bailian Wanxiang 2.7 images (asynchronous)

Expectations are hand-transcribed from token-station-server's native Wanxiang
image arm (`handler/images/bailian.rs` — `build_bailian_image_body`,
`validate_bailian_image_n`, `openai_size_to_wan_size`,
`extract_bailian_image_urls`, `trusted_image_count`,
`bailian_image_result_to_openai`; `images/durable.rs::parse_wan_create`;
`images/observe.rs::normalize_bailian_wan`; and the blocking surface's
`code: detail` failure wording). **Expectations are never regenerated from the
implementation under test.** Not captured live traffic.

Where contract 6 lands in this family: `n` becomes `requested_outputs`, while
`usage.outputs` is the **billed image count** — the upstream's positive
`usage.image_count`, capped at delivered + 1, and otherwise the delivered count.
That is deliberately separate from the number of artifact URLs: when the upstream
reports one more image than the URLs we could extract, the billed count is the
reported one, which is the host's existing "trust the upstream's count" contract.
A SUCCEEDED state carrying zero artifacts is a terminal failure.
