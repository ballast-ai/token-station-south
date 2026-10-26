# Release index

This page is the concise index from repository releases to their authoritative design records.
Release records preserve historical detail; [compatibility.json](../compatibility.json), not a tag
or this index, is the authority for host adoption and verification status.

The current repository release is `0.77.0`. Records may describe a prepared but unpublished tag;
their status line remains authoritative for publication state.

## Current releases

| Release | Current capability | Records |
|---|---|---|
| `0.77.0` | Gemini 3 tool-call signatures through the canonical IR (kernel protocol 0.7.0) and the Chat Completions wire, and parallel Gemini tool results in one turn; prepared as recorded, with publication and host re-pin separate | [release](design/2026-10-26-release-0.77.0.md), [design](design/2026-10-26-gemini-tool-call-signatures.md) |
| `0.76.0` | Bailian bounded reasoning caps, Qwen ASR, and Wan media tasks; prepared as recorded, with publication and host re-pin separate | [release](design/2026-10-21-release-0.76.0.md), [reasoning cap](design/2026-10-21-bailian-thinking-cap.md), [Qwen ASR](design/2026-10-21-speech-qwen-asr.md), [Wan media](design/2026-10-21-bailian-wan-media.md) |
| `0.75.0` | Kiro inline image input; publication and host re-pin remain separate as recorded | [release](design/2026-10-19-release-0.75.0.md), [design](design/2026-10-19-kiro-image-input.md) |
| `0.74.0` | Family model parameters, credential attributes, and the task-v2 value channel | [release](design/2026-10-18-release-0.74.0.md), [model parameters](design/2026-10-18-family-model-parameters.md), [value channel](design/2026-10-18-section-attributes-and-task-values.md) |
| `0.73.1` | Messages pass-through delivery-suite correction | [release](design/2026-10-18-release-0.73.1.md) |
| `0.73.0` | Messages pass-through, forwarded client headers, and count-tokens declarations | [release](design/2026-10-17-release-0.73.0.md), [design](design/2026-10-17-messages-passthrough-and-count-tokens.md) |
| `0.72.0` | Explicit Bailian cache-read accounting | [release](design/2026-10-17-release-0.72.0.md), [design](design/2026-10-17-explicit-cache-reads.md) |
| `0.71.0` | Vertex credentials declared by task packages | [release](design/2026-10-16-release-0.71.0.md), [design](design/2026-10-16-task-vertex-credentials.md) |
| `0.70.0` | Kimi Code provider family and OpenAI-compatible image package | [release](design/2026-10-16-release-0.70.0.md) |
| `0.69.0` | GLM Coding Messages behavior and shared component build target | [release](design/2026-10-16-release-0.69.0.md) |
| `0.68.0` | OpenAI-compatible T9 request follow-ups | [release](design/2026-10-16-release-0.68.0.md) |
| `0.67.0` | Azure speech families, Vertex polling correction, and immutable request headers | [release](design/2026-10-15-release-0.67.0.md) |
| `0.66.0` | Additional provider, task, image, and speech packages migrated from native hosts | [release](design/2026-10-15-release-0.66.0.md) |
| `0.65.0` | Speech packages and speech/media contract completion | [release](design/2026-10-15-release-0.65.0.md) |
| `0.64.0` | Speech world groundwork and task immutable paths | [release](design/2026-10-14-release-0.64.0.md) |
| `0.63.0` | Vertex image package and task poll-interval declaration | [release](design/2026-10-14-release-0.63.0.md) |
| `0.62.0` | Kiro provider and Gemini Omni task packages | [release](design/2026-10-14-release-0.62.0.md) |
| `0.61.0` | Ideogram image package and north-codec stream rendering | [release](design/2026-10-13-release-0.61.0.md) |
| `0.60.0` | Stability image package and GenerateContent pass-through | [release](design/2026-10-13-release-0.60.0.md) |
| `0.59.0` | Gemini image package and media contract 2 | [release](design/2026-10-13-release-0.59.0.md) |
| `0.58.0` | MiniMax and Qwen image packages | [release](design/2026-10-13-release-0.58.0.md) |
| `0.57.0` | Kernel canonical-IR update and provider request coverage | [release](design/2026-10-12-release-0.57.0.md), [kernel design](design/2026-10-12-kc1-canonical-ir-4-south.md) |
| `0.56.0` | Responses Codex family and request fields | [release](design/2026-10-12-release-0.56.0.md), [request design](design/2026-10-12-responses-r3-codex-and-request-fields.md) |
| `0.55.0` | Responses reasoning replay and pass-through delivery | [release](design/2026-10-12-release-0.55.0.md) |
| `0.54.0` | OpenAI Responses component and host credential-arm evidence | [release](design/2026-10-10-release-0.54.0.md) |
| `0.53.0` | Host-only bounded SSE grammar | [release](design/2026-10-10-release-0.53.0.md) |
| `0.52.0` | Image and media worlds | [release](design/2026-10-10-release-0.52.0.md) |
| `0.51.1` | Runtime recovery after guest traps | [release](design/2026-10-09-release-0.51.1.md) |
| `0.51.0` | Inline media for embeddings | [release](design/2026-10-09-release-0.51.0.md) |
| `0.50.0` | Model catalog and independent guest-linked crate versions | [release](design/2026-10-08-release-0.50.0.md) |
| `0.49.0` | Terminal-chunk Gemini stream usage | [release](design/2026-10-08-release-0.49.0.md) |
| `0.48.0` | Embeddings value channel and Vertex embeddings | [release](design/2026-10-08-release-0.48.0.md) |
| `0.47.0` | Embeddings adapter world | [release](design/2026-10-08-release-0.47.0.md) |
| `0.46.0` | Kernel re-pin, combined auth, and declared value channel | [kernel record](design/2026-10-08-kernel-repin-protocol-0.5.0.md) |
| `0.43.0`–`0.45.0` | Host-zero-vendor package, credential, runtime-floor, signing, and framing boundaries | [boundary record](design/2026-09-30-host-zero-vendor-boundary.md) |
| `0.40.0`–`0.42.0` | Usage partitioning and Claude model dialect refinements | [Claude dialect](design/2026-09-29-claude-model-dialect.md) |
| `0.39.0` | Claude reasoning replay | [release](design/2026-09-29-release-0.39.0.md) |
| `0.37.0` | Responses north codec | [release](design/2026-09-28-release-0.37.0.md), [design](design/2026-09-28-responses-north-codec.md), [validation](design/2026-09-28-responses-north-codec-validation.md) |
| `0.32.0`–`0.36.0` | Runtime and package behavior updates without a new world | [task artifact role](design/2026-09-28-task-contract-v7-artifact-role.md), [task facts](design/2026-09-27-task-contract-v6-facts.md) |
| `0.31.0` | Bailian task component | [release](design/2026-09-20-release-0.31.0.md), [component](design/2026-09-20-bailian-video-task-component.md) |
| `0.30.0` | MiniMax task component and estimate facts | [release](design/2026-09-20-release-0.30.0-minimax.md), [component](design/2026-09-20-minimax-v1-task-component.md), [facts](design/2026-09-20-minimax-h3-estimate-facts.md) |
| `0.29.0` | Task adapter v2 and request estimates | [release](design/2026-09-20-release-0.29.0.md), [candidate](design/2026-09-20-task-adapter-v2-candidate.md), [estimate](design/2026-09-20-task-request-estimate.md) |

