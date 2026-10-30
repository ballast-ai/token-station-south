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
| [MiniMax (video)](minimax.md) | `task-minimax-v2` |
| [OpenAI-compatible text upstreams](openai-compatible-text.md) (Xiaomi MiMo, DeepSeek, Meituan LongCat) | `provider-openai-compatible` |
