# task-v2 请求估算候选

2026-09-20，第五批；基线 south `1d73242`。仅演进未发布候选，不升级宿主正式 pin。

`PreparedTaskV2` 新增必需 `request_estimate: TaskRequestEstimateV2`。该类型私有字段为 `requested_seconds: Option<f64>` 与 `milliunits_per_second: Option<i64>`，经构造器验证有限、非负。`estimate_milliunits(host_seconds)` 验证宿主估时后计算 ceil，并拒绝溢出；无单位率返回 None，不能用零替代缺失。它与 `TaskUsageFactsV2` 无类型转换关系。

Kling 从同次最终提交 body、真实型号与 operation 提取依据：managed 的 video_list 按字段存在选费卡；motion 和 Omni base-edit 没有 duration，返回 None。协议默认 duration 为 5 秒时依据为 Some(5)。缺省 mode、音频、四个 family 的协议单位表由组件解释；卡外组合和未知型号保留 None，由宿主选择时长或 flat fallback。宿主仍拥有缺时长回退 5 秒、价格、markup、余额与预占事务。

存在但非法的 duration 拒绝为错误；这是对现有 lenient_secs 接受 NaN/负值后饱和转换的候选收紧。缺失或 null 均不提供时长事实。预估并非实际用量，也不是真实费用的数学上界；不得回填观察或结算事实。

不新增第八个 WIT 操作。严格 prepared JSON 必须提供 request_estimate，旧候选缺字段拒绝；更新 task vocabulary 3→4，release/world 仍为未发布候选。新旧包内容须重新验签／锁定，不能冒充字节兼容。

验收先通过最小类型壳得到真实公共行为红测，再实现。覆盖单位表所有模式轴、managed 字段存在与非空差异、最终 body 时长、零/缺失/非法/溢出、严格 codec 和规范化回读、原生与真实 Wasm。每个场景 ≤60 秒；本批不宣称 server 生产迁移或社区采用。

## 本批验证

公共行为红测 7/7 真实失败后修正；定向回归 42/42 通过，含 59 组 fixture、157 项 suite 检查和真实 Wasm 的费卡组合对拍。前两轮 clippy 分别指出同值 match 臂与测试无谓按值传参，已修复，未放宽 lint。日志均以 `/tmp/target-architecture-batch5-south-` 开头。

最终 README 16 项命令全部通过，汇总 `/tmp/target-architecture-batch5-south-full.log`，逐项结果 `/tmp/target-architecture-batch5-south-full/results.json`。nextest **618/618**，0 skipped，实际测试窗口 **6.054 秒**；一个既有 fixture 用例出现进程退出延迟，单独复验 **1/1、0.015 秒，无 LEAK**，记录在 `...-leak-recheck.log`。没有把延迟隐藏成不存在。

既有 contract_parsers fuzz 增加估算往返检查及可追踪 seed；定时 CI 仅增加将 seed 复制到 corpus 的命令，触发模型与长测时长不变。本地以 20 秒预算短跑，实际 **937704 runs／21 秒，退出 0**，日志 `/tmp/target-architecture-batch5-fuzz-final.log`；没有执行定时长 fuzz。

最终非文档源码 465 项集合摘要为 `d2a40603f1ac586b355720c07769a78c6062df44e9249c2f838c999bcbce9c26`；候选 Wasm 摘要为 `077053ad42a6f3d7e0e53c4c56e73e7f567d71d661fa20a6190a02f451e367c7`。独立规格与质量审查均无遗留阻断。server 的实际消费者验证另记宿主证据，本仓测试不能替代生产接线、持久任务恢复或社区采用。