## Design record index

These records were directly referenced by the former landing-page history. They remain the source
for detailed decisions; this index intentionally does not repeat their narratives.

- Foundations: [repository bootstrap](design/2026-08-16-repository-bootstrap.md),
  [minimal provider call](design/2026-08-16-minimal-provider-call.md),
  [streaming](design/2026-08-17-streaming-provider-call.md),
  [community compatibility](design/2026-08-17-community-host-compatibility-release.md), and
  [quota metadata](design/2026-08-17-provider-quota-response-metadata.md).
- Request security: [header-secret auth](design/2026-08-17-header-secret-auth.md),
  [controlled query](design/2026-08-18-controlled-query-support.md),
  [controlled user agent](design/2026-08-20-controlled-user-agent.md),
  [host prelude](design/2026-08-20-host-prelude.md), and
  [host-signed finalizer](design/2026-08-20-host-signed-request-finalizer.md).
- Component platform: [canonical IR inventory](design/2026-08-21-canonical-ir-inventory.md),
  [provider API promotion](design/2026-08-21-provider-api-promotion.md),
  [component conformance](design/2026-08-21-component-conformance.md),
  [provider runtime](design/2026-08-21-provider-runtime.md), and
  [released artifacts](design/2026-09-10-released-component-artifacts.md).
- Provider references: [OpenAI-compatible parity](design/2026-08-21-openai-compat-host-parity.md),
  [Anthropic](design/2026-08-22-anthropic-provider-component.md),
  [Gemini](design/2026-08-22-gemini-provider-component.md),
  [unmappable-block refusal](design/2026-08-23-renderer-refusal-for-unmappable-blocks.md),
  [Responses upstream](design/2026-09-30-openai-responses-upstream-component.md), and
  [Responses replay](design/2026-10-11-responses-reasoning-replay-and-passthrough-suite.md).
- World definitions: [task vocabulary](design/2026-08-27-task-adapter-vocabulary.md),
  [multi-world manifests](design/2026-08-27-manifest-schema-beyond-one-world.md),
  [embeddings](design/2026-09-30-embeddings-contract.md),
  [image](design/2026-09-30-image-world.md),
  [speech](design/2026-09-30-speech-world.md), and
  [shared task core](design/2026-09-20-shared-task-core.md).
- Media packages: [MiniMax and Qwen image](design/2026-10-12-image-i3-1-minimax-and-qwen.md),
  [Gemini image](design/2026-10-13-image-i3-2-gemini.md),
  [Vertex image](design/2026-10-13-image-i3-3-vertex.md),
  [Stability image](design/2026-10-13-image-i3-4-stability.md),
  [Ideogram image](design/2026-10-13-image-i3-5-ideogram.md), and
  [Veo Vertex task](design/2026-10-15-veo-vertex-task-component.md).
- Speech packages: [V3-1](design/2026-10-14-speech-v3-1-tts.md),
  [V3-2](design/2026-10-14-speech-v3-2-tts.md),
  [V3-3](design/2026-10-14-speech-v3-3-asr.md), and
  [V4](design/2026-10-15-speech-v4-tts.md).
- Host and codec details: [north-codec render gaps](design/2026-09-30-north-codec-render-gaps.md),
  [GenerateContent pass-through](design/2026-10-14-generate-content-request-passthrough.md),
  [Kiro provider](design/2026-09-30-kiro-provider-component.md),
  [Omni task](design/2026-10-01-omni-task-component.md),
  [task poll interval](design/2026-10-14-task-min-poll-interval.md), and
  [task immutable paths](design/2026-10-14-task-minimax-immutable-paths.md).
