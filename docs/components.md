# Component catalog

This catalog lists every official package under `components/`. Each package has a manifest,
component-specific build script, and a native reference compiled into the conformance crate. An
evidence link points to a fixture pack's provenance README when one exists.

Package versions, digests, contract tuples, and minimum runtimes are machine-readable in manifests
and release artifacts. See the [architecture](../ARCHITECTURE.md) for admission rules and the
[release index](releases.md) for historical changes.

## Provider adapter v2

| Package | Families and current purpose | Build | Evidence |
|---|---|---|---|
| [`provider-openai-compatible`](../components/provider-openai-compatible/manifest.json) | OpenAI-compatible, Azure OpenAI, Gemini-compatible, Copilot, GLM, Kimi, and Bailian chat families | `bash scripts/build-reference-component.sh` | — |
| [`provider-anthropic`](../components/provider-anthropic/manifest.json) | Anthropic Messages and Claude Code chat families | `bash scripts/build-anthropic-component.sh` | [Claude Code](../crates/south-component-conformance/fixtures-claude-code/README.md) |
| [`provider-gemini`](../components/provider-gemini/manifest.json) | Gemini `generateContent`, streaming, and pass-through | `bash scripts/build-gemini-component.sh` | — |
| [`provider-bedrock-converse`](../components/provider-bedrock-converse/manifest.json) | AWS Bedrock Converse with host-side SigV4 | `bash scripts/build-bedrock-converse-component.sh` | — |
| [`provider-bedrock-converse-bearer`](../components/provider-bedrock-converse-bearer/manifest.json) | Bedrock Converse with bearer API keys | `bash scripts/build-bedrock-converse-bearer-component.sh` | — |
| [`provider-anthropic-bedrock-invoke`](../components/provider-anthropic-bedrock-invoke/manifest.json) | Anthropic Messages through Bedrock InvokeModel | `bash scripts/build-anthropic-bedrock-invoke-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-anthropic-bedrock-invoke/README.md) |
| [`provider-openai-responses`](../components/provider-openai-responses/manifest.json) | OpenAI Responses, Codex subscription, and Bedrock Mantle | `bash scripts/build-openai-responses-component.sh` | [Responses](../crates/south-component-conformance/fixtures-openai-responses/README.md), [Codex](../crates/south-component-conformance/fixtures-openai-codex/README.md), [Mantle](../crates/south-component-conformance/fixtures-bedrock-mantle/README.md) |
| [`provider-kiro`](../components/provider-kiro/manifest.json) | Kiro chat with rotating credential recipes, event-stream framing, and inline images | `bash scripts/build-kiro-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-kiro/README.md) |
| [`provider-vertex`](../components/provider-vertex/manifest.json) | Gemini text models through Vertex AI | `bash scripts/build-provider-vertex-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-vertex/README.md) |

## Task adapters

| Package | Families and current purpose | Build | Evidence |
|---|---|---|---|
| [`task-kling`](../components/task-kling/manifest.json) | Kling task adapter v1 | `bash scripts/build-kling-task-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-kling-task/README.md) |
| [`task-kling-v2`](../components/task-kling-v2/manifest.json) | Kling task adapter v2 | `bash scripts/build-kling-task-v2-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-kling-task-v2/README.md) |
| [`task-minimax-v2`](../components/task-minimax-v2/manifest.json) | MiniMax Hailuo and H3 task families | `bash scripts/build-minimax-task-v2-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-minimax-task-v2/README.md) |
| [`task-bailian-v2`](../components/task-bailian-v2/manifest.json) | Bailian managed video and Wan media task families | `bash scripts/build-bailian-task-v2-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-bailian-task-v2/README.md) |
| [`task-xai-v2`](../components/task-xai-v2/manifest.json) | xAI video tasks | `bash scripts/build-xai-task-v2-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-xai-task-v2/README.md) |
| [`task-byteplus-v2`](../components/task-byteplus-v2/manifest.json) | BytePlus video tasks | `bash scripts/build-byteplus-task-v2-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-byteplus-task-v2/README.md) |
| [`task-veo-v2`](../components/task-veo-v2/manifest.json) | Gemini and Vertex Veo video tasks | `bash scripts/build-veo-task-v2-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-veo-task-v2/README.md) |
| [`task-wan-image-v2`](../components/task-wan-image-v2/manifest.json) | Wan image tasks | `bash scripts/build-wan-image-task-v2-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-wan-image-task-v2/README.md) |
| [`task-gmi-image-v2`](../components/task-gmi-image-v2/manifest.json) | GMI image tasks with declared organization values | `bash scripts/build-gmi-image-task-v2-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-gmi-image-task-v2/README.md) |
| [`task-gemini-omni-v2`](../components/task-gemini-omni-v2/manifest.json) | Gemini and Vertex Interactions video tasks | `bash scripts/build-gemini-omni-task-v2-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-gemini-omni-task-v2/README.md) |

## Embeddings adapter v1

| Package | Families and current purpose | Build | Evidence |
|---|---|---|---|
| [`embeddings-openai-compatible`](../components/embeddings-openai-compatible/manifest.json) | OpenAI-compatible and Azure text or token-id embeddings | `bash scripts/build-embeddings-openai-compatible-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-embeddings-openai-compatible/README.md) |
| [`embeddings-gemini`](../components/embeddings-gemini/manifest.json) | Gemini text and inline-media embeddings | `bash scripts/build-embeddings-gemini-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-embeddings-gemini/README.md) |
| [`embeddings-vertex`](../components/embeddings-vertex/manifest.json) | Vertex AI text embeddings with declared service-account recipe | `bash scripts/build-embeddings-vertex-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-embeddings-vertex/README.md) |

