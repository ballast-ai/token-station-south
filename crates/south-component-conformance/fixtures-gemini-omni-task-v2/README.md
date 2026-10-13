# Frozen samples: Gemini Omni video over the Interactions API (task contract 8)

Design record: `docs/design/2026-10-01-omni-task-component.md` (accepted 2026-10-13; provisional items pending a
real-account test). **Expectations are written by hand from the record and the sources below, never regenerated from the
implementation under test.**

Sources:

- **`vertex-interactions-video`**: the host team's Vertex AI samples of 2026-09-30 / 2026-10-02 (O0-b, O0-c; record
  §1.2 and §4.5). The `succeeded` row is sample s2 (one thought, then the clip; usage 17 + 57,920 + 234), the
  `succeeded-get` row sample s3 (a leading `user_input` step and three thoughts; usage 1 + 57,920 + 739), the `accepted`
  row the O0-c `background: true` answer, the `progress` and `succeeded-images` rows the O0-c image-to-video poll
  (input text 6 and image 22,000). Every observation input is the **elided view** a host hands the component: the clip
  and the echoed images are `$south.blob` placeholders (sizes from the samples; `head` is a placeholder), the thought
  signatures are truncated, the project id and interaction id are replaced. The two 400 rows are the measured error
  shapes: a JSON `error` object, and an SSE `event: error` frame under `Content-Type: application/json`.
- **`gemini-interactions-video`** (provisional): not sampled. Transcribed from token-station-server's native Omni arm
  (`handler/video/gemini.rs` — `build_omni_interaction_body`, `omni_has_unsupported_visual_input`,
  `extract_omni_video_data`, `proxy_gemini_omni_video`): `POST {base}/v1beta/interactions` with `x-goog-api-key`,
  `delivery: "uri"`, no `background`; a synchronous `completed` answer whose Files API `uri` is a credential-gated URL
  artifact; every visual input key refused. The usage of its rows reuses the Vertex sample's shape.

Rows the record requires by name are listed in `tests/gemini_omni_task_suite_v2.rs`. The three `.meta.json` sidecars
name the usage object and each required bucket's wire member for `UsageNeverDefaulted` and `MissingMeterIsNotZero`.
