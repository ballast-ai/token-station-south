# Google Gemini API and Vertex AI

Upstream facts for the Google packages: [`provider-gemini`](../../components/provider-gemini/manifest.json),
[`provider-vertex`](../../components/provider-vertex/manifest.json),
[`task-veo-v2`](../../components/task-veo-v2/manifest.json),
[`image-gemini`](../../components/image-gemini/manifest.json),
[`image-vertex`](../../components/image-vertex/manifest.json),
[`embeddings-gemini`](../../components/embeddings-gemini/manifest.json) and
[`embeddings-vertex`](../../components/embeddings-vertex/manifest.json). Moved from the
token-station-server host documentation on 2026-10-30. The design records are authoritative for what
each package sends and parses; where these notes disagree with a record, the record wins.

Related records: [Gemini provider component](../design/2026-08-22-gemini-provider-component.md),
[provider-vertex](../design/2026-10-15-provider-vertex.md),
[Veo on Vertex](../design/2026-10-15-veo-vertex-task-component.md),
[image I3-2 Gemini](../design/2026-10-13-image-i3-2-gemini.md),
[image I3-3 Vertex](../design/2026-10-13-image-i3-3-vertex.md),
[embeddings contract](../design/2026-09-30-embeddings-contract.md),
[Gemini tool-call signatures](../design/2026-10-26-gemini-tool-call-signatures.md).

## 1. Veo (Gemini API)

- **Long-running operations.** Submission is `:predictLongRunning`, not `generateContent`. It returns an
  operation name that is model-scoped — `models/{model}/operations/{id}`, not a bare `operations/{id}` —
  and is polled verbatim at `GET /v1beta/{name}` until `done: true`. The result is at
  `response.generateVideoResponse.generatedSamples[].video.uri`; that URI is fetched with the API key
  (`?key=…`).
- **Images use the Vertex shape.** `instances[0].image`, `instances[0].lastFrame` and each
  `instances[0].referenceImages[].image` are flat `{bytesBase64Encoded, mimeType}` objects; `inlineData`
  is refused ("inlineData isn't supported by this model").
- **Three modes on one model id** (`veo-3.1-generate-preview`, `-fast-`): `image` alone animates the image,
  `image` + `lastFrame` interpolates between first and last frame, `referenceImages` (at most 3,
  `referenceType: "asset"`; style references are not supported; base and fast only, Lite refuses) guides
  the subject. Reference-to-video is limited to 16:9 and 8 s.
- **First-and-last-frame is 8 s only**, although the `durationSeconds` matrix in the docs does not say so:
  any other duration is refused with 400 INVALID_ARGUMENT "Your use case is currently not supported.
  Please refer to Gemini API documentation for current model offering." (developer forum
  discuss.ai.google.dev/t/107529; reproduced with a 4 s request on `veo-3.1-fast-generate-preview`).
- **Defaults and prices.** Duration defaults to 8 s and resolution to 720p. Veo 3.1 is priced per second;
  `veo-3.1-lite-generate-preview` lists $0.05/s at 720p and $0.08/s at 1080p.

## 2. Nano Banana image models

- Ids: `gemini-3.1-flash-image` (Nano Banana 2), `gemini-3.1-flash-lite-image` (Nano Banana 2 Lite),
  `gemini-3-pro-image` (Nano Banana Pro). `gemini-2.5-flash-image` (Nano Banana, priced flat at
  $0.039 per image) shut down on 2026-10-02.
- `POST …/models/{model}:generateContent` with `generationConfig.responseModalities = ["IMAGE"]`. The prompt
  is a `{"text": …}` part; an edit sends the source image as an `inlineData` part before the text. One call
  returns one candidate; images come back as `candidates[].content.parts[].inlineData.data`, possibly next
  to a short text part when text output is also allowed. A response without candidates or without any
  `inlineData` part delivered no image.
- **Usage** is modality-tagged: `usageMetadata.promptTokensDetails[]` / `candidatesTokensDetails[]`
  entries like `{"modality": "IMAGE", "tokenCount": 1120}`, plus top-level `promptTokenCount`,
  `candidatesTokenCount` and `cachedContentTokenCount`.
- **Prices** per million tokens (ai.google.dev/gemini-api/docs/pricing, checked 2026-09-14): 3.1 Flash
  Image $0.50 text and image input, $60 image output; 3 Pro Image $2.00 input, $120 image output; 3.1 Flash
  Lite Image $0.25 input, $30 image output.

## 3. Gemini 3.x text models

- Thinking models (`gemini-3.5-flash`, thinking on by default; `gemini-3.1-pro-preview`;
  `gemini-3-flash-preview`) report reasoning as `usageMetadata.thoughtsTokenCount`, separately from
  `candidatesTokenCount`, billed at the output rate. Tool-use prompt tokens are reported as
  `toolUsePromptTokenCount`. `generationConfig.thinkingConfig = {thinkingLevel: "minimal"}` turns thinking
  down per request.
- **Streaming usage.** Vertex puts `usageMetadata` on every chunk (intermediate ones may carry only
  `{"trafficType": "ON_DEMAND"}`; other upstreams send running counts). The terminal frame is the one with
  `finishReason`, not the first one with `usageMetadata`; counts must never decrease, and a count the
  terminal frame omits is zero.
- **Cache reads** on the 3.x series are 10% of the input rate (they were 25% on the 2.5 series).
- `gemini-3.1-pro-preview` input is $2.00/M at ≤ 200K input tokens and $4.00/M above (its predecessor
  `gemini-3-pro-preview` was sunset on 2026-03-09).

## 4. Embeddings

- Native endpoints only: `:embedContent` (one input) and `:batchEmbedContents` (`requests[]`); the
  OpenAI-compatible layer does not serve embeddings. Auth is `x-goog-api-key`.
- `gemini-embedding-001` is text only (2K context); `gemini-embedding-2-preview` also takes images,
  audio, video and PDF as `inline_data` (8K context); media sent to the text-only model is refused with
  400. `outputDimensionality` takes 128–3072. Results are `embedding.values` or `embeddings[].values`.
- **No token counts are returned for embeddings.**
