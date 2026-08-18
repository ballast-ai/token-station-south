# Controlled Query Support Vertical Slice

Status: D1–D5 all ruled as recommended by lv (2026-08-18); implementation authorized

Date: 2026-08-18

Predecessors: `2026-08-16-minimal-provider-call.md` (relative-path grammar and the URL binding
loop), `2026-08-17-header-secret-auth.md` (the frozen-enum + dedicated-suite precedent this slice
follows).

## 1. Problem

The URL contract admits no query string. `RelativePathV1::parse` rejects `?` outright, and
`resolve_against` rejects `query().is_some()` after normalization. That ban is load-bearing for the
minimal slice, but it is now the single largest limit on adoptable provider surface — larger than
the auth-scheme limit the previous slice removed.

Measured against one host's real inventory (that host's adoption record holds the full table), the
text surfaces produce exactly two query shapes:

| Parameter | Provider family | Value source | Example |
|---|---|---|---|
| `api-version` | Azure OpenAI | operator config or catalog row, three-level fallback to a compile-time default | `2024-10-21`, `2025-04-01-preview`, `v1` |
| `alt` | Gemini native streaming | compile-time literal, gated on the request's stream flag | `sse` |

Two more exist off the text surfaces (`GroupId` on one media provider, `task_id` on async media
polling); both are already percent-encoded by that host and neither reaches the call shapes this
library supports today.

The important property of that table is not its size. It is that **no value on it originates from
an end user**. Every one is a compile-time literal, an operator-set configuration field, or an
identifier echoed back by the upstream itself. There is no client-controlled query injection
surface to defend against in the adopting host — which is what makes a narrow, closed-world design
sufficient here, and what makes a general-purpose query validator unnecessary.

Unlocking these two parameters admits Azure OpenAI across chat, responses, and embeddings, and
Gemini native streaming — the providers the previous slice identified as blocked, and blocked on
this, not on authentication.

## 2. What a query actually shakes (and what it does not)

The instinct is that a query might escape the host binding. It cannot, and the reason is
structural rather than defensive: `same_origin` compares scheme, host, and effective port;
`inside_base` compares `reparsed.path()`. A query lives in `reparsed.query()`. Percent-encoded dot
segments inside a query are not normalized across the `?` boundary, and a `#` inside a query
terminates the query rather than extending the path. **The origin and traversal ring stays closed
with or without this slice.** That must be stated plainly so review effort goes where the real
exposure is.

The real exposure is that a query is **the most reliably logged component of an HTTP request**.
Proxies, CDNs, upstream access logs, and `Referer` propagation all capture it; header values
mostly are not captured that way. The v1 design named this exactly once, as a non-goal — "OAuth
exchange, arbitrary authentication headers, SigV4, self-signed JWT, or **query credentials**" —
and again by listing `query` among the values errors and `Debug` must never contain.

So the query channel is the header channel's twin, and the header channel is not governed by
validation: it is governed by `RESERVED_HEADERS`, a closed list that makes it impossible for a
provider to name a secret-bearing header at all. A query design without an equivalent structural
answer would be asymmetric with the story the codebase already tells about headers.

This slice's answer is the same shape as `SecretHeaderV1`: **a frozen enum of sanctioned parameter
names**. A provider cannot name `key`, `access_token`, or `sig`, because those names do not exist
in the type. The secret-exfiltration channel is closed by construction rather than by a
blocklist, exactly as the reserved-header policy closes its twin.

The second protection is unchanged from v1 and must stay stated: no credential value exists in
scope during URL processing. `ProviderAuthV1` remains the sole path a secret takes to the wire,
and query values are resolved from the request declaration, never from `CredentialResolver`
output. This slice adds no code path from a resolved secret to a URL, and the type system should
keep it that way (see D3).

## 3. Contract design (`south-contracts`)

### 3.1 The sanctioned parameter set

```rust
pub enum QueryParameterV1 {
    ApiVersion, // "api-version"
    Alt,        // "alt"
}
```

Ruling requested (**D1**): frozen enum, mirroring `SecretHeaderV1`. Adding a parameter is a
deliberate contract bump with a conformance case, not host configuration. The alternative — a
validated free-form name with a character-class rule — reopens precisely the exfiltration channel
§2 closes, because `key=` and `access_token=` pass any reasonable character class.

