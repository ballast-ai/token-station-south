# Kling 任务查询认证与提交上下文修正

日期：2026-09-20。基线：`196a737`（v0.28.1）。状态：实施中，尚未发布。

## 已确认的缺陷

`build_submit_request` 把 `ProviderConfig.auth` 转成 Bearer 凭证引用，查询构造器却丢失该引用。宿主按既有 `ProviderConfig::authorize` 检查时拒绝查询。修复由组件把同一配置中的凭证引用写入 GET 描述；组件不读取密钥，宿主仍负责按任务绑定解析凭证。未配置认证时仍保持 `None`。

提交时，普通模型 `kling-v3` 搭配 `image` 或 `image_tail` 选择 `image2video`；查询时却用模型名称是否包含 `i2v` 猜测路径。原注释“两个路径都接受任务 ID”没有独立证据，不能作为兼容承诺。

## 契约边界

当前 `SubmitOutcomeV1::Accepted(String)` 与 WIT `accepted(string)` 是原始上游任务 ID，不是组件私有句柄。当前查询参数仅配置、模型名、上游 ID，没有独立提交上下文或 opaque state。不得把路由编码进 ID、修改真实模型名或依靠可变 provider 配置猜测历史提交路径。

查询路由修复需要宿主和组件明确传递提交上下文。完整请求体不能成为持久恢复上下文：其中可能含 prompt、图片地址等不应持久化的原文。应评估新版本 prepare 输出受限、非秘密的定位信息，保持原始上游 ID 不变；不在本次独立认证修复中修改 WIT。

## 迁移前还需关闭的差异

对照宿主 `video/kling.rs` 与 `video/durable.rs`，当前组件不能覆盖整个 Kling 族：

- 缺少 Omni 创建和查询路径，以及 `image_list`、`element_list`、`video_list`、视频与 sound 的冲突校验、base-edit 不带 duration/aspect_ratio 等行为。
- motion-control 只换了 URL，没有构造 `image_url`、`video_url`、`character_orientation` 等必需字段，也没有专用校验。
- 普通视频把 `image_tail` 折成 `image`，丢失首尾帧区别；缺少 sound、分镜、voice、camera、mask 参数及 duration 默认值。
- 宿主按模型行决定模式强制与禁用，关联价目和准入。这些宿主政策不能被“上游模型名包含某词”的组件猜测替代。
- 组件给所有创建路径加 `external_task_id`，宿主仅在有证据支持的 t2v/i2v 路径添加；不能把其他路径的幂等支持当作已证实。
- 当前成功渲染只保留 URL 和宿主 artifact 路径。宿主原响应还有 created、model、provider、原始 task_id、每个产物的 id/duration；现有 observation 不能无损重建全部字段。
- 当前成功观察只保留一种 meter，有 milliunits 时丢弃 duration；宿主未配置 per-unit 费率时仍需使用实际 duration。两项客观证据应能并存，选择费率仍留宿主。
- 宿主原生 Kling API 透传请求和响应，补齐与计费一致的 model_name、duration、mode 默认值；它不是通用视频任务接口的同形别名。

首纵切若只支持明确的 t2v/i2v 子集，必须在宿主覆盖表和切换条件中写清，其余入口继续走既有实现，不宣称全族完成。

## 验证要求

公共参考实现测试必须先复现认证配置下查询授权失败，再验证 GET 引用原凭证槽、无 body，未认证部署保持无认证。路由验收必须覆盖同一普通模型的有图、无图、尾帧图与 motion-control，并验证上游 ID 未被包装。

原生测试通过不等于 wasm 或生产宿主接线完成；真实组件 parity、完整仓库矩阵、宿主持久恢复分别记证据。不修改发布版本或兼容元组来冒充发布。

## 本次验证证据（2026-09-20）

认证公共回归先因 `ProviderConfig::authorize` 返回 `MissingCredential` 真红，日志 `/tmp/target-architecture-batch3-south-auth-red.log`（退出码 101）。最小修复后，参考实现 18 项和原生套件 5 项通过；增加认证 fixture 后，真实 wasm parity 3 项也通过，合计 26 项，日志 `/tmp/target-architecture-batch3-south-auth-wasm-final.log`。

README 全部验证命令已实际执行：fmt、全特性全 target Clippy、全特性 nextest、doctest、无默认特性测试、fuzz locked check、警告视为错误的 rustdoc、固定 1.96.0 全 target check、边界自测及实际边界检查、主包与 fuzz 的 deny/audit/machete，均退出码 0。全特性 nextest 为 **559/559，0 skipped**，运行阶段 13.075 秒，最慢单项 12.730 秒。逐项结果与日志位于 `/tmp/target-architecture-batch3-south-matrix/results.tsv` 及同目录。

本机默认 PATH 优先 Homebrew 的 rustc，虽版本同为 1.96.0，却没有其自己的 wasm 标准库。第一次 wasm 验证因此失败，不能计为通过；改用已安装 wasm 目标的 rustup 工具链，显式把 `~/.cargo/bin` 放到 PATH 首位后重跑通过。构建缓存复用原仓 root target；组件 wasm 使用隔离工作树内独立 target，不覆盖原仓发布包。本记录只证明未发布源码与本地构建，不证明 server 已升级消费或路由缺陷已修复。

另以第二批已有的 server 接缝测试二进制验证本地修订包，前后 SHA-256 均为 `69a6553a2151964f5cac33bab3ac972bd4782ff9cd26ae849ea6fa838f8976d5`，没有重编该二进制。旧发布包仍被漏认证保护用例拒绝；修订包通过正常接缝用例，旧的“查询必须拒绝”断言在修订包上预期失败，证明同一宿主的授权检查已经接受查询描述。日志为 `/tmp/target-architecture-batch3-existing-host-candidate.log`。这是一项带预期失败的跨包诊断，不冒充全绿测试或生产生命周期验收；候选包不发布、不覆盖原包，也没有变更其版本标识。
