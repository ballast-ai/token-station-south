# BytePlus Seedance 视频冻结样例

期望按 token-station-server 原生 BytePlus 臂（`handler/video/byteplus.rs` 的提交体与成功渲染、`durable.rs::parse_byteplus_create` 与
`byteplus_billing_tokens`、`observe.rs::normalize_byteplus`、`VideoTaskAdapter for BytePlus` 失败文案）手写转录；每秒 token 数按
`宽 × 高 × 24 ÷ 1024` 手算（720p 21600、480p 10044、1080p 48960、4K 194400），**不从被测实现重新生成 expected**。不是线上抓包。

与原生的差异：参考图上限按 Seedance 最高 30 张（按型号的更低上限由宿主能力预检把关）；`completion_tokens` 为负或非整数时判 unknown
而不是按 0 结算。上游 `last_frame_url` 按任务合同 7 作 `role = last_frame` 的第二个产物报出（`observation.succeeded`），渲染时写回
`data[0].last_frame_url`（`render.direct`），与原生阻塞体同形；不带尾帧的观察渲染体不多一项（`render.no-last-frame`）；尾帧排在
视频前面判协议错误（`render.frame-before-video`）。
