# BytePlus Seedance 视频冻结样例

期望按 token-station-server 原生 BytePlus 臂（`handler/video/byteplus.rs` 的提交体与成功渲染、`durable.rs::parse_byteplus_create` 与
`byteplus_billing_tokens`、`observe.rs::normalize_byteplus`、`VideoTaskAdapter for BytePlus` 失败文案）手写转录；每秒 token 数按
`宽 × 高 × 24 ÷ 1024` 手算（720p 21600、480p 10044、1080p 48960、4K 194400），**不从被测实现重新生成 expected**。不是线上抓包。

与原生的差异：参考图上限按 Seedance 最高 30 张（按型号的更低上限由宿主能力预检把关）；渲染体**不含** `last_frame_url`
（合同 6 的观察只承载视频产物——见 `reference_byteplus_task_v2` 头注的「能力损失」）；`completion_tokens` 为负或非整数时判 unknown
而不是按 0 结算。
