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
| `south-north-codec` | OpenAI Chat、Anthropic Messages 与 Responses 北向纯映射；typed 与 JSON façade 同源，宿主传入时间、身份及每流状态，准入、计费和 continuation 留宿主 |
| `south-task-core` | 候选：独立 Rust 版本 0.1.0，无生产依赖；共享提交/观察/CAS 赢家回读/等待/取消顺序，宿主保留政策与复合原子效果 |
| `south-task-conformance` | 候选：独立 Rust 版本 0.1.0，无生产依赖；公共原子效果故障套件，宿主适配真实 SQLite / PG 事务，无资金宿主明确不适用 |
| `south-contracts` | Implemented bounded HTTP (JSON POST, body-less GET, multipart POST, and SSML text POST request shapes, and a buffered binary response beside the UTF-8 one), Bearer, sanctioned header-secret, combined Bearer-plus-header-secret, and package-declared header-secret auth, stable error, byte-streaming, and closed quota metadata contracts, plus the sanctioned controlled query and controlled user-agent declarations |
| `south-host-grammars` | Implemented, released in 0.53.0: pure, bounded wire grammars that only hosts call, today the SSE decoder `decode_sse_v1` / `SseDecoderV1` (frame positions for `north_passthrough`, golden vectors, fuzz target). No dependencies; no component may link it (`shipped_packages_v1`, `scripts/check-boundaries.sh`), so it carries the workspace version and a change to it re-identifies no package (boundary record §13.13). A grammar a component may link stays in `south-contracts` |
| `south-core` | Implemented host-neutral buffered and streaming provider-call orchestration and its buffered body-less GET, multipart and binary-response twins, the multipart-binary and text-binary entry points of HTTP contract 12, plus the shared host prelude (`raw` module: raw-call type, its host-signed, GET and multipart twins, contract-parse orchestration, one-shot wrappers for all four, resolver adapters) |
| `south-transport-reqwest` | Implemented hardened buffered and byte-streaming JSON POST transport, the same buffered transport for body-less GET, multipart POST and SSML text POST requests (rendering the latter two's media types and sharing their allocations) and for a JSON POST whose response is buffered as opaque bytes under its own larger cap, bounded quota metadata capture, sanctioned user-agent application, and one-config transport-pair construction |
| `south-provider-conformance` | Implemented immutable provider-call, provider-stream, provider-quota-metadata, header-auth, controlled-query, controlled-user-agent, provider-get, provider-multipart, provider-binary, and provider-media-binary v1 fixtures, the safe-fetch v1 fake-network fixtures, and the host-implemented credential-recipe v1 suite (harness, fake token endpoint, runner) |
| `south-testkit` | Implemented assembled-executor conformance runners and reference executors for all ten suites, the safe-fetch v1 runner with its fake resolver, connector and proxy environment and a reference executor, plus the owned raw-call, host-signed raw-call, raw-GET and raw-multipart builders for host tests |
| `south-provider-api` | Implemented v2 provider component ABI: WIT package `token-station:adapter@2.0.0` (world `provider-adapter-v2`) plus the gate-① manifest schema with the seven-field compatibility tuple; depends on no other south crate by design |
| `south-component-conformance` | Implemented gates ① and ② (package admission + `south.provider-component.v1` behavior suite) with the native `provider-openai-compatible`, `provider-anthropic` and `provider-gemini` references and a frozen fixture pack each; a sanctioned typed consumer of the Canonical IR, pinned to a kernel distribution tag |
| `south-provider-runtime` | Implemented sandboxed component execution: gated loading, locked-down WASI, memory/deadline/payload/stream bounds, `host.sign` behind the manifest's secret allowlist — JSON-face only, never an IR consumer; the typed seam over it is the conformance crate's `sandbox` feature. Also reads the release's model catalog (`ModelCatalogV1`, `south.model-catalog.v1`), here because no guest links this crate |

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
south-provider-conformance ---------------> south-provider-api
south-testkit ----------------------------> south-contracts
south-testkit ----------------------------> south-core
south-testkit ----------------------------> south-provider-conformance
south-component-conformance --------------> south-contracts
south-component-conformance --------------> south-provider-api
south-component-conformance --------------> token-station-protocol (kernel tag)
south-component-conformance (sandbox) ----> south-provider-runtime
south-provider-runtime -------------------> south-provider-api
south-north-codec ------------------------> token-station-protocol (kernel tag)
```

`south-host-grammars` has no edge in either direction: it depends on nothing, and no crate in this workspace
depends on it except the fuzz binary, which is a test-only consumer.

These edges are direct Cargo dependencies. They are one-way and acyclic. Only the reqwest transport
crate owns a network-client dependency, and only the runtime crate owns the wasmtime engine (its
typed seam lives behind the conformance crate's `sandbox` feature so wasm guests, which depend on
the conformance crate for the abi shims, never pull the engine into their build). No South crate owns a database, cache, migration directory,
host repository dependency, or credential source. The kernel-tag edge is the S0-sanctioned
exception to IR independence: conformance gate ② judges typed decode through the distribution
channel at a fixed revision; the shared north codec is the separately sanctioned pure mapping
consumer at that same revision. Other crates may not gain that edge.

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

### embeddings-adapter-v1 boundary (since 0.47.0)

A fourth world sits beside provider-v2, task-v1 and task-v2: WIT package
`token-station:embeddings-adapter@1.0.0`, world `embeddings-adapter-v1`, judged by
`south.embeddings-component.v1`. It has a package of its own so chat-side changes never force an
embeddings version signal. Its five exports (`metadata`, `healthcheck`, `build-embeddings-request`,
`parse-embeddings-response`, `map-provider-error`) are pure translation and the world imports
nothing: the loader refuses any `token-station:` or `host` import for it, and the runtime never
links the signing host. The manifest admits the `bearer` and `header_secret` arms, requires `embed`
and at least one provider family, admits (from 0.48.0) the two sources of `ProviderConfig.declared`
(a family's `config_schema` and exported credential attributes, which the task worlds refuse), and
refuses every other provider-world declaration as the task worlds do. Contract 1 carries text and token-id inputs only (vocabulary `embed`, `batch`, `dimensions`, `token_ids`);
contract 2 (since 0.51.0) adds inline media inputs and the `media` word, which a package may declare only with
`contracts: {"embeddings": 2}`, and a host admitting such a package gives the world at least 192 MiB of guest memory. The runtime stays JSON-only and
routes by world (provider-v2 / task-v1 / task-v2 / embeddings-v1). The host keeps credentials,
HTTP, vector extraction, pricing and settlement; both hosts are `not_verified` for this world in
`compatibility.json`. As with task-v2, the IR-bearing `PreparedEmbeddingsV1`, its single JSON
codec, the guest ABI shims, the sandbox seam and the suite live in `south-component-conformance`;
the suite builds each response case's paired request and runs the host's extraction and checks,
so its fixtures pin what a host decides. The native references for `embeddings-openai-compatible`,
`embeddings-gemini` and `embeddings-vertex` pass it, and so do their wasm packages under
`components/`, which are thin guests over those references and agree with them byte for byte across
the ABI. `embeddings-vertex` reads its location and project from `ProviderConfig.declared` and mints
its bearer through a service-account credential recipe. See the
[embeddings contract record](docs/design/2026-09-30-embeddings-contract.md).

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
on database semantics to be correct. So for minting, OAuth refresh and request signing, the
**execution**, the **material**, the **destinations** they may reach and the **host invariants**
belong to the host, and the **per-provider description** belongs to the component: a package
declares its credential fields, slots and minting recipes as data (`credentials`, a closed step
vocabulary) and its request-signing scheme (`signing`), and the host runs one generic executor over
them. South offers the finalizer seam (`RequestFinalizerV1`) for the *position* of a signature, never
for the material. The host's invariants hold whatever a recipe declares: it never overwrites
non-empty refresh material with an empty value and keeps the previous generation; every expiry is
clamped to 60 seconds to 24 hours; a recipe reaches only endpoints the operator confirmed for that
package digest; and recipes run only for verified first-party packages until package signing exists.
(Revised 2026-10-02, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §3.4, §3.5; this replaces
"a host keeps a per-provider authentication layer above South".)

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
任务合同6（`docs/design/2026-09-27-task-contract-v6-facts.md`）新增：请求估算的`tokens_per_second`（协议单位率，与
`milliunits_per_second`对称）与`requested_outputs`（请求条数/张数，≥1）；用量的`outputs`（实报交付数）；URL产物的
`fetch_with_credential`（须用任务钉住的凭证取回、不得把原URL交给客户端）；prepared的`immutable_body_paths`（宿主不得改写的
请求体点号路径；null=组件不表态、宿主不得注入附加字段，[]=无保留路径）。新键一律必须出现、可为null，合同5形状拒收。
既有task ABI/WIT不变；宿主仍拥有凭证、配置快照、价格、恢复与交付。
见[候选设计](docs/design/2026-09-20-minimax-v1-task-component.md)。


## 共享任务执行核心候选

新增 `south-task-core` 仅使用标准库，不依赖组件 IR、数据库或网络。宿主效果
分成 prepare/dispatch/send/record 和 load/query/normalize/apply/reload；
核心固定执行顺序并确保 CAS 落败返回持久赢家。宿主独占精确绑定恢复、
凭证、计价、任务/资金/outbox 原子提交和交付许可。等待显式注入时钟与取消，
inspect 可调用共享 observe 推进一步，等待到期本身不改变任务或资金。

This library carries its own Rust version, 0.1.0. The component runtime and the workspace-versioned crates are at v0.53.0; `south-contracts`, `south-provider-api` and `south-component-conformance`, which every component links, carry versions of their own (currently 0.51.0) so that a release which leaves them unchanged re-identifies no package (boundary record §13.12, §16 Q47).
**Unreleased (B6-2)**: the Bedrock InvokeModel Anthropic component (design record
`docs/design/2026-10-08-bedrock-invoke-anthropic-component.md`, accepted 2026-10-09). The new package
`provider-anthropic-bedrock-invoke` 1.0.0 (family `anthropic-bedrock-invoke`, `host_signed` with `aws-sigv4` as Converse
declares it, `stream_framing: aws-eventstream`, `south_runtime` 0.46.0) sends `provider-anthropic`'s Messages body
without `model` and `stream` and with `anthropic_version: "bedrock-2023-05-31"`, unwraps each stream `chunk`'s base64
`bytes` itself, and maps Bedrock exception names, then Anthropic error types, then the status. The shared Messages stream
state machine changed for `provider-anthropic` too: a cumulative usage count that shrinks is refused and the
cache-write tiers fold as one group (the host's 03 #86 rule, I-Q7), and an in-band `error` event ends the stream with
`StreamEvent::Error` (I-Q12). Gate ② stream cases may now expect a refusal (`{"error": <envelope>}`). A crate-private
strict base64 codec replaces the credential recipe interpreter's. No release has been cut since v0.50.0, so B6-2 takes
no version of its own beyond the new package: `south-component-conformance` stays at the 0.50.0 #167 gave it, and every
existing package keeps the single bump over v0.50.0 that #167 gave it (`provider-anthropic` 1.0.16 now also carries the
two behavior changes above).
**v0.53.0**: `decode_sse_v1`, early and alone (release record `docs/design/2026-10-10-release-0.53.0.md`, boundary
record §13.13, Q-B6-6). The new host-only crate `south-host-grammars` carries the SSE decoder `decode_sse_v1` and its
incremental form `SseDecoderV1`, whose `position` cuts a stream into whole frames for `north_passthrough`. No component
links the crate, so no guest-linked crate, package, `component.wasm`, contract number or `south_runtime` changes (Q47
does not apply); all nineteen packages keep their version and bytes.
**v0.52.0**: the image world, first batch (release record `docs/design/2026-10-10-release-0.52.0.md`, image world record
§19). `contracts.media` v1 and `contracts.image` v1 (`south_contracts::media`, `south_contracts::image`), HTTP contract 12,
the `image-adapter-v1` world (no host import; world exclusions are now `WorldSchemaV1` properties), gate ②
`south.image-component.v1`, gate ③ `south.provider-media-binary.v1` and `south.safe-fetch.v1`, and the package
`image-azure` 1.0.0 (`south_runtime` 0.52.0, `not_verified`). The three guest-linked crates move to 0.51.0; every other
package takes a patch bump and keeps its `south_runtime`.
**v0.51.1**: runtime fix for host gap #101 (release record `docs/design/2026-10-09-release-0.51.1.md`). A guest trap
inside a tokio runtime used to panic the calling task, because `add_to_linker_sync` runs every blocking `wasi:io`
function under `Handle::block_on` and a trapping guest flushes its panic or allocation-failure message to stderr; and
after any trap the component's shared instance could never be entered again. `nonblocking_io` registers replacements
that drive the same `wasmtime-wasi-io` futures on the calling thread without parking it (a wait that cannot complete is a
trap), and the call that follows a guest error replaces the shared instance. Only `south-provider-runtime` changes: no
package, contract or guest-linked crate (Q47 does not apply), all eighteen packages keep their version and bytes.
**v0.51.0**: embeddings contract 2, inline media inputs (release record `docs/design/2026-10-09-release-0.51.0.md`,
embeddings record §17). `EmbeddingInputV1::Media` and `parse_embeddings_request_v2` carry a `data:<media type>;base64,`
input undecoded; the `media` capability word needs `contracts: {"embeddings": 2}`; `embeddings-gemini` 1.1.0
(`south_runtime` 0.51.0) builds Gemini's `inline_data` parts. A host admitting a `media` package builds its embeddings
runtime with at least 192 MiB of guest memory (`RuntimeLimitsV1::for_embeddings_media()`; south's default is
unchanged) and answers 413 above a 15 MiB request view (E-O2). The three crates every component links took 0.50.0
under Q47, so the other sixteen packages take a patch bump with unchanged behavior and `south_runtime`. The
`gemini-embedding-001` text-only refusal is a hard-code carried over from the native arm (open item E-O1).
**v0.50.0**: the model catalog (release record `docs/design/2026-10-08-release-0.50.0.md`, boundary record §13.11,
§13.12). `catalogs/model-catalog.json` is published as `model-catalog-v0.50.0.json` and listed in the release index
under `catalogs` (`south.model-catalog.v1`, read by `south_provider_runtime::ModelCatalogV1`). It is the first release
under Q47: the three crates every component links keep 0.49.0, so all seventeen packages keep their version,
`component.wasm`, `manifest.json` and `south_runtime`. No contract number changes; the kernel pin is unchanged.
**v0.49.0**: host feedback SF27 (release record `docs/design/2026-10-08-release-0.49.0.md`, boundary record
§13.10). `provider-gemini` 1.1.11 takes a stream's `Usage` only from its terminal chunk (a candidate carries
`finishReason`); an intermediate `usageMetadata` is checked (an object, counts never decreasing) but never emitted, and
any frame after the terminal chunk is refused, so Vertex AI native streams pass. The shared crates every guest links
changed, so the other sixteen packages take a patch bump with unchanged behavior and keep their `south_runtime`:
`provider-openai-compatible` 2.4.3, `provider-anthropic` 1.0.15, `provider-bedrock-converse` 1.0.12,
`provider-bedrock-converse-bearer` 1.0.4, `task-kling` 1.0.11, `task-kling-v2` 0.32.9, `task-minimax-v2` /
`task-bailian-v2` 0.31.8, `task-byteplus-v2` 0.36.8, `task-xai-v2` / `task-veo-v2` / `task-wan-image-v2` /
`task-gmi-image-v2` 0.35.8 (0.46.0), `embeddings-openai-compatible` / `embeddings-gemini` 1.0.2 (0.47.0) and
`embeddings-vertex` 1.0.1 (0.48.0). No contract number changes; the kernel pin is unchanged.
**v0.48.0**: the embeddings value channel and `embeddings-vertex` (release record
`docs/design/2026-10-08-release-0.48.0.md`, embeddings record §16). Gate ① admits a family's `config_schema` and
exported credential attributes in the `embeddings-adapter-v1` world, the two sources of `ProviderConfig.declared`, under
the provider world's rules; `host_values` and every other provider-world declaration stay refused, and the task worlds
are unchanged. `south.embeddings-component.v1` gains `undeclared_values_ignored` (suite still version 1). The new
package `embeddings-vertex` 1.0.0 (family `vertex-ai`, Vertex AI's `:predict`) reads `region` and `project` from
`declared`, mints its bearer through a service-account credential recipe, and declares `south_runtime` 0.48.0, the
first runtime whose gate ① admits those declarations; it is `not_verified` for both hosts. The shared crates every
guest links changed, so the other sixteen packages take a patch bump with unchanged behavior and keep their
`south_runtime`: `provider-openai-compatible` 2.4.2, `provider-anthropic` 1.0.14, `provider-gemini` 1.1.10,
`provider-bedrock-converse` 1.0.11, `provider-bedrock-converse-bearer` 1.0.3, `task-kling` 1.0.10, `task-kling-v2`
0.32.8, `task-minimax-v2` / `task-bailian-v2` 0.31.7, `task-byteplus-v2` 0.36.7, `task-xai-v2` / `task-veo-v2` /
`task-wan-image-v2` / `task-gmi-image-v2` 0.35.7 (0.46.0), and `embeddings-openai-compatible` /
`embeddings-gemini` 1.0.1 (0.47.0). No contract number changes; the kernel pin is unchanged.
**v0.47.0**: the first release of the `embeddings-adapter-v1` world (`docs/design/2026-09-30-embeddings-contract.md`,
v1 scope §15; release record `docs/design/2026-10-08-release-0.47.0.md`): embeddings contract 1 (text and token-id
inputs only), suite `south.embeddings-component.v1`, the host functions `extract_vectors_v1`,
`check_embeddings_response_v1` and `render_vectors_v1`, and the packages `embeddings-openai-compatible` 1.0.0 and
`embeddings-gemini` 1.0.0, both `not_verified` for both hosts. The two packages declare `south_runtime` 0.47.0, the
first runtime that knows the world. The shared crates every guest links changed, so the other fourteen packages take
a patch bump with unchanged behavior and keep `south_runtime` 0.46.0: `provider-openai-compatible` 2.4.1,
`provider-anthropic` 1.0.13, `provider-gemini` 1.1.9, `provider-bedrock-converse` 1.0.10,
`provider-bedrock-converse-bearer` 1.0.2, `task-kling` 1.0.9, `task-kling-v2` 0.32.7, `task-minimax-v2` /
`task-bailian-v2` 0.31.6, `task-byteplus-v2` 0.36.6, and `task-xai-v2` / `task-veo-v2` / `task-wan-image-v2` /
`task-gmi-image-v2` 0.35.6. `compatibility.json` `schema_version` 6 adds `contracts.embeddings` and the
`embeddings_component_capabilities` table. **Breaking for hosts on re-pin**: new `CheckV1` and `ManifestErrorV1`
variants; the kernel pin is unchanged.
**v0.46.0**: the kernel re-pin to protocol 0.5.0 (mirror `v0.4.0`, `c2581f37`, `canonical_ir` 3), B7b, #138 and the
Q14 value channel (#151, #152, #154; `docs/design/2026-10-08-kernel-repin-protocol-0.5.0.md`,
`docs/design/2026-09-30-host-zero-vendor-boundary.md` §13.7, §13.8, §16 Q35–Q45). Every manifest declares
`ir_schema_id` `token-station-protocol@0.5.0/v0.4.0`, `kernel_version` 0.4.0, `kernel_revision` `8e34f5a0…` and
`canonical_ir` 3; `compatibility.json` records `kernel_contracts.canonical_ir` 3. Because no earlier runtime records that
contract, every package declares `south_runtime` 0.46.0, the oldest runtime that admits it (Q37). Protocol 0.5.0 admits
`%2F` inside one path segment (D5) for ARN model ids (#138, with a model-with-a-slash row in the Converse,
Converse-bearer and Gemini packs); the seven task-v2 packages that encode a task id as one observe segment refuse an id
containing `/`. B7b adds the provider-world auth arm `bearer_and_header_secret` and the `gemini-openai-compatible`
family in `provider-openai-compatible`; five more kernel never-credential names become undeclarable secret headers
(Q40). Q14 adds `ProviderConfig.declared` (config keys and exported credential attributes, via
`ComponentManifestV1::declared_keys` / `declared_values`) and `ChatRequest.host_values` (`attempt_id`), gate ②
`undeclared_values_ignored`, and two gate ③ credential-recipe cases (twelve; `token-station-server` goes `not_verified`
until it runs them). Identities: `provider-openai-compatible` 2.2.1 → **2.4.0** (Copilot `plan` required, #150; the
`gemini-openai-compatible` family), and patch bumps for the other thirteen: `provider-anthropic` 1.0.12,
`provider-gemini` 1.1.8, `provider-bedrock-converse` 1.0.9, `provider-bedrock-converse-bearer` 1.0.1, `task-kling` 1.0.8,
`task-kling-v2` 0.32.6, `task-minimax-v2` / `task-bailian-v2` 0.31.5, `task-byteplus-v2` 0.36.5, and `task-xai-v2` /
`task-veo-v2` / `task-wan-image-v2` / `task-gmi-image-v2` 0.35.5. Hosts re-pinning must move the kernel pin and
`HostRangeV1.kernel_contracts.canonical_ir` with the packages and follow the Rust API changes listed in §13.7 and §13.8.
**v0.45.0**: the host's S2 / S4 prerequisites (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §13.6, §16
Q23–Q34, host feedback SF10–SF19). A package's `south_runtime` is now the oldest runtime it needs, not the release
that carries it, so this release re-stamps no manifest: every package keeps `south_runtime` 0.44.0, the digest-stability
check also fails a changed `manifest.json` under an unchanged version, and `scripts/check-declared-runtime.sh` loads
each package under exactly the runtime it declares (SF10). Gate ① now requires every `signing.credentials` entry to name
a declared secret field of the section that applies to each signed family, so a runtime of this release refuses
`provider-bedrock-converse` 1.0.7 (SF13). New: `SandboxedComponentV1::shared` (SF12),
`ComponentManifestV1::endpoint_values` (SF17), the host-implemented gate ③ suites `south.eventstream-framing.v1` and
`south.request-signing.v1`, recorded `not_verified` for both hosts (SF15), and the unreleased `t21-unseen-eventstream`
guest (SF18). Contract numbers are unchanged, and so is the kernel pin. Identities: `provider-bedrock-converse` 1.0.7 →
**1.0.8** (its SigV4 credential fields and the native `accept` / `x-amzn-bedrock-accept` headers; SF13, SF14), the new
`provider-bedrock-converse-bearer` **1.0.0** (family `bedrock-bearer`, `bearer` arm; SF16), and, because the shared
crates changed every `component.wasm`, a patch bump with unchanged behavior for the other twelve:
`provider-openai-compatible` 2.2.1, `provider-anthropic` 1.0.11, `provider-gemini` 1.1.7, `task-kling` 1.0.7,
`task-kling-v2` 0.32.5, `task-minimax-v2` / `task-bailian-v2` 0.31.4, `task-byteplus-v2` 0.36.4, and `task-xai-v2` /
`task-veo-v2` / `task-wan-image-v2` / `task-gmi-image-v2` 0.35.4. Hosts re-pinning to this release must absorb the one
breaking Rust API change in §13.6 (`SandboxedComponentV1::new` and `inner` are no longer `const fn`) and take
`provider-bedrock-converse` 1.0.8 with the runtime.
**v0.44.0**: the S3b prerequisites and the host's S3a feedback (`docs/design/2026-09-30-host-zero-vendor-boundary.md`
§13.5, §16 Q19–Q22): `present` candidates (D1), family-scoped `credentials` (D2), field slots (D3), field descriptions
(D4) and config-key defaults, gate ③ case 10 `TransientFailureIsRetried` (D5), the Kiro draft's four-rule selector (D6),
and the `github-copilot` family with per-plan chat hosts (D7). Contract numbers and the `south.credential-recipe.v1`
suite version are unchanged; a runtime older than 0.44.0 refuses the new manifest fields, and every package declares
`south_runtime` 0.44.0. Identities: `provider-openai-compatible` 2.1.5 → **2.2.0**; the shared crates changed every
`component.wasm`, so the other twelve take a patch bump with unchanged behavior: `provider-anthropic` 1.0.10,
`provider-gemini` 1.1.6, `provider-bedrock-converse` 1.0.7, `task-kling` 1.0.6, `task-kling-v2` 0.32.4,
`task-minimax-v2` / `task-bailian-v2` 0.31.3, `task-byteplus-v2` 0.36.3, and `task-xai-v2` / `task-veo-v2` /
`task-wan-image-v2` / `task-gmi-image-v2` 0.35.3. Hosts re-pinning to this release must absorb the breaking Rust API
changes listed in §13.5.
**v0.43.0**: phases B1–B4 and B7a of `docs/design/2026-09-30-host-zero-vendor-boundary.md` (notes in §6.6 and
§13.1–§13.4): strict usage evidence, descriptor auth admission and request facts, endpoint and config declarations, the
eventstream deframer, the range handshake with per-package isolation and a release index, credential recipe v1, and
package-declared provider instances. Contracts: auth 5, reserved header policy 2, HTTP 10, provider quota metadata 2.
Packages declare `runtime_abi` 1, so from this release on a package whose content is unchanged keeps its identity.
Identities: `provider-openai-compatible` 2.1.5, `provider-anthropic` 1.0.9, `provider-gemini` 1.1.5,
`provider-bedrock-converse` 1.0.6, `task-kling` 1.0.5, `task-kling-v2` 0.32.3, `task-minimax-v2` / `task-bailian-v2`
0.31.2, `task-byteplus-v2` 0.36.2, and `task-xai-v2` / `task-veo-v2` / `task-wan-image-v2` / `task-gmi-image-v2` 0.35.2.
Hosts re-pinning to this release must absorb breaking API changes listed in §13.4.
**v0.42.0**: one more Claude dialect word, `anthropic.sampling.exclusive`: Opus 4.5 through Sonnet 4.6 accept `temperature`
or `top_p` but reject both in one request, so for a model declaring it both components keep `temperature` and drop `top_p`.
Identities: `provider-anthropic` 1.0.7 → **1.0.8**, `provider-bedrock-converse` 1.0.4 → **1.0.5**; the other eleven packages
are unchanged.
**v0.41.0**: per-model Claude request dialect (#120). The host declares the dialect in `supported_parameters`
(`anthropic.sampling.none`, `anthropic.tool_choice.auto_only`, `anthropic.thinking.adaptive`, `anthropic.thinking.budget`,
`anthropic.effort.xhigh`); `provider-anthropic` and `provider-bedrock-converse` drop rejected sampling parameters, refuse a
forced tool choice the model cannot honor, and carry the caller's reasoning effort as adaptive thinking or a bounded budget.
Declaring nothing keeps every request unchanged. Identities: `provider-anthropic` 1.0.6 → **1.0.7**,
`provider-bedrock-converse` 1.0.3 → **1.0.4**; the other eleven packages are unchanged. Design record:
`docs/design/2026-09-29-claude-model-dialect.md`.
**v0.40.0**: IR usage follows the kernel partition contract (`input_tokens` is the whole prompt; the cache
buckets are subsets). `provider-anthropic` and `provider-bedrock-converse` now sum their uncached count with both
cache buckets, Converse accepts the `totalTokens` AWS actually sends (cache counted), and the Anthropic north
renderers subtract the buckets again. Identities: `provider-anthropic` 1.0.5 → **1.0.6**, `provider-bedrock-converse`
1.0.2 → **1.0.3**; the other eleven packages are unchanged. Judge: `usage_ir_contract_v1`.
**v0.39.0**：消费 kernel v0.3.0 / protocol 0.4.0，并以有界 carrier 在 Responses、
Anthropic Messages 与 Bedrock Converse 间保留 Claude reasoning 块身份、签名、脱敏块及
text/tool 顺序。兼容元组改变十三个包的内容，因此所有包身份均递增；South 只记录消费约束，
不取得 canonical IR 所有权。
**v0.38.0**：**任务合同 7**（`docs/design/2026-09-28-task-contract-v7-artifact-role.md`：URL 产物必填 `role`，`null` 为主产物、
`"last_frame"` 为伴随产物，词表封闭、缺键拒收；集合至少一个主产物），宿主只对主产物计数 / 交付 / 转存。首个消费者
`task-byteplus-v2` 把上游尾帧作 `last_frame` 产物报出并渲染进 `data[0].last_frame_url`（与 token-station-server 原生臂同形）——
包身份随内容变化 0.35.0 → **0.36.0**；其余十二个包只随合同重打（fixture 机械补 `role: null`），身份不变；WIT 不变。
v0.37.0 号已被 `feature/p15-responses` 线（Responses codec）占用、未合入 main，本线不移动该 tag、直接发 0.38.0。
**v0.36.0**：`task-kling-v2` 声明请求体禁改路径（`model_name` / `mode` / `sound` / `duration` / `video_list` /
`external_task_id`，与宿主原生 Kling 同一张表），宿主可按任务合同 6 在其余位置注入附加请求配置——包身份随内容变化
0.31.0 → **0.32.0**；合同、WIT 与其余十二个包身份不变。
**v0.35.0**：**任务合同 6**（`docs/design/2026-09-27-task-contract-v6-facts.md`：token 单位率、请求 / 交付条数、产物凭证取回、
请求体禁改路径；合同 5 形状拒收），组件增至十三个——新增五个 task-v2 包身份均 0.35.0：`task-xai-v2`、
`task-byteplus-v2`、`task-veo-v2`（只声明 `header_secret` 臂）、`task-wan-image-v2` 与 `task-gmi-image-v2`（首批图像任务组件）。
现行身份：`provider-openai-compatible` 2.1.4、`provider-gemini` 1.1.4、`provider-anthropic` 1.0.8、
`provider-bedrock-converse` 1.0.5、`task-kling` 1.0.4、`task-kling-v2` 0.32.1、`task-minimax-v2` / `task-bailian-v2` 0.31.1、
`task-byteplus-v2` 0.36.1、其余四个（`task-xai-v2` / `task-veo-v2` / `task-wan-image-v2` / `task-gmi-image-v2`）0.35.1；HTTP9/WIT 不变。每个包的
`compatibility.south_runtime` 随运行时一并声明为 0.42.0——该字段按精确串比对，声明旧版的包会被宿主按名拒绝，
故运行时与十三包必须
同批升。宿主采用须分别用真实存储通过公共故障套件；组件兼容状态不能代替共享核心
采用证据。[设计与边界](docs/design/2026-09-20-shared-task-core.md)。

**Package admission by range (B3).** The paragraph above describes the exact-tuple handshake, which is superseded.
A package now declares `runtime_abi`, the kernel contract numbers and the south contracts it speaks, and a host
admits it with `compatibility_admits` against a `HostRangeV1`: the same epoch, the same kernel contract numbers, a
contract version the host decodes, and `south_runtime` between the host's floor and its own runtime. So a release no
longer re-stamps every package, and a package whose content is unchanged keeps its identity. A package's
`south_runtime` is the oldest runtime it needs, not the release that carries it, and release CI loads each package
under exactly that runtime's release tag (boundary record §13.6), so a host that has not re-pinned still admits every
package that needs nothing newer. A host loading a
directory with `load_package_set` judges each package on its own and follows three rules:

- a refused package makes only its own families unavailable, and startup continues;
- a refused or missing package never falls back to a native reference implementation, which is gate ②'s judge and
  not a stand-in;
- when two admitted packages of one world declare the same family, neither serves it unless the operator pins one
  by its `component.wasm` digest. Load order never breaks the tie.

A release that tightens gate ① or ② says so, and a host enforces the tightening by raising its floor. Design record:
`docs/design/2026-09-30-host-zero-vendor-boundary.md` §8.

**Declared query parameters, quota headers and user-agents (B7a).** These three provider instances move from
closed south sets into the package manifest, so a provider that needs a new one is a new package, not a south
release and a host re-pin. A provider package may declare `query_parameters` (a restricted name plus one value
syntax: `digits`, `token`, `date` or `{"enum": [...]}`), `quota_headers` (a response header feeding one of the
nine closed quota metadata fields), and a per-family `user_agent` under the controlled user-agent grammar. Gate ①
refuses credential-shaped and fixed query names, credential-bearing or framing response headers, and values outside
the grammar. `DeclaredInstancesV1` in `south-component-conformance` validates the manifest and returns the contract
types: `QueryParameterV1::Declared` (HTTP contract 10), `ProviderQuotaHeaderMapV1` (quota metadata contract 2),
and `DeclaredUserAgentV1`, which fills the request's single user-agent slot beside the `'static`
`ControlledUserAgentV1`. Query values still never come from credential resolution. Design record:
`docs/design/2026-09-30-host-zero-vendor-boundary.md` §10.
