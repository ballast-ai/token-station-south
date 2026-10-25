# Token Station South

Token Station South is the host-neutral southbound provider execution boundary shared by the
Token Station community and enterprise hosts. Southbound means the path from a host to model
providers. Routing, tenancy, billing, quotas, persistence, and user-facing behavior remain in each
host.

> [!WARNING]
> This repository ships host-neutral libraries and sandboxed provider components, not a complete
> host product. A package or conformance result does not by itself prove host adoption; consult
> [compatibility.json](compatibility.json) for the recorded status of each host capability.

## Scope

South owns bounded provider-facing contracts, hardened HTTP transport, pure wire grammars,
component APIs, sandbox execution, conformance fixtures, and migration tooling. It must not depend
on `token-station`, `token-station-server`, a database, or a cache client.

Hosts own routing, fallback, retries, admission, credentials and secret storage, tenant policy,
pricing, billing, persistence, settlement, delivery authorization, and product behavior. South
does not discover configuration or perform hidden environment, filesystem, clock, randomness,
network, or secret-store access. Operational capabilities are injected explicitly.

The current release is `0.76.0`. It supports provider chat and streaming, asynchronous tasks,
embeddings, image generation and editing, and speech synthesis and recognition. Current additions
include Kiro image input, bounded Bailian reasoning caps, Qwen ASR, and Wan media tasks.

## Library map

| Area | Crates | Purpose |
|---|---|---|
| Contracts and orchestration | `south-contracts`, `south-core` | Bounded requests, authentication, responses, cancellation, and host call seams |
| Transport and grammars | `south-transport-reqwest`, `south-host-grammars` | Hardened reqwest execution and pure SSE/event-stream parsing |
| Provider conformance | `south-provider-conformance`, `south-testkit` | Frozen public suites and reusable host harnesses |
| Component platform | `south-provider-api`, `south-component-conformance`, `south-provider-runtime` | WIT and manifest schema, package gates, references, and the Wasmtime sandbox |
| North mapping | `south-north-codec` | Pure client-wire to canonical-IR mappings |
| Task workflow candidates | `south-task-core`, `south-task-conformance` | Explicit host-neutral task sequencing and atomic-effect conformance |

See [Architecture](ARCHITECTURE.md) for versions, dependency edges, execution layers, gates, and
security invariants.

## Component worlds

| World | Responsibility |
|---|---|
| Provider v2 | Chat, tools, structured output, streaming, and declared provider request behavior |
| Task v1/v2 | Pure submit, observe, and render translation for asynchronous media tasks |
| Embeddings v1 | Text, token-id, and declared inline-media embedding translation |
| Image v1 | Image generation and editing with bounded media transformations |
| Speech v1 | Speech synthesis, transcription, translation, and declared artifact fetches |

Official packages are listed in the [component catalog](docs/components.md), including every
manifest, build command, capability summary, and available fixture provenance record.

## Compatibility

Package admission checks the runtime ABI, exact kernel contracts, host-supported South contract
ranges, and the package's declared minimum `south_runtime`. Packages are loaded in isolation. A
failed or incompatible package does not fall back to native provider code. When multiple packages
declare the same family, the host pins the selected package digest.

[compatibility.json](compatibility.json) is the authority for contract ranges, conformance suite
versions, package adoption, and host verification status. A published release does not imply that
either host has adopted it. See the [release index](docs/releases.md) for concise release-to-record
navigation.

## Documentation

- [Architecture](ARCHITECTURE.md): current ownership, runtime layers, gates, and invariants.
- [Component catalog](docs/components.md): all official packages and fixture provenance.
- [Release index](docs/releases.md): release records and the historical design index.
- [Compatibility manifest](compatibility.json): machine-readable contract and adoption authority.
- [Security policy](SECURITY.md): vulnerability reporting and security expectations.
- [Contributing](CONTRIBUTING.md): binding development, review, and Rust standards.
- [Repository bootstrap](docs/design/2026-08-16-repository-bootstrap.md) and
  [minimal provider call](docs/design/2026-08-16-minimal-provider-call.md): foundational scope.
- [Host-zero-vendor boundary](docs/design/2026-09-30-host-zero-vendor-boundary.md): package,
  runtime, credential, and compatibility decisions.

## Optional buffered transport observation

south-transport-reqwest can report buffered send/body/header/read stages through an explicit TransportObserver and TransportPairV1::try_new_observed(config, observer). Missing or invalid x-request-id suppresses observation without altering the request. IDs are 1..128 ASCII letters, digits, underscore or hyphen, and must not contain credentials or personal data. Callbacks are synchronous and must be nonblocking and non-panicking. No global subscriber is installed.

Connection IDs identify bounded recent socket-tuple history, not guaranteed physical connection reuse. History holds at most 256 entries for one minute; eviction, expiry or missing information does not prove a fresh connection. Request-body delivery does not mean upload completion, and upstream receipt/queue time remains unknown.

Local connection write/flush is available only through the separately applied [pinned reqwest patch bundle](patches/reqwest-0.13.5-local-write/README.md). The default build keeps official reqwest; Cargo patches are a final-host responsibility and do not propagate. The bundle prepares a complete, separate South snapshot, validates source/patch/archive hashes and checks an independent host's public type identity. It is not a production-host migration or a released SDK. See the [design record](docs/design/2026-10-25-transport-observation.md) for boundaries and verification.

## Local verification

Install the repository toolchain and the tools named by [Contributing](CONTRIBUTING.md), then run
the complete matrix below. The language gate permits technical identifiers but requires new prose
to be English.

```bash
rustup target add wasm32-wasip2  # once; guest components build as child cargo invocations
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
cargo test --workspace --doc --all-features
cargo check --workspace --all-targets --no-default-features
cargo test -p south-component-conformance --no-default-features  # the only crate with a feature
cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
python3 scripts/check_msrv.py  # a no-op while the pinned toolchain is a release of the MSRV
scripts/check-boundaries.sh --self-test
scripts/check-boundaries.sh
python3 -m unittest discover -s scripts -p 'test_*.py'  # release tooling and the build scripts
scripts/check-language.sh --self-test
scripts/check-language.sh
scripts/check-language.sh --commits origin/main..HEAD  # CI checks the pull request's commits
cargo deny check
cargo deny --manifest-path fuzz/Cargo.toml --config deny.toml --locked check
cargo audit
cargo audit --file fuzz/Cargo.lock
cargo machete
(cd fuzz && cargo machete)
```

Release workflows add component builds, gate reports, declared-runtime checks, release-index
generation, and digest comparison. Those checks do not replace this local matrix.
