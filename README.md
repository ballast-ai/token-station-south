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
[canonical IR inventory](docs/design/2026-08-21-canonical-ir-inventory.md), the
[provider-api promotion](docs/design/2026-08-21-provider-api-promotion.md), and the
[component conformance gates](docs/design/2026-08-21-component-conformance.md).

## Implemented library slice

- `south-contracts` defines bounded HTTP, Bearer and sanctioned header-secret authentication,
  stable error, byte-streaming, and closed provider quota metadata contracts — including
  reserved-header enforcement, redacted diagnostics, and the sanctioned controlled query and
  controlled user-agent request declarations.
- `south-core` binds a validated endpoint to one credential slot, resolves the host-owned secret,
  and applies cancellation and caller deadlines around prepared buffered and streaming calls. Its
  `raw` module is the shared host prelude: a borrowed raw-call type, string-in contract parsing
  that names the failing field, zero-side-effect one-shot wrappers, and the pre-resolved and
  size-bounding credential resolver adapters both hosts previously hand-rolled.
- `south-transport-reqwest` executes hardened buffered and byte-streaming JSON POST requests,
  applies the request's sanctioned user-agent declaration exactly once, captures only the nine
  bounded quota metadata fields, and keeps redirects, retries, compression, cookies, referer
  propagation, and implicit system proxies disabled. `TransportPairV1` builds the buffered and
  streaming transports from one timeout configuration.
- `south-provider-conformance` publishes immutable `south.provider-call.v1`,
  `south.provider-stream.v1`, `south.provider-quota-metadata.v1`, `south.header-auth.v1`,
  `south.controlled-query.v1`, and `south.controlled-user-agent.v1` fixtures, while
  `south-testkit` runs them against assembled host executors.
- `south-provider-api` owns the v2 provider component ABI: the WIT package
  `token-station:adapter@2.0.0` (world `provider-adapter-v2`, JSON payloads named by
  canonical type, raw-bytes stream chunks) and the component `manifest.json` schema
  carrying the seven-field compatibility tuple the runtime handshake refuses on mismatch.
- `south-component-conformance` is gates ① and ② of the four-gate layering: package
  admission (manifest, reported identity, tuple handshake) and the
  `south.provider-component.v1` behavior suite (fixture-pinned translation, determinism,
  byte-level stream incrementality, endpoint confinement, error-catalog discipline),
  judged against a typed component seam and shipped with the native reference
  implementation of `provider-openai-compatible`. It is this repository's one sanctioned
  typed consumer of the Canonical IR, taken at a fixed kernel revision.

The slice does not include SSE or eventstream parsing, component runtime loading, a
synchronous transport, retries, fallback, routing, persistence, database access, or host adapters.
Passing a library conformance suite does not by itself verify a host integration; each verified
capability also requires review of the real host adapter wiring.

## Local verification

```bash
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
