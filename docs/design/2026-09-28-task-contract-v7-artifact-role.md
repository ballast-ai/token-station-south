# Task 合同 7：产物角色

基线 `76bf632`（`v0.36.0`）。本记录只定合同形状；发版窗口与 server 改钉由 token-station-server
[P16](https://github.com/GlimpseEngine/token-station-server/blob/dev-v2/docs/product-review-v2/plans/2026-09-28-P16-BytePlus%E5%B0%BE%E5%B8%A7%E4%BA%A7%E7%89%A9%E8%A7%92%E8%89%B2.md)
负责（DP1–DP5 全取建议项 A）。

## 为什么要改合同

server 原生 BytePlus 臂在上游返回尾帧（请求带 `return_last_frame`）时，阻塞体是
`data[0] = {url, last_frame_url}`，上游 URL 原样透传；异步信封恒 1 个产物。`task-byteplus-v2`（0.35.0）
解析观察只读 `content.video_url`，尾帧被丢——开尾帧的型号不能切组件。

合同 6 里只有「产物」一个通道能把尾帧带出观察，而宿主把观察里的**全部**产物都当主产物：信封按产物数出
`art_i`（尾帧会变成取不到的 `art_1`，转存也缺它），渲染体 `data[]` 也会多一项。**组件内还原、不改合同**两头
不能同时对齐原生，所以要让合同区分「主产物」与「随主产物一起报出、但不算输出」的产物。

## 合同形状（D7 同规：新键必须出现、值可为 null）

WIT `task-adapter-v2@2.0.0` 不动：与合同 5→6 同一先例，只改 JSON 串里的契约字段。
`TASK_CONTRACT_VERSION` 6 → 7；`compatibility.json` `contracts.task` 同步。缺 `role` 键的合同 6 观察解码即拒——
`south_runtime` 精确相等已迫使发版时重打全部任务包，给缺省值换不来任何兼容（见合同 6 记录）。

### 产物角色 `artifacts.items[*].role`

URL 产物新增必填键 `role`，封闭词表 `TaskArtifactRoleV2`：

| 线上取值 | 枚举 | 含义 |
|---|---|---|
| `null` | `Primary` | 任务的可交付输出。宿主**只**对这一角色计数、代理、转存、出信封 |
| `"last_frame"` | `LastFrame` | 主视频的尾帧，紧随其主产物报出。宿主不计数、不交付、不转存；组件渲染时自己安放 |

词表外的串（含 `"primary"`）拒收，不做缺省；将来加角色就是再升合同号（P16 DP5：没有第二个消费者，不预留）。

`TaskArtifactV2::new` 签名不变（缺省 `Primary`），新增 `with_role(role)` 与 `role()`；`TaskArtifactRoleV2` 提供
`word()` / `from_word()` / `is_primary()` / `ALL`。`Debug` 输出带角色（不带 URL）。

### 集合校验

`TaskArtifactRefV2::Urls` 必须**至少含一个主产物**：只有伴随产物的集合无物可交付，`urls()` 与 `validate()`
都拒（`InvalidArtifact`）。顺序不作约束。`MAX_ARTIFACT_URLS = 16` 把伴随产物也计在内（上界是线格式体积，
不是输出条数）。

### 宿主义务

- 计数（信封 `art_i`、产物 id 上界、转存对象数）、代理改写与转存只认主产物。
- 回传给组件渲染的 observation JSON 必须**完整**（含伴随产物）——渲染在组件内，宿主不得按角色过滤。
- 取回 / 转存读的是渲染体的 `data[].url`；伴随产物由组件写在 `data[].url` 之外（如 `data[0].last_frame_url`），
  因此不会被当产物取回或改写。
- 凭证门控（合同 6 `fetch_with_credential`）的既有规则不变：门控 URL 出现在 `data[].url` 之外即整份响应拒交付。
  伴随产物**不得**标门控——BytePlus 尾帧是预签名 URL，本不需要。

### 组件义务

- 只把真正的可交付输出报为 `Primary`；`usage.outputs` 按主产物计（尾帧不抬高交付条数）。
- 渲染体把伴随产物放到与原生同形的位置，不进 `data[]` 列表。

## 首个消费者：`task-byteplus-v2`（包身份 0.35.0 → 0.36.0）

- 解析：上游 `content.last_frame_url` 为非空串时，在视频之后再报一项 `role = last_frame` 的产物（不门控）；
  串超出产物字节上界判 unknown（不静默丢帧）；空串或缺席即没有尾帧。
- 渲染：`data` 只收视频；尾帧写进它前面那条视频的 `last_frame_url`（Seedance 单输出即 `data[0]`）；尾帧排在
  任何视频之前判 `provider_protocol_error`。
- fixture：`observation.succeeded` 期望多一项尾帧；`render.direct` 输入带尾帧、期望 `data[0].last_frame_url`；
  新增 `render.no-last-frame`（不带尾帧不多一项）与 `render.frame-before-video`（协议错误）；`artifact.direct`
  输入带尾帧、期望仍为 `null`。
- 头注与 fixtures README 的「能力损失」改写；`shipped_packages_v1` 退役 0.35.0 身份。

T09 金丝雀 guest 加同一模式：卷轴的 `role` 原样抄到产物上（缺席即 `null`），渲染时 `last_frame` 卷轴写进它前面
那条卷轴的 `last_frame_url`——server 侧据此验宿主义务（不计数、不交付、封存往返不丢角色）。

其余十二个任务包只随本合同重打（fixture 期望机械补 `role: null`，行为不变）。

## 不在本合同

- 其他任务族的伴随产物（首帧、缩略图……）：不预留取值。
- 尾帧的代理 / 转存：P16 DP4 取「原样透传」，与原生一致。
- 宿主怎样把伴随产物暴露给客户端：合同只规定组件渲染体，与合同 6 同。

## 验证计划

先 RED：codec 对合同 6 形状（缺 `role`）必须拒、对词表外取值必须拒、只有伴随产物的集合必须拒；再改合同与编解码，
全部参考组件 fixture、T09 guest 补新键。门禁：fmt、严格 clippy、nextest 全量、真实 Wasm 对拍腿
（`byteplus_task_sandbox_parity_v2`）、T09 金丝雀（server 侧）。
