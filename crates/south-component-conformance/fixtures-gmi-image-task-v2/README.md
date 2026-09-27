# GMI Cloud media 图像冻结样例

期望按 token-station-server 原生 GMI 臂（`handler/images/gmi.rs` 的 `build_gmi_submit_body` / `gmi_reference_images` /
`gmi_reference_image_limit` / `gmi_media_urls` / `gmi_media_result_to_openai`，`images/durable.rs::gmi_run` 的 `b64_json` 拒绝与
`parse_gmi_create` 双形回执，`images/observe.rs::normalize_gmi`，阻塞面 `code: detail` 失败文案）手写转录，**不从被测实现重新生成 expected**。

组织头：原生从凭证 `account_id` 取 `X-Organization-ID`；组件看不到凭证行，由宿主以 provider config 扩展 `organization_id` 传入。
GMI 按请求计价、不报用量：`n` 仍是 `requested_outputs`，交付 URL 数即 `usage.outputs`，计价政策归宿主。回执带产物 → `accepted-terminal`。
