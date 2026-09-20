# MiniMax H3 与请求估算事实（未发布0.30候选）

基线 ae06a22。0.30尚未发布；此前能力修复验证记录保留。
本次按已批准M1范围补齐Hailuo/H3，不能以库通过宣称宿主采用完成。

## 合同与职责

TaskRequestEstimateV2增加resolution与input_image_count；None不等于0。
保留new(seconds,rate)，新with_input_facts及两个getter；分辨率为非空、
最多32字节的ASCII字母数字规范串，组件负责协议归一，合同不替组件猜默认。
新计数字段为u32；MiniMax H3实际0..9，Hailuo0..2。JSON两个新key必须存在，
可为null，旧Task4 prepared JSON拒绝，TaskContract升级5；HTTP9和函数WIT保持。
Kling明确不报告这两事实。MiniMax请求秒数、规范分辨率和实际wire图数给宿主，
区域价卡、免费前5张、额外图片金额、货币换算均留宿主，不伪造协议单位率。

## 来源与兼容

输入/输出样例是server现有纯函数转录的合成冻结样例，不是上游抓包。
来源：video/minimax.rs build_minimax_v2_submit、minimax_v2_failure_error，
video/observe.rs normalize_minimax_v2与query路径，video/durable.rs提交/渲染，
以及gemini.rs中的共享parse_reference_image_urls。H3 shape由宿主受限快照提供，
wire保留转售原model；参考图只收字符串数组≤9，首尾模式与参考图互斥。
locator保存v2/query/video_generation，查询只使用原locator和原ID而非当前model。
路径ID逐字节编码，仍需宿主原authorizer授权，不绕开编码分隔符拒绝。
H3直链产物不额外fetch，v1仍FileId二段GET。H3缺失实际秒数保持None，
明确非法/负/nonfinite的报告保守Unknown；合法字符串可trim解析。
错误状态和取消事实保留，文案脱敏。宿主render传当前渲染时钟及公开provider=minimax。

## 验证计划

先新codec事实/旧格式拒绝和H3样例真实RED，再实现；原生、冻结suite、
真实Wasm正反对拍、两档与README16检查。所有日志独立h3前缀，保留失败证据。

## 修复前全量验证（保留历史）

README16项命令全部exit0，逐项见
`/tmp/target-architecture-minimax-h3-verification-summary.log`及
`/tmp/target-architecture-minimax-h3-verify-<检查名>.log`。包含fmt、严格clippy、
all-features nextest、doctest、no-default、fuzz编译、rustdoc、MSRV、边界自测/正测、
主/fuzz deny、主/fuzz audit、主/fuzz machete。没有长fuzz/soak或超过60秒事故窗口。

- nextest：652/652通过，0跳过，实际6.427秒；1条既有
  anthropic_sandbox_parity_v1::the_shipped_package_passes_gate_one_and_the_tuple_handshake
  标记LEAK（测试退出后输出管道仍打开）。本次未声称定位或修复，保留日志交根代理评估。
- no-default：632通过；doctest8目标均成功（实际0个doctest）。
- 定向：Hailuo14、H3原生4、estimate12、codec13、冻结suite1；
  MiniMax真Wasm5、Kling-v2真Wasm4。MiniMax冻结样例46个（20旧+26新增H3正反）。
- 真实RED：`h3-red.log`记录13个H3正向样例不符；`h3-estimate-red.log`记录
  新字段被拒/旧Task4被接受的两条行为失败。上述简称均带前缀
  `/tmp/target-architecture-minimax-`。
- 首轮`h3-native-first.log`中旧测试仍把H3当不支持，改为未知型号；
  `h3-native-green.log`虽名含green，实际冻结套件失败：新样例估算秒数误用JSON整数，
  codec的f64为浮点，独立期望更正为5.0/6.0后`h3-native-final.log`通过。
- `h3-clippy-first.log`因新增u64边界字面量缺分隔符失败；修正后
  `h3-clippy-green.log`与最终完整clippy均通过。

源码546文件验证前后摘要相同：
`c46f1fcc37cfbfb6dc31c56b70ff4bf26bead41084f3647fd76864394b4be4c4`。
范围沿历史记录crates/components/fuzz/scripts/.github与三个根配置，排除target和文档。
测试后仅补中文证据，不改源码；未提交、push、tag或发布。

