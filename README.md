# Token Station South

Token Station South is the host-neutral southbound provider execution boundary shared by the
Token Station community and enterprise hosts. "Southbound" means the path from a host to model
providers; routing, tenancy, billing, quotas, persistence, and user-facing behavior remain in each
host.

> [!WARNING]
> This repository ships host-neutral libraries, not a complete host product. Both community and
> enterprise hosts have verified provider-call v1 adapters in narrow scopes. Provider-stream and
> provider-quota-metadata verification are recorded independently per host in the compatibility
> manifest, and most host production traffic remains outside South.

## Repository boundaries

This repository owns provider-facing contracts, transports, provider component APIs and runtimes,
conformance fixtures, and migration test tooling. It must not depend on `token-station` or
`token-station-server`; both hosts consume this repository in one direction.

South does not read databases, environment variables, config files, keychains, or secret stores.
Hosts resolve credentials and inject capabilities explicitly. Provider components have no network,
filesystem, or secret access by default.

See [Architecture](ARCHITECTURE.md), [Compatibility](compatibility.json),
[Security](SECURITY.md), [Contributing](CONTRIBUTING.md), the
[repository bootstrap design](docs/design/2026-08-16-repository-bootstrap.md), and the
[minimal provider call design](docs/design/2026-08-16-minimal-provider-call.md), the
[streaming provider call design](docs/design/2026-08-17-streaming-provider-call.md), and the
[community compatibility release](docs/design/2026-08-17-community-host-compatibility-release.md),
the
[provider quota metadata design](docs/design/2026-08-17-provider-quota-response-metadata.md), the
[header-secret auth design](docs/design/2026-08-17-header-secret-auth.md), the
[controlled query design](docs/design/2026-08-18-controlled-query-support.md), the
[controlled user-agent design](docs/design/2026-08-20-controlled-user-agent.md), the
[host prelude design](docs/design/2026-08-20-host-prelude.md), the
[host-signed request finalizer design](docs/design/2026-08-20-host-signed-request-finalizer.md)
(shipped in 0.14.0; see its §8 for the one half deliberately left), the
[canonical IR inventory](docs/design/2026-08-21-canonical-ir-inventory.md), the
[provider-api promotion](docs/design/2026-08-21-provider-api-promotion.md), the
[component conformance gates](docs/design/2026-08-21-component-conformance.md), the
[provider runtime](docs/design/2026-08-21-provider-runtime.md), the
[OpenAI-compatible host parity](docs/design/2026-08-21-openai-compat-host-parity.md), the
[Anthropic provider component](docs/design/2026-08-22-anthropic-provider-component.md), the
[Gemini provider component](docs/design/2026-08-22-gemini-provider-component.md), the
[renderer refusal for unmappable blocks](docs/design/2026-08-23-renderer-refusal-for-unmappable-blocks.md),
the [task adapter vocabulary](docs/design/2026-08-27-task-adapter-vocabulary.md) (proposed), the
[manifest schema beyond one world](docs/design/2026-08-27-manifest-schema-beyond-one-world.md)
(proposed), and the
[released component artifacts](docs/design/2026-09-10-released-component-artifacts.md) (proposed).

## Implemented library slice

- `south-contracts` defines bounded HTTP (the JSON POST request, the body-less GET request, and
  the multipart POST request, whose opaque bytes travel under a media type the contract renders
  from a validated boundary),
  Bearer, sanctioned header-secret, and combined Bearer-plus-header-secret authentication, stable
  error, byte-streaming, and closed provider quota metadata contracts, plus a buffered binary
  response beside the UTF-8 one, which keeps its guarantee unchanged — including reserved-header
  enforcement, redacted diagnostics, and the sanctioned controlled query and controlled
  user-agent request declarations.
- `south-core` binds a validated endpoint to one credential slot, resolves the host-owned secret,
  and applies cancellation and caller deadlines around prepared buffered and streaming JSON POST
  calls, buffered body-less GET calls, buffered multipart POST calls, and JSON POST calls whose
  response is buffered as opaque bytes rather than proved to be UTF-8. Its
  `raw` module is the shared host prelude: a borrowed raw-call type, string-in contract parsing
  that names the failing field, zero-side-effect one-shot wrappers, and the pre-resolved and
  size-bounding credential resolver adapters both hosts previously hand-rolled — plus the
  host-signed twin of that raw call and its wrappers, which take a host finalizer in place of a
  credential resolver, the body-less GET twin a task poller hands over, and the multipart twin a
  transcription or image-edit path hands over.
