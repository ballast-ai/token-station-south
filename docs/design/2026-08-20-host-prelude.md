# Host Prelude: Shared Raw-Call Scaffolding

Status: D1–D5 ruled 2026-08-20 (see §7); ready to implement as 0.7.0 (renumbered 2026-08-20:
0.6.0 shipped as the controlled user-agent release before this slice landed; provider-api moves
to 0.8.0, the host-signed slice to 0.9.0)

Date: 2026-08-20

Predecessors: `2026-08-16-minimal-provider-call.md` (the orchestration entry points this layer
wraps), `2026-08-17-header-secret-auth.md` (the two-arm auth shape the raw type mirrors),
`2026-08-18-controlled-query-support.md` (the query declaration the raw type carries).

## 1. Problem

Both adopting hosts — the community client (`token-station`, `apps/cli/src/south_provider_call.rs`,
812 lines, pinned at 0.4.0) and the server gateway (`south_adapter.rs`, 1374 lines, pinned at
0.5.0) — independently wrote the same South-consumption scaffolding:

| Duplicated block | Community host | Server host |
|---|---|---|
| String-in contract parse to `(ProviderBindingV1, JsonPostRequestV1)` | ~90 lines | ~75 lines (`parse_raw_call`) |
| parse→execute / parse→open-streaming wrappers | ~50 lines | ~50 lines |
| Hardened reqwest transport construction (buffered + streaming pair) | ~30 lines | ~80 lines (two `OnceLock` singletons) |
| Credential resolver patterns (pre-resolved / size-bounded) | ~35 lines | ~35 lines |

The duplication is not just cost; it is drift already observed. The two hosts bound the same
un-contracted gap — v1 has no credential-value size contract — with different numbers (16 KiB
community, 8 KiB server) and different mechanisms. Each future host (the enterprise desktop is
next) would write a third copy.

The skeleton is host-neutral: none of it touches scope policy, credential minting, settlement, or
any host type. It belongs in this repository. (The kernel mirror was considered and rejected as a
home: it is a pure-computation, zero-I/O, mirror-governed repository whose release cadence follows
upstream tags; welding a fast-moving I/O convenience layer onto it couples two release gears that
must stay independent.)

## 2. Boundary claim

Shared (this slice): raw-call value type, contract-parse orchestration, one-shot wrappers,
transport-pair construction, two resolver adapters.

Host-owned (explicitly out of scope): eligibility/scope decisions (the community host's
`IneligibleV1` taxonomy, the server's `ProviderType` table and DB-backed kill switches), dynamic
auth material (minting, OAuth refresh, JWT signing), settlement and billing semantics, and the
numeric value of any bound.

## 3. Design (`south-core::raw`, additive)

### 3.1 `RawProviderCallV1`

A borrowed value type carrying exactly what both hosts already assemble:

```rust
pub struct RawProviderCallV1<'a> {
    pub endpoint: &'a str,
    pub relative_path: &'a str,
    pub bound_slot: &'a str,
    pub requested_slot: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a str,
    pub auth: RawAuthV1,          // Bearer | HeaderSecret(SecretHeaderV1)
    pub query: Option<QueryStringV1>, // pre-parsed; URL splitting stays host-side
}
```

`RawAuthV1` mirrors `ProviderAuthV1`'s two frozen arms and adds no expressiveness. `query` takes
the already-parsed contract type: which parameters are sanctioned is a contracts question, and how
a host obtains the raw string (config, catalog row, upstream URL) is a host question; neither
belongs to this layer.

### 3.2 Parse orchestration

```rust
pub fn parse_raw_call(raw: &RawProviderCallV1<'_>)
    -> Result<(ProviderBindingV1, JsonPostRequestV1), RawCallErrorV1>;
pub fn raw_call_parses(raw: &RawProviderCallV1<'_>) -> bool;
```

`RawCallErrorV1` aggregates the existing contract errors and names the failing field; it introduces
no new parsing grammar (every grammar stays where it is, in `south-contracts`, under the existing
fuzz obligations). The predicate exists for pre-admission checks and carries the determinism
guarantee both hosts already rely on: the same inputs parse identically at admission time and at
execution time.

### 3.3 One-shot wrappers

```rust
pub async fn execute_raw_call_v1<R, T>(raw, resolver, transport, deadline, cancellation) -> …;
pub async fn open_streaming_raw_call_v1<R, T>(raw, resolver, transport, deadline, cancellation) -> …;
```

Parse, then delegate to `execute_provider_call_v1` / `open_streaming_provider_call_v1` unchanged.
Invariant, documented and tested: a parse failure returns before the resolver or transport is
invoked — zero side effects, so a host may treat it as a clean fallback signal.

### 3.4 Resolver adapters

```rust
pub struct PreparedSecretResolverV1 { /* redacted Debug, no derives */ }
impl PreparedSecretResolverV1 {
    pub fn new(secret: String) -> Self;
    pub fn expecting_slot(self, slot: CredentialSlotV1) -> Self; // optional check
}
pub struct BoundedResolverV1<R> { /* wraps R, rejects secrets over the host-supplied byte cap */ }
```

`PreparedSecretResolverV1` is the server's fund-invariant pattern (all fallible work front-loaded;
resolution after the commit marker never fails) made host-neutral. `BoundedResolverV1` turns the
"v1 has no credential size contract, hosts must bound it" footnote into a mechanism; the number
stays a host parameter.

