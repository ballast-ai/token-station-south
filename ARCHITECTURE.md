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
| `south-contracts` | Implemented version-one bounded HTTP, Bearer auth, and stable error contracts |
| `south-core` | Implemented host-neutral buffered provider-call orchestration |
| `south-transport-reqwest` | Implemented hardened buffered JSON POST transport |
| `south-provider-conformance` | Implemented immutable `south.provider-call.v1` fixtures |
| `south-testkit` | Implemented assembled-executor conformance runner and reference executor |
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
