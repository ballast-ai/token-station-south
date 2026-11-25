# 常见文本供应商首批型号示例

核实日期：2026-11-25。配套文件：[服务商片段](examples/text-providers.toml)、[型号片段](examples/text-models.toml)、[接入边界](common-chat-providers.md)。

本例包含15家供应商、17条型号：每家一个，OpenRouter和Groq各两个。NVIDIA托管NIM与Kimi本批阻塞，不用零价型号占位。此处记录官方静态声明与运营选择，未调用推理，未完成账号授权、缓存、工具、图像或流式对账验收。复用已有 `openai-compatible` / `anthropic` 族与API Key，不新增协议或认证机制。

## 使用与计价口径

- 型号片段使用宿主已有 `ModelConfig` 字段：`name` 为 `<provider>/<上游简名>`，`provider` 精确对应服务商片段。只声明 `modalities = ["text"]`；`supported_parameters = ["max_tokens"]` 是保守目录metadata，不是能力扩展或已实测凭证。
- 所有价格字段都是 **µUSD/百万token整数**。美元原价乘1,000,000；人民币先统一为每百万token，再除 **6.785 CNY/USD**，用十进制 `ROUND_HALF_UP` 到整数。该值沿用宿主既有目录的固定运营换算，不是实时汇率，也不是厂商美元报价。
- 未获精确整数字段的上下文统一设 **32768**，是人为保守运营上限，绝不代表厂商最大上下文。未向配置添加宿主不存在的 `max_output_tokens` 字段；客户端输出授权仍须尊重上游限制。
- OpenAI形缓存读价写 `cached_input_price_per_million`。Anthropic单独写 `cache_read_input_price_per_million`、`cache_creation_input_price_per_million`（5m）和 `cache_creation_1h_input_price_per_million`（1h）。未公布的缓存价不伪造为零。
- 价格采用标准同步档，不启用batch、priority、fast、US-only、联网搜索、服务器工具等附加计价。聚合平台用其自己的公开价卡，不能拿底层原厂价格代替，更不能保证实际选路成本恒等于本例。
- Cerebras与SambaNova型号加 `preview,development-evaluation-only,local-dev-license` 标签，只适用于持开发许可证的开发评估实例，官方明确不用于生产。Mistral严格兼容端点本身为preview。
- 没有新增 `capabilities`、`price_tiers`、附加请求体或凭证体系。DeepSeek既有 `upstream_requires_reasoning_content = true` 仅声明默认思考时的历史回放要求，不声称工具调用已经验证。

## Mistral必须使用严格兼容端点

