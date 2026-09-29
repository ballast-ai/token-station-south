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
