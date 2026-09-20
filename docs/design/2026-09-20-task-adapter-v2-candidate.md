# task-adapter-v2 候选：可恢复定位与完整任务事实

> 2026-09-20，目标架构第四批。基线 `ac11c3b`；这是未发布的候选契约，不声明宿主生产采用。本文按 lv 本次“所有文档中文”的要求编写。旧 task-v1 保持行为与包身份；本批不修改 release tag，不发布组件。

## 问题与责任

server 实际 managed Kling 消费链证明：v1 查询无法恢复同型号的图／文提交分支，单 meter 丢失同时出现的时长和毫单位，Running 丢失排队事实，render 丢失逐产物 id/duration 及宿主响应身份。不能由宿主重新解析原始 JSON 补洞，也不能伪造型号或包装上游 ID。

South 拥有协议翻译与上游事实；宿主继续拥有费率、资金、模式售卖政策、凭证来源、任务持久化、执行时序和交付授权。新增契约供社区与 server 采用，但本批只有候选验证，不能将其记作两个宿主均已使用。

## 本批范围

1. 新 `token-station:task-adapter@2.0.0` / `task-adapter-v2`，保留 provider-v2 与 task-v1；同一加载器按 world 显式分流，复用权限、资源上限及身份校验。
2. 新 `TaskComponentV2`、唯一 JSON codec、原生 Kling v2、独立 guest/fixture 包；原生与真 WASM 同源对拍。新增官方包同步构建与 release 打包名单，CI 触发模型不变。
3. server 在临时工作树中用本地 south 覆盖依赖，验证候选输出经过真实授权与计价接缝。正式 pin、生产 loader 和 managed 提交路径本批不切换。

## 类型与调用

- `TaskLocatorV2`：`schema_version=1` 和有界相对 `route`。复用 `RelativePathV1` 语法，拒绝绝对 URL、查询、片段、越界路径、未知版本与未知字段。route 由组件解释，宿主只保存并回传；不规定所有后续方言必须用“路径加 ID”。Kling 只从四个固定 API 集合路径中选择，禁止包含请求体、prompt、图像、认证材料、callback nonce 或上游 ID。语法约束不能证明任意字符串无秘密，官方实现还须通过敏感哨兵与定位回读测试。
- `TaskUsageFactsV2`：可同时存在的 `seconds`、`milliunits`、`tokens`；有限、非负，缺失和零不同，不含金额。它只描述观察事实；请求预估与上游实际用量不能混为一份无来源的 meter。
- `TaskObservationV2`：`Progress { running, status_word }`、`Succeeded { artifacts, usage }`、`Failed { kind, code, message }`、`Unknown { reason }`。失败 kind 复用现有三词。失败查询、未知词和本地等待预算不得伪造任务失败。
- `TaskArtifactV2`：URL，以及可为字符串、数字或 null 的 id/duration。封闭 scalar 保留 JSON 数字类型，拒绝对象／数组／布尔；URL、数量与字符串均有界。提供 `TaskArtifactRefV2::Urls/FileId/None`，保留已有额外产物取回形态；公开 enum 即使绕过构造器，也须在 codec 和边界重新验证。
- `TaskRenderContextV2`：宿主传入 task_id、created、公开 model/provider、可选原始 upstream_task_id。没有时钟访问或 callback。Kling 渲染需要上游 ID，缺失即拒绝，不伪造。
- `PreparedTaskV2 { descriptor, locator }`：同一次纯调用产出发送描述和定位，含 IR descriptor，故放在既有唯一 IR 消费者 conformance，不给 contracts/runtime 新增 IR 依赖。

七个翻译操作仍为 build-submit、parse-submit、build-observe、parse-observation、build-artifact、render-success、map-terminal-failure。observe 增加 locator，保留真实 upstream_model 和原始 ID；artifact 操作同样接受 locator；render 改用独立 context。`SubmitOutcomeV2` 的同步受理携带类型化观察，不让宿主二次解析响应。

