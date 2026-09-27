# Task 合同 6：输出计量、token 估算、产物取回凭证与请求体禁改路径

基线 `a923b76`（`feat/t09-canary-modes`，基于 v0.34.0 `cad896d`）。未发布，属 0.35.0 候选。
来源：token-station-server P13（任务族组件化收口）C12 与拍板 D5 / D6 / D7。
本记录只定合同形状；发版窗口与 server 改钉由 server 侧 S9 负责，本次不 tag、不发布。

## 为什么要改合同

server 要把 BytePlus、Veo（Gemini 线）、百炼万相、GMI 从宿主原生实现迁到 task-adapter-v2 组件。
合同 5 表达不了四件事，缺哪件，对应的族就迁不过去：

| 缺的事实 | 谁需要 | 现在的后果 |
|---|---|---|
| 请求的 **token 估算率** | BytePlus（按像素 token 计费：分辨率 × 时长） | 宿主只能自己再写一份像素公式——正是组件化要消掉的「第二份实现」 |
| 请求的**输出数量**、实报的**交付数量** | 图像（按张）、Veo（按条 × 时长） | 按张 / 按条计价无从预占与结算 |
| 产物要**带凭证取回** | Veo（产物 URI 需 `x-goog-api-key`） | 宿主裸 GET 被拒；按域名自动带凭证又会扩大凭证外泄面（D5 否决的 B 案） |
| 请求体里**宿主不得改写**的路径 | 所有要支持附加请求配置的族（D6） | 宿主给不出禁改表，只能对组件族一律不注入——迁移后附加请求配置能力倒退 |

## 合同形状（D7：新键必须出现、值可为 null）

WIT `task-adapter-v2@2.0.0` 不动：全部是 JSON 串里的契约字段，与合同 4→5（分辨率、输入图数）同一先例。
`TASK_CONTRACT_VERSION` 5 → 6；缺任一新键的旧 JSON（合同 5 组件的输出）解码即拒——反正 `south_runtime`
精确相等已迫使发版时重打全部任务包，给缺省值换不来任何兼容。

### 1. 请求估算 `request_estimate`

新增两个键，与既有两个同为「请求侧事实、不含价格」：

- `tokens_per_second`：整数，≥ 0。**协议单位率**，与 `milliunits_per_second` 对称——组件按本族公式（例如分辨率
  决定的每秒 token）给出，宿主乘自己的时间与价格；组件不报价、不算金额。附 `estimate_tokens(host_seconds)`，
  与 `estimate_milliunits` 同一取整与上界规则。
- `requested_outputs`：整数，≥ 1。请求要产出的条数 / 张数。`null` 与 `1` 不同：`null` = 组件不表态。

`TaskRequestEstimateV2::new(seconds, milliunits_per_second)` 与 `with_input_facts` 签名不变；新增
`with_output_facts(tokens_per_second, requested_outputs)` 与两个 getter。

### 2. 用量 `usage`

新增 `outputs`：整数，≥ 0。上游**实报**交付的条数 / 张数；缺席（`null`）与 `0` 是不同事实。
`TaskUsageFactsV2::new(seconds, milliunits, tokens)` 签名不变；新增 `with_outputs(outputs)` 与 getter。
合同不规定宿主怎么用它结算（例如「结算张数 ≤ 交付 + 1」是 server 的政策，不进合同）。

### 3. 产物取回凭证（D5）

URL 产物 `artifacts.items[*]` 新增必填布尔 `fetch_with_credential`：

- `false`：URL 自带访问能力（预签名 / 公开），宿主裸取。合同 5 的全部产物都是这一形。
- `true`：宿主须用**该任务钉住的那把凭证**、按提交时同一认证方式取回，并且不得把原 URL 直接交给客户端
  （宿主改写为自己的代理路径）。

只对 URL 产物成立：`file-id` 形本就经 `build_artifact_request` 由组件描述取回请求（带认证）。
是**组件**声明，不是宿主按域名推断——D5 否决了「与 endpoint 同域就带凭证」，它管不住跨域 CDN，也扩大外泄面。
`TaskArtifactV2::new` 签名不变（缺省 `false`），新增 `with_bound_credential()` 与 getter。

### 4. 请求体禁改路径（D6）

`PreparedTaskV2` 新增 `immutable_body_paths`，JSON 键必填：

- `null`：组件不表态——宿主**不得**向请求体注入任何附加字段（合同 5 下 server 对组件族的现行做法）。
- `[]`：没有保留路径，宿主可注入任意字段。
- 非空数组：点号对象路径（每段 `[A-Za-z0-9_-]+`，不支持数组下标，≤ 64 条、每条 ≤ 256 字节），宿主不得改写
  这些路径**及其任何祖先与后代**——计费相关字段（时长、分辨率、条数）必须列在这里，否则附加配置能改写组件
  据以报估算的字段，让预占与实发脱钩。

数组内不得重复。路径描述的是**发出的请求体**（descriptor body），不是宿主给组件的输入请求。

## 不在本合同

- 宿主如何结算（按秒 / 毫单位 / token / 条数的优先级与缺省估时）——server P13 C2 / D3。
- 回调通道（C13）、Vertex 服务账号铸币——server 侧后置。
- 新组件包（xAI / BytePlus / Veo-Gemini / 百炼万相 / GMI）：随本合同在后续提交里逐个加，不与合同改动混在一个提交。

## 验证计划

先 RED：codec 对合同 5 形状（缺新键）必须拒、对新键的非法值必须拒；再改合同与编解码，全部参考组件、
fixture 期望与 T09 guest 补新键。门禁：fmt、严格 clippy、nextest 全量、真实 Wasm 对拍腿、T09 金丝雀（server 侧）。
