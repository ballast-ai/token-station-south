# Responses codec · P15 S4 验收证据

本记录对应 server P15 A1 的 37 行，源码基线为 South `01b1a7b` / 社区 `8232a57e80d667a4c977c88992440fe648ef030e`。候选代码未发版，kernel/schema/compatibility 未修改。以下证据只证明共享 codec，不代替 server 真实发送、计费准入、社区缓存与 Wasm/proxy 验收。

## 行为测试定位

测试文件统一为 `crates/south-north-codec/tests/responses.rs`。表内测试名以便于检索的唯一前缀表示；每行都明确实际断言和宿主责任。`checked_request`、`checked_response`、`checked_frames` 对事件/请求/响应 fixture 同时调用 typed 与 JSON façade，对成功结果、typed 错误、SSE 字节、终态状态及 continuation 快照做相等断言；流 JSON 使用常驻 typed state，不做每帧状态序列化。

| A1 | 可复现测试前缀 | 断言范围及剩余宿主边界 |
|---|---|---|
| I01 | `string_history_empty_input`、`empty_input_is_a_separate_host_allowance` | 字符串 input、空字符串内容、空列表默认拒绝/独立开关允许；无隐式历史查询 |
| I02 | `request_preserves_semantic_options`、`explicit_message_reports_missing_role` | developer→System；明确 message 缺 role 错误定位到 role；非法 role 分类 |
| I03 | `request_compatibility`、`legacy_messages_precedence` | 显式 allow_messages 允许既有入口并优先 input；是否向某宿主开放由宿主决定 |
| I04 | `image_inputs_and_invalid_roles`、`legacy_messages_precedence`、`text_parts_shape_is_an_independent`、`malformed_text_parts_preserve_only_legacy` | URL/detail；file_id 严格模式拒绝；未知视频块仅显式兼容保留；server file_id 嵌套形状与社区纯文本 Parts 形状 |
| I05 | `request_compatibility`、`replays_all_client_tool_items` | call_id/id 兼容、对象参数 JSON 化与 namespaced 调用 |
| I06 | `request_compatibility`、`legacy_function_results_stringify_every_json_kind` | tool_call_id 别名、全部 JSON 非字符串结果串化、缺字段空串；标准结果路径与社区 fixture 共用 |
| I07 | `community_tool_families`、`replays_all_client_tool_items` | custom 定义、调用回放、结果、响应 input 恢复；server 准入不在 codec |
| I08 | `community_tool_families`、`replays_all_client_tool_items` | tool_search 定义、ID 回退、调用/结果与响应恢复；不执行搜索 |
| I09 | `community_tool_families`、`replays_all_client_tool_items`、`malformed_local_shell` | shell action/结果和固定诊断；不执行命令 |
| I10 | `namespace_descriptions_and_collisions` | 描述合并、strict、扁平化冲突拒绝；server namespace 准入仍拒 |
| I11 | `request_preserves_semantic_options`、`namespace_descriptions_and_collisions` | function strict 扩展、参数、重复名检测 |
| I12 | `reasoning_and_response_context`、`explicit_message_reports_missing_role` | raw content 与 summary 路径及非法 content；重放开关仍由宿主执行 |
| I13 | `reasoning_and_response_context` | encrypted_content 和 ID 入站扩展保存；不是 R3 往返/跨方言证明 |
| I14 | `request_preserves_semantic_options`、`malformed_fields_report_paths` | effort 字符串保留，非字符串字段拒绝；上游支持性属组件/宿主 |
| I15 | `sampling_formats_and_selection`、`request_preserves_semantic_options`、`malformed_fields_report_paths` | text/json_object/json_schema 与非法 schema |
| I16 | `sampling_formats_and_selection`、`request_preserves_semantic_options`、`invalid_roles_and_unsupported_selection` | required/none/命名 function、parallel false；无可执行工具的 required 拒绝；未知选择字符串 Capability/对象 InvalidRequest |
| I17 | `sampling_formats_and_selection`、`malformed_fields_report_paths` | canonical cap 优先、兼容别名、u32 范围、采样/stop/stream；成本限额仍由授权层控制 |
| I18 | `string_history_empty_input` | instructions 前置 System、transient 标记、非法类型；跨轮排除由社区缓存壳负责 |
| I19 | `string_history_empty_input`、`continuation_snapshot` | 仅当前输入，无暗取历史；成功终态才给 canonical 快照；cache/TTL/scope 属宿主 |
| I20 | `unsupported_capabilities`、`malformed_fields_report_paths`、`untrusted_input_never_panics` | 类型/路径/无原文泄漏；32 样本递归 JSON 分别进入 body/input/tools/content 路径 |
| I21 | `community_tool_families`、`unsupported_capabilities`、`known_hosted_tool_names_remain_diagnostic` | disabled web_search 保留扩展，其余托管工具 Capability；已知标准名称保留用于宿主拒绝回执，任意未知值脱敏；server 不开放该例外 |
| O01 | `output_items_keep_all_choices`、`community_tool_families` | 全 choices、工具族恢复、参数字节和 output_text 聚合 |
| O02 | `reasoning_and_response_context`、`output_items_keep_all_choices` | response ID/model/time 来自 context，item ID 确定性；无 clock/UUID |
| O03 | `reasoning_and_response_context` | RawContent 与 Summary 两呈现；选择权为宿主 |
| O04 | `missing_identity_and_signature_compatibility` | legacy 签名呈现独立开关，关闭后不重标密文；不关闭 #42 |
| O05 | `output_items_keep_all_choices`、`overflowing_usage` | 基本桶、cache/read/write/reasoning、总数不重复加细桶、溢出拒绝 |
| O06 | `reasoning_and_response_context`、`stream_lifecycle_indices_usage` | ContentFilter/Length 的 incomplete/details；未知 finish 正常 completed |
| O07 | `output_items_keep_all_choices` | 不伪造 tools/tool_choice/parallel/instructions 已执行策略 |
| E01 | `stream_lifecycle_indices_usage`、`reasoning_block_order_and_summary_lifecycle`、`empty_batch_explicitly_starts_once` | created/added/delta/done 生命周期与 summary 事件完整顺序；空 batch 显式启动独立 typed 测试，不假称单事件 JSON 对拍 |
| E02 | `stream_lifecycle_indices_usage` | 每个事件 sequence_number 从 0 递增 |
| E03 | `stream_lifecycle_indices_usage`、`reasoning_block_order_and_summary_lifecycle` | 文本/思考/多工具共享 output_index；交错参数、二段思考次序 |
| E04 | `missing_identity_and_signature_compatibility`、`finish_waits_for_done`、`tool_streams_restore`、`tool_search_parameter_events_reference` | strict/tolerant 身份、身份突变拒绝、四工具族流恢复，search 参数事件引用已宣布 item |
| E05 | `raw_reasoning_closes_before_tools`、`reasoning_block_order_and_summary_lifecycle` | raw 在正文/工具前收口；Summary 单独生命周期 |
| E06 | `finish_waits_for_done`、`stream_lifecycle_indices_usage` | Finish 不结束；后段 usage 保留前段桶；Done 才终态 |
| E07 | `stream_lifecycle_indices_usage` | Length 对应 response.incomplete 与一致 status/details |
| E08 | `errors_are_terminal`、`render_error_allows_one_explicit_failed_terminal`、`failed_frames_use_client_error_codes`、`finish_waits_for_done` | 明确错误后 Done、Done 后错误、重复 Done 均不再输出终态；映射错误后只允许一次 Error，created/序号无缺口；13 种 IR 错误码映射 |
| E09 | `stream_batching_and_isolation`、`continuation_snapshot` | 32 样本分批等价、两流交错、终态快照；stream_id/context 校验和有界 tombstone 留宿主 |

