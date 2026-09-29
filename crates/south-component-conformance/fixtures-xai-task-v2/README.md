# Frozen samples: xAI video

Expectations are hand-transcribed from token-station-server's native xAI arm
(`handler/video/xai.rs` for construction and the success rendering,
`durable.rs::parse_xai_create`, `observe.rs::normalize_xai` for its four-state
normalisation, and the failure wording of `VideoTaskAdapter for Xai`), plus the
xAI documentation's 1–15 second duration range and its ceiling of 7 reference
images, plus synthesised boundary negatives. **Not captured live traffic, and
expectations are never regenerated from the implementation under test.**

Differences from the native arm (the component's design record is the header
comment of `src/reference_xai_task_v2.rs`): input images are forwarded as-is
rather than fetched on the caller's behalf; an out-of-range duration is a 400 at
the component instead of being handed to the upstream to refuse; no declared
seconds are reported when the request carries no duration, so the reservation
follows the host's default estimation policy; `model` and `duration` are declared
immutable body paths; and one submission yields one output, reported on success
as `outputs = 1`.
