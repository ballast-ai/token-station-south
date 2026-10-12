# `fixtures-image-minimax`

Gate ② pack (`south.image-component.v1`) of `image-minimax`, image world step I3-1
(`docs/design/2026-10-12-image-i3-1-minimax-and-qwen.md` §2, §4).

The inputs were written from the record's account of the host's native MiniMax image arm (the server repository's
`handler/images/minimax.rs`, `minimax_status.rs` and the `GroupId` arm of `engine/upstream.rs`), not from captured
traffic. The `prepare.group-id-sent` and `response.base-resp-1026` expectations were written by hand before the reference
ran; the others were produced by the reference and reviewed against the record row by row.

Rows: every pre-dispatch refusal (prompt missing, not a string, elided, over 1500 characters; `n` zero, ten,
fractional, a string; `image_url` of the wrong type, scalar and in an array; a `tokens` metering requirement; a
multipart request); `GroupId` sent and absent; every forwarded and dropped field; `image_url` as one string and as an
array, wrapped as a `subject_reference`; `subject_reference` as sent; both artifact forms and their order; every
`base_resp` code class (1004, 2049, 1026, 2013, 1002, 1008, 2153, and an unlisted code); `images_reported` equal to,
one above and far above the delivered count, and absent.
