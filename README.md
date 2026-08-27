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
the [task adapter vocabulary](docs/design/2026-08-27-task-adapter-vocabulary.md) (proposed), and the
[manifest schema beyond one world](docs/design/2026-08-27-manifest-schema-beyond-one-world.md)
(proposed).

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
  applies the request's sanctioned user-agent declaration exactly once, applies every auth header
  the prepared request carries (one for the credential arms, the finalizer's diffed set for the
  host-signed arm), adds exactly `TRANSPORT_ADDED_HEADERS_V1` and nothing else, captures only the
  nine bounded quota metadata fields, and keeps redirects, retries, compression, cookies, referer
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
