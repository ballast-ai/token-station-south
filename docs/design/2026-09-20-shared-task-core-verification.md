# M3 South 共享核心候选验证记录

本次基线为已发布组件源码 `8077743c213602e375c0ee17d08c051abb0bcd2c`。
新增 `south-task-core` / `south-task-conformance` 独立 Rust 库版本均为 0.1.0；
八个既有库、七个 Wasm 包、Task5/HTTP9 及组件 runtime 保持 v0.31.0。
本记录不宣称部署，也不以库测试代替两个宿主的真实存储与采用验收。

## 核心行为的红绿证据

- 公开 21 条行为测试先以未实现行为全部失败，再全部通过：
  `/tmp/target-architecture-m3-core-red.log`、`/tmp/target-architecture-m3-core-green.log`。
- 增补取消发生于查询/等待中、非 Sync 宿主的 Send future、独立版本身份检查。
- 等待入口预检额外真实红测：已取消/到期时宿主 inspect 方法仍被调用一次。
  `/tmp/target-architecture-m3-core-preflight-red.log` 为实际 1/预期 0；
  延迟构造宿主 future 后 `/tmp/target-architecture-m3-core-preflight-green.log` 25/25。
- 首次 clippy 仅发现四处测试 unit pattern 写法，已按提示修正；生产策略没有改变。
- 公共原子效果套件由独立实施者完成并修复两项审查缺口，最终 6/6，包含错误
  实现拒绝；本轮全 workspace 验证覆盖最终文件，不拼接旧通过记录。

## 完整矩阵与 LEAK 调查

首轮完整 16 项通过（705/705），但 nextest 0.9.140 标记两个既有百炼测试
LEAK；单独复跑两项均通过。修复公共套件后第二轮完整 16 项通过（707/707），
却在另两个纯原生测试出现 LEAK：`the_shipped_pack_still_carries_every_decided_behaviour`
及 `all_nonnegative_finite_reported_seconds_preserve_their_value`。首两轮日志保留，
不以定向复跑覆盖完整结果。

nextest 的 LEAK 指进程退出后 stdout/stderr 管道仍未关闭，不等于业务或凭证泄漏。
机器上的 0.9.140 默认等待 200ms。官方 [PR #3553](https://github.com/nextest-rs/nextest/pull/3553)
修复 Apple 并行 spawn 时 FD_CLOEXEC 设置非原子、兄弟进程继承 capture pipe 的竞争；
该修复随 [0.9.145](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.145)
于 2026-09-16 发布。本次症状与该问题相符，完整修后对照未再出现 LEAK。

仅在 `/tmp/target-architecture-nextest-0.9.145/bin` 安装官方工具，不替换全局。
官方 SHA 文件核验通过：archive SHA256
`52ecaedb4f5af9267ef7ed02bc937d2a15a94ff96cb663080e81311f798c9905`；
binary SHA256 `84882f70f095269a4d89ac2255c4fe7490545d0adf42d8556905401ed4236b33`。
版本为 0.9.145（00af4550e）。临时 tool config 仍为 200ms，并把 LEAK 改为失败；
没有增加超时或弱化判据。下载、官方修复及版本证据均在同一临时目录。

第三轮使用该局部工具完整执行 README 16 项，**16/16 PASS**：
全功能 **707/707，0 LEAK、0 skipped**；无默认功能 **683/683，0 ignored**。
全功能实际测试运行 2.559 秒。没有长 fuzz/soak，没有全局 CARGO_TARGET_DIR；
Rust 仍由 `/Users/actly/.cargo/bin` 的 rustup shim 提供。

| # | 实际命令 | 结果 | 日志 |
| --- | --- | --- | --- |
| 1 | `cargo fmt --all -- --check` | PASS | `/tmp/target-architecture-m3-south-final-v3-01.log` |
| 2 | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS | `/tmp/target-architecture-m3-south-final-v3-02.log` |
| 3 | `cargo nextest run --workspace --all-features --tool-config-file audit:/tmp/target-architecture-nextest-0.9.145/leak-fail.toml` | PASS | `/tmp/target-architecture-m3-south-final-v3-03.log` |
| 4 | `cargo test --workspace --doc --all-features` | PASS | `/tmp/target-architecture-m3-south-final-v3-04.log` |
| 5 | `cargo test --workspace --no-default-features` | PASS | `/tmp/target-architecture-m3-south-final-v3-05.log` |
| 6 | `cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked` | PASS | `/tmp/target-architecture-m3-south-final-v3-06.log` |
| 7 | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` | PASS | `/tmp/target-architecture-m3-south-final-v3-07.log` |
| 8 | `rustup run 1.96.0 cargo check --workspace --all-targets` | PASS | `/tmp/target-architecture-m3-south-final-v3-08.log` |
| 9 | `scripts/check-boundaries.sh --self-test` | PASS | `/tmp/target-architecture-m3-south-final-v3-09.log` |
| 10 | `scripts/check-boundaries.sh` | PASS | `/tmp/target-architecture-m3-south-final-v3-10.log` |
| 11 | `cargo deny check` | PASS | `/tmp/target-architecture-m3-south-final-v3-11.log` |
| 12 | `cargo deny --manifest-path fuzz/Cargo.toml --config deny.toml --locked check` | PASS | `/tmp/target-architecture-m3-south-final-v3-12.log` |
| 13 | `cargo audit` | PASS | `/tmp/target-architecture-m3-south-final-v3-13.log` |
| 14 | `cargo audit --file fuzz/Cargo.lock` | PASS | `/tmp/target-architecture-m3-south-final-v3-14.log` |
| 15 | `cargo machete` | PASS | `/tmp/target-architecture-m3-south-final-v3-15.log` |
| 16 | `cd fuzz && cargo machete` | PASS | `/tmp/target-architecture-m3-south-final-v3-16.log` |

## 源码冻结

第三轮检查前后逐文件 SHA256 清单完全相同（只排除 Markdown 文档）：
`/tmp/target-architecture-m3-south-source-v3-before.json`、`-after.json`；
清单文件 SHA256 均为 `8b0510554ba2883ef007a74cd0cc5dd8d00a2bc2c861b3844a8221159e30e827`。
结果索引 `/tmp/target-architecture-m3-south-final-v3-results.json`；
汇总 `/tmp/target-architecture-m3-south-final-v3-summary.log` 明确 `source_unchanged: true`。
最终只新增本验证文档；未由本子任务提交、推送、打 tag 或发布。
