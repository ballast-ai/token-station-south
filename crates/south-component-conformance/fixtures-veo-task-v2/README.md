# Google Veo（Gemini API 线）视频冻结样例

期望按 token-station-server 原生 Veo 臂（`handler/video/gemini.rs` 的 `build_veo_submit_body` / `build_veo_instance` /
`encode_veo_image_part`、`durable.rs::parse_veo_create` 的「无 LRO name 且确为终态才直喂」、`observe.rs::normalize_veo` 的三处回退键与
RAI 过滤、`VideoTaskAdapter for Veo` 失败文案）手写转录，**不从被测实现重新生成 expected**。不是线上抓包；base64 负载为占位串。

合同 6 在本族的落点：凭证经 `x-goog-api-key` 头；产物一律 `fetch_with_credential: true`（渲染体给的仍是上游 URI，宿主须改写为自有代理路径）；
`sampleCount` = `requested_outputs`、交付样本数 = `usage.outputs`；回执即终态 → `accepted-terminal`。

与原生的差异：输入图须已是 `data:` URI（宿主预取），普通 URL 400 并指路；非正整数时长 400（原生静默丢弃、按默认 8 秒预占）；未带时长不报
申报秒数；Vertex 操作名（`projects/…`）拒收——该线需服务账号铸币与按 region 派生的端点，不在本组件。
