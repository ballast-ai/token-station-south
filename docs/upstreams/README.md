# Upstream notes

Facts about upstream services that official packages talk to: endpoints, field quirks, failure
modes, deployments and list prices, each with the date it was last checked. Hosts keep only generic
mechanisms in their own documentation and link here for vendor facts.

Design records stay authoritative for what a package sends and parses. These notes hold the
surrounding facts that no record repeats, and point to the records instead of restating them.

| Upstream | Packages |
|---|---|
| [Alibaba Cloud Model Studio (Bailian / DashScope)](alibaba-model-studio.md) | `provider-openai-compatible` (Bailian families), `task-bailian-v2`, `task-wan-image-v2`, `image-qwen`, `speech-qwen` |
| [Ideogram](ideogram.md) | `image-ideogram` |
| [Stability AI](stability.md) | `image-stability` |
| [MiniMax](minimax.md) | `task-minimax-v2` (video); chat and speech billing facts |
| [OpenAI-compatible text upstreams](openai-compatible-text.md) (Xiaomi MiMo, DeepSeek, Meituan LongCat) | `provider-openai-compatible` |
| [Google Gemini API and Vertex AI](google-gemini.md) | `provider-gemini`, `provider-vertex`, `task-veo-v2`, `image-gemini`, `image-vertex`, `embeddings-gemini`, `embeddings-vertex` |
| [xAI (video)](xai.md) | `task-xai-v2` |
| [Anthropic](anthropic.md) | `provider-anthropic`, `provider-anthropic-bedrock-invoke` |
| [Subscription backends: OpenAI Codex and GitHub Copilot](subscription-backends.md) | `provider-openai-responses` (`openai-codex`), `provider-openai-compatible` (`github-copilot`) |
| [Kling AI (video)](kling.md) | `task-kling-v2` |
| [How upstreams report token usage](usage-reports.md) (cross-vendor) | every package that parses usage into the IR |
| [BytePlus (Seedance video)](byteplus.md) | `task-byteplus-v2` |
| [Reve (images)](reve.md) | `image-reve` |
