# The Task Vocabulary, Measured the Same Way: one type cannot carry its payload

Status: **survey** 2026-09-19, the second of two. No code, no version.

Date: 2026-09-19

Predecessors: `2026-09-19-task-world-fit-survey.md` (the first survey — it
measured the world's **signatures** and found four defects, all corrected in
#81 before any tag carried them), `2026-08-27-task-adapter-vocabulary.md`
(froze `TaskObservationV1`, shipped in 0.19.0).

## 1. Why a second survey

The first survey asked "do the parameters carry what an implementation needs".
It did not ask "do the **types** carry what an implementation needs", and those
are different questions: a signature can name exactly the right types and still
be unimplementable if one of them is too narrow.

The gap showed up on the first day of writing a reference implementation. The
honest summary is that the first survey stopped one layer short, so this one
finishes the job: **every parameter and return of all seven functions, against
the six families the adopting host runs in production.**

## 2. The finding: `TaskObservationV1` carries no payload

```rust
pub enum TaskObservationV1 {
    Running,
    Succeeded,               // no fields
    Failed(TaskFailureKindV1),
    Unknown,
}
```

The adopting host's equivalent (`handler/video/observe.rs:204`):

```rust
pub enum VideoObservation {
    Progress  { running: bool, status_word: String },
    Succeeded { result: VideoTaskResult },
    Failed    { kind: VideoFailureKind, code: Option<String>, message: Option<String> },
    Unknown   { reason: String },
}
```

`VideoTaskResult` carries five fields, and **three of them are metering**:

| Field | Why it exists |
|---|---|
| `video_urls` | the artifact. Without it `render-success` has nothing to render |
| `file_id` | one family returns an id instead of a URL — the reason `build-artifact-request` exists |
| `duration_secs` | one family bills **per second**; this is its settlement input |
| `completion_tokens` | one family bills **per token** |
| `kling_final_milliunits` | one family reports the **actual** upstream deduction; exact-first billing settles against it |

So a component returning today's `TaskObservationV1` can say *"it succeeded"*
and nothing else. The host would then have to parse the dialect's terminal body
itself to learn what was produced and what it cost — which is precisely the
dialect knowledge the component exists to hold.

**This makes the world unimplementable as specified**, not merely awkward.

## 3. Why this is the same hole as `usage-intent`, seen from the other side

The world record deferred an eighth function, `usage-intent`, on the grounds
that "a metering vocabulary is admitted only with a second consumer in sight".
That reasoning was sound for a *separate metering call*. It missed that
**metering does not need its own function**: every family already reports its
meter inside the terminal observation it must return anyway. The upstream says
`duration`, `completion_tokens`, `final_unit_deduction` in the same body that
says "succeeded".

A component that forwards those numbers is not authoring a metering vocabulary
— it is transcribing what the upstream reported, which is exactly the half
`ARCHITECTURE.md`'s vocabulary line assigns to South ("metering: what an
upstream *reported*"; pricing stays host-side). Nothing here prices anything.

So the correct shape was never "seven functions plus a deferred eighth". It is
**seven functions whose terminal observation carries the meter**.

## 4. The rest of the audit

Every other parameter and return checked against the six families:

| Function | Type | Verdict |
|---|---|---|
| `build-submit-request` | → `HttpRequestDescriptor` | **fits.** `method`/`url`/`headers`/`body`/`auth` cover every family, including the one needing a custom async header |
| `parse-submit-response` | → `submit-outcome` | **fits** (as corrected in #81) |
| `build-observe-request` | → `HttpRequestDescriptor` | **fits.** A buffered GET |
| `parse-observation` | → `TaskObservationV1` | ❌ **§2** |
| `build-artifact-request` | → `option<HttpRequestDescriptor>` | **fits** |
| `render-success` | → the success body, as `json` | **fits.** Free-form by nature: it is the client-facing V1 body, whose shape is the host's public contract, not South's |
| `map-terminal-failure` | → `ErrorEnvelope` | **fits** |

Two further checks worth recording because they could have gone wrong:

- **`TaskFailureKindV1` is exactly right.** Its three words
  (`failed`/`cancelled`/`provider-expired`) match the host's `VideoFailureKind`
  one for one. The 2026-08-27 ruling that expiry needs an explicit upstream
  statement holds across all six.
- **`task-request` as `json` is right, not lazy.** There is no IR type for a
  media task request, and inventing one would put the Canonical IR — whose
  authority is a host that runs no media tasks — in the position of versioning
  a shape it never executes. The vocabulary record already ruled this way for
  the task types themselves.

## 5. Recommendation

**Extend `TaskObservationV1`'s terminal arms to carry what the upstream
reported**, as `TASK_CONTRACT_VERSION = 2`:

- `succeeded` gains the artifact reference (URLs *or* an id) and an optional
  reported meter;
- `failed` gains the upstream's code and message, which the host surfaces and
  `map-terminal-failure` maps;
- `running` gains the upstream's own status word, which the host snapshots for
  diagnosis;
- `unknown` gains its reason, for the same purpose.

The meter should be a small closed set of named quantities (seconds, tokens,
milliunits), **not** a free map: a free map is how a metering vocabulary turns
into a pricing vocabulary one key at a time.

### On changing a published contract

`TASK_CONTRACT_VERSION = 1` shipped in 0.19.0, so this is not the free edit the
first survey enjoyed. Three facts make it the right call anyway:

1. **It has no consumers.** Nothing in either repository reads these types —
   the adopting host has never compiled against `south-contracts::task`, and no
   component exists. The version counter exists to protect consumers; there are
   none to protect.
2. **The alternative is worse.** Shipping a world that cannot carry its own
   results would be discovered by the first implementer anyway, at which point
   the fix is the same edit plus a deprecation cycle.
3. **A second type beside it is worse still.** `TaskObservationV1` and a
   payload-carrying twin would be two adjacent types answering one question,
   and the wrong one is always one import away.

## 6. Method note

Both surveys found what they found by the same move: take the contract, take a
real implementation, and check them against each other **before** the contract
is frozen rather than after. The first found four defects, this one found a
fifth that the first structurally could not see.

The generalisable rule is narrower than "review your designs". It is: **a
contract derived from a summary of an implementation must be checked against
that implementation, not against the summary.** The adopting host's plan 46 §2
table is an accurate description of its seam; every defect both surveys found
is a detail that table did not need to mention and a signature cannot omit.
