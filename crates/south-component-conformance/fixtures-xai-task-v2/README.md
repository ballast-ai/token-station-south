# xAI 视频冻结样例

期望按 token-station-server 原生 xAI 臂（`handler/video/xai.rs` 构造与成功渲染、`durable.rs::parse_xai_create`、
`observe.rs::normalize_xai` 四态归一、`VideoTaskAdapter for Xai` 失败文案）手写转录，加 xAI 文档的时长 1–15 秒与参考图 ≤ 7
上限、边界合成负例；**不是线上抓包，也不得从被测实现重新生成 expected**。

与原生的差异（组件设计记录见 `src/reference_xai_task_v2.rs` 头注）：输入图原样转发不代取；越界时长在组件处 400 而不是交给上游拒；
未带时长时不报申报秒数（由宿主缺省估时政策决定预占）；声明禁改路径 `model` / `duration`；每次提交 1 条输出、成功报 `outputs = 1`。
