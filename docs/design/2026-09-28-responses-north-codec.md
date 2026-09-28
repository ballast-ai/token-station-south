# Responses 共享北向 codec

本变更承接 server P15 A1 双宿主差异表（社区基线 `8232a57e80d667a4c977c88992440fe648ef030e`），三方向直接使用既有 kernel IR，不经过 Chat wire，不改变 IR 或组件兼容元组。

公共接口为 `chat_request_from_responses(body, options)`、`responses_response(ir, context)` 和 `responses_frames(events, state)`。上下文显式提供 response_id、model、created_at、入站工具定义与 RawContent/Summary 思考呈现方式。请求兼容选项分别控制既有 messages 入口、空 input、调用 ID/参数别名、未知内容保留及纯文本数组形状，不能代表某个完整宿主。codec 不拥有准入、计费、历史缓存、路由、时间、随机数或网络。

首次空 batch 是显式启动调用：仅 Active 且尚未 created 时发一个 created，后续空 batch、等待错误或已终态不输出。此启动入口是 typed 批接口语义；单事件 JSON façade 没有空 batch 输入，不伪造 IR 事件，也不把该独立测试宣称为 façade 对拍。

每流状态固定上下文，维护统一 output_index、单调 sequence_number、分类型增量槽、Usage::absorb、待完成原因和终态。Finish 不收尾，Done/明确 Error 仅收尾一次；映射失败转入等待 Error 状态，忽略后续正文/Done，只允许宿主明确补发一次失败终态，并回滚本次未返回帧的 sequence/created；不完整原因决定 incomplete 事件。正文补齐 added/delta/done 生命周期，raw 思考在后继正文/工具前收口。工具恢复包含 namespace/custom/tool_search/local_shell。JSON façade 复用 typed 映射；流状态只持 typed 值，不在帧间 JSON 往返。

A1 的宿主边界：I03/I04/I05/I06/I12/I13/I17/I19/O02/O03/O04/E04/E05/E09 的产品选择留宿主；I07–I10/I21 的工具支持不放宽 server 准入。I13/O04 只保留既有 opaque 数据，不能将 Anthropic signature 等同于 OpenAI 密文；R 轨来源与跨家策略未完成。

测试以 A1 编号覆盖输入、响应与流事件；typed/JSON 使用同一 fixture。未知项和非法形状报告字段路径、不输出原始内容。属性样本固定 32，由宿主 nextest 的 14 秒截止执行。先观察行为测试失败，再实现映射；只完成本 crate 定向验证不代表整个工作区发行验证完成。


## 接口补充与边界

`ResponsesContext.allow_incomplete_tool_calls` 单独控制首片调用身份不完整的历史兼容；默认使用处明确 false，server 兼容场景可 true。`render_legacy_encrypted_reasoning` 单独控制既有签名呈现，server false、社区既有行为可 true；Summary 不隐含打开该开关。

`ResponsesSseState::terminal_response()` 仅在成功渲染 Done 后返回 canonical 快照（包含 incomplete），错误或未完成时返回 None。快照按原 IR index 分组，思考、正文、工具保持社区 continuation 的既有顺序；缓存、作用域、过期、重复请求 tombstone 仍由宿主管理。JSON façade 不序列化此状态。

社区未建模顶层扩展的保留及 `token_station_private_*`／continuation scope 过滤留社区外壳，不能由客户端字段覆盖宿主可信元数据。provider-hosted / 未知协议能力用 `CodecError::UnknownValue` 表示；非法形状使用 `Unrenderable`（既有公共错误类型），路径与固定诊断不回显请求内容。

`ResponsesRequestOptions` 的五个字段均默认 false；社区显式启用 `allow_empty_input` 与 `preserve_text_parts`，server 显式启用另外三项。文本数组形状保留与空历史准入相互独立，不能借其中一项推导另一项。legacy function_call_output 的非字符串结果完整 JSON 串化，缺字段为空串；默认路径保留社区 Content 解析。tool_search 流 item 固定带 `fc_<call_id>`，使参数帧引用始终指向已宣布的 item。

失败帧错误码按既有社区 Responses wire 映射枚举（如 RateLimit→rate_limit_exceeded），不直接泄漏 IR 枚举拼写。server Native 上游错误帧保留仍在宿主壳，不经此映射。

逐行覆盖、行为 RED 记录和验证命令见[验收证据](2026-09-28-responses-north-codec-validation.md)。
