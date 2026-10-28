# `fixtures-image-qwen`

Gate ② pack (`south.image-component.v1`) of `image-qwen`, image world step I3-1
(`docs/design/2026-10-12-image-i3-1-minimax-and-qwen.md` §3, §4).

The inputs were written from the record's account of the host's native synchronous Bailian arm (the server repository's
`handler/images/bailian.rs`), not from captured traffic. The `prepare.text-defaults` expectation was written by hand before
the reference ran; the others were produced by the reference and reviewed against the record row by row.

Rows: every pre-dispatch refusal (prompt missing or elided; `n` zero, seven, fractional; `image_url` of the wrong
type, as an object and in an array; a `tokens` metering requirement); every forwarded parameter and the dropped fields;
`size` as `WxH`, as `W*H` and as a word (no fact); the resolution tier word (0.79.0, south #297) by pixel count on
either side of 2048 × 1024 and as a size word (`2k`, and `4K`, which the card does not price); `qwen-image-edit-plus` with three images, two of them elided; images
flattened across choices with empty ones skipped; a 2xx carrying a DashScope `code`; non-2xx answers including content
moderation; `images_reported` equal to, one above and far above the delivered count, and absent.
