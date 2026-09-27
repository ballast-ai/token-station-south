# 百炼万相 2.7 图像（异步）冻结样例

期望按 token-station-server 原生万相图像臂（`handler/images/bailian.rs` 的 `build_bailian_image_body` / `validate_bailian_image_n` /
`openai_size_to_wan_size` / `extract_bailian_image_urls` / `trusted_image_count` / `bailian_image_result_to_openai`，
`images/durable.rs::parse_wan_create`，`images/observe.rs::normalize_bailian_wan`，阻塞面的 `code: detail` 失败文案）手写转录，
**不从被测实现重新生成 expected**。不是线上抓包。

合同 6 在本族的落点：`n` → `requested_outputs`；`usage.outputs` 是**结算张数**（上游正数 `usage.image_count`、不超过交付 + 1，否则交付数），
与产物 URL 数刻意分开——上游报数比抽到的 URL 多一张时按报数结算，这是宿主既有的「信上游计数」契约。SUCCEEDED 零产物判终态失败。
