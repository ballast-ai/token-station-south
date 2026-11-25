# 供应商入口展示资料与部署映射示例

核验日期：2026-11-25。本文依据供应商官方资料，记录人类可识别的品牌、公司常用展示名及入口角色，供部署管理员配置展示映射；厂商事实集中保存在 South，宿主只提供通用的映射消费机制。

## 使用边界与部署条件

- [TOML 示例](examples/provider-presentations.toml)是 29 个部署供应商 ID 的映射示例，不是 South 组件清单或所有部署通用的默认配置。内部 ID 必须由管理员逐项匹配实际供应商行；同品牌可以有多个内部 ID，不能只凭 ID 推导公司或能力。
- 片段使用 `[ui.provider_presentations."id"]`，每项包含 `display_name`、`company_name`、`category`。将片段合并到宿主部署配置前，应检查是否与已有同名表重复。
- **旧版 `UiConfig` 会拒绝 `ui.provider_presentations` 新键。必须与支持该展示映射的宿主版本同时部署**，不能先把片段投入旧版本。本文不表示 South 实现了宿主配置解析，也不声明某个历史宿主版本已支持它。
- `company_name` 使用常用品牌或公司展示名，便于用户识别；不是法律主体认证、工商核验或合同相对方承诺。`display_name` 可区分同品牌的不同入口；WeCoding 三个入口统一使用 WeCoding，内部 ID 由界面另行展示。
- `category` 按**入口角色**分类，不是对整家公司的评级，也不描述该入口下每个模型的作者。`model_vendor` 表示模型厂商直供入口，`cloud_platform` 表示云平台入口，`third_party` 表示第三方推理或聚合平台入口。
- 托管、运行开放权重模型的推理服务与跨供应商路由聚合是不同形态。第三类展示文案宜使用“第三方推理/聚合平台”：OpenRouter 属于路由聚合；Together AI、Fireworks AI、Cerebras、SambaNova、Groq 等有自身推理服务，不能统一称作转售聚合商。
- OpenAI 兼容是协议特征，不等于运营者属于 OpenAI。WeCoding 的品牌归 WeCoding，百炼兼容入口仍归阿里云；上游模型作者是独立事实。
- 本文不声明实际端点、鉴权协议、套餐可用性、数据驻留或媒体能力；地域括号用于识别示例入口，具体服务范围仍需核对实际端点和合同。

## 官方资料与推荐展示

下表的分类是根据官方产品说明作出的入口角色判断，不是供应商颁发的分类认证；TOML 对应值分别为 `model_vendor`、`cloud_platform`、`third_party`。

