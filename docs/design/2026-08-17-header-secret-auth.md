# Header-Secret Auth Vertical Slice

Status: D1–D4 all ruled as recommended by lv (2026-08-17); implementation authorized

Date: 2026-08-17

Predecessors: `2026-08-16-minimal-provider-call.md` (auth contract v1),
`2026-08-17-streaming-provider-call.md` (both call shapes must support the new scheme).

## 1. Problem

Auth contract v1 knows exactly one scheme: a bearer secret delivered as `Authorization: Bearer …`.
The enterprise host's provider inventory (its adoption record, product-review #34) splits real
authentication into five families; the second-largest — **header injection**, where the secret
travels in a provider-specific header — is entirely locked out today:

| Header | Provider family |
|---|---|
| `x-api-key` | Anthropic |
| `x-goog-api-key` | Gemini |
| `api-key` | Azure OpenAI / Azure AI Foundry |
| `ocp-apim-subscription-key` | Azure Speech |
| `xi-api-key` | ElevenLabs |
| `api-key` (`Api-Key`) | Ideogram |

Unlocking this family multiplies the adoptable provider surface of both existing call shapes
(buffered and streaming) without touching orchestration, transport hardening, or conformance
philosophy. The remaining families (AWS SigV4, self-signed JWT, OAuth-with-impersonation) involve
host-side signing or refresh flows and stay out of scope pending the cross-team review the
migration checklist (§7.1) already mandates.

## 2. The reserved-header tension

The v1 reserved-header blacklist deliberately contains every one of these names: a provider-facing
plugin must never smuggle a secret (or a secret-shaped header) through the plain-header channel.
That protection must survive this slice unchanged. The new scheme is therefore **not** a loosening
of `SafeHeaders` — plain headers still reject all reserved names — but a second sanctioned auth
shape, declared through the auth type, resolved through the same one-secret `CredentialResolver`
flow, and injected by the transport at the last moment exactly as the bearer header is today
(sensitive, zeroized owner, never present in `SafeHeaders`).

## 3. Contract design (`south-contracts`)

### 3.1 Auth scheme type

```rust
/// Frozen set of sanctioned secret-bearing headers. Fieldless, closed.
pub enum SecretHeaderV1 {
    ApiKey,                  // "api-key"        (Azure OpenAI / Foundry / Ideogram)
    XApiKey,                 // "x-api-key"      (Anthropic)
    XGoogApiKey,             // "x-goog-api-key" (Gemini)
    XiApiKey,                // "xi-api-key"     (ElevenLabs)
    OcpApimSubscriptionKey,  // "ocp-apim-subscription-key" (Azure Speech)
}

pub enum ProviderAuthV1 {
    Bearer(BearerAuthV1),
    HeaderSecret { header: SecretHeaderV1, auth: BearerAuthV1 /* slot carrier, renamed? see D2 */ },
}
```

Ruling embedded here (**D1**): the sanctioned header set is a **frozen enum**, not a validated
string. Every variant maps to a vetted provider family; adding one is a deliberate contract bump
with a conformance case, not a host-side configuration. The frozen set is the same closed-world
discipline as the error codes, and it keeps `RESERVED_HEADERS` and the sanctioned set from
drifting apart silently (a unit test asserts every `SecretHeaderV1` name is on the reserved list).

Naming (**D2**): `BearerAuthV1` today is really "a credential-slot declaration"; the header-secret
arm needs the same slot carrier. Options: reuse `BearerAuthV1` under the `HeaderSecret` arm
(cheap, slightly misleading name) vs introduce `CredentialSlotClaimV1` and re-export
`BearerAuthV1` as an alias (cleaner, one more name). Recommendation: **reuse** — v1 froze the
name, the doc comment can carry the nuance, and both hosts already import it.

### 3.2 Versioning

`AUTH_CONTRACT_VERSION: 1 → 2` (additive: v1 requests are exactly v2 requests with the `Bearer`
arm). `compatibility.json` mirrors it. `JsonPostRequestV1::new` keeps its signature via
`impl Into<ProviderAuthV1>` from `BearerAuthV1` — existing host call sites compile unchanged
(**D3**; the alternative, a parallel `new_with_auth` constructor, leaves two front doors forever).