## RED 证据

- 首版 9 组公开行为测试在可编译骨架上全失败，原因是 `not implemented`／字段不符；run ID `89ef0e3e-e871-4a3d-9e83-0898769cf997`。
- namespace 描述丢失断言先失败，补继承描述后通过；run ID `385db133-f728-496a-971d-4b69cb66c9a8`。
- continuation snapshot 缺失、Usage 总数溢出 panic 两项先失败，再实现成功快照和 checked_add；run ID `b4d04384-8cc5-473b-b91c-0e655138c2b6`。
- 明确 message 缺 role 错误路径先失败；run ID `b184f1c1-d34e-4a3c-8a36-c10600a79498`。
- 不支持能力与非法形状的 typed 类别先失败；run ID `31012d46-e8af-4c7e-976c-420a7cbe2845`。
- 社区 local_shell 错误措辞兼容断言先失败，保留字段路径并补固定 `invalid arguments` 短语；run ID `418e5098-3a5f-43fd-811a-f3f198a4072c`。

- 空 input 社区 allowance 先失败：`637145ac-fc8d-49cb-9f18-e5398ce04e50`；legacy file_id 形状先失败：`bf900505-e07f-4447-8adf-236d648fc7c5`。
- 文本数组形状、全部 JSON 结果串化、search item ID 三项先失败：`c3b7a65c-e106-4900-b9ae-227fb8133da1`；非法 role 分类先失败：`ddfa678c-ebf2-49ab-9234-6a184eebd16e`。
- 映射错误吞掉宿主显式 Error 先失败：`c1df75a0-7746-4b2e-828f-29f29bd414f1`；标准错误码呈现先失败：`2002cbda-3a24-42f7-8745-f7454dbe9c81`。
- fuzz 正向依赖 fixture 在旧边界脚本先拒绝；获准后只允许当前精确 `fuzz/Cargo.toml`、`south-contracts-fuzz`、`publish=false`、非空纯 bin 目标。另三项生产路径/可发布/库目标反例均拒绝，不扩大生产消费者名单。