Values are **not** free-form either. Each parameter carries a value grammar:

- `ApiVersion` — `[A-Za-z0-9._-]`, non-empty, at most `MAX_QUERY_VALUE_BYTES` (64). This
  deliberately admits `2024-10-21`, `2025-04-01-preview`, and `v1`, and rejects anything carrying
  a separator, percent sign, or space. Note this is *stricter than the adopting host's own
  handling*, which applies no sanitization to that field today; the contract closes that gap
  rather than inheriting it. The bound is 64 rather than a tighter fit around today's longest
  real value (18 bytes) because the character class, not the length, is what does the security
  work here — a length that tracked observed values would break on the next provider's naming
  scheme without adding any protection.
- `Alt` — a closed set: `sse` or `json`.

Ruling requested (**D2**): per-parameter value grammar (above) versus one shared unreserved-character
rule for all values. Recommendation: **per-parameter**. The set is small, the grammars are known
exactly, and a shared rule would admit `alt=anything`, which no upstream accepts and which would
turn a contract error into a runtime 400.

### 3.2 Where the query attaches

The query belongs to the request, not to the path:

```rust
pub struct QueryStringV1 { /* ordered, deduplicated, bounded */ }

impl JsonPostRequestV1 {
    pub fn query(&self) -> Option<&QueryStringV1>;
}
```

`RelativePathV1::parse` keeps its current grammar **unchanged** — it continues to reject `?` and
`#`. This is the central structural decision, and it is what keeps this slice additive (§3.4): a
path is still only a path, and no existing accepted path changes meaning.

Ruling requested (**D3**): attach the query to the request (above) versus widening
`RelativePathV1` to carry an optional query. Recommendation: **attach to the request**. Three
reasons. It leaves the frozen path grammar and its fuzz invariants untouched. It keeps the
adopting host's `decompose_upstream_url` honest — that helper splits a URL by string prefix and
would otherwise hand a `?`-bearing remainder to a parser that now silently accepts it. And it
makes the "no secret in a URL" property checkable at one place: `QueryStringV1` is constructed
from the sanctioned enum and never from resolver output, so no `impl From<SecretValue>` can exist
for it.

Bounds: at most 4 parameters, at most 256 bytes serialized, no duplicate names. Duplicates are a
contract error rather than a last-wins normalization, because parameter pollution — where the
gateway and the upstream disagree about which duplicate wins — is the classic failure mode of
permissive query handling.

### 3.3 The join, and why the parser alone is not enough

The current join is `format!("{}{}", endpoint.path(), relative)` followed by `set_path`. Passing a
query through that path would percent-encode `?` into `%3F`, producing a literal path segment
rather than a query — a silent, hard-to-debug corruption. The join therefore gains an explicit
step:

```rust
destination.set_path(&destination_path);
if let Some(query) = query { destination.set_query(Some(query.as_str())); }
let reparsed = Url::parse(destination.as_str())?;
```

and the post-normalization recheck changes from "query must be absent" to "query must equal what
we set, and everything else must be unchanged":

- `same_origin`, `inside_base`, empty username, absent password, absent fragment — **all
  unchanged**;
- `reparsed.query()` must equal the serialized `QueryStringV1` **byte for byte**. If `url`
  re-encodes anything, the values were not what the grammar promised, and that is a preparation
  error rather than something to accept.

That byte-for-byte equality is the load-bearing new check. It is what makes the recheck a proof
rather than a formality, and it is why `#` inside a value cannot survive: `set_query` would place
it such that `reparsed.query()` no longer matches the input, and preparation fails.

### 3.4 Versioning

`HTTP_CONTRACT_VERSION: 1 → 2`. Additive by the established test: a v1 request is exactly a v2
request with no query. `compatibility.json` mirrors it. `JsonPostRequestV1::new` keeps its
signature; the query arrives through a separate builder method so existing host call sites compile
unchanged, following the `impl Into<ProviderAuthV1>` precedent.

`ProviderEndpointV1` keeps its query ban. Since the endpoint contributes no parameters, the
request's query is the entire query and **there is no merge semantics to define** — no
host-versus-provider precedence rule, and therefore no parameter-pollution surface at the join.
Recommend keeping it that way; relaxing the endpoint later would require answering the merge
question, and every answer to it has a pollution failure mode.