### 3.3 Transport obligation

`PreparedHttpRequestV1` generalizes `bearer_secret()` to `auth_header() -> (&'static str, &[u8])`
— name plus secret bytes, computed by core, never exposed to plain headers. Both transports
(buffered + streaming) inject that pair as a sensitive, zeroized header; for the `Bearer` arm the
value keeps its `Bearer ` prefix, for `HeaderSecret` the secret is the verbatim value. Everything
else (redirect denial, decompression bans, bounds) is untouched.

### 3.4 Migration note (breaking, ships as 0.3.0)

A host that implements its own `AsyncHttpTransport` / `AsyncStreamingTransport` must update:

- `PreparedHttpRequestV1::bearer_secret()` is **removed**; use `auth_header()`, which returns the
  header name alongside the value.
- **The `Bearer ` prefix is now assembled by core.** A transport that keeps prefixing the value
  itself produces `Bearer Bearer …` and fails every upstream. Inject the returned bytes verbatim.
- `JsonPostRequestV1::auth()` returns `&ProviderAuthV1` (was `&BearerAuthV1`), and
  `JsonPostRequestV1::new` is no longer `const` (it takes `impl Into<ProviderAuthV1>`); passing a
  `BearerAuthV1` still compiles unchanged.

Hosts consuming only the crate-provided transports need no source changes. Because cargo treats
`0.2.x → 0.2.y` as compatible, this slice must be released as **0.3.0**, never as a patch. The
`0.2.0` slot was taken by the bounded provider quota response metadata release, which merged to
`main` and was tagged before this branch integrated it.

## 4. Conformance (D4)

The `south.provider-call.v1` and `south.provider-stream.v1` fixture tables are frozen. Options:

- **(a)** bump both suites to v2 with one added case each (header-secret success, asserting the
  wire carries the sanctioned header and no `Authorization`), keeping all existing cases
  byte-identical;
- **(b)** a separate tiny `south.header-auth.v1` suite (2 cases: buffered + streaming success)
  that hosts run alongside the existing suites.

Recommendation: **(b)**. The existing suites' IDs are burned into two hosts' verified status and
their evidence records; a version bump would force both hosts to re-run adoption paperwork for a
purely additive scheme. A dedicated suite keeps capability provenance clean —
`host_capabilities` gains `header_auth` per host when its adoption slice lands.

Implemented as **three** frozen cases rather than the two sketched above: a negative
`HeaderSecretSlotMismatch` case was added so the suite also anchors the zero-call evidence
(binding mismatch must reach neither resolver nor transport) for the new auth arm. The frozen
table is the authority; this paragraph records the deliberate delta from the approved sketch.
Coverage split noted for host adoption reviews: the mismatch case runs the buffered entry point,
while the streaming arm's mismatch behavior is pinned by core tests (both arms share the binding
check and auth assembly code paths).

## 5. Scope

In: the contract additions above; core auth-resolution generalization; both transports; the new
conformance suite + testkit runner extension; wire-level tests asserting the exact header, its
absence from plain headers, and `Authorization` absence for header-secret calls.

Out: GET support (separate slice), SigV4 / JWT / OAuth families (blocked on the §7.1 cross-team
review), any host adoption (separate slices per host, as always), multi-secret schemes (e.g.
bearer + org-id header: the org id is not a secret and already travels as a plain header).

## 6. Decisions for lv

- **D1 — sanctioned header set as a frozen enum** (vs validated open string). Recommended: frozen.
- **D2 — reuse `BearerAuthV1` as the slot carrier** under the header arm (vs a renamed type).
  Recommended: reuse.
- **D3 — `Into<ProviderAuthV1>` keeps the v1 constructor signature** (vs a second constructor).
  Recommended: `Into`.
- **D4 — separate `south.header-auth.v1` conformance suite** (vs bumping the two frozen suites).
  Recommended: separate suite.
