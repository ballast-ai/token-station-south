# Controlled User-Agent Support Vertical Slice

Status: slice authorized by lv (2026-08-20, host issue #25 prioritized as "small surface, unlocks
two host batches, existing template"); D1–D4 below ship as recommended and are flagged for review
on the pull request.

Date: 2026-08-20

Predecessors: `2026-08-18-controlled-query-support.md` (the sanctioned-channel template this slice
follows, including the measured-probe lesson its postscript records),
`2026-08-17-header-secret-auth.md` (the frozen-whitelist + dedicated-suite precedent).

## 1. Problem

`RESERVED_HEADERS` bans `user-agent`, so `SafeHeaders` refuses it and no provider request can carry
one. That ban is correct as a default — the header names a client identity, and a host smuggling
arbitrary values through it would be indistinguishable from the generic header hole the reserved
list exists to close. But it is now the single capability gap blocking the enterprise host's next
two migration batches (host issue #25). Measured against that host's real inventory:

| Provider | Requirement | Value today |
|---|---|---|
| `glm-coding` (Z.AI Coding Plan) | upstream whitelisted-tools policy rejects other agents | `opencode/1.15.6` |
| `github-copilot` | editor impersonation alongside the minted bearer | `GitHubCopilotChat/0.43.0` |
| `kiro` (CodeWhisperer) | one of five mandatory headers on its calls | `aws-sdk-js/1.0.0 KiroIDE` |
| `claude-code` (login-token form) | CLI identity; the setup-token form needs none | `claude-cli/2.1.114 (external, cli)` |

The important property of that table is the same one the controlled-query slice found in its own:
**no value originates from an end user, an operator, or a credential.** Every one is a compile-time
literal in the host's program text — a `const`. They drift only when the host releases a new
impersonation target version, which is a host source change by definition.

We are not asking to relax the reserved list. The ask, as the host issue words it, is a
**sanctioned, explicitly-versioned opt-in**: the host declares intent through a typed contract
surface instead of smuggling a header.

## 2. What a user-agent shakes (and what it does not)

A user-agent is a plain request header. It is logged wherever headers are logged — less reliably
than a query, which was the previous slice's exposure — and it moves neither the URL nor the auth
channel. The three things worth stating precisely:

- **It must not become a generic header channel.** The sanctioned surface sets exactly one header
  with a fixed, non-configurable name. A design that let the host name the header would reopen the
  reserved-list hole with extra steps. The name `user-agent` is baked into the capability, the way
  `authorization` is baked into the Bearer arm.
- **The value must not become a secret channel.** The query slice closed its channel by freezing
  the parameter *names* while letting values stay runtime data, because Azure's `api-version`
  genuinely arrives from operator configuration. That shape does not transplant here — see D1 —
  so this slice closes the channel one level stronger: the sole constructor takes
  `&'static str`, so a value must exist in the host's program text. No path exists from
  `CredentialResolver` output, configuration, or request data to a user-agent value, not because a
  validator rejects it but because the type cannot be built from it. (A `Box::leak` defeats this,
  as `'static` provenance is a discipline claim, not a proof against a hostile host — the same
  standing caveat as adapter-reported evidence.)
- **The reserved list stays intact.** `SafeHeaders` continues to reject `user-agent`. The
  sanctioned field and the reserved list together give the exactly-once property structurally:
  the prepared request has one optional typed slot for this header and no other source of it.

## 3. Contract design (`south-contracts`)

### 3.1 The value type

```rust
pub struct ControlledUserAgentV1(&'static str); // Copy

impl ControlledUserAgentV1 {
    pub const fn try_from_static(value: &'static str) -> Result<Self, ContractErrorV1>;
    pub const fn as_str(self) -> &'static str;
}
```

Ruling requested (**D1**): `'static` provenance versus an owned validated string (the
`QueryStringV1` shape). Recommendation: **`'static`**. The query precedent froze names and left
values runtime because its values *are* runtime (operator-configured `api-version`). Freezing
user-agent *values* into a south enum would be the naive transplant of that precedent, and it is
wrong here for the opposite reason: the values are host product identities that version-drift with
host releases (`claude-cli/2.1.114` → next), so a value enum would force a south release per host
UA bump and would encode host impersonation policy into this library. What is actually invariant
across the audited inventory is the values' *provenance* — compile-time literals — so that is what
the type freezes. Widening to runtime-composed values later is an additive new constructor;
narrowing back would be impossible. The constructor is `const fn`, so a host can build its
user-agents in `const` context and a malformed literal fails at host compile time.

Ruling requested (**D2**): the value grammar. Recommendation: non-empty, at most
`MAX_USER_AGENT_BYTES` (256), every byte printable ASCII including space (`0x20..=0x7E`), and no
leading or trailing space. This admits all four inventory values (product tokens, slashes, dots,
parentheses, commas, single spaces) and rejects control bytes, CR/LF, DEL, and non-ASCII — so an
accepted value is by construction a valid HTTP header value, and the transport's
`HeaderValue::from_str` cannot fail on it. The bound is 256 rather than a tight fit around today's
longest value (34 bytes) for the same reason `api-version` got 64: the character class does the
security work, and a bound that tracked observed values would break on the next host release
without protecting anything.

### 3.2 Where the declaration attaches

```rust
impl JsonPostRequestV1 {
    pub fn with_user_agent(self, user_agent: ControlledUserAgentV1) -> Self;
    pub const fn user_agent(&self) -> Option<ControlledUserAgentV1>;
}
```

The same builder shape as `with_query`, for the same reason: existing call sites compile
unchanged, and a v2 request is exactly a v3 request with no user-agent. `SafeHeaders`,
`RESERVED_HEADERS`, and every existing grammar stay byte-identical.

New error variant: `ContractErrorV1::InvalidUserAgentValue` (`INVALID_USER_AGENT_VALUE`),
following the four query variants added by the previous slice.

`Debug` for the new type prints shape only (byte count), matching `QueryStringV1`: the values are
provider-adjacent strings that land in logs, and the redaction discipline is uniform rather than
per-field-judged.

### 3.3 Versioning

`HTTP_CONTRACT_VERSION: 2 → 3`, additive by the established test: a v2 request is exactly a v3
request with no user-agent declaration. `compatibility.json` mirrors it. Because `ContractErrorV1`
gains a variant (a compile-breaking change for exhaustive matches, exactly as the query slice's
variants were), this ships as **0.6.0**, never as a patch.

## 4. Core and transport

`PreparedHttpRequestV1` gains a private `user_agent: Option<ControlledUserAgentV1>` field,
populated in `assemble`, with a `user_agent()` accessor. The binding, resolution, and auth logic
is untouched — the declaration does not interact with the URL or the credential path.

`south-transport-reqwest`'s `assemble_headers` inserts the header when the prepared request
declares one. Exactly-once needs no runtime check: ordinary headers cannot contain `user-agent`
(reserved), the auth header is a different name by the same argument, and the transport's client
is built without a default user-agent — a fact the transport tests now pin from both sides
(declared value on the wire verbatim, and no `user-agent` at all when undeclared).

### Migration note (behavioral widening, ships as 0.6.0)

- **A host implementing its own transport** must start applying
  `PreparedHttpRequestV1::user_agent()`, or every declared user-agent silently vanishes from the
  wire — the same silent-breakage class as the query slice's URL-logging note: it compiles, and
  the provider rejects at runtime. The conformance suite catches this for hosts that run it, which
  is the point of adopting per host.
- **A host that forwards client traffic** must not confuse this channel with client-supplied
  user-agents. The type cannot be built from request data (`'static`), which enforces the
  distinction structurally.

## 5. Conformance

Ruling requested (**D3**): a dedicated `south.controlled-user-agent.v1` suite (five cases below)
versus extending frozen suites. Recommendation: **dedicated**, per the two-slice precedent —
existing suite ids are burned into both hosts' evidence records.

1. `BufferedUserAgentSuccess` — buffered exchange declaring a user-agent; evidence
   `(One, One, wire_user_agent_exact: true)`.
2. `StreamingUserAgentSuccess` — streaming exchange declaring one; `(One, One, true)`.
3. `InvalidUserAgentValueRejected` — a declaration violating the grammar (leading space), refused
   before any boundary; `(Zero, Zero, false)`.
4. `UserAgentFreeRequestReachesTheWire` — no declaration, transport reached, still `false`. This
   is the measured-probe row the query suite had to add after a real adapter passed with a
   hardcoded probe (its design's postscript records the incident and its second catch). This suite
   inherits the row — and the table test freezing "exactly one case reaches the transport and
   expects `false`" — from day one rather than rediscovering the hole.
5. `ReservedHeaderDeclarationStillRejected` — no typed declaration, but the input's ordinary
   headers carry a plain `user-agent` pair; the assembled path must refuse at header validation
   with zero calls. This is clause (b) of the host issue: the sanctioned channel is an opt-in,
   not a relaxation, and this row turns that from prose into a machine-checked fact against the
   assembled path (the contract-level rejection was already pinned in `safe_headers.rs`).

`wire_user_agent_exact` carries the same *presence* polarity as `wire_query_exact`: it can only
become true by observing a transport-boundary request whose declared user-agent exists and matches
byte for byte, so the declaration-free rows expect `false` for two different, individually
load-bearing reasons (never reached; reached but nothing declared).

Ruling requested (**D4**): stable failure codes for the two negative rows, given the frozen
19-code set (which deliberately has no header-policy code). Recommendation: case 3 folds
`InvalidUserAgentValue` into `INVALID_RELATIVE_PATH`, joining the query contract errors — the
frozen set has exactly one preparation-time provider-declaration code, and every sanctioned-channel
declaration error now folds there; case 5 surfaces through the existing header-policy fold to
`REQUEST_FAILED`, whose "canonical fixtures never carry policy-violating headers" comment is
updated, since this suite now deliberately does. In both rows the zero-call evidence is what
distinguishes the deterministic rejection from a broken transport, and the finer `ContractErrorV1`
reason stays available to hosts.

`host_capabilities` gains a `controlled_user_agent` key per host, `not_verified` until each host
runs its own adoption slice; the evidence-freshness test learns the new capability so a future
case-table change demotes stale evidence automatically.

## 6. Fuzz and property obligations

None added, and the absence is deliberate rather than an oversight. The fuzz targets exist for
parsers that consume untrusted or unbounded input. This grammar's input is `&'static str` by
type — program text, the trusted end of the spectrum — and the check is a single stateless pass
(length, byte class, edge bytes) with no parse state to explore. The boundary behavior is pinned
instead by exhaustive literal tests: every rejected byte class, both length edges, both space
edges, and all four inventory values. The existing query fuzz and property obligations are
untouched because no existing grammar changes.

## 7. Scope

In: the value type and grammar; the request attachment; prepared-request and reqwest transport
application; the dedicated conformance suite, reference executor, and runner; manifest and version
bump.

Out: the multi-auth-header gap (Gemini `/openai/` dual header — tracked by the host's gap list,
issue #26 adjacent); any host-configurable header name; runtime-composed user-agent values (an
additive later constructor if a real consumer appears); request signing (issue #26); any host
adoption (separate slices per host, as always).