### 3.5 Transport pair (`south-transport-reqwest`)

```rust
pub struct TransportPairV1 { pub buffered: ReqwestTransportV1, pub streaming: ReqwestStreamingTransportV1 }
impl TransportPairV1 { pub fn try_new(config: ReqwestTransportConfigV1) -> Result<Self, …>; }
```

Both hosts build both transports from one timeout config; this constructor removes that
duplication. Process-singleton policy (the server memoizes construction failure as a permanent
legacy fallback; the community host does not) stays host-side — the two hosts have genuinely
different rulings there, which is the signal that `OnceLock` wiring is policy, not scaffolding.

## 4. Obligations

- Unit suite for every item above; the zero-side-effect invariant of §3.3 gets a counting-resolver
  test (both hosts already have one to donate).
- No conformance changes: fixtures pin the contract and orchestration layers, which do not move.
- No new fuzz targets: no new grammar (per §3.2).
- `south-testkit` gains a `RawProviderCallV1` builder so host tests stop hand-rolling one.

## 5. Measured impact

Additions here: ~160 lines in `south-core::raw`, ~60 lines of resolver adapters, ~40 lines in
`south-transport-reqwest`. Deletions enabled: ~150 lines in the community host, ~200 lines in the
server host, plus equivalent test scaffolding on both sides. Every addition is convenience-layer
code; the invariant-enforcing core (cancellation safety, binding checks, secret hygiene) is
untouched.

## 6. Versioning

Additive only; no contract or orchestration signature changes; ships as **0.7.0**. Hosts adopt at
their own pace — the community host from 0.4.0 (spanning the 0.5.0 quota-metadata reset and the
0.6.0 controlled user-agent it has not yet taken), the server from 0.6.0.

## 7. Decisions — ruled 2026-08-20

- **D1 — placement: `south_core::raw`.** Same dependency set, same semver cadence, crate count
  stays flat.
- **D2 — `RawAuthV1` shape: mirror the two frozen arms, but mark the enum `#[non_exhaustive]`.**
  This departs from the drafted recommendation. The host-signed slice
  (`2026-08-20-host-signed-request-finalizer.md`) adds a third arm shortly after 0.7.0; an
  exhaustive enum would make that addition a breaking change for every host `match`. The cost is
  one `_ =>` arm per host match today. The same reasoning applies to `ProviderAuthV1` in
  `south-contracts`, which `assemble` and the reqwest transport both match exhaustively: that enum,
  and `PreparationErrorV1` (which the host-signed slice extends with two variants), gain
  `#[non_exhaustive]` in the same 0.7.0 release so the host-signed slice (0.9.0) stays additive.
- **D3 — transport scope: `TransportPairV1::try_new` only.** Singleton and failure-memoization
  policy stay host-side.
- **D4 — resolver adapters: ship both** `PreparedSecretResolverV1` (+ `expecting_slot`) and
  `BoundedResolverV1`.
- **D5 — version: additive minor.** Ruled as 0.6.0; renumbered to **0.7.0** on 2026-08-20 (0.6.0
  was consumed by the controlled user-agent release).

## 8. Implementation addenda — 2026-08-20

Three deltas from the §3 sketch, all consequences of rulings rather than new decisions:

- **`RawProviderCallV1` carries `user_agent: Option<ControlledUserAgentV1>`.** The §3.1 sketch
  predates the controlled user-agent landing in 0.6.0; the server's raw call already assembles
  the declaration, so the shared type must carry it or the server cannot adopt. Same "pre-parsed
  contract type" rule as `query`.
- **`PreparationErrorV1::UnsupportedAuthShape` (`UNSUPPORTED_AUTH_SHAPE`), the D2 wildcard's
  fail-closed landing.** Once `ProviderAuthV1` is `#[non_exhaustive]`, `south-core`'s `assemble`
  — a cross-crate match — must carry a wildcard arm. A panic there would turn a future contract
  arm into a runtime abort, so `assemble` became fallible and the arm returns this code instead.
  Structurally unreachable while the two frozen arms are the whole enum; the resolved secret is
  dropped (zeroized) on that path.
- **`RawAuthV1`, `RawCallErrorV1`, and `RawProviderCallErrorV1` are `#[non_exhaustive]` from
  birth** (`RawAuthV1` per D2; the two error aggregates by the same reasoning, since the
  host-signed slice may parse new fields). Hosts consume the aggregates through `code()` /
  `field()`, where growth is invisible.
