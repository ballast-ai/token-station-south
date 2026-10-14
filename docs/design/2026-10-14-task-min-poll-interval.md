# A task package's minimum poll interval (`min_poll_interval_seconds`)

Status: proposed (south side of the host's H5-5 MiniMax video step, ruled by lv on 2026-10-14: V-Q8 (a) as
recommended in the host's tier price table plan, `server:docs/product-review-v2/plans/2026-10-13-H5-*.md` §9.2, §9.9).
Not released; proposed for 0.63.0 (§6).

Date: 2026-10-14

Baseline: south `origin/main` = `bcfcd45` (task contract 8, after release 0.61.0). Host references carry a `server:`
prefix and refer to token-station-server `origin/dev-v2` as read on 2026-10-14. Statements marked **unverified** are
conclusions from reading code, not from running anything.

## 1. Problem

A task host polls each in-flight task: it calls `build-observe-request`, sends the request, and hands the answer to
`parse-observation`, then schedules the next poll. How often it may poll is an upstream fact (rate limits, the
provider's own advice), but nothing in the task worlds carries it: the observe exports take a task and return a
request, and the manifest has no cadence. The server host therefore keeps its own per-family floors
(`server:gateway/src/modules/tasks/domain.rs` `PollFamily::floor_secs`: 12 s for MiniMax Hailuo v1, 10 s for MiniMax
H3, 5 s otherwise) keyed on its native provider families, and every component task family gets the generic 5 s
because the host cannot tell one component family from another (`PollFamily::Component`). When the MiniMax video models
move to `component_task` routes (host step H5-5), the host would poll MiniMax at 5 s, faster than either native floor
and, at full concurrency, above H3's documented 300 requests per minute (the host's own note on the 10 s floor).

Keeping the floor in the host keyed on a vendor is what the host's zero-vendor boundary removes, so the package says it.

## 2. Ruling

V-Q8, ruled (a) by lv on 2026-10-14:

- **(a)** One package-level minimum interval. `task-minimax-v2` serves both MiniMax generations (Hailuo v1 and H3,
  told apart by the locator route), so it declares one value, 12 s; H3 polls go from the host's 10 s to 12 s, which is
  accepted.
- (b) One interval per locator route. Not chosen: a larger contract for a 2 s difference.

## 3. The field

`min_poll_interval_seconds`, an optional integer in the manifest of a task package (`task-adapter-v1` or
`task-adapter-v2`):

- **Meaning.** The shortest interval, in whole seconds, between two observation requests a host sends for one task of
  the package. A host schedules no observation of a task sooner than this after the previous one; its own backoff and
  jitter may wait longer, never shorter. It applies to every family and every task of the package.
- **Absent** means the host's own default (the server host's generic floor, 5 s today). Every package published before
  this field is absent, so nothing changes for them.
- **Bounds.** `1..=600`, inclusive (`MIN_TASK_POLL_INTERVAL_SECONDS`, `MAX_TASK_POLL_INTERVAL_SECONDS` in
  `south-provider-api`). Zero would let a host poll in a busy loop (`south-task-core` refuses a zero wait interval for
  the same reason); ten minutes is far above any upstream's advice and keeps a unit mistake (milliseconds written as
  seconds) from parking every task of a package for hours. A number outside the bounds is refused with
  `ManifestErrorV1::MinPollIntervalOutOfBounds`; a negative, fractional or string value fails to parse.
- **World.** Gate ① refuses it outside the two task worlds
  (`ManifestErrorV1::MinPollIntervalIsATaskWorldDeclaration`).
- **No contract number.** It changes no value a component and its host exchange, so it does not move
  `contracts.task`. A runtime before the one that ships it refuses the unknown field (`ComponentManifestV1` refuses
  unknown fields), so a package declaring it must declare that runtime as its `south_runtime` (§6); an older host then
  refuses the package through the declared runtime alone instead of misreading it.

Not in scope: a delay before the first poll after submit (the host's own scheduling), per-family or per-route
values (V-Q8 (b)), and anything a component returns at run time (an upstream `Retry-After`, for instance).

## 4. Which packages declare it

| Package | Value | Source |
|---|---|---|
| `task-minimax-v2` | 12 | V-Q8 (a): the Hailuo v1 floor the host keeps today; H3 rises from 10 s |
| every other task package | absent | No upstream cadence is documented in this repository for Kling, Bailian, xAI, BytePlus, Veo, Wan image, GMI image or Gemini Omni; the host's 5 s generic floor is what they get today, natively and as components |

A package may add the field later from an upstream's documentation or a real-account capture; that is a manifest change
and so a new package version.

## 5. What the host does

The host reads the admitted manifest: `LoadedComponentV1::manifest()` (`south-provider-runtime`) or the manifest that
`gate_manifest` returns, then `ComponentManifestV1::min_poll_interval_seconds: Option<u32>`. For a task bound to a
component, its poll floor is that value when present and its own default otherwise, replacing the per-family table for
component families. Hosts that match `ManifestErrorV1` exhaustively add the two new variants on re-pin. **Unverified**:
where in the server host the bound component's manifest is reachable from the poller's scheduling step.

## 6. Release

- `south-provider-api` changes (the field, two constants, two error variants). Its version already moved past the
  0.61.0 tag in this cycle (0.54.0 at `v0.61.0`, 0.55.0 now, task contract 8), so `check_crate_versions.py` passes
  without a further bump; whether the release takes one more step, as task contract 8 did for the Kiro step's versions,
  is the release's call. Every guest links the crate, so every `component.wasm` may change.
- `task-minimax-v2`'s manifest changes, so the release gives it a new version (today 0.31.20, unpublished) and raises
  its `south_runtime` to 0.63.0, the first runtime that parses the field; `declared_runtime_now` in
  `shipped_packages_v1` and the pins of 0.31.20 there move with it. `scripts/check-declared-runtime.sh` would refuse the
  package under 0.57.0, which rejects the unknown field.
- No contract number, `runtime_abi` or host range changes.
