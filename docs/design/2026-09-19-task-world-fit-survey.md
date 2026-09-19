# The Task World, Measured Against a Real Family: four signature defects

Status: **survey** 2026-09-19. No code, no version. Written *before* releasing
the task world, because a world that has shipped in a tag can only be changed
by version negotiation, while a world that has not can be changed by editing a
file.

Date: 2026-09-19

Predecessors: `2026-09-18-task-adapter-world.md` (the world this survey tests —
its seven functions were lowered from the adopting host's plan 46 §2 decomposition
**on paper**, with no implementation to check them against),
`2026-08-27-task-adapter-vocabulary.md` (D1–D3, unaffected by anything here).

## 1. Why this survey exists

The task world's seven exports were derived from a table in the adopting host's
plan. That table is a faithful summary of a seam that host runs in production —
but a summary is not a signature. Nothing had yet tried to *implement* the world
against a real provider family, so nothing had tested whether its parameters
carry what an implementation needs.

The adopting host's own rule is the reason to look now rather than later
(its plan 44 §5): sinking a contract before it is settled amplifies rework to
two repositories plus ABI version management. A world inside an unreleased
branch is free to change. The same world inside `v0.28.0` is a negotiation.

**Method.** Take `VideoTaskAdapter for Kling` and the four other families it
shares an observe path with, in the adopting host at `dev-v2`, and check each of
the world's seven functions against the inputs and outputs the real code uses.

**Result: four defects. Three are missing parameters; one is a wrong return
type. None invalidates the world's shape — the seven-function decomposition and
the describe/execute split both hold.**

## 2. D1 — `parse-submit-response` returns the wrong type

**The world says:**

```wit
parse-submit-response: func(response-parts: json) -> result<string, json>;
```

with the doc "the upstream's task id … A 2xx that carries no id is an error".

**The host's actual return** is a four-variant enum
(`handler/video/durable.rs:73` `CreateOutcome`):

| Variant | Meaning | Reachable via `result<string, json>`? |
|---|---|---|
| `Accepted { upstream_id }` | the normal path | yes |
| `AcceptedSync { raw_terminal }` | submit *was* the terminal answer — Veo's synchronous LRO edge returns no operation name, just the finished result | **no** |
| `Rejected { error, reason }` | HTTP 2xx, business-layer refusal (MiniMax's `base_resp`) | as an error, but see below |
| `Unknown` | shape unexpected; **may already have been accepted upstream** | **no** |

Two of the four have no representation, and the gap is not cosmetic:

- **`Unknown` is the funds-critical one.** "The upstream might have accepted
  this and might be billing for it" is materially different from "this failed".
  The host keeps the reservation and lets reconciliation settle it. Collapsing
  it into the error arm tells the host to release a reservation for work that
  may be running — the same class of mistake D3 rule 5 forbids on the observe
  side, arriving through the submit door.
- **`AcceptedSync` is a real family's real path**, not a hypothetical.

**Recommendation.** Return a `submit-outcome` variant mirroring
`TaskObservationV1`'s discipline — a closed set, with the uncertain case named
rather than folded into failure:

```wit
variant submit-outcome {
    accepted(string),        // upstream task id
    accepted-terminal(json), // submit was the terminal answer
    rejected(json),          // 2xx + business refusal → ErrorEnvelope
    unknown,                 // shape unexpected; may be running upstream
}
parse-submit-response: func(response-parts: json) -> result<submit-outcome, json>;
```

The outer `result` stays for "the component could not process this at all",
which is distinct from every variant above.

## 3. D2 — `build-artifact-request` cannot build the only request it exists for

**The world says:** `build-artifact-request: func(observation: json) -> …`,
documented as MiniMax's case.

**MiniMax's implementation** (`durable.rs:2345`) reads three things:

1. `raw_body.file_id` — in `observation` ✅
2. `provider_config` and `model_config`, to build the retrieve URL
   (`minimax_file_retrieve_url(cx.provider_config, cx.model_config, &file_id)`) ❌
3. `model_config.shape_model()`, to decide the v2 family needs no fetch at all
   (it returns `None` early) ❌

So the one function whose existence is justified by one family cannot be
implemented by that family. A component with no `provider-config` does not know
the host, the region, or the API version to fetch from.

**Recommendation.** `build-artifact-request: func(provider-config: json,
observation: json) -> result<json, json>`. `ProviderConfig` is already the
policy-fenced four-field subset every other southbound function takes, and
`models` inside it carries what the v2 discrimination needs.

## 4. D3 — `build-observe-request` is missing the model

**The world says:** `build-observe-request: func(provider-config, upstream-task-id)`.

**Kling's query URL** derives the path from the *model name* and from whether
the request had an image (`observe.rs:236`):

```rust
let path = KlingDispatch::from_model_name(model_name)
    .create_endpoint
    .create_path_for(has_image);
format!("{base}{path}/{upstream_id}")
```

Text-to-video, image-to-video and motion-control are different paths on the
same provider. A component given only the task id polls the wrong endpoint for
two of the three.

`provider-config.models` could carry the model, but *which* model this task used
is per-task state, not provider configuration.

**Recommendation.** Add the submitted model: `build-observe-request:
func(provider-config: json, upstream-model: string, upstream-task-id: string)`.

`has_image` needs no parameter — the host already stores the request snapshot,
and Kling's path choice can be derived from the model plus the task id's
endpoint, which the component itself chose at submit. **This is the one input
this survey deliberately does not add**, because adding a bool that only one
family reads is how a signature starts collecting per-family flags.

## 5. D4 — `render-success` needs the host's artifact base

**The world says:** `render-success: func(observation, fetched)`.

**The host's `RenderCx`** (`durable.rs:2138`) carries a fourth field the
signature omits: `gateway_task_id`. Its doc records why it exists — artifacts
are returned as the gateway's own relative path
(`/v1/video/tasks/{id}/artifacts/art_N`), and that field is what replaced the
credential the renderer used to need, closing the B-3 leak.

The component must place that id in the body it renders. It cannot invent it:
it is the host's own task id, which is exactly the shape `HostMintedValuesV1`
already governs on the submit side (D2 of the vocabulary record: *the component
places them; it never invents them*).

**Recommendation.** Pass the same `HostMintedValuesV1` to `render-success` that
`build-submit-request` already receives. No new type, and the "place, never
invent" rule extends to the render side unchanged.

## 6. What the survey did **not** find

Worth stating, because a survey that only reports problems invites the
assumption that everything else was checked and failed:

- **The seven-function decomposition holds.** No family needs an eighth
  function, and none of the seven turned out to be unnecessary.
- **The describe/execute split holds.** Every one of the six families'
  `build_artifact_request` / `render_success` pairs is already pure in the host;
  the WIT shape matches what production runs.
- **`timed-out` correctly does not lower.** No family's implementation consults
  a clock.
- **D4 of the world record (auth arms unchanged) holds.** Kling's HS256 JWT goes
  through `host_signed`, as predicted.
- **`map-terminal-failure` is right as specified.** All six families read only
  the terminal observation.

## 7. Consequence for the release

The four fixes are edits to an unreleased file. Applying them costs one commit;
discovering them after `v0.28.0` costs a major or a deprecation cycle, because
three of the four **change existing signatures** rather than adding to them.

**Recommendation: fold D1–D4 into the world before any tag carries it**, then
release. The adopting host's plan 86 §9.1 sequencing (P3a waits on a south
release) is unaffected in shape — only in which bytes get released.
