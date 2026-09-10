# Host Prelude: The Host-Signed Raw Call

Status: D1–D3 ruled 2026-09-08 (see §6); shipped as 0.21.0

Date: 2026-09-08

Predecessors: `2026-08-20-host-prelude.md` (the raw-call scaffolding this extends; its D2
anticipated this slice as "a third arm"), `2026-08-20-host-signed-request-finalizer.md` (the
seam and the two orchestration entry points the new wrappers delegate to).

## 1. Problem

The host prelude gives a host one shape for consuming South: a borrowed string-in raw call,
parsed through the contract grammars with the failing field named, then handed to a one-shot
wrapper whose parse failure is guaranteed to have no side effects. The server's whole text
funnel is built on that shape — every South plan it constructs before its commit point is a
`RawProviderCallV1`, pre-checked with `raw_call_parses`.

The host-signed arm has no such shape. Its orchestration entry points exist
(`execute_signed_provider_call_v1`, `open_streaming_signed_provider_call_v1`) and the server's
`BedrockRequestFinalizerV1` wraps its `SigV4` signer exactly as the finalizer record foresaw,
but the only consumer of either is a test. To route its SigV4 Bedrock traffic through South the
server would have to assemble `ProviderBindingV1` + `JsonPostRequestV1` by hand for this one
arm — the very duplication the prelude was written to retire — or the arm stays where it is:
written, tested, and unreachable from production.

That gap, not the streaming or response-encoding gaps the server once listed against this arm,
is what has kept SigV4 on the legacy path. `StreamChunkV1` is already bytes and the streaming
signed entry point already exists; a Bedrock event stream fits through both untouched.

## 2. Boundary claim

Unchanged from the prelude record. Everything here is convenience-layer orchestration over the
existing contracts: no new grammar, no new stable code, no signing algorithm. South still never
resolves the slot for this arm (finalizer record, D2) and still never sees signing material.

## 3. Design (`south-core::raw`, additive)

### 3.1 `RawSignedProviderCallV1`

```rust
pub struct RawSignedProviderCallV1<'a> {
    pub endpoint: &'a str,
    pub relative_path: &'a str,
    pub bound_slot: &'a str,
    pub requested_slot: &'a str,
    pub headers: &'a [(String, String)],
    pub body: &'a str,
    pub emits: &'a SignedHeaderSetV1,          // in place of `auth`
    pub query: Option<QueryStringV1>,
    pub user_agent: Option<ControlledUserAgentV1>,
}
```

The same field set as `RawProviderCallV1` with the finalizer's declaration where the credential
scheme would be. It is a sibling type, not the third `RawAuthV1` arm D2 planned — see D1.

### 3.2 Parse orchestration

`parse_raw_signed_call` and `raw_signed_call_parses` parse the shared fields through one private
helper `parse_raw_parts` that `parse_raw_call` now uses too, so the two shapes cannot drift on a
grammar or on a `RawCallErrorV1` field name. The request's auth is
`ProviderAuthV1::HostSigned { slot, emits: emits.clone() }`.

### 3.3 One-shot wrappers

```rust
pub async fn execute_signed_raw_call_v1<F, T>(raw, finalizer, transport, deadline, cancellation) -> …;
pub async fn open_streaming_signed_raw_call_v1<F, T>(raw, finalizer, transport, deadline, cancellation) -> …;
```

Parse, then delegate to the signed orchestration entry points unchanged. The prelude's invariant
holds verbatim: a parse failure returns before the finalizer or the transport is invoked.
Everything after the parse is the finalizer record's: one signing per call inside the deadline
and cancellation scope, the declaration diff before any byte is sent, the same three preparation
codes.

### 3.4 `south-testkit`

`RawSignedProviderCallBuilderV1`, the owned twin of `RawProviderCallBuilderV1`, defaulting to a
session-token-less SigV4 declaration.

## 4. Obligations

- `south-core/tests/raw_call_v1.rs`: pre-check agrees with parse; the declaration, query, and
  user agent travel; a parse failure invokes neither finalizer nor transport; a slot mismatch is
  refused before the finalizer; execution binds exactly the declared header set; a signer that
  breaks its declaration is rejected before the transport; the streaming twin hands a non-UTF-8
  chunk through untouched.
- The consumer: `token-station-server`'s Bedrock SigV4 South plan, the first production caller
  of `BedrockRequestFinalizerV1`. This slice ships with that consumer in sight, per the standing
  rule against API without a consumer.

## 5. Versioning

Additive only; no contract or orchestration signature changes; ships as **0.21.0**.

## 6. Decisions — ruled 2026-09-08

- **D1 — a sibling type, not a third `RawAuthV1` arm.** The arm would have to carry
  `SignedHeaderSetV1`, and `RawAuthV1` is `Copy` and lifetime-free; a host that copies it today
  would stop compiling, and a borrowed variant would put a lifetime on every host `match`. The
  declaration therefore travels where the scheme would have, on a type whose only difference is
  that one field. `RawAuthV1`'s D2 note now says so.
- **D2 — `emits` is borrowed, like every other raw field.** The host owns the declaration (it is
  a property of its finalizer); parse clones it into the request exactly once.
- **D3 — one private parse helper for both shapes.** Two copies of the grammar sequence would be
  the prelude's own duplication problem one level down.
