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

| Crate | Ownership |
| --- | --- |
| `south-contracts` | Canonical provider-facing HTTP, auth, stream, and error contracts |
| `south-core` | Host-neutral single-call orchestration |
| `south-transport-ureq` | Synchronous native transport |
| `south-transport-reqwest` | Asynchronous native transport |
| `south-provider-api` | Provider WIT and manifest schemas |
| `south-provider-runtime` | Sandboxed provider component execution |
| `south-provider-conformance` | Fixtures and contract verification |
| `south-testkit` | Public tests reusable by consumers |
| `south-migration` | Offline fixture comparison; never production double-send |

Only `south-contracts` has a behavioral API in the bootstrap release. Other crates are unpublished
ownership markers and must not gain behavior without an English design record and a failing public
behavior test.

## Host-owned concerns

South does not own routing, fallback across upstreams, retry budgets, admission, tenants, billing,
quota ledgers, audit persistence, task persistence, credential sources, or tracing initialization.
It never reads a database directly. Transport I/O, time, cancellation, component bytes, and runtime
permissions must be explicit capabilities at their operational boundaries.

During migration, `token-station-protocol` may re-export South types under old Rust paths. It must
not define duplicate nominal types, and South must never depend back on that compatibility layer.
