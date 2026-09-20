# MiniMax v1 任务组件候选

基线：South e18eeff，server a8dd0ace。新增 task-minimax-v2，复用 task-adapter-v2，
保持 task ABI/WIT 不变，已准备 HTTP9/runtime0.30.0 与六包新身份；
v0.29.0 已发布的五包和原发布树不变，0.30.0 尚未发布。
本文与样例来自宿主代码转录，不是上游抓包或在线供应商验收。

## 行为范围

- 仅 Hailuo v1：MiniMax-Hailuo-02、MiniMax-Hailuo-2.3、MiniMax-Hailuo-2.3-Fast。
- 提交 POST v1/video_generation；locator 只存 v1/query/video_generation。
  查询 GET 只接受受控数字 task_id，绝不改写持久 ID。
- 输入沿 server handler/video/minimax.rs 的 build_minimax_submit：prompt、首帧
  三个别名、末帧两个别名、duration、resolution、prompt_optimizer、fast_pretreatment。
- 默认 6 秒/768P；末帧只允许 Hailuo-02，拒 512P。型号档位和价格最终仍由宿主准入。
- prepare 只报告归一请求时长；费率 None。宿主按自己已有价格卡/转售政策预占，
  不把价格卡硬塞成组件报告用量，也不把请求时长填进成功实际用量。
- Success 必须有有效 file_id；额外 GET v1/files/retrieve?file_id=...，
  render 从 fetched 的 file.download_url 输出产物，created/model/provider/ID来自宿主上下文。
- 缺失计量保留未知；v1 没有已经核实的用量字段，不臆造 usage 字段语义。
- submit HTTP4xx 与明确 base_resp 非零为 Rejected；5xx、坏JSON、缺ID为 Unknown。
  query HTTP/解析失败或未知状态为 Unknown；文件取回非2xx保留HTTP状态。
  submit/取回的业务错误沿宿主保留401/400/429/503，未知业务码502；
  上游自由文案不回显，Fail终态仍502。
- 所有请求只持有配置 SecretRef；无网络、时钟、凭证读取。

## 配置与授权

以 ProviderConfig 现有扩展字段 group_id 承载可选非秘密 GroupId，
只有 group_id 声明进入 URL，其余扩展不转发；locator 不保存组号或 prompt/image。
upstream_model_family 是宿主提供的具名协议家族，缺省取原 model；只允许上述三个
Hailuo 家族。转售原 model 原样进入上游 body；客户端同名字段不决定家族。
该字段也必须进入宿主受限恢复快照。
三类 descriptor 均由真实 ProviderConfig.authorize 验证来源及凭证绑定。
国际配置缺省不追加 GroupId；中国配置按实际绑定显式提供并 trim。旧 query 分支遗漏 GroupId，本候选查询补齐，
这是已明确的修复，不声称与旧遗漏行为字节相同。

基线 QueryStringV1 有 GroupId/task_id，但缺少 file_id，无法通过共享 GET
执行文件取回。本次已用真实红测补齐最小 QueryParameterV1::FileId：wire 名
file_id，非空 ASCII 数字，最长沿用 MAX_QUERY_VALUE_BYTES=64，保留前导零，
拒重复。追加在规范顺序末尾，既有声明字节不变；受控GET真实socket已通过。
HTTP合同升9，0.30.0版本/六包身份与发布名单准备已完成，尚未发布；
不能由纯组件和GET测试推导生产宿主任务链已完整接入。
恢复时宿主必须保存具名、有界非秘密 group_id 配置快照，不能重读当前组号。

2026-09-20 核 [官方查询文档](https://platform.minimax.io/docs/api-reference/video-generation-query)：
task_id/file_id 声明为字符串，官方示例为十进制数字。文档没有数字限定的强保证，
本批保持已有 TaskId 收窄合同；非数字 ID 原值保留，查询明确不兼容而不改写。
文件查询采用相同最小已证范围，未来扩大须新证据与合同评审。

## 验证

task-v2 codec/估算基线、公开行为红绿、20个冻结样例与同源guest真Wasm
对拍均已通过，覆盖真实授权、错误凭证/跨源反向、未知受理、ID、文件取回
失败与宿主时间身份。README16项全绿，nextest642/642、no-default623/623。
完整日志及修复历史见[验证记录](2026-09-20-minimax-v1-verification.md)。
本仓验证完成仍不等于生产宿主 M1 已完成。

## 发布登记

第六包登记到既有构建/打包流程，触发模型不变；尚未发布。正式 v0.29.0
仍只有五包。本工作树已准备0.30.0身份，HTTP9/runtime0.30.0与六包版本联动，
详见发布准备记录；不得重发已有不可变身份。

## 兼容与完整接缝

- 保留的是从响应解析后的上游 ID：仅解析时 trim 首尾空白，保留前导零；
  持久化后查询不改写。空白/负数 ID 保守未知。非数字字符串不伪造，查询受控门拒绝。
- submit HTTP4xx 明确 Rejected；5xx/成功状态下坏JSON为 Unknown。旧宿主在
  parse 前已处理HTTP，本候选把同一语义纳入纯接缝，尚无生产行为切换。
- files 非2xx保留状态并使用脱敏消息。自由上游文案不进入返回或诊断。
- descriptor query统一受控规范顺序（GroupId在task_id/file_id前），不能手拼
  然后假定与transport规范序列化字节相同。
- 宿主为旧公开响应同形应传 provider="minimax"、created=渲染时钟；不得
  用配置实例名或task创建时钟冒充。通用context透传单测另保留。
- HTTP合同升9；task/WIT/IR保持原版本。

转售末帧资格改为宿主提供Hailuo-02家族，修复旧纯builder按原名比较导致
别名末帧拒绝的问题，不宣称此处与旧限制完全等价。