### 3.5 Migration note (behavioral widening, ships as 0.4.0)

Hosts consuming the crate-provided transports need no source change: a request with no query
behaves exactly as before. Two obligations for hosts that do more:

- **A host implementing its own transport** already receives a full `Url` from
  `PreparedHttpRequestV1::url()`, so a query flows through transparently with no compile error.
  That is the hazard: **a transport that logs the request URL begins logging provider-authored
  bytes the moment any provider declares a query.** Such a host must re-check its URL logging and
  redaction before upgrading. This is the same class of silent breakage as the `Bearer ` prefix
  in 0.3.0 — it compiles, and it fails or leaks at runtime.
- **A host that splits a full URL into base and relative** (the adopting host does) must not hand
  a `?`-bearing remainder to `RelativePathV1::parse` and expect the new support to apply. The path
  grammar is unchanged and still rejects it; the query must be lifted out and declared explicitly.
  A host that skips this sees the same silent fallback it sees today, which is safe but means the
  slice delivers nothing.

Because cargo treats `0.3.x → 0.3.y` as compatible, this ships as **0.4.0**, never as a patch.

## 4. Conformance

Ruling requested (**D4**): a dedicated `south.controlled-query.v1` suite versus extending the
frozen `south.provider-call.v1` and `south.provider-stream.v1` tables. Recommendation:
**dedicated suite**, following the D4 precedent from the header-secret slice — existing suite IDs
are burned into two hosts' verified status and their evidence records, and a version bump would
force both hosts to redo adoption paperwork for a purely additive capability.

Proposed frozen cases:

1. buffered success carrying `api-version`, asserting the wire query is exactly the declared one;
2. streaming success carrying `alt=sse`, same assertion;
3. a negative case: a value violating its parameter grammar must fail preparation with **zero**
   resolver and transport calls — the zero-call evidence discipline the header-auth suite
   established for its slot-mismatch case.

As implemented the suite carries five cases, not three: a fourth proving canonical serialization
order is independent of declaration order, and a fifth — added the same day, see below — proving
the wire-query probe is actually measuring.

`host_capabilities` gains a `controlled_query` key per host, `not_verified` until each host runs
its own adoption slice.

**A blind spot in this suite, measured during the first host adoption (2026-08-18) and closed the
same day by a fifth case.** Recorded here in full because the shape of the hole is more instructive
than the fix.

*The hole.* The original four cases expected `wire_query_exact` of `true / true / false / true`,
and the only `false` belonged to the zero-call case — where the probe is never invoked at all.
That `false` was therefore held by "structurally never reached", not by "measured and found
false". An adapter whose probe unconditionally reported `true` without ever reading the prepared
URL passed the whole suite: the three `true` expectations were satisfied by the hardcoded value,
and the `false` one by the zero call. This was confirmed by mutation on a real host adapter,
alongside two mutations the suite *did* catch (dropping the query entirely, and comparing
declaration order instead of canonical order).

*The fix.* Case five, `QueryFreeRequestReachesTheWire`: a request declaring **no** query at all,
reaching the transport via a `Response(...)` upstream, with expected evidence `resolver_calls:
One`, `transport_calls: One`, and `wire_query_exact: false`. It is the table's first and only case
that both reaches the transport and expects `false`, which is exactly the cell the original four
left empty.

It closes the hole because of the field's *presence* polarity. A correct probe reads
`PreparedHttpRequestV1::url()`, finds `query() == None`, compares that against a request that
declared nothing, and reports `false` — nothing was observed carrying a declared query, because
none was declared. Note the claim is not "the wire query matches the declaration": under that
reading `None == None` would be `true` and the case would prove nothing. It is "a declared query
was observed on the wire", which requires a declaration to exist. An adapter hardcoding `true`
reports `true` here and fails with a `WireQuery` mismatch. The mutation that previously passed the
suite now fails on exactly this case and no other.

Two implementation notes for anyone extending the table. The empty declaration must **not** be
handed to `QueryStringV1::try_from_iter` — that constructor has no empty representation and answers
`ContractErrorV1::EmptyQuery`, which would silently convert this case into a second zero-call
rejection case and restore the blind spot while appearing to have five cases. And the fixture table
freezes the property, not just the case: a test asserts that exactly one case reaches the transport
while expecting `false`, so deleting or flipping this case reopens the hole loudly.

