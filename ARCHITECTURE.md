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
| `south-contracts` | Implemented version-one bounded HTTP, Bearer auth, stable error, and byte-streaming contracts |
| `south-core` | Implemented host-neutral buffered and streaming provider-call orchestration |
| `south-transport-reqwest` | Implemented hardened buffered and byte-streaming JSON POST transport |
| `south-provider-conformance` | Implemented immutable provider-call and provider-stream v1 fixtures |
| `south-testkit` | Implemented assembled-executor conformance runners and reference executors |
| `south-transport-ureq` | Placeholder for a future synchronous native transport |
| `south-provider-api` | Placeholder for future provider WIT and manifest schemas |
| `south-provider-runtime` | Placeholder for future sandboxed component execution |
| `south-migration` | Placeholder for future offline fixture comparison; never production double-send |

The implemented crate dependency graph is:

```text
south-core -------------------------------> south-contracts
south-transport-reqwest ------------------> south-core
south-transport-reqwest ------------------> south-contracts
south-provider-conformance ---------------> south-contracts
south-testkit ----------------------------> south-contracts
south-testkit ----------------------------> south-core
south-testkit ----------------------------> south-provider-conformance
```

These edges are direct Cargo dependencies. They are one-way and acyclic. Only the reqwest transport
crate owns a network-client dependency. No South crate owns a database, cache, migration directory,
host repository dependency, or credential source.

## Host-owned concerns

South does not own routing, fallback across upstreams, retry budgets, admission, tenants, billing,
quota ledgers, audit persistence, task persistence, credential sources, or tracing initialization.
It never reads a database directly. Transport I/O, time, cancellation, component bytes, and runtime
permissions must be explicit capabilities at their operational boundaries.

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
