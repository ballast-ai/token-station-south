# Architecture

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
| `south-contracts` | Implemented bounded HTTP (JSON POST, body-less GET, and multipart POST request shapes), Bearer, sanctioned header-secret, and combined Bearer-plus-header-secret auth, stable error, byte-streaming, and closed quota metadata contracts, plus the sanctioned controlled query and controlled user-agent declarations |
| `south-core` | Implemented host-neutral buffered and streaming provider-call orchestration and its buffered body-less GET and multipart twins, plus the shared host prelude (`raw` module: raw-call type, its host-signed, GET and multipart twins, contract-parse orchestration, one-shot wrappers for all four, resolver adapters) |
| `south-transport-reqwest` | Implemented hardened buffered and byte-streaming JSON POST transport, the same buffered transport for body-less GET and multipart POST requests (rendering the latter's media type and sharing its allocation), bounded quota metadata capture, sanctioned user-agent application, and one-config transport-pair construction |
| `south-provider-conformance` | Implemented immutable provider-call, provider-stream, provider-quota-metadata, header-auth, controlled-query, controlled-user-agent, provider-get, and provider-multipart v1 fixtures |
| `south-testkit` | Implemented assembled-executor conformance runners and reference executors for all eight suites, plus the owned raw-call, host-signed raw-call, raw-GET and raw-multipart builders for host tests |
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