## Image adapter v1

| Package | Families and current purpose | Build | Evidence |
|---|---|---|---|
| [`image-azure`](../components/image-azure/manifest.json) | Azure MAI generation and edit | `bash scripts/build-image-azure-component.sh` | — |
| [`image-minimax`](../components/image-minimax/manifest.json) | MiniMax generation | `bash scripts/build-image-minimax-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-image-minimax/README.md) |
| [`image-qwen`](../components/image-qwen/manifest.json) | Bailian Qwen generation | `bash scripts/build-image-qwen-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-image-qwen/README.md) |
| [`image-gemini`](../components/image-gemini/manifest.json) | Gemini generation and edit | `bash scripts/build-image-gemini-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-image-gemini/README.md) |
| [`image-stability`](../components/image-stability/manifest.json) | Stable Image generation | `bash scripts/build-image-stability-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-image-stability/README.md) |
| [`image-ideogram`](../components/image-ideogram/manifest.json) | Ideogram generation | `bash scripts/build-image-ideogram-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-image-ideogram/README.md) |
| [`image-vertex`](../components/image-vertex/manifest.json) | Vertex AI Gemini image generation | `bash scripts/build-image-vertex-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-image-vertex/README.md) |
| [`image-xai`](../components/image-xai/manifest.json) | xAI Grok Imagine generation and edit | `bash scripts/build-image-xai-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-image-xai/README.md) |
| [`image-reve`](../components/image-reve/manifest.json) | Reve bridge generation and edit | `bash scripts/build-image-reve-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-image-reve/README.md) |
| [`image-openai-compatible`](../components/image-openai-compatible/manifest.json) | OpenAI-compatible, Azure, and GLM generation and edit | `bash scripts/build-image-openai-compatible-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-image-openai-compatible/README.md) |

## Speech adapter v1

| Package | Families and current purpose | Build | Evidence |
|---|---|---|---|
| [`speech-openai-compatible`](../components/speech-openai-compatible/manifest.json) | OpenAI-compatible and Azure synthesis, transcription, and translation | `bash scripts/build-speech-openai-compatible-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-speech-openai-compatible/README.md) |
| [`speech-xai`](../components/speech-xai/manifest.json) | xAI synthesis and transcription | `bash scripts/build-speech-xai-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-speech-xai/README.md) |
| [`speech-groq`](../components/speech-groq/manifest.json) | Groq synthesis | `bash scripts/build-speech-groq-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-speech-groq/README.md) |
| [`speech-elevenlabs`](../components/speech-elevenlabs/manifest.json) | ElevenLabs synthesis and transcription | `bash scripts/build-speech-elevenlabs-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-speech-elevenlabs/README.md) |
| [`speech-mimo`](../components/speech-mimo/manifest.json) | Xiaomi MiMo synthesis | `bash scripts/build-speech-mimo-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-speech-mimo/README.md) |
| [`speech-minimax`](../components/speech-minimax/manifest.json) | MiniMax synthesis | `bash scripts/build-speech-minimax-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-speech-minimax/README.md) |
| [`speech-azure`](../components/speech-azure/manifest.json) | Azure fast transcription and SSML synthesis | `bash scripts/build-speech-azure-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-speech-azure/README.md) |
| [`speech-qwen`](../components/speech-qwen/manifest.json) | Bailian Qwen synthesis, Qwen ASR, and declared artifact fetch | `bash scripts/build-speech-qwen-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-speech-qwen/README.md) |
| [`speech-vertex`](../components/speech-vertex/manifest.json) | Vertex AI Gemini synthesis | `bash scripts/build-speech-vertex-component.sh` | [fixtures](../crates/south-component-conformance/fixtures-speech-vertex/README.md) |

## Package and evidence rules

Every build script compiles its package for `wasm32-wasip2` and emits the component used by gate 2
parity tests. Fixture README files record how expectations were derived and which uncertainties
remain. Fixtures are evidence, not credentials or live-provider tests.

The [synthetic speech fixture provenance](../crates/south-component-conformance/fixtures-speech-synthetic/README.md)
belongs to the synthetic conformance guest, not to an official component package.

Release artifacts include `south-release-index.json` with each package identity, manifest digest,
component digest, declared runtime, contract tuple, and gate 2 report. Digest-stability checks reject
changed package bytes under an unchanged identity. A package's `south_runtime` is its minimum
runtime, not necessarily the repository release in which the package appears.

Upstream facts that no design record repeats (deployments, field quirks, failure modes, list
prices with the date they were checked) are collected per upstream in [upstream notes](upstreams/README.md).

The model catalog is a separate signed release artifact read by the host runtime. It does not alter
component identity or grant a package new capabilities. Host adoption, suite status, and supported
ranges remain authoritative in [compatibility.json](../compatibility.json).
