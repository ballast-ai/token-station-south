# xAI (video)

Upstream facts for the video family of [`task-xai-v2`](../../components/task-xai-v2/manifest.json). Moved
from the token-station-server host documentation on 2026-10-30. Image facts are in the
[image I3-7 record](../design/2026-10-15-image-i3-7-xai.md); speech facts are in the speech records.

- **Models and prices.** `grok-imagine-video` $0.05/s and `grok-imagine-video-1.5-preview` $0.08/s, billed
  on the returned `video.duration`. `grok-imagine-video-1.5-2026-05-30` is xAI's dated alias of the preview.
- **Image input is not fetched by xAI.** The image must be sent inline as an object with a nested URL,
  `"image": {"url": "data:{mime};base64,{data}"}`, not as a flat `image_url` string.
- **`grok-imagine-video-1.5-preview` is image-to-video only**, although its model page lists
  text-to-video: a text-only request is refused with 400 "Text-to-video is not supported for this model".
  The base model accepts text, image and video input.
- **Flow.** `POST /v1/videos/generations` returns a `request_id`; `GET /v1/videos/{request_id}` until
  `status == "done"` (`failed` and `expired` are terminal failures).
