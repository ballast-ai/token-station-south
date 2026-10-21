# Frozen samples: Bailian managed video

Hand transcriptions from server `a4ea39cb`'s pure functions, from the official
API documentation's own examples, and from synthesised boundary cases — not
captured live traffic. `prepare`/`render` mirror the old managed-video behaviour;
the HappyHorse `usage.duration` and the UNKNOWN-expiry semantics come from the
official documentation as checked on 2026-09-20 (links in the design record);
the conflicting, zero, illegal-metering and path-anomaly cases are synthesised
negatives. The native reference and the Wasm guest share one pack, and
expectations must never be regenerated from the implementation under test —
doing so hides a regression instead of catching it.

The `wan3-*`, `wan27-*` and `wan26-image` samples and the corrected `wan-image`
expectation (wan2.7-i2v takes only `input.media[]`, never `input.img_url`) are
hand-written from the Wan3.0, Wan 2.7 image-to-video, reference-to-video and
video-editing API references as read on 2026-10-21
(`docs/design/2026-10-21-bailian-wan-media.md`); `wan3-reference-video` copies the
usage fields of the Wan3.0 reference, where `duration` is input plus output seconds.
