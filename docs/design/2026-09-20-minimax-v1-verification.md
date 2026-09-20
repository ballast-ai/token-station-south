# MiniMax v1 / v0.30.0 候选验证记录

日期：2026-09-20。隔离分支 feature/target-architecture-minimax，基线 e18eeff。
本记录仅证明候选库、组件和本地构建；仅作本地候选提交，未推送、打 tag 或发布，不证明
server生产持久绑定/恢复/资金/交付闭环。正式v0.29.0仍为原五包。

## 最终源码与范围

新增MiniMax v1纯参考/同源guest、20个冻结样例、公开行为/property与Wasm测试；
受控FileId查询升HTTP9，现有ABI/WIT/kernel/IR与其他词汇不变。
0.30.0八crate与六包身份/tuple/lock同步，release只增加构建/打包两行，触发不变。

最终源码集合493文件SHA256：
`b439dae0902315ee30e19df4d5606196f51c458623ba09bf0790cad86276997d`。
范围为crates、components、fuzz、scripts、.github、Cargo.toml、compatibility.json、
rust-toolchain.toml；按路径排序，以“路径\0字节\0”拼接求摘要，不含target与文档。
验证前后摘要相同，见 `/tmp/target-architecture-minimax-source-{sha256,final-check}.log`。
末尾仅文档更正TaskContract当前4/引入v2时3、收口设计的历史/当前语气
并增加本记录，不改变已测源码。

## README最终16项

全部PASS，原始摘要 `/tmp/target-architecture-minimax-verification-summary.log`。
逐项日志 `/tmp/target-architecture-minimax-verify-<下表键>.log`。

| 键 | 检查及实际结果 |
|---|---|
| fmt | cargo fmt --all -- --check |
| clippy | workspace/all-targets/all-features，-D warnings |
| nextest | workspace/all-features：642/642，0 skipped，运行31.281秒 |
| doc-tests | workspace doc/all-features；8个目标均成功，实际0个doctest |
| no-default | workspace/no-default-features：623通过，0失败，0 ignored |
| fuzz-check | fuzz workspace all-targets --locked编译 |
| docs | RUSTDOCFLAGS=-Dwarnings，workspace/no-deps/all-features |
| msrv | rustup run 1.96.0 cargo check --workspace --all-targets |
| boundary-self | check-boundaries.sh --self-test |
| boundary | check-boundaries.sh |
| deny | cargo deny check |
| fuzz-deny | fuzz清单+deny.toml+--locked |
| audit | cargo audit |
| fuzz-audit | cargo audit --file fuzz/Cargo.lock |
| machete | workspace cargo machete |
| fuzz-machete | fuzz workspace cargo machete |

最终nextest包含MiniMax原生14项（含property）、Wasm4项、冻结suite1项、
六包守卫5项及FileId受控GET真实本地socket测试。没有执行长fuzz/soak；
本批fuzz入口已挂载并编译，未执行fuzz campaign。

## 红测与修复

日志前缀均为 `/tmp/target-architecture-minimax-`。

- `reference-red.log`：临时typed stub下5个真实行为失败，随后实现。
- `file-query-red.log`：FileId未获准入，随后最小增加具名数字参数。
- `regression-red.log`：错误HTTP分档与duration trim两项失败，按旧宿主修复。
- `reseller-red.log`：原始转售名被硬名单拒绝，改用宿主具名协议家族。
- `canonical-red.log`：query顺序不符合受控规范，改统一QueryString序列化。
- `http-red.log`：HTTP4xx未明确Rejected，补既有宿主parse前的HTTP语义；
  文件取回错误同样保留原HTTP状态并脱敏。
- `id-red.log`：带空白ID未trim；仅解析时trim一次，保留前导零，负数/空白未知。
- `package-red.log`：第六包未登记，沿既有release流程补两项，不增豁免。
- `version-red.log`：拒复用0.29身份守卫失败，统一准备0.30与六包新身份。
- `fixtures-green.log` 是早期失败日志：auth期望误写kind而实际IR为scheme，
  更正独立期望后native/final suite通过。该文件名不能当作成功证据。
- clippy先发现文档反引号、分号、条件写法、测试字面量/参数约定问题，
  修复后完整严格clippy通过；fuzz fmt的无关safe_headers改动已还原。
- `verify-nextest-first-failed.log`：第一轮134/642运行，133通过、1失败、
  508未跑。HTTP声明9而常量仍8，机械替换误用u32导致漏改。
  精确断言1处后修成u16=9；`http9-final-green.log` 兼容1+HTTP70+FileId1绿；
  然后README16项从头重跑全绿。第一轮80秒左右项目含新树guest冷编译，
  不是睡眠/事故窗口；最终运行31.281秒。

## 本地产物

六个包都经过真实Wasm加载/对拍，并独立打包检查两普通成员
manifest.json/component.wasm与源字节一致。没有执行release。
目录 `/tmp/target-architecture-minimax-packages/`，归档名
`<组件>-0.30.0-candidate.tar.gz`；完整manifest/wasm/archive摘要在
`package-sha256.json`，检查日志 `/tmp/target-architecture-minimax-package-check.log`。

| 组件/版本 | Wasm字节数 | Wasm SHA256 |
|---|---:|---|
| provider-openai-compatible / 2.1.2 | 365237 | `e6e6ec2cc17dcb2e321c536e0de8d8872f5ab4894b8d05c62f2fe269ddc6d5e2` |
| provider-anthropic / 1.0.3 | 369553 | `55efea5fe1c0c0b428a978196687ea81c684a46a1ab6f1942f50b5ed74cd4c90` |
| provider-gemini / 1.1.2 | 372310 | `3757dcdc3740783d96da349c8ac6857a7f9b025569b9012b5828117e6344b715` |
| task-kling / 1.0.2 | 318425 | `c84147a9a2c1711927a97ed00f73b864e663689f598ca25dff7a4a3c75e8f127` |
| task-kling-v2 / 0.30.0 | 387197 | `7e9ae1106ccd5438c7bfe9bd939e0c928fa09087d0f665ff0b4277a88b1ab702` |
| task-minimax-v2 / 0.30.0 | 387677 | `52759f8ec4714212b2131cf0559690b0beb06c713dd6936fb390556f755ea5e2` |

新guest实际路径：
`components/task-minimax-v2/target/wasm32-wasip2/release/task_minimax_v2.wasm`。
新manifest SHA256：`2b80ed19d047d1cf864f66819248185c4a2381a0ab5d361117ac1d835f8c6974`。
现有五guest/fuzz锁逐块核查只有内部version变化，无第三方依赖漂移；
新MiniMax锁除自身包身份外与Kling-v2锁一致，见
`/tmp/target-architecture-minimax-lock-audit.log`。

独立规格/代码与版本审查均未发现Critical/Important；宿主真实接缝探针及
生产采用仍需后续完成，compatibility中的task-v1/v2状态保持not_verified。