Adding this case does **not** invalidate evidence already recorded by a verified host. The suite
version stays at `1`: the four existing cases are byte-identical in declaration, upstream, outcome,
and expected evidence, so any adapter that genuinely measured its wire still passes unchanged.
What the fifth case changes is the *class of adapter that can pass* — it excludes the ones that
were never measuring. A previously verified host must therefore re-run the suite to keep claiming
`verified`, not because its evidence was wrong, but because the suite no longer accepts the
evidence being unmeasured. That is a strengthening of the same assertion, not a new one.

With this case in place, an adopting host's probe wiring moves from a **review item** to a
machine-checkable fact for the specific failure of not measuring at all. The broader rule in
`ControlledQueryEvidenceV1` — adapter-reported evidence is insufficient on its own for host
verification — still holds for everything the runner cannot reach, including the next paragraph's
failure mode.

**A second, host-side failure mode found in the same review, worth naming because it is easy to
repeat.** The negative case tempts an adapter to short-circuit: construct the `QueryStringV1`,
find it invalid, and return a failure observation without ever entering the assembled path. An
adapter that then *hardcodes* the evidence as `(0, 0, false)` rather than reading its own counters
reports the expected values by construction — so a host that quietly sent the rejected request
anyway would still pass. That is precisely the case `wire_query_exact`'s presence polarity was
designed to catch, defeated by the adapter never measuring anything.

Nothing in the suite can detect this, because the evidence is adapter-reported by definition. The
guidance for adopting hosts is therefore explicit: **evidence values must be read from the same
instrumentation the success cases use, on every path including early returns.** A host review
should ask to see the negative case's evidence expression and confirm it loads counters rather
than naming constants. The first adopting host shipped this bug and fixed it on review.

## 5. Fuzz and property obligations

`fuzz/fuzz_targets/contract_parsers.rs` asserts that **every accepted relative path resolves
successfully against every valid binding**, and `parser_properties.rs` mirrors it. Because §3.3
keeps the path grammar unchanged, that invariant survives untouched — which is a deliberate
benefit of D3's shape. Had the query been folded into `RelativePathV1`, widening the parser without
simultaneously widening `resolve_against` would panic the fuzzer; the tripwire is correct and this
design routes around it rather than defusing it.

New obligations:

- extend the fuzz target to construct `QueryStringV1` from arbitrary input and assert: bounded
  serialization, no duplicate names, idempotent parse, and — the important one — that a
  successfully constructed query always survives `resolve_against` byte for byte;
- a property asserting that for every `QueryParameterV1` variant, a value accepted by its grammar
  round-trips through `set_query` unchanged.

## 6. Scope

In: the sanctioned parameter enum and value grammars; `QueryStringV1` and its request attachment;
the join and post-normalization recheck; the conformance suite; fuzz and property extensions.

Out: GET support (still a separate slice — this one is a prerequisite for it, not a substitute);
endpoint-level query; free-form or host-configurable parameter names; any host adoption (separate
slices per host, as always); query values sourced from credentials, which stay a permanent
non-goal rather than a deferred feature.

## 7. Decisions for lv

- **D1** — frozen `QueryParameterV1` enum versus validated free-form names. Recommend frozen; the
  free-form option reopens the exfiltration channel §2 closes.
- **D2** — per-parameter value grammars versus one shared character class. Recommend
  per-parameter.
- **D3** — query attaches to the request versus widening `RelativePathV1`. Recommend the request;
  it preserves the frozen path grammar and its fuzz invariants, and keeps the no-secret-in-URL
  property checkable at a single type.
- **D4** — dedicated `south.controlled-query.v1` suite versus bumping the two frozen suites.
  Recommend dedicated, per the header-auth precedent.
- **D5** — the initial sanctioned set. Recommend starting at exactly `api-version` and `alt`, the
  two the text surfaces need. `GroupId` and `task_id` are real but serve media surfaces this
  library's call shapes do not yet cover; admitting them now would freeze two names into the
  contract ahead of any conformance case exercising them.
