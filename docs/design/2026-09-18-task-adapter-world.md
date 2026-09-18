# The Task Adapter World: the second world, proposed

Status: **proposed** 2026-09-18. Issue: #52's WIT lowering and #53's
`KNOWN_WORLDS` entry, both of which their records deferred to "the world's own
slice". This is that slice, scoped to admission only: the world, its
vocabulary, and gate ① learning to admit a task manifest. No component is
authored here and no host is wired here.

Date: 2026-09-18

Predecessors: `2026-08-27-task-adapter-vocabulary.md` (D1–D3 froze the
semantics this world lowers to WIT; shipped as `south-contracts::task` in
0.19.0), `2026-08-27-manifest-schema-beyond-one-world.md` (D1 made the suite
name, capability vocabulary, auth arms and WIT package properties of a
declared world, shipped in 0.17.0 — the parameterisation this record fills a
second row of), `2026-09-08-buffered-get-request.md` (`GetRequestV1`, the
observe half's transport, shipped in 0.24.0).

## 1. Problem

Two records ruled a task component into existence and then stopped at the same
line. The vocabulary record froze `HostMintedValuesV1`, `TaskObservationV1` and
`TaskFailureKindV1`, and said "the WIT lowering and the task world's
`KNOWN_WORLDS` entry land with the world's own slice". The manifest-schema
record generalised the schema so a second world could exist, named that world
`task-adapter-v1`, and said its capability vocabulary "belongs with that
world's own slice".

So the types exist, the schema can describe more than one world, and the
transport the observe half needs shipped three weeks ago — but
`KNOWN_WORLDS` still has one row, `wit/` still has one file, and gate ① still
refuses every task manifest with `ApiVersionIsNotTheWorld`. A component author
has a frozen vocabulary and no world to declare.

This record fills the second row.

## 2. Scope: admission only

The adopting host's plan 44 §5 warns that sinking a contract before it is
settled amplifies rework across two repositories plus ABI version management,
and this repository has its own precedent in the same direction: an empty
`south-transport-ureq` was removed because "the reservation cost maintenance
without protecting anything".

Both cautions point the same way, so this slice deliberately stops at
admission:

- **In:** the WIT world, its capability vocabulary, its `WorldSchemaV1` row,
  gate ① admitting a task manifest, and the refusals that keep the two worlds
  from being confused for each other.
- **Out:** any real component, the conformance suite's *fixtures*, the host
  seam, and the artifact-fetch execution path.

The suite *name* is frozen here because it is tuple field 6 and a manifest
cannot be validated without it. Its *content* is authored per family, as
fixtures always are in this repository.

**Why the suite name can be frozen before its fixtures exist.** The name is
what a manifest declares; the fixtures are what a component is judged by. The
vocabulary record already fixed one required fixture row per family (a 404
query body, D3's consequence) — that obligation attaches to the family's
author, not to this slice.

## 3. D1 — A separate WIT package, not a second world in `token-station:adapter`

**The question.** `provider-adapter-v2` lives in
`token-station:adapter@2.0.0`. A second world could join that package or start
its own.

**Recommendation: its own package, `token-station:task-adapter@1.0.0`.**

Three reasons:

1. **Independent lifecycles.** A package version is shared by every world in
   it. Housing both means a chat-side change forces a task-side package bump
   and vice versa, and tuple field 3 (`compatibility.wit_package`) is how a
   component declares which bytes it was built against. Coupling them makes
   that field lie about what actually changed.
2. **The schema already supports it.** `WorldSchemaV1.wit_package` is a
   per-world field and `validate_compatibility` compares a manifest's declared
   package against *the declared world's* package. Nothing needs generalising
   — the 0.17.0 slice did that work. Sharing one package would waste it.
3. **The two worlds do not share a function.** A chat world exports
   `build-http-request` / `parse-response` / `parse-stream-chunk`; a task world
   exports a lifecycle. There is no type they both name that is not already in
   `south-contracts` or the IR, and both reach those as JSON documents, not as
   WIT imports.

**Rejected: one package, two worlds.** It reads tidier and costs a false
version signal on every release.

## 4. D2 — The function set is seven, and `usage-intent` is not in this slice

The host's plan 46 §2 decomposes its proven in-binary seam into pure halves.
Lowered, and keeping this repository's `build-*` / `parse-*` naming:

| Host hook | World function | Returns |
|---|---|---|
| `submit` | `build-submit-request` | `HttpRequestDescriptor` |
| | `parse-submit-response` | the upstream task id |
| observe | `build-observe-request` | `HttpRequestDescriptor` (a GET) |
| | `parse-observation` | `TaskObservationV1` |
| `render_success` | `build-artifact-request` | `option<HttpRequestDescriptor>` |
| | `render-success` | the success body |
| `map_failure` | `map-terminal-failure` | `ErrorEnvelope` |

`timed_out` does not lower: a waiting budget is host policy, and D3 rule 5
already forbids either side from synthesising a terminal from a clock.

`option<…>` on the artifact request is the MiniMax shape: five of six families
have everything in the terminal observation, and one must be fetched. `None`
means "already have it", not "not supported".

**`usage-intent` is deliberately absent from this slice.** The host's T-5
ruling puts metering in the component and pricing in the host, and that is the
right split — it is also the one the adopting host has already implemented
internally, so its shape is known rather than guessed. But it is a *funds*
adjacent contract, and this slice admits components that cannot yet be run by
anyone. Freezing a metering vocabulary with no executing consumer is precisely
what `ARCHITECTURE.md` refuses when it says a metering vocabulary "is admitted
only with a second consumer in sight". It lands with the host seam.

A world may gain an export in a minor; it may not change one. Leaving
`usage-intent` out now costs a later package minor and protects the vocabulary
from being frozen blind.

## 5. D3 — The capability vocabulary

The provider world's four words mix two kinds: `chat` and `stream` name world
functions, `tool_call` and `json_schema` name request fields the component
promises to translate. The task world needs only the first kind, because its
request body is the dialect's own and it promises nothing about IR fields.

**Recommendation:** `["submit", "observe", "render", "artifact_fetch"]`.

- `submit`, `observe`, `render` — the three lifecycle stages, each naming the
  function pair above. All three are required; a component missing one cannot
  complete a task.
- `artifact_fetch` — **optional**, and the only optional word. It declares that
  `build-artifact-request` may return `Some`. A component that never fetches
  omits it, and a host reading the manifest knows before the first call whether
  the family needs that execution path.

**Why `artifact_fetch` is a capability and not just a runtime `None`.** The
host must decide whether to wire an execution path, and D3's discipline is that
a host should not have to infer a component's shape by calling it. The runtime
`Option` stays as the per-request answer; the capability is the per-component
promise.

**Rejected: mirroring the provider world's `chat`-is-mandatory rule verbatim.**
`validate_role` hard-codes `PROVIDER_WORLD` today. The task world's mandatory
set is three words, not one, so the check becomes per-world rather than an
`if` on one constant.

## 6. D4 — Auth arms: the same four, unchanged

A task component authenticates exactly as a chat component does: it names a
credential, never holds one. `bearer`, `header_secret`, `oauth` and
`host_signed` all apply unchanged, and `host_signed` matters more here than in
chat — Kling's HS256 JWT and Bedrock's SigV4 are both task-side families.

No new arm. D1 of the vocabulary record is the reason there is nothing to add:
the task entry points take a **pre-resolved** secret
(`PreparedSecretResolverV1`), so the arm vocabulary describes what the manifest
declares, not how the task half resolves it.

**The pinned-credential rule is a host obligation** and stays out of the
component suite, exactly as that record ruled. It belongs to a gate ③ host
suite, which this slice does not author.

## 7. What this record does not decide

- **Fixture content and per-dialect grammars** — authored per family.
- **`usage-intent`** — §4; lands with the host seam.
- **The host seam** (`SouthTaskAdapter`, the poller's call sites) — the
  adopting host's own slice.
- **Whether a task component may also be a provider component.** One manifest
  declares one world. Nothing here forbids shipping two components in one
  package, and no one has asked.

## 8. Versioning

A south **minor**. The WIT package `token-station:task-adapter@1.0.0` is new,
so nothing existing changes version: `token-station:adapter@2.0.0` is
untouched, and every existing manifest keeps validating against the row it
already matched.

`compatibility.json` gains no contract counter. The `task` vocabulary counter
(`"task": 1`) already exists from 0.19.0 and this slice does not change those
types; the world is described by `KNOWN_WORLDS`, which is code, not a counter.