六包真实本地归档在`/tmp/target-architecture-minimax-h3-packages/`，两成员字节
与源manifest/Wasm一致，完整六包摘要为该目录`package-sha256.json`。
六包manifest/Cargo/reference身份以及0.30runtime再次逐项核验；Task5由全局合同
与compatibility清单声明，manifest不擅自增加新字段；HTTP9/WIT函数保持。
MiniMax新Wasm为404333字节，SHA256：
`6b1a4295429f7ef47f8622ba918697c5e55e1e5d8ef9d1237c1df2fc5caa79a4`。
manifest SHA256：
`424aaa167ed1a477ff16327bf965fab517b0df7283cff08ecc35db1cd7930558`。

补充兼容界限：H3-Max转售别名按宿主提供的shape实施参考图限制，纠正旧builder
按wire model比较可能绕过该限制的情况。parse_submit_response没有locator参数，
两族成功均为顶层task_id。后续审查裁定：有效task_id与非零base_resp并存时
返回Unknown，避免可能已受理却释放资金；无ID明确错误仍Rejected。受限ID、错误文案脱敏及非法用量Unknown
沿候选明确的保守边界。生产宿主接线、固定绑定恢复与正式发行产物验证尚需单独验收。

## 发布前审查修复

- H3路径ID`.`/`..`会被URL规范化吞掉，新增两个公开负测取得真实失败，
  提交解析不再把它们Accepted（Unknown），query不生成descriptor且错误不回显ID。
  两个真实Wasm冻结负例同步覆盖。日志`h3-dot-red.log`、`h3-dot-green.log`。
- 有效task_id与非零base_resp同时出现，原Rejected会在宿主造成可能错误释放；
  `h3-contradiction-red.log`确认真实失败，按本轮裁定最小改为Unknown。
  无ID明确base_resp错误仍Rejected；HTTP4xx/5xx既有语义保持。
- 三个旧codec负例补两个Task5必填null键，避免因缺字段先失败掩盖负数/未知键检测。
- `h3-review-green.log`：H3原生7、Hailuo14、estimate12、MiniMax真Wasm5均通过，
  冻结样例现49（原20+H3新增29）。独立审查无剩余Critical/Important，
  输入数组接受属于既有serde行为，本文不声称全部输入必须是JSON object。

修复后全部README检查改用独立前缀`/tmp/target-architecture-minimax-h3-final-`，
旧检查与包不覆盖。最终结果在下节追加。

## 修复后最终结果

README全部16项再次完成且exit0，精确命令沿前节/README，日志在
`/tmp/target-architecture-minimax-h3-final-verification-summary.log`及同前缀
`verify-*.log`：fmt、clippy、nextest、doc-tests、no-default、fuzz-check、docs、msrv、
boundary-self、boundary、deny、fuzz-deny、audit、fuzz-audit、machete、fuzz-machete。
nextest为655/655、0跳过、8.722秒，未标记LEAK；no-default为635通过；
doctest仍8目标0例。此轮没有LEAK不等于定位或修复历史管道异常。

最终552文件源码摘要验证前后相同，见同前缀source-before.log/source-after.log：
`3ba20442670fbf3c395c4f260dfb0ffd28f96a42ae662330082ac5a2b6d988ea`。
最后仅补本文中文证据，生产与测试源码不变。独立规格审查的点段问题已真红绿关闭，
金额/用量与请求事实、恢复locator边界经静态复核；根代理仍需最终复核与提交。

重新打包六包：`/tmp/target-architecture-minimax-h3-final-packages/`，
每包只含普通manifest.json/component.wasm，字节与已测试源文件一致；
包版本/参考metadata/world/0.30runtime再次全核一致，Task5/HTTP9不漂移。
完整摘要见该目录package-sha256.json。最终MiniMax Wasm 404789字节：
`578b0740316620cf75d4f4847af279dc453714e846d958037ebf75a4349b0e96`。
manifest仍为`424aaa167ed1a477ff16327bf965fab517b0df7283cff08ecc35db1cd7930558`。
最终候选归档SHA256：
`b84bae5e2238f498cd136220636f3fb6300bb2254d2243ce7a29d88f692e4369`。
不复用前轮包pin；宿主须使用最终源码/产物，之后正式发行仍以实际release产物摘要为准。
本子任务未提交、push、tag、release，未修改server，也未把临时验证当生产采用。