- `south-transport-reqwest` executes hardened buffered and byte-streaming JSON POST requests,
  buffered body-less GET requests, buffered multipart POST requests (emitting the media type
  the prepared request renders, and sharing the body's allocation rather than copying it), and
  binary-response JSON POST requests under their own larger body cap,
  applies the request's sanctioned user-agent declaration exactly once, applies every auth header
  the prepared request carries (one for the credential arms, the finalizer's diffed set for the
  host-signed arm), adds exactly `TRANSPORT_ADDED_HEADERS_V1` and nothing else, captures only the
  nine bounded quota metadata fields, and keeps redirects, retries, compression, cookies, referer
  propagation, and implicit system proxies disabled. `TransportPairV1` builds the buffered and
  streaming transports from one timeout configuration.
- `south-provider-conformance` publishes immutable `south.provider-call.v1`,
  `south.provider-stream.v1`, `south.provider-quota-metadata.v1`, `south.header-auth.v1`,
  `south.controlled-query.v1`, `south.controlled-user-agent.v1`, `south.provider-get.v1`,
  `south.provider-multipart.v1`, and `south.provider-binary.v1` fixtures, while `south-testkit`
  runs them against assembled host executors.
- `south-provider-api` owns the v2 provider component ABI: the WIT package
  `token-station:adapter@2.0.0` (world `provider-adapter-v2`, JSON payloads named by
  canonical type, raw-bytes stream chunks) and the component `manifest.json` schema
  carrying the seven-field compatibility tuple the runtime handshake refuses on mismatch.
- `south-component-conformance` is gates ① and ② of the four-gate layering: package
  admission (manifest, reported identity, tuple handshake) and the
  `south.provider-component.v1` behavior suite (fixture-pinned translation, determinism,
  byte-level stream incrementality, endpoint confinement, error-catalog discipline),
  judged against a typed component seam and shipped with the native reference
  implementations of `provider-openai-compatible`, `provider-anthropic` and
  `provider-gemini`, each with its own frozen fixture pack — sharing one pack would freeze
  whichever dialect was written first. It is this repository's one sanctioned typed consumer of the Canonical IR, taken
  at a fixed kernel revision.

- `south-provider-runtime` executes provider components inside a wasmtime sandbox:
  gated loading (manifest, forbidden-import scan, reported identity), locked-down WASI
  (no preopens, no environment, no sockets/http by refusal), per-store memory limits,
  epoch call deadlines, boundary payload ceilings, one instance per stream — with a
  deliberately JSON-only API face, so the runtime never consumes the Canonical IR. The
  conformance crate's `sandbox` feature provides the typed seam over it, and `components/`
  packages each native reference as an official `wasm32-wasip2` component:
  `provider-openai-compatible` (`scripts/build-reference-component.sh`) covers the
  OpenAI-compatible and Azure dialects, `provider-anthropic`
  (`scripts/build-anthropic-component.sh`) covers Anthropic Messages, and
  `provider-gemini` (`scripts/build-gemini-component.sh`) covers Gemini
  `generateContent` — including its streaming operation, which the dialect selects with a
  different URL suffix rather than a body field. A component and its
  native reference are the same code, so sandbox parity is a property of construction that
  the parity tests then prove end to end.

The slice does not include a
synchronous transport, retries, fallback, routing, persistence, database access, or host adapters.
Passing a library conformance suite does not by itself verify a host integration; each verified
capability also requires review of the real host adapter wiring.

## task-v2 与 v0.29.0 发布记录

本地候选新增 `task-adapter-v2`，与既有 provider-v2、task-v1 分别装载。
`TaskLocatorV2` 保存有界定位；`TaskUsageFactsV2` 保留并存用量；观察保留排队／运行、
逐产物 id/duration；渲染身份和时间由宿主显式传入。Kling 原生参考实现与
`components/task-kling-v2` 共享代码，使用独立 fixture 和真实 Wasm 对拍。

构建候选：`bash scripts/build-kling-task-v2-component.sh`。
`0.29.0` 已发布；发布不代表两个宿主已采用。此前 server 的临时依赖覆盖
只验证授权／计价接缝，生产注册表、持久执行绑定、旧包恢复与社区采用另行验收。
边界与候选行为收紧见[候选设计](docs/design/2026-09-20-task-adapter-v2-candidate.md)。

第五批候选增加独立 `TaskRequestEstimateV2`：组件给出最终请求的时长与协议单位率，
宿主显式选择估时，再用经过范围校验的 helper 估算单位。它不能代替供应商实际用量；
价格、加价、缺失时长的默认值和资金事务仍属于宿主。prepared JSON 新增必需字段，
任务词汇版本为 4，旧候选 prepared JSON 不再兼容；宿主生产采用仍待验证。
详见[请求估算设计](docs/design/2026-09-20-task-request-estimate.md)。

五个组件包随本次构建更换不可变身份，不能用原版本覆盖不同内容。兼容清单 schema 4
单列 `task_component_capabilities`；两个宿主的 task-v1／v2 生产采用均为
`not_verified`，不继承历史 provider-call 的 verified。runtime 元组继续严格匹配，
保留旧包不等于新 runtime 能执行旧包。版本与验证范围见
[发布准备记录](docs/design/2026-09-20-release-0.29.0.md)。

## v0.30.0 MiniMax 候选（未发布）

v0.29.0 已发布，正式内容为此前五包。本工作树准备 v0.30.0：新增
`task-minimax-v2`、HTTP9 `file_id` 受控查询，并递增六包不可变身份。
构建候选：`bash scripts/build-minimax-task-v2-component.sh`。
范围与尚未完成的宿主验收见[MiniMax设计](docs/design/2026-09-20-minimax-v1-task-component.md)
和[下一版准备](docs/design/2026-09-20-release-0.30.0-minimax.md)。

## Local verification

```bash
rustup target add wasm32-wasip2  # once; guest components build as child cargo invocations
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
cargo test --workspace --doc --all-features
cargo test --workspace --no-default-features
cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
rustup run 1.96.0 cargo check --workspace --all-targets
scripts/check-boundaries.sh --self-test
scripts/check-boundaries.sh
cargo deny check
cargo deny --manifest-path fuzz/Cargo.toml --config deny.toml --locked check
cargo audit
cargo audit --file fuzz/Cargo.lock
cargo machete
(cd fuzz && cargo machete)
```

All source code, documentation, diagnostics, logs, and commit messages in this repository are
written in English.