| 部署供应商 ID | display_name | company_name | 入口角色 | 官方资料与依据 |
|---|---|---|---|---|
| `ali-bailian` | 阿里云百炼 | 阿里云 | 云平台 | [百炼产品页](https://www.aliyun.com/product/bailian)：阿里云大模型服务平台，提供百余款模型。 |
| `ali-bailian-compat` | 阿里云百炼（OpenAI 兼容） | 阿里云 | 云平台 | [百炼文本 API](https://help.aliyun.com/zh/model-studio/qwen-api-reference/)：DashScope 与 OpenAI 兼容接口均属于百炼；兼容协议不是公司归属。 |
| `gemini-api` | Google Gemini API | Google | 模型厂商 | [Gemini API 官方文档](https://aistudio.google.com/docs)：直接提供 Gemini 模型 API；应与 Google Cloud 云平台入口区分。 |
| `glm` | 智谱 GLM | 智谱 | 模型厂商 | [智谱平台介绍](https://docs.bigmodel.cn/cn/guide/start/introduction)：智谱大模型开放平台和 GLM 系列模型 API。 |
| `glm-coding` | 智谱 GLM Coding Plan | 智谱 | 模型厂商 | [GLM Coding Plan](https://docs.bigmodel.cn/cn/coding-plan/overview)：智谱 GLM 的订阅套餐，不是另一家公司。 |
| `minimax-media` | MiniMax（媒体） | MiniMax | 模型厂商 | [MiniMax 接口概览](https://platform.minimax.cn/docs/api-reference/api-overview)：语言、视频、图像、语音、音乐等自有模型服务。 |
| `openai` | OpenAI | OpenAI | 模型厂商 | [OpenAI 模型目录](https://developers.openai.com/api/docs/models/all)：OpenAI 官方 API 的自有模型目录。 |
| `vertex-global` | Google Cloud Vertex AI（全球） | Google Cloud | 云平台 | [Google Cloud 产品页](https://cloud.google.com/products/gemini-enterprise-agent-platform)和[产品改名说明](https://docs.cloud.google.com/gemini-enterprise-agent-platform/vertex-ai-name-changes)：企业云平台可用 Google、第三方及开放模型；global 是入口地域标识。 |
| `vertex_ai` | Google Cloud Vertex AI | Google Cloud | 云平台 | [Google Cloud 产品页](https://cloud.google.com/products/gemini-enterprise-agent-platform)：同一云平台体系；与 Gemini 官方 API 区分。 |
| `wecoding` | WeCoding | WeCoding | 第三方推理/聚合平台 | [官方 Terms](https://wecoding.ai/legal/terms)与[官网](https://wecoding.ai/)确认 WeCoding 品牌及独立托管服务商定位；此处不据内部 ID 推断媒体产品、协议或能力。 |
| `wecoding-media` | WeCoding | WeCoding | 第三方推理/聚合平台 | [官方 Terms](https://wecoding.ai/legal/terms)与[官网](https://wecoding.ai/)确认 WeCoding 品牌及独立托管服务商定位；此处不据内部 ID 推断媒体产品、协议或能力。 |
| `wecoding-vedio` | WeCoding | WeCoding | 第三方推理/聚合平台 | [官方 Terms](https://wecoding.ai/legal/terms)与[官网](https://wecoding.ai/)确认 WeCoding 品牌及独立托管服务商定位；此处不据内部 ID 推断媒体产品、协议或能力。 |
| `openrouter` | OpenRouter | OpenRouter | 第三方推理/聚合平台 | [开发者页](https://openrouter.ai/developers)：统一端点接入多个模型供应商，提供自动故障回退；属于路由聚合。 |
| `together` | Together AI | Together AI | 第三方推理/聚合平台 | [官方推理平台介绍](https://www.together.ai/blog/the-production-platform-for-open-weight-ai-inference)：部署开放权重模型、管理部署和扩缩容；不是仅路由到其他 API。 |
| `fireworks` | Fireworks AI | Fireworks AI | 第三方推理/聚合平台 | [官方推理产品页](https://fireworks.ai/inference)：开放模型与用户定制模型的推理服务；[公司页](https://fireworks.ai/about)说明平台涵盖推理、集成及训练。 |
| `cerebras` | Cerebras Inference | Cerebras | 第三方推理/聚合平台 | [Cerebras Inference](https://www.cerebras.ai/inference)：用自研芯片运行开放权重模型并提供兼容 API；芯片厂商身份不等于本入口提供的所有模型都由它研发。 |
| `sambanova` | SambaNova Cloud | SambaNova | 第三方推理/聚合平台 | [云服务计划](https://cloud.sambanova.ai/plans)及[官方接入说明](https://docs.sambanova.ai/docs/en/integrations/vscode)：直接提供 SambaNova 推理 endpoint 与模型服务。 |
| `siliconflow-cn` | 硅基流动（中国） | 硅基流动 | 第三方推理/聚合平台 | [硅基流动官网](https://www.siliconflow.cn/)：自研推理引擎、一站式模型 API 与独占算力服务；其另有 TokenPlus 聚合产品，不能把所有硅基流动入口等同 TokenPlus。 |
| `volcengine-ark` | 火山引擎方舟 | 火山引擎 | 云平台 | [火山引擎官网](https://www.volcengine.com/home)和[方舟官方产品条款说明](https://docs.volcengine.com/docs/ark/volcengine-ark-platform-terms?lang=zh)：火山引擎提供训练、推理、评测、精调与模型生态平台。 |
| `baidu-qianfan` | 百度智能云千帆 | 百度智能云 | 云平台 | [百度千帆产品页](https://cloud.baidu.com/product/qianfan.html)：百度智能云大模型服务与 Agent 开发平台，同时提供文心及第三方模型。 |
| `stepfun-cn` | 阶跃星辰（中国） | 阶跃星辰 | 模型厂商 | [官方开放平台](https://platform.stepfun.com/)：Step 基模与 StepAudio；[官方用户协议](https://platform.stepfun.com/legal/user-agreement.html)直接说明阶跃星辰运营关系。 |
| `tencent-tokenhub-cn` | 腾讯云 TokenHub（中国） | 腾讯云 | 云平台 | [TokenHub 产品页](https://cloud.tencent.com/product/tokenhub)：腾讯自研混元与第三方模型统一平台；[API 文档](https://cloud.tencent.com/document/product/1823/130078)说明中国大陆地域接口。 |
| `deepseek` | DeepSeek | DeepSeek（深度求索） | 模型厂商 | [官方 API 文档](https://api-docs.deepseek.com/)提供 DeepSeek 模型调用；[官网](https://www.deepseek.com/)列 DeepSeek 研究及开放平台，并列深度求索公司名称。 |
| `groq` | Groq | Groq | 第三方推理/聚合平台 | [Groq 官网](https://groq.com/)定位快速推理服务与 LPU；[官方 API Quickstart](https://console.groq.com/docs/quickstart)说明 Groq API。不可与模型品牌 Grok 混淆。 |
| `mistral` | Mistral AI | Mistral AI | 模型厂商 | [官方公司页](https://mistral.ai/about/)说明自建前沿模型；[官方文档](https://docs.mistral.ai/)提供自有模型与推理产品。 |
| `kimi` | Kimi | 月之暗面（Moonshot AI） | 模型厂商 | [Moonshot AI 官网](https://www.moonshot.cn/)链接 Kimi 与其开放平台；[Kimi API 文档](https://platform.kimi.com/docs/get-api-key)说明调用 Kimi 模型并给出 Moonshot API 基址。 |
| `minimax` | MiniMax | MiniMax | 模型厂商 | [MiniMax API 概览](https://platform.minimax.cn/docs/api-reference/api-overview)：MiniMax 的自有语言与多模态模型。 |
| `nvidia-nim` | NVIDIA NIM | NVIDIA（英伟达） | 第三方推理/聚合平台 | [官方 API 快速开始](https://docs.api.nvidia.com/nim/docs/api-quickstart)：远程 NIM API 由 NVIDIA 在其 DGX Cloud 托管；[NIM 概览](https://docs.api.nvidia.com/nim/docs/overview)说明多模型容器与可自托管形态。此分类针对托管 API 入口，不概括 NVIDIA 整家公司。 |
| `anthropic` | Anthropic Claude | Anthropic | 模型厂商 | [Claude 官方文档](https://platform.claude.com/docs/en/intro)：Claude 由 Anthropic 构建，并提供直接模型 API。 |

## 名称维护说明

Google 官方已公布 [Vertex AI 产品改名对应关系](https://docs.cloud.google.com/gemini-enterprise-agent-platform/vertex-ai-name-changes)，旧产品入口会跳转至 Gemini Enterprise Agent Platform。本示例保留 Vertex AI 作为存量配置的可识别名称；是否统一迁移到新品牌应由部署管理员决定。

WeCoding 官方条款描述独立托管服务及 GLM，当前首页另展示视频服务。本文只采纳它们共同支持的品牌与平台身份，不用条款或内部 ID 推导三个现有入口的具体媒体链路。三个入口的展示名和公司名均为 WeCoding。

NVIDIA NIM 官方文档同时涵盖 NVIDIA 托管 API 与可下载自托管容器。本示例的 `nvidia-nim` 按 NVIDIA 托管 API 入口分类；不能将该分类推广至所有 NIM 部署，也不能由 NVIDIA 公司拥有自研模型推导所有 NIM 模型都是 NVIDIA 原厂模型。

本轮只核验公开官方资料和配置片段语法，没有请求推理 API，也没有据此宣称真实上游验收通过。公司更名或入口形态改变时，应同时复核引用、展示映射及部署行对应关系。
