# 常见文本供应商配置

核实日期：2026-11-25。

本批补齐 17 家供应商的元数据：16 家使用现有 OpenAI Chat 兼容族和单 Bearer API Key，Anthropic 使用已有专用族及绑定单 workspace 的 API Key。复用现有组件，不新增认证、协议或私有接入体系。

[可用配置片段](examples/text-providers.toml)只含供应商，适用于已有 `provider-openai-compatible` / `provider-anthropic` 组件并允许相应族的宿主。公开文档兼容不代表已用真实账号完成验收。

## 官方依据和边界

下表 OpenAI Chat 兼容项的认证均为 `Authorization: Bearer <该平台 API key>`，对话完整地址均为所列 base 后追加 `/chat/completions`。协议为 OpenAI Chat Completions。优先级表达本次补目录建议，而非服务质量排名。

| 优先级 | 平台 | 精确 base | 兼容边界与接入建议 | 官方依据 |
| --- | --- | --- | --- | --- |
| P1，可补 | OpenRouter | `https://openrouter.ai/api/v1` | 使用带作者/组织前缀的 model slug；归因头可选。流式含 SSE 注释心跳及流中错误；模型与路由后端的能力不同，不能把兼容声明当全参数支持。 | [兼容指南](https://openrouter.ai/docs/guides/features/compatibility)、[快速开始](https://openrouter.ai/docs/quickstart) |
| P1，可补 | Together AI | `https://api.together.ai/v1` | 使用平台模型 ID；Chat 支持流式、工具和结构化输出；Assistants/Threads 不支持。Embeddings 当前仅专用部署；本轮只补 Chat，不借此扩充其他能力。 | [兼容矩阵及 Bearer 示例](https://docs.together.ai/docs/inference/openai-compatibility) |
| P1，可补 | Fireworks AI | `https://api.fireworks.ai/inference/v1` | 模型名必须来自平台目录；超上下文默认会向下调整 max_tokens，可用平台参数改成报错。流式 usage 在最后一个带 finish_reason 的块内；需现有解析器能接。 | [兼容指南及 Bearer 示例](https://docs.fireworks.ai/tools-sdks/openai-compatibility) |
| P1，可补 | Cerebras | `https://api.cerebras.ai/v1` | Shared 用精确模型 ID，Dedicated 用 endpoint ID；只支持 n=1；图像只收 base64 PNG/JPEG data URI，不收外部 HTTPS 图址。思考参数和其他能力按模型区分。 | [兼容指南](https://inference-docs.cerebras.ai/resources/openai)、[Bearer 示例](https://inference-docs.cerebras.ai/quickstart) |
| P1，可补 | SambaNova Cloud | `https://api.sambanova.ai/v1` | Chat 可流式/非流式；presence_penalty/frequency_penalty 仅 0；未知参数返回 400。n 可为 1–8，但工具调用时不支持 n>1。私有 SambaStack 端点另由管理员给出，不能复用云端点。 | [端点](https://docs.sambanova.ai/docs/en/get-started/api-keys-urls)、[兼容限制](https://docs.sambanova.ai/docs/en/features/openai-compatibility)、[官方 OpenAI SDK 示例](https://github.com/sambanova/ai-starter-kit/blob/main/quickstart/README.md) |
| P1，可补 | SiliconFlow 中国站 | `https://api.siliconflow.cn/v1` | Chat API 明确 Bearer，模型与参数能力以具体型号为准；流式可能有空 choices 块。国际站是独立平台，不能把中国站 key 发给国际站。 | [快速开始](https://docs.siliconflow.cn/docs/userguide/quickstart)、[Chat API](https://docs.siliconflow.cn/docs/api/chat-completions-post)、[流式](https://docs.siliconflow.cn/docs/userguide/capabilities/stream-mode) |
| P1，可补，分站点 | StepFun | 中国站 `https://api.stepfun.com/v1`（本批） | 官方指南明确按 key 颁发区域选择 endpoint，不应在认证失败后自动向另一站重发 key。模型必须来自对应站点；不能把 Step Plan 订阅当普通 API key 套用。 | [官方分区和 Bearer 调用指南](https://platform.stepfun.ai/cookbook/step-5-tool-calling)、[国际站 OpenAI 示例](https://platform.stepfun.ai/) |
| P1，可补 | 火山方舟 Ark | `https://ark.cn-beijing.volces.com/api/v3` | 路径为 api/v3，不是 v1。数据面可用单 API key；管控面的 AK/SK 签名不在范围内。普通 API 与 Coding Plan 端点不同；model 需平台模型 ID/推理接入点，能力先开通。 | [Base URL 与鉴权](https://console.volcengine.com/ark/region%3Acn-beijing/docs/base-url-and-authentication)、[兼容 OpenAI SDK](https://docs.volcengine.com/docs/ark/compatible-with-openai-sdk?lang=zh) |
| P1，可补 | 百度千帆 V2 | `https://qianfan.baidubce.com/v2` | V2 使用 IAM 页面签发且获应用授权的 API key，不是旧版 AK/SK 换取 access_token。模型必须取 V2 支持列表；应用 appid 可经自定义 header 区分，普通单 key 对话不因此需要新签名系统。 | [OpenAI SDK 兼容介绍](https://ai.baidu.com/ai-doc/WENXINWORKSHOP/2m3fihw8s)、[Python Chat 示例](https://cloud.baidu.com/doc/qianfan-docs/s/Fm9l6ocai) |
| P2，优先新平台 | 腾讯 TokenHub | 中国站广州 `https://tokenhub.tencentmaas.com/v1`（本批） | 标准 Chat + Bearer 单 API key；账号地域与站点必须对应。国际腾讯云文档另列 tencentcloudmaas.com 域，不能与中国站端点混用。模型能力、usage 缓存字段因模型不同。Token Plan 用另一个 plan/v3 路径，不当普通 API key。 | [中国站 API 使用说明](https://cloud.tencent.com/document/product/1823/130078)、[协议概览](https://cloud.tencent.com/document/product/1823/130079)、[国际站概览](https://intl.cloud.tencent.com/ind/document/product/1300/80632) |


## 补充的常见文本入口

| 候选 | 精确 base / 完整对话路径 | 认证与现有族判断 | 官方来源与兼容边界 |
| --- | --- | --- | --- |
| DeepSeek | 官方 OpenAI base `https://api.deepseek.com`；完整 `https://api.deepseek.com/chat/completions` | `Authorization: Bearer <DeepSeek API key>`；可作为已有 Chat 族供应商元数据候选。按官方根地址配置，不自行假设组件一定需要 v1。 | [首次调用](https://api-docs.deepseek.com/) 明确 OpenAI 兼容、流式/非流式和 Bearer。Anthropic 兼容是另外的 `/anthropic` base，本次不混用。 |
| Groq | `https://api.groq.com/openai/v1`；后追加 `/chat/completions` | `Authorization: Bearer <Groq API key>`；可复用 Chat 族。 | [官方兼容指南](https://console.groq.com/docs/openai)、[官方端点/Bearer FAQ](https://community.groq.com/t/what-is-the-base-url-path-for-groq-api/487)。兼容指南列 `logprobs`、`logit_bias`、`top_logprobs`、`messages[].name` 不支持，n 只能 1；不能宣称全参数兼容。 |
| Mistral | `https://api.mistral.ai/v1`；后追加 `/chat/completions` | `Authorization: Bearer <Mistral API key>`；官方 Chat 请求/响应为 OpenAI Chat 外形，可作现有 Chat 族候选。 | [官方 Chat API](https://docs.mistral.ai/api)、[官方 key 与 curl 示例](https://docs.mistral.ai/getting-started/quickstarts/studio/activate-and-generate-api-key)。本轮仅核实文本 Chat，不把 OCR、FIM、Agents 等原生产品当同一路径能力。 |
| Kimi / Moonshot 中国站 | `https://api.moonshot.cn/v1`；后追加 `/chat/completions` | `Authorization: Bearer <Moonshot API key>`；可复用 Chat 族。 | [官方 Chat API](https://platform.kimi.com/docs/api/chat)。旧 platform.moonshot.cn 文档已重定向 platform.kimi.com，但官方 OpenAI 示例仍用 api.moonshot.cn。思考/工具等参数按具体型号校验，不在本轮声明能力。 |
| MiniMax 中国站普通文本 | 当前官方 `https://api.minimax.cn/v1`；后追加 `/chat/completions` | `Authorization: Bearer <MiniMax API key>`；可复用 Chat 族。不要把已有 minimax-media 项视为普通文本接入已存在。 | [官方 OpenAI SDK](https://platform.minimax.cn/docs/api-reference/text-openai-api)、[官方 Chat API 与 Bearer](https://platform.minimax.cn/docs/api-reference/text-chat-openai)。旧 platform.minimaxi.com 文档会重定向，新文档明确 api.minimax.cn；不沿用记忆中的 api.minimaxi.com。部分采样/惩罚参数忽略，n 仅 1；多轮工具对话须保留完整返回和相应思考内容。 |
| Anthropic 官方 | 根 `https://api.anthropic.com`；完整 `https://api.anthropic.com/v1/messages` | 复用现有 anthropic 族，以 `x-api-key: <Anthropic API key>` 加 `anthropic-version`，由组件负责头与协议。不能误配为 OpenAI Chat Bearer 族。 | [官方 API 概览](https://platform.claude.com/docs/en/api/overview)。当前文档仍支持 x-api-key；版本头必需。最新说明要求未绑定单 workspace 的 key 额外传 anthropic-workspace-id，因此当前单 secret 接入应使用绑定单 workspace 的 key，或先确认现有组件能声明该附加字段；本轮不新增凭证机制。 |
| NVIDIA Hosted NIM / API Catalog | `https://integrate.api.nvidia.com/v1`；后追加 `/chat/completions` | `Authorization: Bearer <NVIDIA API key>`；托管 LLM Chat 可复用 Chat 族。不是把所有 NVIDIA NIM 产品都视为一个 Chat 接口。 | [官方 LLM 端点说明](https://docs.api.nvidia.com/nim/reference/llm-apis)、[官方请求参考的 Bearer 凭证说明](https://docs.api.nvidia.com/nim/reference/openai-gpt-oss-20b-infer)、[NVIDIA 官方凭证配置](https://github.com/NVIDIA/OpenShell/blob/main/providers/nvidia.yaml)。自托管 NIM 地址与认证由部署方指定，不能复用托管事实；托管模型可用性不固定。 |

## 本批暂不自动加入

- xAI 的 Chat 仍提供服务，但已标为 Legacy；新功能优先 Responses。保留为后续单独验收，不将新 Responses 能力套给旧 Chat。[官方说明](https://docs.x.ai/developers/model-capabilities/legacy/chat-completions)
- Perplexity 的新旧入口有迁移边界，本批不混用旧 Sonar 与新 Orchestrator；需要独立验收后再配置。[官方 Orchestrator 入口](https://docs.perplexity.ai/docs/orchestrator/quickstart)

## 导入与验收

1. 新部署：选择需要的供应商段合入首启目录文件，不覆盖已有同名段；目录入库仍遵循宿主的默认停用规则。
2. 存量部署：先读管理 API 的现有供应商列表，只创建缺项；已经存在的行无论启用状态如何都保留，不重写端点、凭证、价格或路由。单改首启 TOML 不会更改数据库。
3. 创建使用 name、endpoint、south_dialect 三个字段，默认停用；凭证由现有密钥功能录入。按已采购的账户站点选择端点，不跨站点重发密钥。
4. 本批不添加型号行。型号 ID、计费桶、单价和能力必须按对应平台目录及价卡核实，不能借用另一平台的同名型号价格，也不能默认填零。
5. 导入后对照清单回读本批配置，确认来源端点和方言正确、保持停用，现有行未改。准备好平台凭证或 BYOK 密钥及已定价型号后，再显式启用并用既有连接验证功能验收。

本批只核实静态配置与官方协议说明，没有调用推理接口；不声称工具、视觉、缓存、思考或流式用量已逐项实测。当前范围仅为普通 API Key 的文本入口；订阅套餐、管控面签名与旧版换票端点不在范围内。
