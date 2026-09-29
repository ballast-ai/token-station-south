# Frozen samples: BytePlus Seedance video

Expectations are hand-transcribed from token-station-server's native BytePlus arm
(`handler/video/byteplus.rs` for the submit body and the success rendering,
`durable.rs::parse_byteplus_create` and `byteplus_billing_tokens`,
`observe.rs::normalize_byteplus`, and the failure wording of
`VideoTaskAdapter for BytePlus`). The per-second token rates are worked out by
hand as `width × height × 24 ÷ 1024` (720p 21600, 480p 10044, 1080p 48960,
4K 194400). **Expectations are never regenerated from the implementation under
test.** Not captured live traffic.

Differences from the native arm: the reference-image ceiling is Seedance's
highest, 30 — a lower per-model ceiling is the host capability pre-check's job;
and a negative or non-integral `completion_tokens` is reported as unknown rather
than settled as zero.

The upstream `last_frame_url` is reported under task contract 7 as a second
artifact with `role = last_frame` (`observation.succeeded`), and written back to
`data[0].last_frame_url` when rendering (`render.direct`), which matches the
native blocking body. An observation carrying no last frame renders without the
extra entry (`render.no-last-frame`), and a last frame ordered ahead of the video
is a protocol error (`render.frame-before-video`).