- 托管工具名称过度脱敏导致真实 HTTP 拒绝回执失真，白名单标准名称测试先失败：`a470dcd8-6dd5-4d4b-9608-702e7fe132bd`，修复后单用例通过（`701d71f7-d755-4e33-889b-0c981889d050`）。进一步发现 expected 提示中的无关工具名会污染宿主回执分类；无无关名称断言先失败 `6e9435bb-96cc-4edb-b1c2-6b8497167395`，改为通用固定描述后通过 `1c7b7c59-78a9-46ea-98f2-7d20c59de1cf`。

- 角色首片解码为空 batch 后 created 丢失，独立 typed 启动测试先失败 `68798efe-4b82-42a5-b4e9-3f1f514c6a1d`；malformed input_text 既有 Unknown 兼容先失败 `3fbf2c54-cb40-4948-9c71-2d3af81a4bae`。修复后 37 个 Responses 用例全绿（0.057 秒）。

## 验证环境与限制

`CARGO_TARGET_DIR=/tmp/p15-south-codec-target`；`PROPTEST_CASES=32`。nextest 使用 `/tmp/p15-codec-nextest.toml`，来自 server 配置的 profiles 原文（14 秒 terminate/fail、retries=0、2 秒 leak fail），只移除引用 server 二进制的 override；原文件直接用于 South 会因不存在的 binary 名报配置错，该错误不计 RED。

发行元数据调整前的完整验证为 34 个 Responses 用例，加既有 35 个用例，共 69 个。全 features 69/69（0.071 秒），无默认 features 69/69（0.091 秒）；最长用例 0.055 秒。两档 `clippy --all-targets -- -D warnings`、`cargo fmt --all -- --check`、doctest（当前无 doctest）、`RUSTDOCFLAGS='-D warnings' cargo doc --no-deps`、fuzz 锁定离线编译、边界 self-test/真实工作区检查、`git diff --check` 均通过。

复现命令（仓库根，先按上文准备 nextest 配置）：

```sh
CARGO_TARGET_DIR=/tmp/p15-south-codec-target PROPTEST_CASES=32 cargo nextest run -p south-north-codec --all-features --config-file /tmp/p15-codec-nextest.toml --profile default
CARGO_TARGET_DIR=/tmp/p15-south-codec-target PROPTEST_CASES=32 cargo nextest run -p south-north-codec --no-default-features --config-file /tmp/p15-codec-nextest.toml --profile default
CARGO_TARGET_DIR=/tmp/p15-south-codec-target cargo clippy -p south-north-codec --all-targets --all-features -- -D warnings
CARGO_TARGET_DIR=/tmp/p15-south-codec-target cargo clippy -p south-north-codec --all-targets --no-default-features -- -D warnings
cargo fmt --all -- --check
CARGO_TARGET_DIR=/tmp/p15-south-codec-target cargo test -p south-north-codec --doc
CARGO_TARGET_DIR=/tmp/p15-south-codec-target RUSTDOCFLAGS='-D warnings' cargo doc -p south-north-codec --no-deps
CARGO_TARGET_DIR=/tmp/p15-south-fuzz-target cargo check --manifest-path fuzz/Cargo.toml --bin contract_parsers --locked --offline
bash scripts/check-boundaries.sh --self-test
bash scripts/check-boundaries.sh
git diff --check
```

补充托管工具白名单、启动与 malformed 内容兼容后 Responses 为 37 个用例，最终发布门结果另见 v0.36.0 发行记录。

未进行 fuzz 长跑，不把已挂入既有 scheduled `contract_parsers` 目标等同于 fuzz 覆盖完成；未运行完整 South 工作区发布矩阵，不据此宣称可以发布。
