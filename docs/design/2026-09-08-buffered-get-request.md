# The Buffered GET Request: Task Polling Through South

Status: D1–D4 ruled 2026-09-08 (see §6); shipped as 0.24.0

Date: 2026-09-08

Predecessors: `2026-08-16-minimal-provider-call.md` (the JSON POST shape everything else extends),
`2026-08-18-controlled-query-support.md` (D5 deferred `task_id` alongside `GroupId`),
`2026-08-27-task-adapter-vocabulary.md` (§5 sized this slice and declined it "until the slice
that needs it"), `2026-09-08-controlled-query-group-id.md` (the `GroupId` precedent for admitting
a deferred parameter with its consumer).

## 1. Problem

Every buffered and streaming surface South offers is a JSON POST by construction:
`PreparedHttpRequestV1` sets `Method::POST` at both construction sites, its body is a mandatory
`JsonBodyV1`, and `south-contracts` has no `Method` at all. The task-adapter record sized the fix
and declined it because nothing consumed a GET.

Something does now. The server host's task poller (`modules/tasks/poller.rs`) issues one
**body-less GET** per poll for every asynchronous media task — the same endpoint, the same
credential slot, the same authentication arms as the submit leg that South already carries, a
20-second timeout, and a buffered JSON body it caps and parses. Per the host's 37-号 ledger this
is the largest remaining gap by far: 70 of 143 non-text models are blocked **only** by it (59
video, 9 image polling, 2 TTS). Their submit legs go through South today; their polling legs
cannot.

The six polling URL shapes the host produces:

| family | path | query |
| --- | --- | --- |
| Kling | `v1/videos/{endpoint}/{task_id}` | — |
| MiniMax | `v1/query/video_generation` | `task_id=<digits>` |
| Veo (AI Studio / Vertex) | `v1beta/{operation}` / `v1/{operation}` | — |
| Bailian | `api/v1/tasks/{task_id}` | — |
| BytePlus | `api/v3/contents/generations/tasks/{task_id}` | — |
| xAI | `v1/videos/{task_id}` | — |

Five carry the task id in the path, which `RelativePathV1` already accepts. One carries it as a
query parameter, which the frozen set does not name — the second of the two names D5 deferred.

## 2. Boundary claim

Unchanged. A poll is a provider call with no body; everything South refuses to own about a POST
(routing, retry, admission, persistence, credential sources) it equally refuses to own about a
GET. Task *state* stays host-owned, as the task-adapter record already rules. What South adds is
the ability to *send* the poll under the same containment, binding, and header discipline as the
submit.

## 3. Design

### 3.1 Contract (`south-contracts`)

```rust
/// A bounded provider request for one body-less GET operation.
pub struct GetRequestV1 {
    relative_path: RelativePathV1,
    headers: SafeHeaders,
    auth: ProviderAuthV1,
    query: Option<QueryStringV1>,
    user_agent: Option<ControlledUserAgentV1>,
}
```

The JSON POST request's field set minus the body, with the same builders (`with_query`,
`with_user_agent`) and the same grammars. `JsonPostRequestV1` is untouched — see D1.

`QueryParameterV1::TaskId`: wire name `task_id`, grammar a non-empty ASCII digit string of at
most `MAX_QUERY_VALUE_BYTES` (the only upstream that carries a task id in the query issues
numeric ids; a host that meets another shape falls back rather than widening the grammar —
the same posture `GroupId` took). Appended last in canonical order.

`HTTP_CONTRACT_VERSION: 5 → 6`, additive: a version-five request is exactly a version-six
request that is not a `GetRequestV1` and declares no `task_id`.

### 3.2 Seam (`south-core`)

`PreparedHttpRequestV1` gains a method and an optional body: `method()` already exists; `body()`
becomes `Option<&JsonBodyV1>`; `FinalizeViewV1::body()` is the empty slice for a GET (a SigV4
signer hashes the empty payload exactly as AWS specifies, so the host-signed arm works unchanged).
Two new entry points, buffered only:

```rust
pub async fn execute_get_call_v1<R, T>(binding, request: &GetRequestV1, resolver, transport, deadline, cancellation) -> …;
pub async fn execute_signed_get_call_v1<F, T>(binding, request, finalizer, transport, deadline, cancellation) -> …;
```

Same validation order, same binding check, same biased cancellation race as their POST twins.
There is no streaming GET — see D2.

The prelude gains `RawGetProviderCallV1` (the raw call minus `body`) with `parse_raw_get_call`,
`raw_get_call_parses`, and `execute_get_raw_call_v1`; `south-testkit` gains the owned builder.

### 3.3 Transports (`south-transport-reqwest`)

`execute_one` already takes the method from the prepared request; it stops attaching a body
when there is none. Nothing else changes: containment, redirect denial, bounds, and the
added-header set are method-independent.

### 3.4 Conformance

A dedicated `south.provider-get.v1` suite (D4), four frozen cases: buffered GET success under the
Bearer arm; the same under a sanctioned header; a slot mismatch refused before resolver and
transport; a GET carrying `task_id` whose wire query is exact. Evidence: resolver and transport
call counts and three wire-shape booleans measured at the transport boundary — `wire_method_get`
(presence polarity: `false` until a transport call observes `GET`), `wire_body_absent` (absence
polarity: vacuously `true` when the transport is never reached, `true` at the boundary only when
the prepared request has no body slot at all), and `wire_query_exact` (the controlled-query
suite's polarity: `true` only when a query was declared *and* the wire carried it exactly). Two
rows reach the transport and still expect `wire_query_exact == false`, so a probe that hardcodes
`true` fails a row. The fixture input is the provider-call input minus its body: a GET fixture
cannot carry one.

## 4. Consumer

The server host's task poller: for every provider already in South's Bearer or header-secret
scope, the poll leg becomes `execute_get_raw_call_v1` with the credential pinned at submit time.
The host keeps its Vertex minting (a prepared Bearer) and its per-family URL builders; South
receives the assembled relative path and, for MiniMax, the `task_id` declaration. Kill-switch key:
a new `SouthSurface::TaskPoll`, default off until the host records its parity run.

## 5. Versioning

Additive; ships as **0.24.0** with `compatibility.json` `http: 6` and the new suite. Both hosts
are annotated `provider_get: not_verified` until each runs the suite through its own adapter; the
evidence-freshness rule applies from the first run.

> 2026-09-09: the server host ran the suite 4/4 through its production adapter (dev-v2
> `89139b4d`; task poller behind a `task_poll` switch, default off) and is annotated
> `verified, cases: 4`. The community host has no task poller and stays `not_verified`.

## 6. Decisions — ruled 2026-09-08

- **D1 — a separate `GetRequestV1`, not a method field on `JsonPostRequestV1`.** The POST
  type's name, its mandatory body, and every invariant the streaming path relies on stay exactly
  as frozen; a method field would make "a JSON POST request with method GET and a body"
  constructible and force every consumer to reject it. The two shapes meet in one private
  projection inside `south-core`, so the binding check, the auth-header assembly, and the
  finalizer view are written once.
- **D2 — buffered only, no streaming GET.** Polling is a bounded JSON reply by nature, no host
  consumes a streamed GET, and the task-adapter record's rule against reserving unconsumed shapes
  applies.
- **D3 — `task_id` grammar: digits only.** The `GroupId` posture: the one upstream that uses it
  issues numeric ids; a host meeting another shape falls back rather than widening the grammar.
  The two digit grammars are deliberately separate arms of one decision each, so narrowing one
  never narrows the other.
- **D4 — a dedicated `south.provider-get.v1` suite.** Per the header-auth and controlled-query
  precedents: the frozen provider-call table is burned into two hosts' evidence, and a GET is a
  different call shape, not a variant of the same one.
