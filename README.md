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

## Release notes

Each release has a design record; those records are the detail, and this table is
the index. A published tag does **not** mean either host has adopted the packages
it carries — adoption is recorded per host in `compatibility.json`.

| Release | What it added | Record |
|---|---|---|
| `0.29.0` | The `task-adapter-v2` world alongside provider-v2 and task-v1: `TaskLocatorV2` for bounded locators, `TaskUsageFactsV2` for coexisting usage, observation keeping queued/running and per-artifact id and duration, and rendering identity and time passed in explicitly by the host. Also the separate `TaskRequestEstimateV2`: the component states the final request's duration and protocol unit rate, the host chooses the estimate, and price, markup, missing-duration defaults and the funding transaction all stay with the host. | [candidate](docs/design/2026-09-20-task-adapter-v2-candidate.md), [request estimate](docs/design/2026-09-20-task-request-estimate.md), [release](docs/design/2026-09-20-release-0.29.0.md) |
| `0.30.0` | `task-minimax-v2` (Hailuo and H3), the HTTP9 `file_id` controlled query, and the Task 5 canonical request-input facts. | [component](docs/design/2026-09-20-minimax-v1-task-component.md), [H3 facts](docs/design/2026-09-20-minimax-h3-estimate-facts.md), [release](docs/design/2026-09-20-release-0.30.0-minimax.md) |
| `0.31.0` | `task-bailian-v2`, covering managed video; images and native pass-through are not migrated. Reuses the Task 5 / HTTP9 / Task V2 WIT with no new runtime capability. | [component](docs/design/2026-09-20-bailian-video-task-component.md), [release](docs/design/2026-09-20-release-0.31.0.md) |
| `0.32.0`–`0.36.0` | Runtime and `compatibility.south_runtime` only; package identities unchanged except where a component's own behaviour changed. | — |
| `0.37.0` | Responses in `south-north-codec`: request→IR, IR response→Responses, and IR stream events→SSE, plus the same-source JSON façade. The host passes identity, time, inbound tools and compatibility options and holds the per-stream state; admission, billing, the continuation cache and Native pass-through stay with the host. | [release](docs/design/2026-09-28-release-0.37.0.md), [design](docs/design/2026-09-28-responses-north-codec.md), [evidence](docs/design/2026-09-28-responses-north-codec-validation.md) |
| `0.39.0` | Responses reasoning replay on kernel v0.3.0 / protocol 0.4.0: a bounded `tsr.c1.` carrier preserves Claude thinking, signature, redacted thinking, and text/tool reference order. Only Anthropic Messages and Bedrock Converse models that explicitly declare `reasoning_replay.claude.v1` may receive it; OpenAI-compatible and Gemini refuse. `canonical_ir` ownership stays with the kernel, so South's `compatibility.json.contracts.canonical_ir` remains `null`. | [release](docs/design/2026-09-29-release-0.39.0.md) |
| `0.40.0` | IR usage follows the kernel partition contract: `provider-anthropic` (1.0.6) and `provider-bedrock-converse` (1.0.3) report the whole prompt as `input_tokens`, Converse accepts a `totalTokens` that counts the cache buckets (as AWS sends it), and the Anthropic north renderers subtract the buckets. `usage_ir_contract_v1` judges every reference against its provider's documented prompt formula. | — |
| `0.41.0` | Per-model Claude request dialect declared by the host in `supported_parameters` (`anthropic.*` words): `provider-anthropic` (1.0.7) and `provider-bedrock-converse` (1.0.4) drop sampling parameters the model rejects, refuse a forced tool choice it cannot honor, and carry the caller's reasoning effort as adaptive thinking or a bounded budget; declaring nothing keeps every request unchanged. | — |
| `0.42.0` | `anthropic.sampling.exclusive`: for a model that accepts `temperature` or `top_p` but not both (Opus 4.5 through Sonnet 4.6), `provider-anthropic` (1.0.8) and `provider-bedrock-converse` (1.0.5) keep `temperature` and drop `top_p` instead of sending a request the upstream rejects. | — |

## Component packages

Every official component is built by its own script, and each ships as the exact
directory shape the runtime loads — `manifest.json` beside `component.wasm`.

| Component | Family | Build | Status |
|---|---|---|---|
| `task-kling-v2` | kling | `bash scripts/build-kling-task-v2-component.sh` | released |
| `task-minimax-v2` | minimax | `bash scripts/build-minimax-task-v2-component.sh` | released |
| `task-bailian-v2` | bailian | `bash scripts/build-bailian-task-v2-component.sh` | released |
| `task-xai-v2` | `xai-video` | `bash scripts/build-xai-task-v2-component.sh` | contract 6 candidate, unreleased |
| `task-byteplus-v2` | `byteplus-video` | `bash scripts/build-byteplus-task-v2-component.sh` | contract 6 candidate, unreleased |
| `task-veo-v2` | `veo-video` | `bash scripts/build-veo-task-v2-component.sh` | contract 6 candidate, unreleased |
| `task-wan-image-v2` | `wan-image` | `bash scripts/build-wan-image-task-v2-component.sh` | contract 6 candidate, unreleased |
| `task-gmi-image-v2` | `gmi-image` | `bash scripts/build-gmi-image-task-v2-component.sh` | contract 6 candidate, unreleased |

Each pack's `README.md` records how its expectations were derived and how the
component differs from the host's native arm — read that before changing a
fixture. The shared facts the contract-6 families rely on, and the artifact roles
contract 7 added, have their own records:
[contract 6 facts](docs/design/2026-09-27-task-contract-v6-facts.md),
[contract 7 artifact roles](docs/design/2026-09-28-task-contract-v7-artifact-role.md).


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
scripts/check-language.sh --self-test
scripts/check-language.sh
cargo deny check
cargo deny --manifest-path fuzz/Cargo.toml --config deny.toml --locked check
cargo audit
cargo audit --file fuzz/Cargo.lock
cargo machete
(cd fuzz && cargo machete)
```

All source code, documentation, diagnostics, logs, and commit messages in this repository are
written in English, and `scripts/check-language.sh` enforces it rather than leaving it to
recollection. `scripts/language-baseline.txt` records the CJK still left in the design records so
the gate blocks growth today instead of waiting for the backlog; translate, then run
`scripts/check-language.sh --regen`, which only ever lowers a count.
