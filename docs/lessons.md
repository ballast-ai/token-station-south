# Engineering Lessons

## Paused Tokio time and real sockets

- Do not start a Tokio runtime with time paused while a test still depends on operating-system
  socket readiness. A server-side `write_all` signal proves only that bytes reached the kernel; it
  does not prove that the client runtime has observed readiness. Tokio may therefore auto-advance
  a transport timer before the socket event is dispatched.
- For real loopback timeout tests, complete the connection, response headers, and any required
  first body chunk under the normal clock. Pause time only immediately before the specific pending
  read whose timer is under test.
- A flaky test fix is not accepted from isolated runs alone. Re-run the complete test binary under
  its normal parallel schedule enough times to exercise cross-test runtime and I/O scheduling.

## A conformance case only closes the mutations it can observe

- Adding a case to a frozen table closes some surviving mutations and not others. Which ones is an
  empirical question, and re-measuring is cheap. When `CredentialSlotMismatch` was added to
  `south.provider-quota-metadata.v1`, the adoption record predicted it would kill both mutations
  the two-case table had let through. It killed one. Literal `(1, 1)` evidence now fails on two
  count categories, because the new case expects `(Zero, Zero)`. But an executor that bypasses the
  host assembly layer and calls `south-core` directly still passes, because the binding check that
  rejects the mismatched slot lives in `south-core` — both paths reach it and produce the same
  failure and the same zero counts.
- The general shape: a case can only distinguish two implementations if they behave differently on
  it. A negative case pins the layer that performs the rejection, not every layer the call would
  otherwise traverse. To pin a wrapper you need a case where the wrapper itself changes the
  outcome.
- So do not carry a prediction about mutation coverage into a status. Re-run the mutations against
  the new table and write down what actually happened, including the ones that still survive. An
  adoption note claiming a closed gap that is still open is worse than one that names the gap,
  because the next reader stops looking.

## A judge that never fires is not a judge

- The host-signed allow-list diff shipped with two checks that both looked load-bearing: a count
  comparison between what was declared and what arrived, and a per-declared-header lookup while
  binding in canonical order. Deleting the count check left the whole suite green. It could never
  fire: the first pass already proves every arrival is declared and unique, so a count mismatch
  means a declared header is missing — exactly what the lookup rejects one line later.
- Two checks for one condition is not defence in depth. It is one check and one decoration, and
  nothing tells you which is which until you delete one. Redundant validation also reads as
  thoroughness in review, so it survives.
- The deletion is the evidence, and it belongs in the code. A comment saying "removing this left
  every test green, which is what dead judges look like" stops the next reader from adding it back
  for the same plausible reason it was added the first time.

## A promise about bytes is settled by bytes

- The design record promised the transport adds "only `host` and `content-length`" to a signed
  request. The first run of a fixture that counted headers on a real socket found `accept: */*` —
  a `reqwest` client default, invisible in every unit test because nothing had ever enumerated the
  wire.
- The record was not wrong about what South *should* do. It was wrong about what the dependency
  *does*. No amount of reading South's code would have found it; the header was never written by
  South at all.
- The fix is not to delete the default but to take ownership of it: set it explicitly, publish it
  as a constant, and have the fixture compare against that constant exactly. An unowned default is
  a byte outside every contract that mentions bytes.

## A literal one step ahead of the release is indistinguishable from one that tracks it

- A tuple-handshake fixture asserted a mismatch by naming "the next version". Three release bumps
  in a row, a blanket version replacement collapsed it into the matching value, silently turning a
  negative case into a tautology. One of those rounds left a comment warning the next person. The
  warning did not help, because the trap is not carelessness.
- `sed` cannot tell the two apart, and neither can a reviewer skimming a diff full of version
  bumps. The structural fix is a sentinel no release can ever equal, so the two kinds of literal
  stop looking alike.
- Generalisation: when a mechanical edit is going to sweep a file, the values that must *not* move
  need to be shaped differently from the values that must — not merely commented differently.

## 2026-09-20：迁移前核对完整纯行为

- 品牌范围不得从别家限定类推：只限制 OpenAI 官方不等于限制 MiniMax 官方。
- 翻译迁移需同时核输入别名/trim、错误状态映射、原始型号与协议家族；
  不得凭合理猜测加型号或时长新限制。先加可失败回归，再修源码。

- 版本机械替换必须先断言精确匹配数量，再读取常量/manifest交叉核对；
  2026-09-20把u16误写成u32匹配导致HTTP9声明与源码8不一致，由真实兼容门捕获。

## 2026-09-20：组件能力必须覆盖实际返回的执行请求

- descriptor通过认证/端点授权不等于包级能力已获授权；返回额外取物请求的组件
  必须声明artifact_fetch，测试须同时读取真实manifest并调用真实Wasm方法。
- 发布审查要沿宿主实际消费门逐项核对，不能用库对拍与版本一致性代替能力检查。

## 2026-09-20：路径段编码不能保护点段

- 将原始任务ID放入路径前，除了编码分隔符还须拒绝独立`.`/`..`，
  因URL规范化会消除点段；先公开负测证明零可发送descriptor，再修边界。