服务商 `mistral` 的base须为 **`https://api.mistral.ai/openai/v1`**，完整Chat路径为 `/openai/v1/chat/completions`。官方当前明确 `/v1/chat/completions` 是native Mistral格式，不能仅凭外形相似当作严格OpenAI兼容。兼容端点为public preview，参数和错误可能变化。存量行必须显式读取、核对后更新，不能依赖修改首启片段覆盖数据库。来源：[官方Chat说明](https://docs.mistral.ai/inference/conversations/chat-completion)。

## 首批型号和基础价

下表原价单位除特别注明外均为每百万token；配置中的换算后整数见TOML。`未知→32768` 表示运营上限。型号都只声明文本。

| provider | 精确upstream_model | 上下文配置 | 原币输入 / 输出 | 缓存原价与限制 |
|---|---|---:|---|---|
| `openrouter` | `openai/gpt-oss-120b` | 131072 | USD 0.05 / 0.25 | 0.05 |
| `openrouter` | `meta-llama/llama-3.3-70b-instruct` | 128000 | USD 0.71 / 0.71 | 0.71 |
| `together` | `openai/gpt-oss-120b` | 131072 | USD 0.15 / 0.6 | 未公开，省略 |
| `fireworks` | `accounts/fireworks/models/glm-5p3-flash` | 未知→32768 | USD 0.15 / 0.5 | 0.03 |
| `cerebras` | `gpt-oss-120b` | 未知→32768 | USD 0.35 / 0.75 | 未公开，省略；仅开发评估 |
| `sambanova` | `gpt-oss-120b` | 未知→32768 | USD 0.22 / 0.59 | 未公开，省略；仅开发评估 |
| `groq` | `openai/gpt-oss-120b` | 131072 | USD 0.15 / 0.6 | 0.075 |
| `groq` | `openai/gpt-oss-20b` | 131072 | USD 0.075 / 0.3 | 0.037 |
| `mistral` | `mistral-small-2603` | 未知→32768 | USD 0.15 / 0.6 | 未公开，省略 |
| `anthropic` | `claude-sonnet-4-6` | 未知→32768 | USD 3 / 15 | 读0.3；5m写3.75；1h写6 |
| `siliconflow-cn` | `Qwen/Qwen3.6-27B` | 262144 | CNY 3 / 18 | 未公开，省略 |
| `volcengine-ark` | `doubao-seed-2-1-lite-260915` | 1048576 | CNY 0.8 / 2.7 | 0.16 |
| `baidu-qianfan` | `ernie-4.5-turbo-128k` | 131072 | CNY 0.0008 / 0.0032 / 千token | 未公开，省略 |
| `stepfun-cn` | `step-3.5-flash` | 262144 | CNY 0.7 / 2.1 | 0.14 |
| `tencent-tokenhub-cn` | `hy3` | 262144 | CNY 1 / 4 | 0.25 |
| `deepseek` | `deepseek-flash` | 1048576 | USD 0.3 / 1.2 | 0.006；高峰基础价，闲时优惠不自动切换 |
| `minimax` | `MiniMax-M3.1-Flash` | 1000000 | CNY 1 / 4 | 0.1 |

## 官方来源和取舍

### openrouter/gpt-oss-120b

官方来源：[来源1](https://openrouter.ai/api/v1/models)、[来源2](https://openrouter.ai/docs/api_reference/streaming)。

- 价格为OpenRouter模型目录当前挂牌起价，实际选中的上游端点价格、窗口、能力可能不同；不可当作固定实际成本保证。
- context取模型目录与top_provider两者较小值；supported_parameters取保守交集，不承诺任意路由均支持。

### openrouter/llama-3.3-70b-instruct

官方来源：[来源1](https://openrouter.ai/api/v1/models)、[来源2](https://openrouter.ai/docs/api_reference/streaming)。

- 价格为OpenRouter模型目录当前挂牌起价，实际选中的上游端点价格、窗口、能力可能不同；不可当作固定实际成本保证。
- context取模型目录与top_provider两者较小值；supported_parameters取保守交集，不承诺任意路由均支持。

### together/gpt-oss-120b

官方来源：[来源1](https://docs.together.ai/docs/serverless/models)、[来源2](https://docs.together.ai/docs/inference/chat/overview)、[来源3](https://docs.together.ai/docs/inference/chat/parameters)。

- 官方serverless当前目录明确可直接调用；未在该官方目录找到独立max output，不填猜测值。

### fireworks/glm-5p3-flash

官方来源：[来源1](https://fireworks.ai/models/fireworks/glm-5p3-flash)、[来源2](https://docs.fireworks.ai/getting-started/quickstart)、[来源3](https://docs.fireworks.ai/api-reference/post-chatcompletions)。

- 官方当前quickstart明确使用该公共serverless ID，非个人deployment ID。
- 官方型号页只给1040k上下文，未公布独立max output；因此本例使用32768运营上限。
- 只声明文本；尽管官方型号支持图像，此次不扩大现有族能力。

### cerebras/gpt-oss-120b

官方来源：[来源1](https://inference-docs.cerebras.ai/models/openai-oss.md)、[来源2](https://www.cerebras.ai/pricing)、[来源3](https://inference-docs.cerebras.ai/resources/openai)。

- Developer价卡明确仅开发评估，不适用于生产；企业生产报价须联系销售。
- 官方仅用k标注上下文/输出，未把k的精确基数展开，因此本例使用32768运营上限。
- system角色被映射到developer层级；避免min_tokens；模型有可能生成未声明的工具调用。
- n只能为1；gpt-oss-120b不能同时传tools与response_format；不要发送tool_stream。

### sambanova/gpt-oss-120b

官方来源：[来源1](https://docs.sambanova.ai/docs/en/models/sambacloud-models)、[来源2](https://cloud.sambanova.ai/plans/pricing)、[来源3](https://docs.sambanova.ai/docs/en/get-started/quickstart)。

- 官方当前所有SambaCloud开发账号型号均为Preview，明确不得用于生产，可能短期撤除。
- 官方只给128k上下文；未在当前cloud目录找到独立max output，不填猜测值。
- 只声明max_tokens；不要按OpenAI全量参数能力推断。

### groq/gpt-oss-120b

官方来源：[来源1](https://console.groq.com/docs/models)、[来源2](https://console.groq.com/docs/model/openai/gpt-oss-120b)、[来源3](https://console.groq.com/docs/text-chat)、[来源4](https://console.groq.com/docs/openai)。

- 不可传logprobs、logit_bias、top_logprobs、messages[].name；n只能为1。
- temperature=0会转换为1e-8；本次不启用供应商内建搜索/代码执行等额外计费能力。

### groq/gpt-oss-20b

官方来源：[来源1](https://console.groq.com/docs/models)、[来源2](https://console.groq.com/docs/model/openai/gpt-oss-20b)、[来源3](https://console.groq.com/docs/text-chat)、[来源4](https://console.groq.com/docs/openai)。

- 不可传logprobs、logit_bias、top_logprobs、messages[].name；n只能为1。
- temperature=0会转换为1e-8；本次不启用供应商内建搜索/代码执行等额外计费能力。

### mistral/mistral-small-2603

官方来源：[来源1](https://docs.mistral.ai/models/mistral-small-4)、[来源2](https://docs.mistral.ai/inference/pricing?currency=usd)、[来源3](https://docs.mistral.ai/inference/conversations/chat-completion)。

- 官方明确native为/v1/chat/completions，OpenAI兼容public preview为/openai/v1/chat/completions；现目录基址/v1需主代理核验。
- 当前美元标准价卡Small4的缓存列为-，不采搜索摘要中的0.015。
- 型号页未公布独立max output；只给256K上下文，因此本例使用32768运营上限。

### anthropic/claude-sonnet-4-6

官方来源：[来源1](https://platform.claude.com/docs/en/models/sonnet-4-6/overview)、[来源2](https://platform.claude.com/docs/en/about-claude/pricing)、[来源3](https://platform.claude.com/docs/en/build-with-claude/streaming)。

首批只选仍为Active legacy的Sonnet4.6，退役不早于2027-02-17；未选择默认adaptive thinking且禁止temperature/top_p/top_k的Sonnet5.5，不新增型号适配。

### siliconflow-cn/Qwen3.6-27B

官方来源：[来源1](https://www.siliconflow.cn/models/Qwen%2FQwen3.6-27B)、[来源2](https://docs.siliconflow.cn/docs/api/chat-completions-post)、[来源3](https://docs.siliconflow.cn/docs/release-notes/overview)。

- 官方详情最大输出写暂未提供；262144是平台披露上下文。
- 当前价格不再按上下文分档；未声明缓存价不能当零价。
- 思考开关、视觉和工具先不扩展到默认chat能力；宿主需确认思考计量口径。

### volcengine-ark/doubao-seed-2-1-lite-260915

官方来源：[来源1](https://console.volcengine.com/ark/region:cn-beijing/docs/model-list)、[来源2](https://console.volcengine.com/ark/region:cn-beijing/docs/model-pricing)、[来源3](https://console.volcengine.com/ark/region:cn-beijing/docs/base-url-and-authentication)、[来源4](https://console.volcengine.com/ark/region:cn-beijing/docs/online-inference-standard)。

- API Key Bearer可直接用公共Model ID；无需私人ep-ID。Access Key认证则要求Endpoint ID，本任务不用。
- 默认深度思考；仅文本首批可在已有组件支持时传thinking.type=disabled，不能擅增新参数机制。
- 缓存存储按小时另外计费，首批不主动开启显式缓存。

### baidu-qianfan/ernie-4.5-turbo-128k

官方来源：[来源1](https://cloud.baidu.com/product-s/qianfan_home)、[来源2](https://cloud.baidu.com/doc/qianfan-api/s/3m7of64lb)、[来源3](https://cloud.baidu.com/doc/qianfan-api/s/omwbqlomb)。

- 官方产品页披露128K、输入123K+、输出12K；该展示不是精确token整数，保守4096可作为人为配置上限。
- V2数据面支持Authorization: Bearer API Key，不要采用管控面bce-auth签名示例。
- 不自动开启搜索、前缀缓存等额外计费功能。

### stepfun-cn/step-3.5-flash

官方来源：[来源1](https://platform.stepfun.com/docs/zh/guides/models/step-3.5-flash)、[来源2](https://platform.stepfun.com/docs/zh/guides/pricing/details)。

- 官方纯文本推理模型，不把同厂语音/视觉模型套入chat。
- 最大输出未在本次读到的模型介绍页披露。
- 同价替代step-3.5-flash-2603增加low/high推理强度；本批不额外增加。

### tencent-tokenhub-cn/hy3

官方来源：[来源1](https://cloud.tencent.com/document/product/1823/130051)、[来源2](https://cloud.tencent.com/document/product/1823/130055/)、[来源3](https://cloud.tencent.com/document/product/1823/132252)。

- 不用即将下线的hy4-preview；hy3无本次文档的时段或上下文阶梯。
- Hy4官方目录6/18/0.3，2026年底前50%折扣；首批hy3避开限时目录与折扣混淆。
- 上下文、最大输入、最大输出三种限制须同时尊重，不应输入上限等同上下文。

### deepseek/deepseek-flash

官方来源：[来源1](https://api-docs.deepseek.com/quick_start/pricing/)、[来源2](https://api-docs.deepseek.com/api/create-chat-completion/)。

- 当前版本V4.1-Flash-1017；旧deepseek-chat/reasoner不作为新配置首选。
- 高峰为UTC工作日01–04与06–10，中国公众假日除外；其余空闲。
- 默认思考；如首批只开普通文本，可通过已有支持的reasoning_effort=none或thinking.type=disabled关闭。

### minimax/MiniMax-M3.1-Flash

官方来源：[来源1](https://platform.minimax.cn/docs/api-reference/text-openai-api)、[来源2](https://platform.minimax.cn/docs/api-reference/text-chat-openai)、[来源3](https://platform.minimax.cn/docs/pricing/overview)。

- OpenAI兼容API与中国站基址官方明确；大小写保持MiniMax-M3.1-Flash。
- 强制思考；禁用thinking返回400；reasoning_split不能false。
- temperature固定1、top_p固定0.95，客户端传值不会生效，故不声明支持。
- max_tokens已弃用但仍列兼容参数，官方建议max_completion_tokens；生成上限包含思考。
- M3.1-Flash不支持service_tier；不套M3的priority计价。

## 阻塞：Kimi K3.1缓存TTL分桶

Kimi `kimi-k3.1-pro` 的总缓存写入字段 **能被现有组件解析**：官方Chat为 `usage.prompt_tokens_details.cache_write_tokens`，现有OpenAI兼容解析器读同名字段，和 `cached_tokens` 读桶一起映射到IR。不能把当前缺口误写为完全不支持cache-write。

真正缺口是 **5m/1h写入计量**。官方Chat通过响应头 `Msh-Usage-Cache-Write-Tokens-5m` 与 `Msh-Usage-Cache-Write-Tokens-1h` 区分TTL，JSON usage只有写入总桶；现有 `reference.rs::usage_of` 只接JSON并填总 `cache_write_tokens`，未解析这两个响应头或填充TTL子桶。宿主有1h价字段并不意味着组件提供了相应证据。

默认自动缓存使用5m，但同一组织中已有条目的TTL在首次写入时锁定；后续请求可能继续写既有1h条目。因此只配5m价、或用单一总桶价格计所有写入都不能冒充完整计量。本例不给Kimi写可用型号；需要独立完善解析与验收后再配置，本批不扩协议。依据：[官方缓存字段、TTL与默认行为](https://platform.kimi.com/docs/guide/context-caching)、[官方计费说明](https://platform.kimi.com/docs/pricing/chat)。组件证据：[OpenAI兼容参考实现](../../crates/south-component-conformance/src/reference.rs)的 `usage_of`；[组件壳](../../components/provider-openai-compatible/src/lib.rs)复用该实现。

## 阻塞：NVIDIA托管NIM价格

未取得 `integrate.api.nvidia.com` 托管端点明确的生产按token价卡。Developer Program免费访问只用于原型、研发和测试；AI Enterprise按GPU许可证计价不是托管token价，不能填永久零美元。本例保留服务商入口，但没有假可用零价型号。依据：[官方NIM FAQ](https://docs.api.nvidia.com/nim/docs/product)、[API试用条款](https://assets.ngc.nvidia.com/products/api-catalog/legal/NVIDIA%20API%20Trial%20Terms%20of%20Service.pdf)。

## 导入与后续核对

1. 先读取宿主目录，服务商存在则不重复创建，型号以全局name比对，只创建缺项；不要自动覆盖既有价格、路由和凭证。
2. Mistral按上节明确更新严格兼容端点；Cerebras/SambaNova只进入开发许可证实例。选择对应站点签发的key，禁止跨站重试密钥。
3. 新库可选择片段合入首启目录，存量库通过既有管理API显式创建；型号片段本身不包含启用、凭证或额外认证配置。
4. 导入后回读名称、provider、upstream_model、价格桶、标签与端点。实际调用和逐桶对账应另外授权执行；本文件不声称完成。
