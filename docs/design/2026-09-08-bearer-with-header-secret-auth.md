# Auth: The Combined Bearer-and-Header-Secret Arm

Status: D1–D2 ruled 2026-09-08 (see §5); shipped as 0.23.0

Date: 2026-09-08

Predecessors: `2026-08-17-header-secret-auth.md` (the sanctioned header-secret arm and the
`south.header-auth.v1` suite this extends), `2026-08-20-host-prelude.md` (D2: `RawAuthV1` is
`#[non_exhaustive]` so an arm can be added without breaking host matches).

## 1. Problem

Gemini's `OpenAI`-compatible surface (`/v1beta/openai/*`, which the server host uses for its
`/v1/responses` and `/v1/messages` operations against Gemini) accepts an API key only when it
arrives twice: verbatim in `x-goog-api-key` **and** `Bearer `-prefixed in `authorization`. Its
native surface rejects the Bearer header, so the host already branches on the URL shape.

`ProviderAuthV1` binds one header per credential arm, and both names are on `RESERVED_HEADERS`,
so the plain header channel cannot carry the second copy either. The host's only option has been
to refuse the South route for that shape — every Gemini `/openai/` text call falls back to the
legacy path (37-号 S4; the host's `south_text_plan` narrows Gemini to its native transport).

## 2. Boundary claim

Unchanged. South still resolves one slot, exactly once, and still decides nothing about *which*
headers exist: the arm is a closed shape naming one sanctioned header, not a list. A "list of auth
headers" would reopen the choice the header-secret record closed (D1 there) — a provider could
then name any reserved header it liked.

## 3. Design

### 3.1 Contract (`south-contracts`)

```rust
pub enum ProviderAuthV1 {
    Bearer(BearerAuthV1),
    HeaderSecret { header: SecretHeaderV1, slot: BearerAuthV1 },
    HostSigned { slot: BearerAuthV1, emits: SignedHeaderSetV1 },
    BearerAndHeaderSecret { header: SecretHeaderV1, slot: BearerAuthV1 },   // new
}
```

`AUTH_CONTRACT_VERSION: 3 → 4`, additive: every version-three request is a version-four request
that does not use the new arm. `RawAuthV1` gains `BearerAndHeaderSecret(SecretHeaderV1)`; the
enum stays `Copy`.

### 3.2 Seam (`south-core`)

`assemble` binds the one resolved secret twice for the new arm, in a fixed order —
`authorization` (prefixed, a fresh zeroizing allocation) then the sanctioned header (verbatim,
taking over the resolver allocation). `auth_headers()` yields two elements for this arm; the
transports already apply every element the prepared request hands over (the host-signed arm
made them multi-header), so they change only in their comments.

### 3.3 Conformance

`south.header-auth.v1` gains a fourth case, `BufferedBearerAndHeaderSecretSuccess`, appended
after the frozen three: a buffered exchange under the combined arm expecting the sanctioned
header exact **and** `authorization_header_absent == false` — the table's only such row. An
adapter that hardcodes the absence claim, or resolves the slot once per header, fails here and
nowhere else. The fixture carries a `bearer_alongside` flag so the reference executor and the
wire probe branch on the fixture, not on the case id.

The suite version stays at `1`, by the precedent of the controlled-query suite's later cases: the
three existing cases are byte-identical. Evidence, however, must cite the table it actually ran
(`host_capability_evidence_freshness_v1`): the server host re-ran the four-case suite through its
own adapter before this shipped and is annotated `cases: 4`; the community host has not, so its
`header_auth` capability is recorded `not_verified` until it re-runs the suite.

## 4. Versioning

Additive; ships as **0.23.0** with `compatibility.json` `auth: 4`.

## 5. Decisions — ruled 2026-09-08

- **D1 — one closed arm, not an auth-header list.** The combined shape is the whole of what any
  covered upstream needs; a list would let a provider choose reserved names.
- **D2 — fixed binding order, `authorization` first.** Byte-identical wires across hosts for the
  same declaration, the same reason the query serializes in canonical order.
