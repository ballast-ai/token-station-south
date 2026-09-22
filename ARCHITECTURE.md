# Architecture

> 2026-09-20 第五批候选补充：task-v2 的请求估算依据与实际观察用量分开建模。
> `PreparedTaskV2.request_estimate` 携带实际请求时长和协议单位率，纯 helper 只按
> 宿主显式估时计算单位；宿主保留价格、加价、估时默认值及预占事务。详见
> [请求估算设计](docs/design/2026-09-20-task-request-estimate.md)。该接口随 v0.29.0 发布，宿主任务采用仍待验收。

Token Station South uses dependency inversion: it owns the provider-facing contracts and runtime,
while community and enterprise hosts own business policy and consume South.

```text
token-station-south
  contracts / provider API / provider runtime / conformance / transports
                  ^
                  |
       +----------+----------+
       |                     |
token-station         token-station-server
community policy      enterprise policy
```

## Crate ownership

| Crate | Current status and ownership |
| --- | --- |
| `south-task-core` | 候选：独立 Rust 版本 0.1.0，无生产依赖；共享提交/观察/CAS 赢家回读/等待/取消顺序，宿主保留政策与复合原子效果 |
| `south-task-conformance` | 候选：独立 Rust 版本 0.1.0，无生产依赖；公共原子效果故障套件，宿主适配真实 SQLite / PG 事务，无资金宿主明确不适用 |
| `south-contracts` | Implemented bounded HTTP (JSON POST, body-less GET, and multipart POST request shapes, and a buffered binary response beside the UTF-8 one), Bearer, sanctioned header-secret, and combined Bearer-plus-header-secret auth, stable error, byte-streaming, and closed quota metadata contracts, plus the sanctioned controlled query and controlled user-agent declarations |
| `south-core` | Implemented host-neutral buffered and streaming provider-call orchestration and its buffered body-less GET, multipart and binary-response twins, plus the shared host prelude (`raw` module: raw-call type, its host-signed, GET and multipart twins, contract-parse orchestration, one-shot wrappers for all four, resolver adapters) |
| `south-transport-reqwest` | Implemented hardened buffered and byte-streaming JSON POST transport, the same buffered transport for body-less GET and multipart POST requests (rendering the latter's media type and sharing its allocation) and for a JSON POST whose response is buffered as opaque bytes under its own larger cap, bounded quota metadata capture, sanctioned user-agent application, and one-config transport-pair construction |
| `south-provider-conformance` | Implemented immutable provider-call, provider-stream, provider-quota-metadata, header-auth, controlled-query, controlled-user-agent, provider-get, provider-multipart, and provider-binary v1 fixtures |
| `south-testkit` | Implemented assembled-executor conformance runners and reference executors for all nine suites, plus the owned raw-call, host-signed raw-call, raw-GET and raw-multipart builders for host tests |
| `south-provider-api` | Implemented v2 provider component ABI: WIT package `token-station:adapter@2.0.0` (world `provider-adapter-v2`) plus the gate-① manifest schema with the seven-field compatibility tuple; depends on no other south crate by design |
| `south-component-conformance` | Implemented gates ① and ② (package admission + `south.provider-component.v1` behavior suite) with the native `provider-openai-compatible`, `provider-anthropic` and `provider-gemini` references and a frozen fixture pack each; the one sanctioned typed consumer of the Canonical IR, pinned to a kernel distribution tag |
| `south-provider-runtime` | Implemented sandboxed component execution: gated loading, locked-down WASI, memory/deadline/payload/stream bounds, `host.sign` behind the manifest's secret allowlist — JSON-face only, never an IR consumer; the typed seam over it is the conformance crate's `sandbox` feature |

## Removed ownership markers

Two bootstrap placeholders were removed once they held no code and no live obligation. Both are
recorded here rather than silently dropped, because the bootstrap design lists them and a reader of
that record needs to know where they went.

- `south-migration` owned offline fixture comparison for host migrations. The capability-scoped
  conformance suites replaced it: each host proves an adapter against a frozen case table before a
  status turns `verified`, which is a stronger and earlier check than comparing two runs after the
  fact. Its "never production double-send" rule survives as a repository rule, not as a crate.
- `south-transport-ureq` reserved a synchronous native transport. `south-transport-reqwest` shipped
  first because the migration-critical host pins reqwest, and no host has since asked for a
  synchronous stack. An empty crate did not constrain the transport traits either way, so the
  reservation cost maintenance without protecting anything.

Reintroducing either is a normal new crate with its own design record. Neither name is reserved.

The implemented crate dependency graph is:

```text
south-core -------------------------------> south-contracts
south-transport-reqwest ------------------> south-core
south-transport-reqwest ------------------> south-contracts
south-provider-conformance ---------------> south-contracts
south-testkit ----------------------------> south-contracts
south-testkit ----------------------------> south-core
south-testkit ----------------------------> south-provider-conformance
south-component-conformance --------------> south-contracts
south-component-conformance --------------> south-provider-api
south-component-conformance --------------> token-station-protocol (kernel tag)
south-component-conformance (sandbox) ----> south-provider-runtime
south-provider-runtime -------------------> south-provider-api
```

These edges are direct Cargo dependencies. They are one-way and acyclic. Only the reqwest transport
crate owns a network-client dependency, and only the runtime crate owns the wasmtime engine (its
typed seam lives behind the conformance crate's `sandbox` feature so wasm guests, which depend on
the conformance crate for the abi shims, never pull the engine into their build). No South crate owns a database, cache, migration directory,
host repository dependency, or credential source. The kernel-tag edge is the S0-sanctioned
exception to IR independence: conformance gate ② judges typed decode through the distribution
channel at a fixed revision; no production crate may gain that edge.

## Host-owned concerns

### task-v2 边界（v0.29.0 已发布，宿主采用待验收）

任务词汇当前为 4（引入独立 v2 类型时为 3，后续增加请求估算）；既有 v1 类型和 world 保留。
新 `token-station:task-adapter@2.0.0` world 仍只描述纯翻译，不拥有执行时序、
凭证读取、价格和持久化。contracts 保存有界定位、并存计量、观察与渲染上下文；
包含 IR descriptor 的 `PreparedTaskV2` 和唯一 JSON codec 位于 conformance。
runtime 继续仅消费 JSON，按 provider-v2／task-v1／task-v2 明确分流。

Kling v2 的提交、查询、观察与渲染由同源原生／Wasm 实现承担；宿主必须保存并回传
locator，依据用量事实应用自身计价策略，并在公开返回前实施交付授权。
候选 manifest 当前只准入已验证的 bearer/header_secret；不新增未使用的 WIT 签名
import，未验收的宿主签名能力也不据此宣称支持。
详细说明和仍待完成的持久绑定／兼容恢复见[候选设计](docs/design/2026-09-20-task-adapter-v2-candidate.md)。

South does not own routing, fallback across upstreams, retry budgets, admission, tenants, billing,
quota ledgers, audit persistence, task persistence, credential sources, or tracing initialization.
It never reads a database directly. Transport I/O, time, cancellation, component bytes, and runtime
permissions must be explicit capabilities at their operational boundaries.

### Where the vocabulary line runs: objective facts in, business choices out

South may carry vocabulary for what an upstream *reported* and for what *happened* — never for
what a host *decided*. The line, ruled 2026-09-08 for the task and (future) metering vocabularies:

| May live in South | Stays host-side |
| --- | --- |
| **Metering**: tokens, seconds, images, characters, milliunits an upstream reported | **Pricing**: unit prices, tiers, discounts, rate cards |
| **Metering uncertainty**: how "the upstream reported no usage" is expressed | **Business model**: BYOK fee splits, routing attribution, tenant policy |
| **Settlement outcome vocabulary**: the closed set of ways a settlement can end | **Funds policy**: when to reserve, whose ledger to debit, how much to hold |

A metering vocabulary is admitted only with a second consumer in sight: a shared library that
freezes one host's persisted format is not sharing, it is exporting that host's migration burden.

### What never enters South, even when it could be moved

Two tests, ruled 2026-09-08. Material whose **leak impact exceeds one API key** stays host-side:
service-account private keys, key-encryption keys, anything that decrypts every tenant's rows.
Logic whose **wrong decision is money or an unrecoverable credential** stays host-side: BYOK wallet
selection and its exclusivity rules, single-use rotation's concurrency guard, anything that depends
on database semantics to be correct. Minting, OAuth refresh, and request signing therefore remain
host code by design; South offers the finalizer seam (`RequestFinalizerV1`) for the *position* of a
signature, never for the material. A host keeps a per-provider authentication layer above South,
and that layer is not a gap South intends to close.

During migration, `token-station-protocol` may re-export South types under old Rust paths. It must
not define duplicate nominal types, and South must never depend back on that compatibility layer.

## Host-side dependency gate

`scripts/check-boundaries.sh` enforces the strict reqwest gate (exactly one package, exact version,
exact feature set) **inside this workspace only**. A host workspace that consumes
`south-transport-reqwest` cannot satisfy that gate verbatim: hosts legitimately enable their own
reqwest features (for example `json` or `multipart`) and may carry unrelated reqwest major versions
elsewhere in their graph. The equivalent host-side gate, agreed during the first host adoption
(token-station-server, 2026-08-17), is:

1. `south-transport-reqwest` and the host's primary stack resolve to the **same** reqwest node at
   the exact version this workspace pins.
2. The unified feature set of that node includes `rustls-tls` and `stream`, and does **not** include
   `default-tls`, `native-tls`, `cookies`, or `system-proxy`.
3. Pre-existing unrelated reqwest versions in the host graph gain no new dependents.

Hosts are expected to script these checks (`cargo tree` and lockfile inspection) into their own CI.
This section records the agreed interpretation so a host failing the workspace-local script is not
misread as a boundary violation.

## v0.30.0 MiniMax 候选

新增同源MiniMax Hailuo v1/H3 v2参考/guest与受控file_id查询，HTTP合同9。
任务合同5新增有界resolution/input_image_count请求事实，供宿主既有定价函数使用。
既有task ABI/WIT不变；宿主仍拥有凭证、配置快照、价格、恢复与交付。
见[候选设计](docs/design/2026-09-20-minimax-v1-task-component.md)。


## 共享任务执行核心候选

新增 `south-task-core` 仅使用标准库，不依赖组件 IR、数据库或网络。宿主效果
分成 prepare/dispatch/send/record 和 load/query/normalize/apply/reload；
核心固定执行顺序并确保 CAS 落败返回持久赢家。宿主独占精确绑定恢复、
凭证、计价、任务/资金/outbox 原子提交和交付许可。等待显式注入时钟与取消，
inspect 可调用共享 observe 推进一步，等待到期本身不改变任务或资金。

该库独立 Rust 版本为 0.1.0；八个库与组件运行时随本次发布为 v0.32.0，
七个组件身份各自不变（`provider-openai-compatible` 2.1.3、`provider-gemini`
1.1.3、`provider-anthropic` 1.0.4、`task-kling` 1.0.3、三个 task-v2 均 0.31.0），
Task5/HTTP9/WIT 不变。新增 Rust 编排不要求旧组件更换身份，但每个包的
`compatibility.south_runtime` 随运行时一并声明为 0.32.0——该字段按精确串
比对，声明旧版的包会被宿主按名拒绝，故运行时与七包必须同批升。宿主采用
须分别用真实存储通过公共故障套件；组件兼容状态不能代替共享核心采用证据。[设计与边界](docs/design/2026-09-20-shared-task-core.md)。