JSON 表示只由 conformance 的 v2 codec 定义，contracts 不直接发布 serde wire 形状。持久 locator 严格拒绝未知字段；供应商响应允许无关新增字段，两者不能共用 v1 的“所有输入容忍未知字段”测试策略。

严格封闭检查针对 v2 定位、观察、上下文及受理外层；嵌套的 IR descriptor／ErrorEnvelope 继续遵守原 IR 的扩展字段语义，不在本批另造一套解析规则。成功解码还须保证规范化再编码不会越过同一字节上限，不能仅校验较短的原始 JSON。

v2 本批不声明 `host.sign` import：Kling 只需 descriptor 中的 Bearer SecretRef，由宿主签发 JWT。没有实际消费者的签名 ABI 不在这次顺便冻结；后续基础能力按 D3 版本化加入。现有 provider/task-v1 world 不因此改动。

候选 manifest 只接纳本批已有验证的 `bearer`／`header_secret` 认证臂。`host_signed` 是宿主 HTTP 最终签名能力，与 WIT 的 `host.sign` import 是两件事；限制前者的理由是尚未完成 v2 接缝验收，不能从缺少后者推导它理论上不可支持。

## Kling 输入及兼容边界

v2 输入明确给出组件 operation：`text-or-image`、`omni`、`motion-control`；model 必须是真实上游型号。operation 是组件输入词汇，宿主后续应通过目录配置选择，不能在通用编排中新增 Kling 枚举。宿主先应用收费行的 mode 禁止／覆写政策，再把有效 mode 传给组件；Omni 要求已解析的 mode。组件负责字段映射、协议默认值、image/image_tail 区分、Omni 引用视频冲突与 motion 必填校验。

既有可选嵌套对象按原 allowlist 透传；没有证据的 masks 等字段内部 schema 不凭空严格化。普通请求缺省 duration 为字符串 `"5"`；Omni base edit 省略 duration/aspect_ratio；motion 省略 duration。只有普通 text/image 放 host external_task_id，不承诺 Omni/motion 有同样幂等锚。

候选的提交确定性采用 server 的保守口径：HTTP 2xx 优先非空嵌套 ID，其次顶层 ID；没有有效 ID、非 2xx 或坏 JSON 都为 Unknown，包括仅有非零 code 的响应。不能照搬 v1 的非零 code→Rejected 而在迁移时无声改变退款行为；明确 HTTP 拒绝仍按宿主执行层的既有策略处理。

上游任务 ID 在持久化和渲染中保持原文，只在构造查询 URL 时编码为单个路径段，防止 `/`、`?`、`#`、`%` 改变路由。拒绝空值、超限值和 URL 规范化会吞掉的 `.`／`..`；错误消息不回显输入 ID。这是请求构造边界，不是通过包装 ID 保存提交类型。

真实授权测试进一步限定：现有 kernel 明确拒绝 `%2F`／`%5C` 等编码分隔符，所以原始 ID 含 `/` 或反斜杠时，即使组件正确编码仍不能由当前宿主发送；`?`／`#`／`%` 编码可通过。不能为了候选测试通过而放宽宿主路径防线，本批不承诺任意 opaque ID 均可查询。

渲染返回既有 `{created,model,provider,task_id,data:[{url,duration,id}]}` 形状，原 URL 只供宿主内存消费，公开交付与产物代理仍由宿主决定。候选显式收紧畸形事实：缺／空 URL 的混合列表、id/duration 的对象／数组／布尔及无效用量不得伪装正常成功。不能过滤个别产物后重编号造成索引错位。这些属于候选拒绝边界，生产切换前须完成确认与回归，不能称为对所有历史畸形回包逐字等价。

