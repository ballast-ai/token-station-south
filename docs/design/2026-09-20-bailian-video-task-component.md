# 百炼 managed 视频 Task V2 组件

基线：South fb0c4fd / 已发布 v0.30.0；server a4ea39cb。本文新增第七包
`task-bailian-v2`，当前进入 v0.31.0 七包发布准备，未发布，不覆盖正式六包身份。
仅迁移 managed 视频；百炼图片与原生透传入口不在范围内。

## 合同与边界

复用 TaskContract 5 / HTTP 9 / Task V2 WIT。公开入口为
`south_component_conformance::reference_bailian_task_v2::BailianTaskComponentV2`。
组件只翻译，不持有凭证、时钟、费卡或任务数据库。宿主通过
`upstream_model_family` 扩展提供协议形态，wire model 保留原字节。
提交声明一次 `X-DashScope-Async: enable` 和 JSON Content-Type，Bearer 只携
SecretRef；查询 GET 不带异步头，固定 locator `api/v1/tasks`，ID 编码为单一路径段。
点段、空值、控制符和超限 ID 拒绝，不改写上游 ID。

输入从 server video/bailian.rs 纯函数转录：Wan 普通 img_url、HappyHorse
首帧 media、HappyHorse 参考图 1–9 张和 ratio、kf2v 首尾帧与固定 5 秒。
保留既有 prompt 必填和尾帧可选行为，不借迁移擅自收紧。可用字段与 null
行为以冻结样例为准；图片 URL/data URI 不存入 locator。
估算时长缺省 5 秒，协议单位率 None。分辨率按宿主原有 1080P/480P
识别规则规范化（忽略空白/大小写、数字维度含前导零）；其他可表达值保留规范
字符串，不能表达的 hint 为 None，不猜费率。输入图片计数是事实，不是费用。

## 观察与错误

PENDING/RUNNING 分别保留排队/运行，成功必须有非空 video_url。
usage.duration 有效时优先，其次 video_duration；零是有效事实、null/非法值
可回退。绝不读取 output_video_duration 推测收费；实际 seconds 与请求估算分开。
FAILED 保留受限 code/message，CANCELED 为取消事实。官方 UNKNOWN 包括查询
超过 24 小时的情况，新组件映射 Unknown，宿主保留预占；取消无可靠费用亦 held。
该项是明确修正旧 UNKNOWN→Failed 政策，不冒称旧退款行为等价。
明确 HTTP 4xx 拒绝保持状态；5xx/坏 JSON/矛盾受理为 Unknown；不将未知变免费。
成功直接产生 URL，artifact_request=None，manifest 不虚报 artifact_fetch。
render 的 provider 为宿主业务配置名，created 为宿主渲染时钟，task_id 为上游 ID。
内部 Err 诊断固定且不回显正文；Ok 内失败 code/message 是有界上游业务事实，
不宣称已脱敏，宿主负责签名 URL、凭证等最终公开输出脱敏。

## 来源与验证

源码转录不是线上抓包。样例分别标记仓内既有行为、官方文档转录和边界合成。
- server video/bailian.rs、video/observe.rs、video/durable.rs，task_blocking_durable.rs。
- https://www.alibabacloud.com/help/en/model-studio/legacy-image-to-video-by-first-and-last-frame-api-reference
- https://www.alibabacloud.com/help/en/model-studio/happyhorse-reference-to-video-api-reference

TDD：公开行为先红，再最小实现；同一冻结 pack 运行原生和真实 Wasm；包权限与
实际返回请求对照；原生/沙箱都经 ProviderConfig.authorize。最后运行 README
全部短验证，单例不超过 60 秒，不运行长 fuzz。费用、恢复、代理、转存的真实宿主
接线由 server 独立验收；库通过不等于 M2 完成。

独立复核补充：未知模型没有内置能力档时，零时长／超出宿主持久时长范围的请求
曾可产出估算，宿主因无法落合法绑定返回 500。公开组件测试和真实 HTTP 均先红；
请求准备阶段现返回 InvalidRequest/400。观察响应中的零计费秒数仍允许，不混淆
请求合法性与上游用量事实。最终验收须覆盖此修复后源码。
