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
| `south-provider-conformance` | Implemented immutable provider-call, provider-stream, provider-quota-metadata, header-auth, controlled-query, controlled-user-agent, provider-get, provider-multipart, provider-binary, and provider-media-binary v1 fixtures, the safe-fetch v1 fake-network fixtures, the host-implemented credential-recipe v1 suite (harness, fake token endpoint, runner), and the host-implemented north-passthrough-delivery v1 suite (harness over the host's pass-through executor, per-call verdict scripts, timeline runner; from 0.55.0) |
| `south-testkit` | Implemented assembled-executor conformance runners and reference executors for all ten suites, the reference pass-through delivery executor with its deliberate mistakes (from 0.55.0), the safe-fetch v1 runner with its fake resolver, connector and proxy environment and a reference executor, plus the owned raw-call, host-signed raw-call, raw-GET and raw-multipart builders for host tests |
| `south-provider-api` | Implemented v2 provider component ABI: WIT package `token-station:adapter@2.0.0` (world `provider-adapter-v2`) plus the gate-① manifest schema with the seven-field compatibility tuple; depends on no other south crate by design |
| `south-component-conformance` | Implemented gates ① and ② (package admission + `south.provider-component.v1` behavior suite) with the native `provider-openai-compatible`, `provider-anthropic` and `provider-gemini` references and a frozen fixture pack each (the Responses package's two, `fixtures-openai-responses` and `fixtures-openai-codex` (from 0.56.0), judged as one; the Anthropic package's two, `fixtures-anthropic` and `fixtures-claude-code` (provider-anthropic 1.2.0), likewise); a sanctioned typed consumer of the Canonical IR, pinned to a kernel distribution tag |
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
south-provider-conformance ---------------> south-host-grammars
south-testkit ----------------------------> south-contracts
south-testkit ----------------------------> south-core
south-testkit ----------------------------> south-host-grammars
south-testkit ----------------------------> south-provider-api
south-testkit ----------------------------> south-provider-conformance
south-component-conformance --------------> south-contracts
south-component-conformance --------------> south-provider-api
south-component-conformance --------------> token-station-protocol (kernel tag)
south-component-conformance (sandbox) ----> south-provider-runtime
south-provider-runtime -------------------> south-provider-api
south-north-codec ------------------------> token-station-protocol (kernel tag)
```

`south-host-grammars` depends on nothing. Besides the fuzz binary (a test-only consumer), the two host-side
conformance crates depend on it, because the pass-through delivery suite cuts frames exactly where a host must
(`south-provider-conformance`, and the reference executor in `south-testkit`; NC-1 record §3.2). No component-linked
crate may depend on it.

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

This library carries its own Rust version, 0.1.0. The component runtime and the workspace-versioned crates are at v0.67.0; `south-contracts`, `south-provider-api` and `south-component-conformance`, which every component links, carry versions of their own (currently 0.56.0, 0.59.0 and 0.67.0, the last unreleased) so that a release which leaves them unchanged re-identifies no package (boundary record §13.12, §16 Q47).
**Unreleased (`provider-openai-compatible` 2.8.0: the T9 cap field)**: design record
`docs/design/2026-10-15-openai-compatible-t9.md` §4 D6 (revision of 2026-10-15, under lv's directive of the same day).
Native sends an `azure_openai` row's output cap as `max_completion_tokens` unless the row opts out, so the two Azure
OpenAI families, `azure-openai-v1` and `azure-openai-dated`, now write the cap under the config key `max_tokens_field`
(`enum` of `max_completion_tokens` and `max_tokens`, default `max_completion_tokens`, native's); a host migrating a row
copies the row's `max_tokens_field`, else `max_tokens` when the row cleared `accepts_max_completion_tokens`. A value
outside the two is a configuration error (500) before any request is built. Every other family keeps `max_tokens`.
Gate ② gains three request rows and the dated row now expects `max_completion_tokens`. The record also corrects §2.1
(native `glm_coding` serves Responses through the Chat fallback rather than refusing it). T9-Q9 (a): a client's
`thinking` or `enable_thinking` is forwarded verbatim, only when sent, on `openai-compatible`, `azure-openai-v1`,
`github-copilot`, `azure-openai-dated` and `glm-coding`, whose native types send the client body (not on
`gemini-openai-compatible`); gate ② gains four more rows. Q47 against v0.67.0: `south-component-conformance` 0.67.0; `provider-openai-compatible`
takes the minor 2.8.0 and every other package a patch with unchanged behaviour, each keeping its `south_runtime` and
contracts. No contract number changes; the kernel pin is unchanged.
**v0.67.0 (speech on Azure OpenAI, the Veo on Vertex poll prefix, `immutable_request_headers`)**: release record
`docs/design/2026-10-15-release-0.67.0.md`.
`speech-openai-compatible` 1.1.0: the Azure OpenAI families: design record
`docs/design/2026-10-15-speech-azure-openai.md` (accepted under lv's 2026-10-15 ruling; AO-Q1 to AO-Q7 at their
defaults). Speech on Azure OpenAI and Azure Foundry rows, which native serves through the same OpenAI-compatible TTS and
ASR arms with an Azure URL and the `api-key` header, becomes two families of `speech-openai-compatible`, named as the
provider world names them: `azure-openai-v1` (`audio/speech`, `audio/transcriptions`, `audio/translations` below
`{endpoint}/openai/v1`) and `azure-openai-dated` (`openai/deployments/{deployment}/audio/…?api-version=`, the
deployment one escaped segment, the config key `api_version` with `provider-openai-compatible`'s declaration byte for
byte, default `2024-10-21`; an undated value is a 500, an empty deployment a 400). Both present the key as
`header_secret` `api-key`; bodies, parts, metering and readers are `openai-compatible`'s. Not to be confused with
`speech-azure` (the Azure Speech service). Q47 against v0.66.0: `south-component-conformance` 0.66.0;
`speech-openai-compatible` takes the minor 1.1.0 and every other package a patch with unchanged behaviour, each keeping
its `south_runtime` and contracts. No contract number changes.
`task-veo-v2` 0.36.1: the Vertex poll prefix (Q-VV1, design record `docs/design/2026-10-15-veo-vertex-task-component.md`).
A Vertex operation name of the shape `projects/*/locations/*/publishers/google/models/{m}/operations/{id}` is polled
under the binding's own location prefix, so a name carrying the project number stays inside the authorized endpoint;
a matching name gives native's URL unchanged, other shapes are polled verbatim. Unverified: no capture of a Vertex Veo
create response.
`immutable_request_headers` (Q-CC3, design record `docs/design/2026-10-15-immutable-request-headers.md`):
`south-provider-api` 0.59.0 adds a per-family provider-world manifest declaration, shaped like `immutable_body_paths`,
of the request headers a host's extra request configuration must never set; lower-case names, no implicit members.
`provider-anthropic` 1.2.1 declares nine for `claude-code` and `claude-code-setup-token` and so declares
`south_runtime` 0.67.0, the first runtime that parses the field. `ManifestErrorV1` gains
`InvalidImmutableRequestHeaders`, so a host matching it exhaustively adds the arm when it re-pins.
**v0.66.0 (`provider-vertex`, Veo on Vertex, `image-xai`, `image-reve`, the Claude Code families, Bedrock Mantle, T9,
`speech-vertex` 1.1.0)**: release record `docs/design/2026-10-15-release-0.66.0.md`; every change below was written
under lv's 2026-10-15 directive (migrate by the native code, doubts registered as unverified).
`task-veo-v2`: the `vertex-veo-video` family: design record
`docs/design/2026-10-15-veo-vertex-task-component.md` (accepted and revised 2026-10-15: VV-Q1 (b), VV-Q2 (b), VV-Q3;
"Implementation"). `task-veo-v2` gains a second family that reproduces the host's native Vertex Veo arm, not Google's
documented Vertex wire (an **unverified** doubt the record registers):
`POST {base}/publishers/google/models/{model}:predictLongRunning` under the location prefix the host binds, with the
Gemini line's body; `GET {origin}/v1/{operation name}` for a `projects/…` name; the Gemini LRO shape
(`generateVideoResponse.generatedSamples[].video.uri`) as a credential-gated URL artifact; the host-minted token as `bearer` (the Gemini family keeps `x-goog-api-key`). A create response is accepted
with either line's operation name, a `projects/…` name up to the wire's upstream-id bound; each family polls only its
own names and locator route; a client `storageUri` is neither refused nor sent; an undeclared family is refused. The
manifest gains the provider and the `bearer` arm, keeps task contract 7 and its `south_runtime`, and the package takes a
minor version (0.36.0, past `provider-vertex`'s patch 0.35.24 in this cycle, so no further Q47 round). No contract
changes. Per L-Q2 (lv 2026-10-14) the family re-launches Vertex Veo after the native line goes offline; the gated clip
fetch has no native baseline.
`provider-openai-responses` 1.4.0: the `bedrock-mantle` family: design record
`docs/design/2026-10-15-bedrock-mantle.md` (accepted under lv's 2026-10-15 directive; BM-Q1 to BM-Q5 at their defaults).
AWS Bedrock's OpenAI-compatible host becomes a third family of `provider-openai-responses`, whose behaviour is
`openai-responses`'s unchanged; it adds the endpoint template `https://bedrock-mantle.{region}.api.aws` with one required
`aws_region` key, the native arm's derivation of the host. Pack `fixtures-bedrock-mantle/`, transcribed from the host's
native Mantle tests rather than captured; a trailing `data: [DONE]` on the Responses stream is refused, as native
refuses it. The chat surface stays servable by `openai-compatible` with the origin as row endpoint (two rows in
`fixtures/`). No further Q47 step: `south-component-conformance` is already 0.65.0 in this cycle; `south_runtime` stays
0.60.0.
`image-reve` 1.0.0: the Reve bridge, image world step I3-6: design record
`docs/design/2026-10-13-image-i3-6-reve.md` (accepted 2026-10-13; RV-Q0 lifted 2026-10-15). `image-reve` (image world,
family `reve-image`, `bearer` on `provider_api_key`, `generate` and `edit`) reproduces the host's two OpenAI-surface Reve
bridges: a JSON generation to `v1/image/create`, a multipart edit sent as JSON to `v1/image/edit` or `v1/image/remix` with
the images inlined through `base64` references, the endpoint read from the route's `endpoint` pin. It meters `credits`
only (`credits_used`, at most six decimal places, never negative; otherwise `unknown`) and is the first package to return
`charged_failure` (a content violation, content policy 400, with its credits). `contracts` `{image: 1, media: 1}`,
`south_runtime` 0.57.0. The image suite's `missing_meter_is_not_zero` now also checks `charged_failure` for every
required fact of §9.1's last column (`credits`, `images_reported`, declared token buckets); no published package returns
the outcome, so none changes. No version moves: `provider-vertex` already moved `south-component-conformance` to 0.65.0
in this cycle. No contract number changes.
`speech-vertex` 1.1.0: Q7's refusal: speech batch V4 record `docs/design/2026-10-15-speech-v4-tts.md`
§3.4 (lv's Q7 ruling of 2026-09-30; lv moved 1.1.0 into 0.66.0 on 2026-10-15 instead of waiting for the host's Vertex
TTS dual run). `response_format` `mp3`, `opus`, `aac` and `flac`, which native and 1.0.x serve as WAV, are
`invalid_request` before dispatch ("Gemini TTS on Vertex cannot emit response_format='{f}'. Supported: wav, pcm.");
the "Unknown …" text drops its parenthesis; an omitted format stays WAV. The five 1.0.x pack cases for those values
become refusal cases. A host dual run on 0.66.0 sees the refusal as a registered intentional difference from native. A
behaviour change, so a minor version (1.0.1 → 1.1.0); the manifest changes nothing else and keeps `south_runtime` 0.65.0
and `{media: 1, speech: 1}`. No crate version moves: `provider-vertex` already did this cycle's Q47 round.
`provider-vertex` 1.0.0: Gemini models on Vertex AI for text: design record
`docs/design/2026-10-15-provider-vertex.md` (accepted 2026-10-15; VX-Q1 (a), `generateContent` on every surface; VX-Q2,
family `vertex-ai`). The Gemini reference's request body, response reader, stream parser (with SF27) and refusals move
into the crate-private module `gemini_text`, which `provider-gemini` and the new `provider-vertex` both call; each
reference keeps only its family check, route and auth (`provider-gemini`'s fixed `config.provider != "gemini"` becomes the
per-package `gemini_text::serves`). `provider-gemini`'s behaviour and fixtures are unchanged. `provider-vertex` (provider
world, `chat` / `stream` / `tool_call`, `bearer` on the `vertex_sa`-minted slot `provider_api_key`) builds the Vertex
path with the shared `vertex_location` rules, declares `image-vertex`'s configuration keys and credentials section byte
for byte, `north_request_passthrough` and `north_passthrough` `generate_content`, request facts with the model at
`/publishers/google/models/{model}:`, and `south_runtime` 0.60.0, the first runtime that parses
`north_request_passthrough`. Gate ② pack `fixtures-vertex/` (with `image-vertex`'s five credential cases). Q47 against
v0.65.0: `south-component-conformance` 0.65.0; every package published with 0.65.0 takes a patch with unchanged
behaviour and keeps its `south_runtime` and its contracts. No contract number changes; the kernel pin is unchanged.
`image-xai` 1.0.0: image world step I3-7, xAI Grok Imagine: design record
`docs/design/2026-10-15-image-i3-7-xai.md` (accepted under lv's 2026-10-15 directive). `image-xai` (family `xai-image`,
`bearer`, `generate` and `edit`, contracts `{image: 1, media: 1}`, `south_runtime` 0.57.0) serves the six Grok Imagine
ids of the native arm. Both operations are JSON; a body with a non-null source-image field is an edit (XI-Q2). The
client's body passes through as the native OpenAI-compatible arm forwards it, with `model` set, `size` removed and
`resolution` normalised; on an edit `image_url` / `image_urls` fold into `image` / `images`. It is the first package to
report tier candidates and the `upstream_cost` evidence fact (record §8, §9.1, §9.4 item 4): `resolution` always,
`quality` when `grok-imagine-image-2.0` pins it, else the candidates `low` / `medium` with the surface's default, and
`usage.cost_in_usd_ticks` as decimal dollars; the host recognises the served tier and bills its own tier-keyed price
list (H5). Gate ② pack `fixtures-image-xai/`. No Q47 bump of its own: the reference lands in the same
`south-component-conformance` 0.65.0 cycle. No contract number changes; the kernel pin is unchanged.
`provider-openai-compatible` 2.7.0: host plan row T9, `glm-coding` and `azure-openai-dated`: design
record `docs/design/2026-10-15-openai-compatible-t9.md` (accepted under lv's 2026-10-15 directive). Two families
transcribed from the host's native arms. `glm-coding` (Z.ai GLM Coding Plan): Chat Completions at
`{base_url}/api/coding/paas/v4/chat/completions` below the native origin endpoint, `bearer`, the manifest's
`user_agent` `opencode/1.15.6` for this family alone, and a `subscription_quota_families` entry; native's Messages leg
to Z.ai's Anthropic-compatible endpoint is not reproduced (a registered difference). `azure-openai-dated`: Azure
OpenAI's deployment-addressed surface, `{base_url}/openai/deployments/{deployment}/chat/completions?api-version={version}`
with the deployment as one escaped segment, `header_secret` `api-key`, the version from the config key `api_version`
(`api_version_date`, default `2024-10-21`, native's), an undated value refused as a configuration error. Both share the
package's translation; an undeclared family is still refused. Gate ② gains six request rows in `fixtures/`. The package
takes a minor version (2.7.0, past its patch 2.6.10 in this cycle, so no further Q47 round) and keeps `south_runtime`
0.60.0. No contract changes; the kernel pin is unchanged.
**v0.65.0 (the speech world: batches V3-1 to V4 and `contracts.speech` 1; `contracts.media` 3; Kiro native rewrites)**:
release record `docs/design/2026-10-15-release-0.65.0.md`. Speech world step S-S-5, `contracts.speech` 1: speech record
`docs/design/2026-09-30-speech-world.md` §14, §18 and "S-S-5 implementation" (released in 0.65.0 with V3-1 to V4).
`compatibility.json` records `contracts.speech` 1 (the speech module of `south-contracts` as released in 0.64.0 plus the
`url` form's optional `upgrade_insecure` member of V4, SV-Q10; no number moves), the suite fields
`speech_component_v1_suite_id` `south.speech-component.v1` and `speech_component_v1_suite` 1, and a `speech_v1` entry in
`media_component_capabilities`, `not_verified` for both hosts. As ruled for Q9 (the same ruling as image Q5 and
embeddings E-Q5), South states in writing that **a synchronous speech implementation in the community host is
recommended** (P21 §7); until a host lands it, `speech_v1` stays `not_verified`, and a gate ② pass of the packages is not
host adoption. All nine speech packages declare `contracts` `{"media": 1, "speech": 1}` (`speech-elevenlabs`
`{"media": 3, "speech": 1}`) at their unpublished 1.0.0. No published runtime records the `speech` key, so a host
linking 0.64.0 or older refuses every speech package; each declares this tree's release as `south_runtime`, moved by the
release commit with the workspace version. A host admits them with a range listing `speech` 1 beside `media` `{1, 2,
3}`. No crate version moves (this cycle already moved the three guest-linked crates), and no other package changes.
`speech-vertex` 1.1.0 (Q7's refusal) is not part of this release.
Speech world batch V4: Bailian, Vertex and Azure SSML TTS; SV-Q10, SV-Q11: design record
`docs/design/2026-10-15-speech-v4-tts.md` (accepted 2026-10-15, SV-Q10 and SV-Q11 as recommended; released in
0.65.0, with V3-1 to V3-3). Two new synthesis packages, each 1.0.0, media 1 and `south_runtime` 0.64.0: `speech-qwen`
(family `bailian-qwen-speech`, `bearer`, `artifact_fetch`; the audio is a `url` artifact) and `speech-vertex`
(`vertex-ai-speech`, `image-vertex`'s configuration keys and `vertex_sa` credentials section byte for byte; an `inline`
artifact in the `wav_pcm_s16le` container; Q7's silent downgrade of `mp3` / `opus` / `aac` / `flac` to WAV until the dual
run is accepted). `speech-azure` gains SSML synthesis at its unpublished 1.0.0 (a `text` body over `TextPostRequestV1`,
native's template and escaper, `x-microsoft-outputformat` from native's table, a document above 1 MiB refused before
dispatch). Contract (inside `contracts.speech` 1, which S-S-5 records first, so no number moves): the `url` artifact
form gains the optional `upgrade_insecure` member and `south-contracts` the pure function
`upgrade_insecure_artifact_url_v1` (an `http://` prefix, any case, becomes `https://`; nothing else changes), which a
host applies before the unchanged `ArtifactUrlV1` grammar and safe fetch rules (vectors `artifact_url_upgrade` in
`speech-v1.json`). Suite: `second_hop_artifact` applies the upgrade first; `ssml_escaping` compares `input` less the
characters XML 1.0 forbids and requires a voice holding an XML special character only as an attribute substring, and
`ssml_check.rs` refuses those forbidden characters raw or as references (SV-Q11). Shared modules: the DashScope error
reader moves into `bailian_image.rs` (`image-qwen` unchanged), Google's error envelope and policy table become
crate-visible in `gemini_image.rs`, `speech_shared.rs` gains the `url` and declared-type `body` successes. No crate
version moves: this unreleased cycle already moved the three guest-linked crates.
Speech world batch V3-3: OpenAI-compatible, xAI, ElevenLabs and Azure ASR: design record
`docs/design/2026-10-14-speech-v3-3-asr.md` (accepted 2026-10-15, SV-Q8 and SV-Q9 as recommended; released in
0.65.0, with V3-1 and V3-2). Recognition arrives as a later version of each family's package (SV-Q1 (b)); the three
V3-1 packages are unpublished, so they gain it at 1.0.0: `speech-openai-compatible` `transcribe` and `translate` (the
latter on a row carrying `speech.translate`), `speech-xai` and `speech-elevenlabs` `transcribe`; the new `speech-azure`
1.0.0 (family `azure-speech`, `header_secret` `ocp-apim-subscription-key`, media 1, `south_runtime` 0.64.0) transcribes
only. Every package reads the host's multipart view, refuses a body that is not multipart, an elided text part, a
missing `file` part and `stream=true` before dispatch, rewrites a part list instead of the native arms' byte surgery,
declares `immutable_form_fields`, and reports `seconds` by the value rule (a JSON number above 0; `0`, negative or
malformed is `unknown`). A `speech.verbose_json` row asks for `verbose_json` and renders every client format itself
(`srt` / `vtt` from the segments); ElevenLabs renders from its words (cues by SV-Q8, `verbose_json` by SV-Q9) and bills the
last word's end as `last_timestamp`; Azure keeps native's formats and refusals. The shared pieces are the crate-private
`asr_shared.rs`; the suite now reads the client format trimmed (record §10 decision 3). No contract changes and no crate
version moves: V3-1 already moved `south-component-conformance` in this unreleased cycle.
Speech world batch V3-2: MiMo and MiniMax TTS: design record
`docs/design/2026-10-14-speech-v3-2-tts.md` (accepted 2026-10-14, SV-Q7 as recommended; released in 0.65.0,
with V3-1). Two synthesis-only speech packages, each 1.0.0, media 1 and `south_runtime` 0.64.0, transcribed from the
host's native TTS arms: `speech-mimo` (family `mimo-speech`, `bearer`; `v1/chat/completions` with the input as the
assistant turn and an `audio` object, the base64 audio of `choices[0]` as an `inline` artifact) and `speech-minimax`
(`minimax-speech`, `bearer`; `v1/t2a_v2` with `?GroupId=` from the digits config key `group_id`, the hex `data.audio`
as an `inline` artifact, `base_resp` failures decided in the component, the weighted character count as `upper`, and
`extra_info.usage_characters` as the upstream's count; `0` or a malformed count is `unknown`, SV-Q7). The `base_resp`
table moves to the crate-private `minimax_shared.rs`, which `image-minimax` reads unchanged. No contract changes, and
no crate version moves again: V3-1 already moved `south-component-conformance` to 0.64.0 in this unreleased cycle.
Speech world batch V3-1: OpenAI-compatible, xAI, Groq and ElevenLabs TTS; `contracts.media` 3: design
record `docs/design/2026-10-14-speech-v3-1-tts.md` (accepted 2026-10-14, SV-Q1 to SV-Q6 as recommended; released in
0.65.0). Four synthesis-only speech packages, each 1.0.0 and transcribed from the host's native TTS arm:
`speech-openai-compatible` (family `openai-compatible`, `bearer`; the client's JSON to `audio/speech` under the API
root, `binary` audio billed by request characters, or `stream_format: "sse"` with a `segments` artifact over the
`speech.audio.delta` events and `tokens` from `speech.audio.done` when the host requires `tokens`), `speech-xai`
(`xai-speech`, `v1/tts`), `speech-groq` (`groq-speech`, Orpheus personas) and `speech-elevenlabs` (`elevenlabs-speech`,
`header_secret` `xi-api-key`, `v1/text-to-speech/{voice_id}` with a voice-id grammar refusal, SV-Q4). The artifact is
typed by the upstream's `content-type` with `audio/mpeg` as fallback (SV-Q2); an empty 2xx, a 3xx and an invalid token
count are `unknown`. `contracts.media` 3 (additive): a media descriptor's query may name a parameter the same package
declares in `query_parameters` (`MediaRequestDescriptorV1::parse_with_declared`,
`parse_prepared_speech_call_with_declared_v1`; `minimum_contract_version` 3), and gate ① refuses a speech manifest that
declares `query_parameters` under media below 3 (`ManifestErrorV1::SpeechQueryParametersRequireMedia3`, SV-Q5). The
suite and the sandbox seam parse prepared calls with the package's own declarations. Three packages declare media 1 and
`south_runtime` 0.64.0; `speech-elevenlabs` declares media 3 and this tree's release, moved by the release commit.
`contracts.speech` is not recorded by this batch (S-S-5 records it, above). Under Q47 `south-contracts`
moves to 0.56.0, `south-provider-api` to 0.58.0 and `south-component-conformance` to 0.64.0, so every package published
with 0.64.0 takes a patch with unchanged behaviour and keeps its `south_runtime` and its `contracts`. **Breaking for
hosts on re-pin**: `ManifestErrorV1` gains `SpeechQueryParametersRequireMedia3` (exhaustive matches), and a host's media
range moves to `{1, 2, 3}` to admit `speech-elevenlabs`.
The Kiro native rewrites: KR2–KR5 withdrawn: Kiro record
`docs/design/2026-09-30-kiro-provider-component.md` §17 and §18 (lv, 2026-10-14: the component must not refuse a
request the native arm serves today). `provider-kiro` 1.1.0 makes the native arm's rewrite where 1.0.x refused: an
unmodelled content part is read by its `type` as the native second hop reads the block the first hop sends verbatim
(text kept, `tool_use` an assistant `toolUses` entry, `tool_result` a user `toolResults` entry, `image` refused,
anything else dropped), `response_format` is not read, `required` and a named tool choice send the tools as `auto`
does, and a final assistant turn is sent as the current user message. Every request row was checked against the
native composition. Under the same ruling an image in a system message or a tool result is dropped as natively (KD15
withdrawn); KR1 refuses images in user and assistant messages only. No WIT, manifest
vocabulary, runtime, suite or `compatibility.json` change. No version moves beyond the package's own: speech V3-1 already
moved the guest-linked crates and patched every package published with 0.64.0 for this cycle.
**v0.64.0 (speech world steps S-S-1 to S-S-3; `task-minimax-v2` immutable body paths; image refuses an `sse` response form)**:
release record `docs/design/2026-10-14-release-0.64.0.md`. The speech world's contract, world and suite ship without
`contracts.speech` and without packages (S-S-4 / S-S-5), so no host behaviour changes until a host adopts them.
Speech world step S-S-1, the `speech` contract module: speech record
`docs/design/2026-09-30-speech-world.md` §18 and its S-S-1 implementation section. `south_contracts::speech` carries
`contracts.speech` v1's types and codecs: the pre-dispatch facts (`characters {count, upper?}`, `metering_forms`,
`reservation`, `output_media_type`), `SpeechMeteringV1` (characters with `source`, seconds with `basis`, tokens),
`SpeechArtifactV1` (`body`, `inline`, `segments`, `url`, an optional `wav_pcm_s16le` container), the transcript
deliveries, the two dialect words, `immutable_form_fields` and `PreparedSpeechCallV1`. The outcome enum lands with the
component trait in the conformance crate (S-S-3), as the image world's did. No WIT, manifest, runtime or suite change and
no `compatibility.json` entry yet. Under Q47 `south-contracts` moves to 0.55.0 and `south-component-conformance` (whose
reference version constants move) to 0.63.0, so every package published with 0.63.0 takes a patch with unchanged
behaviour and keeps its `south_runtime` and its `contracts`.
Speech world step S-S-2, the `speech-adapter-v1` world: speech record §18 and its S-S-2 implementation
section. `wit/speech-adapter.wit` (`token-station:speech-adapter@1.0.0`, world `speech-adapter-v1`: `metadata`,
`healthcheck`, `model-capabilities`, `prepare`, `parse-response`; no `host` import), the `SPEECH_*` constants and
`SPEECH_WORLD_SCHEMA` (capabilities `synthesize`, `transcribe`, `translate`, `artifact_fetch`; auth arms `bearer`,
`header_secret`). Gate ① requires `synthesize` or `transcribe` (`SpeechOperationRequired`), refuses `translate`
without `transcribe` (`TranslateRequiresTranscribe`) and `artifact_fetch` without `synthesize`
(`ArtifactFetchRequiresSynthesize`), three new `ManifestErrorV1` variants; it admits `config_schema` and
exported credential attributes in this world, and `query_parameters` alone: `WorldSchemaV1::instance_declarations` is
now the three-valued `InstanceDeclarationsV1` (`quota_headers` and `user_agent` stay provider-only). The runtime gains `InstanceKind::Speech` and
`call_speech_model_capabilities`, `call_speech_prepare`, `call_speech_parse_response`, with the import scan of a pure
world and unchanged limits. Gate ②'s `UndeclaredValuesIgnored` row for the `declared`-only worlds is one shared function
(`undeclared_values.rs`) that the embeddings, image and speech suites call. No
`compatibility.json` entry yet. Under Q47 `south-provider-api` moves to 0.57.0; the packages already took this release's
patch in S-S-1 and take no second one.
Speech world step S-S-3, the suite `south.speech-component.v1` and the `sse` view: speech record §11,
§18 and its S-S-3 implementation section. `south-contracts` builds the `sse` response view from the events a host
decoded with `decode_sse_v1` (`build_media_sse_response_view_v1`, `SseEventFieldsV1`): the list `[{event, data}]` is
elided as one document whose root is the array (`{"sse": [...]}`, the new `MediaResponseBodyV1::Sse`, breaking for an
exhaustive match), so declared paths read `/*/data/delta`; `build_media_response_view_v1` still refuses the form, so the
image world is unchanged. Host-obligation vectors for the WAV container and the `sse` view are in
`tests/vectors/speech-v1.json`. The conformance crate gains `SpeechComponentV1`, `SpeechOutcomeV1` (with
`SpeechDeliveryV1`: audio or transcript), the outcome codec, the ABI shim, the sandbox seam, `speech-v1.*` fixture packs
and `run_speech_component_suite_v1`, with six additive `CheckV1` variants (`AbsentDurationIsNullOrUnknown`,
`ClientFormatRendering`, `UndeclaredOperationRefused`, `SsmlEscaping`, `SecondHopArtifact`, `EmptyAudioIsUnknown`)
beside the image world's rows, the metering rows included. Its native reference is synthetic (family
`synthetic-speech`, a test double that no host routes to; the real packages are S-S-4), runs as the test guest
`tests/guests/synthetic-speech` with byte-identical ABI answers, and every row fails on a mutated reference. No
`compatibility.json` entry yet and no version moves: both changed crates already moved in this unreleased cycle.
`task-minimax-v2` immutable body paths: design record
`docs/design/2026-10-14-task-minimax-immutable-paths.md` (ruled by lv on 2026-10-14, host step H5-5c). `task-minimax-v2`
prepared `immutable_body_paths: null`, so a host injected no request extras for it. It now declares the host-native
MiniMax arm's protected list for both generations: `model`, `duration`, `resolution`, `content`, `first_frame_image`,
`last_frame_image` (a path a body does not carry is legal under contract 6 and keeps an extra from adding it).
The package takes a minor version, 0.33.0 (0.32.0 and the S-S-1 patch 0.32.1 retire), and keeps `south_runtime` 0.63.0 and
`contracts.task` 7. No crate version moves: `south-component-conformance` already moved to 0.63.0 since v0.63.0.
**v0.63.0 (image world step I3-3: `image-vertex`; the task poll interval)**: release record
`docs/design/2026-10-14-release-0.63.0.md`. Image world step I3-3: Nano Banana on Vertex AI (design record
`docs/design/2026-10-13-image-i3-3-vertex.md`, accepted 2026-10-14). `image-vertex` 1.0.0
(family `vertex-ai-image`, `bearer` on the slot `provider_api_key`, which the `vertex_sa` credential recipe mints) is the
first image package with a credential recipe; its `config_schema` (`region`, optional `project`) and `credentials` section
are `embeddings-vertex`'s byte for byte, and the two references share the crate-private location rules
`vertex_location`. It serves generation only, `POST v1/projects/{project}/locations/{region}/publishers/google/models/
{model}:generateContent` with each segment percent-encoded; the body, `n` as `repeat`, the response reader, metering and
render are `image-gemini`'s shared module. It declares `contracts` `{image: 1, media: 2}` and `south_runtime` 0.59.0, the
first runtime with media 2. No contract or `south-provider-api` change: the image world has admitted `config_schema` and
exported credential attributes since 0.52.0. Under Q47 `south-component-conformance` moves to 0.62.0, so every package
published with 0.62.0 takes a patch with unchanged behaviour and keeps its `south_runtime` and its `contracts` (except
`task-minimax-v2`, below).
The task poll interval (V-Q8): design record `docs/design/2026-10-14-task-min-poll-interval.md` (ruled V-Q8
(a) by lv on 2026-10-14). A task package (`task-adapter-v1` or `task-adapter-v2`) may
declare `min_poll_interval_seconds`, the shortest interval in whole seconds between two observation requests a host
sends for one of its tasks, `1..=600` inclusive (`MIN_TASK_POLL_INTERVAL_SECONDS`, `MAX_TASK_POLL_INTERVAL_SECONDS`);
absent means the host's own default. Gate ① refuses it in any other world and outside the bounds. It moves no contract
number; a package declaring it needs the runtime that parses it. `task-minimax-v2` declares 12 s for both MiniMax
generations; no other task package declares one. `task-minimax-v2` 0.32.0 declares
`south_runtime` 0.63.0. Under Q47 `south-provider-api` moves to 0.56.0.
**v0.62.0 (the Kiro step: `provider-kiro`; task contract 8 and `task-gemini-omni-v2`)**: release record
`docs/design/2026-10-14-release-0.62.0.md`. The Kiro step: the Kiro upstream as a package (design record
`docs/design/2026-09-30-kiro-provider-component.md`, accepted 2026-10-13 with six rulings provisional until a
real-account capture). `provider-kiro` 1.0.0 (family `kiro`, `bearer` on the minted slot
`provider_api_key`) translates the IR straight to the `generateAssistantResponse` `conversationState` body, one history
entry per message, with `conversationId` from the host's `attempt_id` and `profileArn` from the credential's exported
attribute; it declares `stream_framing: aws-eventstream`, `request_facts.stream: "none"` (the buffered path for
non-streaming callers), `output_cap: []`, `usage_evidence: absent`, the `user-agent` the native arm sends, the
selector `kiro` over the two rotating refresh recipes `social` and `idc`, and `subscription_quota_families`
(`south_runtime` 0.60.0); #207 sends tool-call arguments exactly as the native arm does. It ships as 1.0.1 (contract
8's Q47 bump; 1.0.0 was never published). Under Q47 `south-component-conformance` moved to 0.60.0, so every published
package takes a patch with unchanged behaviour and keeps its `south_runtime` and its `contracts`.
Task contract 8 and `task-gemini-omni-v2`: design record
`docs/design/2026-10-01-omni-task-component.md` (accepted 2026-10-13, provisional items pending a real-account test).
Task contract 8 is additive: `TaskArtifactRefV2::Inline` names an artifact whose bytes
are a string in the upstream response by a pointer into the view the host elided with `elide_v1` and the package's
`response_blob_paths`; `TaskUsageFactsV2` gains declared token buckets from a closed fourteen-word vocabulary (the image
world's nine plus `audio_input`, `video_input`, `audio_output`, `video_output` and `reasoning_output`, the last outside
`total_output`); the request estimate gains `bucket_ceilings`, which may only tighten the host's bound. Every new key is
optional and written only when present, so a contract 7 value decodes and re-encodes unchanged and the published task
packages keep declaring `task: 7`; a host decodes both (`TASK_CONTRACT_VERSIONS`). Gate ① admits `response_blob_paths`,
`token_buckets` and `required_token_buckets` in the task-v2 world with contract 8 only; gate ② adds
`DeclaredBucketsOnly` and runs `ReferenceIntegrity`, `UsageNeverDefaulted` and `MissingMeterIsNotZero` on contract 8
packages; `resolve_task_inline_artifact_v1` and `fill_task_artifact_markers_v1` are the host's pure halves. The WIT is
unchanged. `task-gemini-omni-v2` 0.1.0 serves Gemini Omni video over the Interactions API in two families:
`vertex-interactions-video` (sampled: `background` submit, poll by id, inline clip, bearer) and
`gemini-interactions-video` (provisional, the host's native arm: synchronous, Files API `uri` fetched with
`x-goog-api-key`, text-to-video only). It declares `contracts` `{task: 8, media: 1}` and `south_runtime` 0.62.0, the
first runtime that decodes contract 8. Under Q47 the three guest-linked crates move to 0.54.0, 0.55.0 and 0.61.0 (one step past the
Kiro step's 0.60.0), so every package takes a patch with unchanged behaviour and keeps its `south_runtime` and its `contracts`.
**v0.61.0 (image world step I3-5: `image-ideogram`; north codec G1)**: release record
`docs/design/2026-10-13-release-0.61.0.md`. North codec G1 (`docs/design/2026-09-30-north-codec-render-gaps.md`, G1
accepted 2026-10-14): `OpenAiChatStream` renders complete OpenAI chat stream chunks from a host-supplied context (chunk
identity, `role` on the first delta, frame kinds, the usage fold; N-Q5 = JSON-value equality over the canonical frame
sequence); `south-north-codec` is not guest-linked, so G1 re-identifies no package. I3-5: Ideogram 4.0 generation (design
record `docs/design/2026-10-13-image-i3-5-ideogram.md`, accepted 2026-10-13). `image-ideogram`
1.0.0 (family `ideogram-image`, `header_secret` `api-key`) serves the three v4 ids the host's native arm maps, generation
only: a multipart POST of text parts to `v1/ideogram-v4/generate` (`text_prompt`, the `rendering_speed` the id pins, and
`resolution` snapped from `size` to the native 38-entry list, which is also the `size` fact). Each safe entry of the
answer is a `url` artifact with no declared media type (the host sniffs the bytes), delivered as `url` or, through the
host's safe fetch, `b64_json`; a non-empty answer with every entry filtered is a content-policy refusal. It meters
`images` only and declares `contracts` `{image: 1, media: 1}` and `south_runtime` 0.57.0; no contract changes. Under Q47
`south-component-conformance` moves to 0.59.0, so every published package takes a patch with unchanged behaviour and
keeps its `south_runtime` and its `contracts`.
**v0.60.0 (image world step I3-4: `image-stability`; D7: request pass-through for `generateContent`)**: release record
`docs/design/2026-10-13-release-0.60.0.md`. D7 (design record `docs/design/2026-10-14-generate-content-request-passthrough.md`,
accepted 2026-10-14): `NorthProtocolV1` gains `generate_content`, and the manifest field `north_request_passthrough`
lets a family declare that the host may send the client's own body to the URL the component builds for the probe IR
(gate ① requires the same protocol in `north_passthrough` and the Gemini wire in `request_facts`; gate ② checks that the
descriptor does not depend on the body; gate ③'s `south.north-passthrough-delivery.v2` is parameterised by protocol).
`provider-gemini` declares both for `gemini`. Riding along, `subscription_quota_families` lets a provider-world package
name its subscription families (`github-copilot` in `provider-openai-compatible`, `openai-codex` in
`provider-openai-responses`). The three packages declare `south_runtime` 0.60.0, the first runtime that parses the new
fields. I3-4 (Stability AI's Stable Image generation; design record `docs/design/2026-10-13-image-i3-4-stability.md`,
accepted 2026-10-13): `image-stability`
1.0.0 (family `stability-image`, `bearer`) serves the five ids the host's native arm classifies, generation only: a
multipart POST to `v2beta/stable-image/generate/{ultra,core,sd3}` with `accept: application/json`, the native parts in the
native order, and a client's source image (`image_url` or `image`, always elided) as the last part through
`from_data_url`. It meters `images` only (`images_reported` always `null`), delivers `CONTENT_FILTERED` like `SUCCESS`,
and declares `contracts` `{image: 1, media: 1}` and `south_runtime` 0.57.0; no contract changes. Under Q47
`south-component-conformance` moves to 0.58.0, so every published package takes a patch with unchanged behaviour and
keeps its `south_runtime` and its `contracts`.
**v0.59.0 (image world step I3-2: `image-gemini`)**: Google's Nano Banana image models through the Gemini API (design
record `docs/design/2026-10-13-image-i3-2-gemini.md`, accepted 2026-10-13; release record
`docs/design/2026-10-13-release-0.59.0.md`).
`contracts.media` 2 adds one transform word, `data_url_payload` (a base64 `data:` URL to its base64 payload, as text, so
a JSON template can carry the bare base64 of a client's data URL); `MEDIA_CONTRACT_VERSION` stays 1,
`MEDIA_CONTRACT_VERSION_V2` is 2 and `MEDIA_CONTRACT_VERSIONS` lists both, as embeddings contract 2 did, so packages
declaring media 1 keep loading on a host whose range lists both. `image-gemini` 1.0.0 (family `gemini-image`,
`header_secret` `x-goog-api-key`) posts `v1beta/models/{model}:generateContent` for generation and multipart edit, sends
`n` as `repeat` (one image per upstream call; the host's multi-round dispatch is a cutover prerequisite), meters
`tokens` (`total_input` / `total_output` required, the modality split only when it adds up) or `images`
(`images_reported` always `null`), and reports the `resolution` tier word only when the host prices the call per image
and the client sent one. The request and response pieces are the crate-private module `gemini_image`, for the Vertex
package of I3-3. It declares `contracts.media` 2 and, until the release commit, this tree's release as its
`south_runtime`. Under Q47 `south-contracts` moves to 0.53.0 and `south-component-conformance` to 0.57.0, so every
published package takes a patch and keeps its `south_runtime` and its `contracts`; the behaviour is unchanged except in
`image-minimax` 1.0.1, which also fixes what the host's dual run found in I3-1 (each `image_url` array string becomes its
own character reference, as native).

**v0.58.0 (image world step I3-1: `image-minimax` and `image-qwen`)**: the first synchronous image packages after
`image-azure` (design record `docs/design/2026-10-12-image-i3-1-minimax-and-qwen.md`, accepted 2026-10-12; release record
`docs/design/2026-10-13-release-0.58.0.md`). `image-minimax` 1.0.0 (family `minimax-image`) posts `v1/image_generation` with `bearer`, appends
`?GroupId=` only when the family's declared config key `group_id` (syntax `digits`) is set, and judges a 2xx whose
`base_resp.status_code` is non-zero as failed in the body, its envelope carrying the host's status for the code (1026 is
`unknown`). `image-qwen` 1.0.0 (family `bailian-qwen-image`) posts DashScope's synchronous multimodal generation,
reports a `size` fact only when the client sent one, and judges a 2xx carrying a DashScope `code` `unknown`; the
`image_url` parts, the `WxH → W*H` size and the forwarded parameters are shared with `task-wan-image-v2` (module
`bailian_image`, its behaviour unchanged). Both generate only, meter `images` with `images_reported` as evidence, and
declare `south_runtime` 0.57.0. Under Q47 `south-component-conformance` moves to 0.56.0, so every published package
takes a patch with unchanged behaviour and keeps its `south_runtime`.

**v0.57.0 (KC1: canonical IR 4)**: the kernel re-pin to protocol 0.6.0 (mirror `v0.5.0` = `356445c1`, upstream
`kernel-v0.6.0` = `c0bca10c`, `canonical_ir` 4; release record `docs/design/2026-10-12-release-0.57.0.md`, design record
`docs/design/2026-10-12-kc1-canonical-ir-4-south.md`). Prompt-cache markers, `top_k`, the caller's reasoning
configuration and the R-Q15 promotions cross the IR; the Anthropic Messages and Bedrock Invoke builders emit them, the
Converse packages map markers to `cachePoint` for models declaring `bedrock.converse.cache_point` (`.ttl_1h`, `.tools`),
and the other dialects drop markers without refusing. New host value `end_user_hash` (`metadata.user_id`). Every package
declares `south_runtime` 0.57.0, the first runtime that records `canonical_ir` 4.

**v0.56.0 (R3: the Codex family and the four request fields)**: step R3 of the OpenAI Responses upstream (release
record `docs/design/2026-10-12-release-0.56.0.md`; design record `docs/design/2026-10-12-responses-r3-codex-and-request-fields.md`,
accepted 2026-10-12). Released before its §3.2 staging measurement by lv's ruling of 2026-10-12: no host row may be cut
over to `openai-codex` until the measurement is recorded, and a package change it requires ships as a patch release. `provider-openai-responses` 1.2.0 serves a second family, `openai-codex`:
`{base_url}/codex/responses`, `stream: true` forced and a non-streaming request refused at build time, default
`instructions`, no `temperature` or `max_output_tokens` (`output_cap: []`), `chatgpt-account-id` from the credential's
exported `account_id` attribute only, and a `credentials` section scoped to the family whose recipe `codex` refreshes
the Codex CLI's OAuth token (rotation with write-back, a fixed 3000-second window, the `id_token` account checked
against the stored one, `scope` sent, import from `auth.json`). The Responses north codec validates `user`, `metadata`,
`prompt_cache_key` and `truncation` and writes them as request extensions under the Chat codec's names (new public
bounds `RESPONSES_REQUEST_FIELD_MAX_BYTES` and `RESPONSES_METADATA_*`); the `openai-responses` family sends them, the
Codex family does not. Gate ② runs `UsageNeverDefaulted` on a stream fixture that names its usage chunk (`CaseV1` gains
`usage_chunk`). Under Q47 `south-component-conformance` moves to 0.54.0, so every package takes one new version and
keeps its `south_runtime`.

**v0.55.0**: OpenAI reasoning replay through the IR (NC-1) and the gate ③ suite
`south.north-passthrough-delivery.v1` (release record `docs/design/2026-10-12-release-0.55.0.md`; design record
`docs/design/2026-10-11-responses-reasoning-replay-and-passthrough-suite.md`, accepted 2026-10-12), with `image-azure` 1.0.2. The north codec keeps every OpenAI reasoning item that carries
`encrypted_content`, in order and with its position, under the replay markers the Claude carrier uses (family
`openai-encrypted-reasoning`, a `reasoning_item` layout entry, `responses_opaque_reasoning_items`), and no longer writes
the two single-valued keys; it exports `OPENAI_REASONING_REPLAY_CAPABILITY` (`reasoning_replay.openai.v1`) and the G3
`responses_error_event`. `provider-openai-responses` 1.1.0 replays such a turn for a model declaring the word and asks
for `include: ["reasoning.encrypted_content"]` on every request to it; the OpenAI-compatible (2.5.0) and Gemini (1.2.0)
references refuse the family. `image-azure` 1.0.2 declares the multipart edit's immutable part names (`model`, `n`,
`size`, `width`, `height`, `response_format`, `image`, `mask`), and `contracts.image` defines `immutable_body_paths` on a
multipart descriptor (a single-segment path names a part, its `[]` form included). Under Q47 `south-component-conformance`
moves to 0.53.0 and `south-contracts` to 0.52.0 (`south-provider-api` is unchanged), so every package takes one new
version and keeps its `south_runtime`. The suite lives in
`south-provider-conformance` (abstract per-call verdicts, no IR, a normal dependency on `south-host-grammars`), with the
reference executor and eighteen deliberate mistakes in `south-testkit`; it is `not_verified` for both hosts.

**v0.54.0**: the OpenAI Responses upstream component, step R1, and the gate ③ credential-arm bit (release record
`docs/design/2026-10-10-release-0.54.0.md`; Responses design record
`docs/design/2026-09-30-openai-responses-upstream-component.md` §17). The new package `provider-openai-responses` 1.0.0
(family `openai-responses` only; `openai-codex` is step R3) maps the IR onto a stateless Responses body (`store: false`
always, leading system messages as `instructions`, `input_file` and `file_id` refused), reads the response and the
`response.*` stream strictly (a closed event list, `sequence_number`, one `response.id`, item and part identity, one
terminal, truncation without a terminal), settles `incomplete` only for `max_output_tokens` and `content_filter`,
recognises the three failure-frame shapes first, and refuses any non-zero `tool_usage`. It splits SSE with the
conformance crate's own `sse_split` (it links no host-only crate); the decoder's golden vectors run against that splitter
as a dev-dependency. Its manifest declares the record's two new provider-world fields, `immutable_body_paths` and
`north_passthrough` (gate ① admits them for declared families only; `NorthProtocolV1`, `are_immutable_body_paths`, three
`ManifestErrorV1` variants), so it declares `south_runtime` 0.54.0, the first runtime that admits them. Gate ② gains the
request-refusal form (`{"error": <envelope>}` on a request case) and `CheckV1::ImmutablePathsHonoured`. The Responses
wire vocabulary lives in `responses_vocabulary`; a test depending on the conformance crate and the north codec asserts
their spellings agree. `south-provider-api` and `south-component-conformance` move to 0.52.0 (`south-contracts` stays
0.51.0), so every other package takes a patch bump with unchanged behavior and keeps its `south_runtime`. Gate ③
`south.provider-media-binary.v1`, `south.provider-multipart.v1` and `south.provider-get.v1` each gain the wire claim
`wire_auth_exact` (mismatch category `WireAuth`): the transport's complete auth header list must be exactly the pair the
row's declared arm builds. Each suite's evidence constructor takes it as a final argument; case counts and suite
versions are unchanged, and a host re-verifies all three suites after measuring it.
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
**v0.51.0 (B6-2)**: the Bedrock InvokeModel Anthropic component (design record
`docs/design/2026-10-08-bedrock-invoke-anthropic-component.md`, accepted 2026-10-09), merged as #168 after #167 and
shipped in the `v0.51.0` tag with it. The package `provider-anthropic-bedrock-invoke` 1.0.0 (family
`anthropic-bedrock-invoke`, `host_signed` with `aws-sigv4` as Converse declares it, `stream_framing: aws-eventstream`,
`south_runtime` 0.46.0) sends `provider-anthropic`'s Messages body without `model` and `stream` and with
`anthropic_version: "bedrock-2023-05-31"`, unwraps each stream `chunk`'s base64 `bytes` itself, and maps Bedrock
exception names, then Anthropic error types, then the status. The shared Messages stream state machine changed for
`provider-anthropic` too: a cumulative usage count that shrinks is refused and the cache-write tiers fold as one group
(the host's 03 #86 rule, I-Q7), and an in-band `error` event ends the stream with `StreamEvent::Error` (I-Q12). Gate ②
stream cases may expect a refusal (`{"error": <envelope>}`). A crate-private strict base64 codec replaces the credential
recipe interpreter's. B6-2 took no version of its own beyond the new package: `south-component-conformance` kept the
0.50.0 #167 gave it, and every existing package kept the single bump over v0.50.0 that #167 gave it
(`provider-anthropic` 1.0.16 also carries the two behavior changes above).
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