Kling 毫单位换算沿用 south v1 的 `10^15` 上限，超过上限视不可观察；旧 server 只按 `i64` 转换限幅，所以这一点同样属于候选收紧。通用用量 DTO 仍容纳非负 `i64`，该上限不冒充所有供应商共有规则。缺失／null 是未上报，合法零是实际零；非法字符串、非有限数和负值必须保持可区分。

## 尚不属于本批完成项

- 请求单位估算表仍在 server：下沉前需区分宿主估算时长与协议单位率，保留现有 video_list“字段存在”与“非空”差异的证据。不能将此残留藏在“全链已迁移”表述中。
- prepare/reserve 同事务保存完整执行绑定、持久恢复、旧包留存、worker 升降级门和真实 managed 切换仍待后续批次。
- 同一兼容元组下三 world 共存，不等于跨 `south_runtime` release 可恢复；仍严格匹配元组，不从包反填宿主期望。
- 本地候选版本号不构成发布承诺。正式发布前统一确定版本、兼容清单和发布包；宿主只能在已发布后升级正式 pin。

## 验收

先观察公共行为红测，再实现；每场景 ≤60 秒。验证 locator 严格边界／敏感哨兵、queued/running、双计量和零/缺失/无效、多产物 scalar 与顺序、text/image/tail/Omni/motion 字段、真实包错 world／坏身份／资源上限、原生和 WASM 一致。保留所有 v1 验证。最后跑 README 全量命令和 server 候选消费者，再记录实际通过、失败与未验证范围。

## 本批验收结果（2026-09-20）

公共行为红测后，契约 6 条、codec 13 条、Kling reference 13 条、七方法 suite 5 条、
真实 Wasm parity 3 条分别通过；fixture 实际为 52 组。运行时验证含旧 task 3 条、
新 task 8 条和既有沙箱 12 条。清单／WIT 校验 38 条、官方包对账 2 条通过。
这些定向数量描述本次执行，不作为以后不可改变的总数。

最终 README 全部 16 项检查通过；all-features nextest **610/610、0 skipped**，
测试窗口 **3.833 秒**。codec 加入既有定时 `contract_parsers` target，并做 21 秒
本地短 fuzz，**664439 runs，退出 0**；没有运行定时长 fuzz 或 soak。
明细位于本机 `/tmp/target-architecture-batch4-south-full/`，完整摘要日志
`/tmp/target-architecture-batch4-south-full.log`。新解析器还具有属性测试。

server 以真实宿主授权和计价函数验证候选，最终 **6/6、0.052 秒**，目录
`/tmp/target-architecture-batch4-host-candidate-verified/`；测试只跑 balance 形态。
生产 v1 loader 拒绝 v2 仍是一项必过断言。正式 server pin 没有升级，正式路径的
严格短测矩阵为 **58 PASS／0 FAIL／0 SKIP**。候选通过临时 patch 重编，
不是固定宿主二进制的免重编验收。

失败证据同样保留：空变体忽略未知字段、规范化 JSON 超限、native artifact 未
重验、旧 loader 错 world／签名 import 未准入拒绝、release 名单漏包均经修复；
编码斜杠由真实宿主拒绝是现有边界，未改作放行。宿主工具首轮 patch 未传递给
nextest 子构建，实际编译旧发布版并失败；改在一次性 Cargo.toml 覆盖后，先以
metadata 断言八个 crate 的唯一真实路径，再运行测试，不能把首轮失败算作通过。

最后全量验证前后，449 个非文档文件的路径／内容集合摘要保持
`c38458889f7f8e12de12ac3cc409b2bd385ffffd9963270a24ad42d031e33342`。
候选 Wasm 字节摘要为
`07124010f9a4c31a52213dec18d447e67892b1b8cab0c1f19429c03ed485901c`，
来自同次官方构建与真实 parity，随后在宿主工具中复验。之后仅回填文档。

本批关闭的是候选协议和接缝验证。请求估算、执行绑定／持久恢复、旧包留存与
worker 兼容窗口、生产 managed 调用、社区真实采用和共享 task-core 均继续开放。
