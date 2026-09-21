# MiniMax 产物取回能力声明修复

基线为本地候选 e1295af；v0.30.0 尚未发布。上一轮验证记录保留为历史证据。

## 缺口与最小修复

MiniMax 的 `build_artifact_request` 已对成功观察中的 FileId 返回 GET，
但包 manifest 仅声明 submit/observe/render。宿主按 capability 授权时会拒绝该请求。
此前候选探针只验证 descriptor 授权，未覆盖包级能力门，不能证明完整接线。

先在真实 Wasm 测试验证实际返回 Some、原生/Wasm身份及请求一致、descriptor授权成功，
再断言 manifest 声明 artifact_fetch，取得真实失败后仅补该能力。
ComponentMetadataV1 只含 name/version/api_version，不增公共字段或新的权限通道。
未发布候选维持0.30.0；manifest摘要改变，旧候选pin不能替换成新内容。

## 验证

本次日志使用独立前缀 `/tmp/target-architecture-minimax-capability-`，不覆盖历史记录。
真实红绿、README全部16项、六包身份与最终摘要完成后追加。本记录不证明宿主生产采用。

## 本次实测结果

- `red.log`：真实Wasm已返回Some且通过descriptor授权，最终因缺artifact_fetch失败，
  1失败/4过滤，0.19秒。随后仅在MiniMax manifest增加该能力。
- `guest-build.log` 与 `green.log`：重新执行真实guest构建；MiniMax Wasm 5/5通过，
  0.10秒，包含20个冻结样例对拍以及新增能力一致性回归。
- `verification-summary.log`：README全部16项命令exit0；严格clippy、fmt、文档、
  两档测试、fuzz编译、边界/依赖/安全检查均执行。no-default 623通过。
- `verify-nextest.log`：643/643通过，0跳过，4.058秒；其中既有Gemini身份测试
  被nextest标记1次LEAK，不能称为无异常全绿。
- `gemini-recheck.log`：对应Gemini目标3/3通过，无LEAK，0.337秒。
- `nextest-recheck.log`：在根代理要求停止后续全矩阵之前已启动的全workspace
  nextest复查643/643，0跳过，2.452秒；LEAK改出现在既有Anthropic包身份测试。
  本轮未修改这两个测试，未定位或宣称修复输出管道泄漏，提交给根代理裁定。
- `identities.json`：六包manifest/Cargo/reference身份及runtime逐一一致；完整矩阵
  包含六包真实加载/对拍。本次不修改reference metadata，因为其仅有三项身份字段。
- 辅助摘要脚本最初被环境Python缺tomllib阻止，切换明确Python3.12后又识别出
  provider的world使用常量，修正核查脚本使其按常量核对后成功；均非产品测试失败。

493文件源码验证前后SHA256相同：
`7a079084c45acd4893815f2922f7b852596cc06705b686d8f3b5417e93e00a94`。
范围与历史验证记录一致，见`source-before.log`及`source-after.log`。
测试后仅补本中文记录，旧e1295af记录与日志均保留；未提交、推送、打tag或发布。

MiniMax新manifest SHA256：
`424aaa167ed1a477ff16327bf965fab517b0df7283cff08ecc35db1cd7930558`。
Wasm仍为387677字节，SHA256：
`52759f8ec4714212b2131cf0559690b0beb06c713dd6936fb390556f755ea5e2`。
实际路径为`components/task-minimax-v2/target/wasm32-wasip2/release/task_minimax_v2.wasm`。
旧候选归档未被覆盖；后续打包必须使用本次manifest，不能复用旧候选包pin。
