# Served models, required prefetch and failure categories: three family declarations

Status: **proposed**, for review by the host owner (lv). Nothing is implemented. Issues #299 and #302.

Date: 2026-10-28

Baseline: south `origin/main` = `c74e1a0e` (release 0.79.0 plus the media canary guests). Host references carry a
`server:` prefix and refer to token-station-server `dev-v2` = `06987c78`. Statements marked **unverified** come from
reading code, not from running anything or capturing traffic.

## 1. Problem

The host must not branch on a family name (server discipline rule 6). Three host decisions still lack the package
fact they need.

1. **Does this family serve this model?** (#299, server 03 #116.) A model route names a family, and nothing checks
   that the family serves the routed model. Measured on the server: a `qwen-image-3.0-pro` model routed to
   `{"family": "wan-image"}` returns 200; the upstream receives Wan's asynchronous endpoint instead of Qwen-Image's
   synchronous one, and the call is priced as Wan.
   - `task-adapter-v2` has no model query.
   - The image world's `model-capabilities` is not a membership test: several packages answer for any id they are
     given (`image-stability` filters by its own table, most do not).
2. **Must the host prefetch this request field?** (#299, server 03 #111.) `task-veo-v2` and
   `task-gemini-omni-v2` accept input images only as `data:` URIs and refuse a plain URL at request time with "configure
   the host to pre-fetch this field". The host learns that a route lacks `request.fetch_images` only when a client
   sends an image. `task-xai-v2` forwards whatever it is given, so a route without prefetch sends the client's URL
   upstream and the host's fetch guard (destination checks, size limits) never applies.
3. **Why did this task fail?** (#302, server 03 #188.) The host's task envelope carries `error.reason`, a fixed word set
   (`upstream_rejected`, `upstream_timeout`, `content_policy`, `artifact_unavailable`, `cancelled`, `expired`,
   `internal_error`). A failed observation (`TaskObservationV2::Failed`) gives the host only a kind (`failed`,
   `cancelled`, `provider_expired`) and the upstream's raw `code` and `message`, so a content-moderation failure lands
   on `upstream_rejected`.

## 2. Declaration 1: `served_models` (manifest, per family)

```json
"served_models": {
  "<family with a model table>": { "ids": ["<upstream id>", "<upstream id>"] },
  "<family with a naming scheme>": { "prefixes": ["<prefix>"] },
  "<router family>": { "any": true }
}
```

(The values are placeholders; §2 "How the shipped values are found" says where the real ones come from.)

- `ComponentManifestV1::served_models: BTreeMap<String, ServedModelsV1>`, keyed by a family the manifest declares.
- `ServedModelsV1` is exactly one of three forms:
  - `{"ids": [...]}`: the upstream model ids the family accepts;
  - `{"prefixes": [...]}`: id prefixes, for families whose upstream adds models under a stable naming scheme;
  - `{"any": true}`: the family accepts any id (OpenAI-compatible routers, where the operator chooses the upstream).
- "Not declared" is not "accepts everything": once a release ships the field, gate ① refuses a manifest that leaves one
  of its families undeclared (SM-Q2). `any` is a deliberate statement, reviewed like any other.
- Worlds: task (v1 and v2), image and speech. Provider (text) and embeddings families are out of scope (SM-Q3).
- The match is on the **upstream** model id, the one the package receives (`ImageCallContextV1.upstream_model`, the task
  request's model), not on the host's catalog name.

**Host use.** At route write time the host looks up the routed family's declaration in the bound package and refuses
the write with 400 when the model's upstream id is outside it. The server's P13 / P22 / P23 deployment checks read the
same declaration. A request is not re-checked: the route write is the guard, and a package swap that narrows the set is
caught by the deployment check.

**How the shipped values are found.** For each family, the reference implementation's own model table where it has
one (pricing tables, capability tables, per-model request shapes); `any` where it has none and the upstream is a
router; otherwise a prefix that the family's documentation states. The implementation PR lists each family's value with
its source and marks every value without one **unverified**; reviewing that table is part of reviewing the PR.

## 3. Declaration 2: `required_prefetch` (manifest, per family)

```json
"required_prefetch": {
  "veo-video": ["image", "image_url", "last_frame", "last_frame_url", "reference_images"]
}
```

The Veo list is what `reference_veo_task_v2.rs` reads through `inline_image` (`image` or `image_url`, `last_frame` or
`last_frame_url`, `reference_images`); both Veo families take the same list. The Omni families take the fields their
reference passes to `image_part` (`reference_images` at least; the full list is read off the code in the
implementation PR).

- `ComponentManifestV1::required_prefetch: BTreeMap<String, Vec<String>>`: per family, the request fields whose client
  URLs the package cannot accept, so the host must have inlined them as `data:` URIs before `prepare`. Field names use
  the same spelling as the host route's `request.fetch_images`.
- Declared only where the package **refuses** a plain URL today (Veo, Omni). A family that accepts URLs declares
  nothing, whatever the host prefers (RP-Q1).
- Worlds: task v2 only. Image packages receive request views where the host already resolves blobs.

**Host use.** At route write time, a route whose `request.fetch_images` does not cover the family's declared fields is
refused with 400. Today the same mistake surfaces only when a client sends an image.

## 4. Observation field: `category` on a failed task observation

```json
{ "state": "failed", "kind": "failed", "code": "1026", "message": "...", "category": "content_policy" }
```

- `TaskObservationV2::Failed` gains `category: Option<TaskFailureCategoryV1>`, omitted when absent.
- `TaskFailureCategoryV1` words: `content_policy` (the upstream refused or filtered the output for its content) and
  `upstream_timeout` (the upstream says it timed out). The package sets it only from an explicit upstream signal, the same
  discipline as `provider_expired`: nothing is inferred from a message text.
- The host maps `content_policy` → `error.reason` `content_policy` and `upstream_timeout` → `upstream_timeout`; absent
  keeps today's mapping. No host branch on a family.
- **`artifact_unavailable` is not a package category** (FC-Q2). The packages report artifact references; whether the
  host can fetch and store them is the host's own fact, so the host sets `artifact_unavailable` when its artifact
  transfer fails after a succeeded observation.
- **Submission refusals need no new field.** A refused submission already carries an `ErrorEnvelope` whose `code` is
  from the error catalog; `task-minimax-v2` maps MiniMax `1026` to `content_policy` there today
  (`reference_minimax_task_v2.rs:124`). The host can map that code to `error.reason` now (server checklist §6 item 4).

**Which families set it in this round.** Only where an explicit signal is known:
- `veo-video` / `vertex-veo-video`: `raiMediaFilteredCount` above 0 with no video (`reference_veo_task_v2.rs:288`
  already reads it) → `content_policy`.
- `task-minimax-v2`: a failed query whose `base_resp.status_code` is `1026` → `content_policy`. The reference maps
  `1026` only on the submission refusal today; whether a query ever reports it is **unverified**, so this row ships
  only if the implementation finds it in a fixture or capture.

Other families set it when someone captures their wire signal; until then their failures keep landing on
`upstream_rejected`.

## 5. Compatibility, versioning, tests

- **Manifest fields** (`served_models`, `required_prefetch`): `ComponentManifestV1` is `deny_unknown_fields`, so a
  runtime older than the release that ships them refuses a package that declares them. Following the 0.67.0 and 0.74.0
  precedent, the feature PR leaves `south_runtime` alone and the release commit raises it on every declaring package.
  Because SM-Q2 makes `served_models` required for task, image and speech packages, that is every package of those
  worlds.
- **Observation field**: the wire form is decoded with `deny_unknown_fields`
  (`south-component-conformance/src/task_v2_json.rs`, the `state`-tagged observation enum), so a host whose decoder
  predates `category` refuses an observation that carries it. This is therefore a task contract change: **task
  contract 9**, a superset of 8, admitted by hosts whose range includes it and declared only by the packages that emit
  the field (FC-Q1). A package that never sets `category` keeps declaring 7 or 8.
- **Q47**: `south-provider-api` and `south-contracts` take a minor (public fields and types);
  `south-component-conformance` takes a minor; every package re-identifies.
- **Tests**: manifest round-trip, absent-is-omitted, the one-form rule, undeclared families refused by name, the
  world restriction, and (after SM-Q2) the required-declaration refusal; per-package pins of the shipped values; the
  Veo and MiniMax fixtures for `category`, and a fixture showing a failure without a signal leaves it absent.

## 6. Host checklist (after re-pinning)

1. Route writes (task and image routes): refuse an upstream id outside the family's `served_models` (400), and a
   `fetch_images` that misses the family's `required_prefetch` (400).
2. P13 / P22 / P23 deployment checks: the same two rules, read from the installed packages.
3. `TaskFailureReason`: map `category` (§4).
4. Independently of this record: map a submission refusal's `ErrorEnvelope.code` `content_policy` to `error.reason`
   `content_policy`, and set `artifact_unavailable` when the host's own artifact transfer fails.

## 7. Questions for review

- **SM-Q1** Is the three-form `served_models` (ids, prefixes, any) enough, or should a family also be able to exclude
  ids? Recommended: the three forms; exclusions can be added when a family needs them.
- **SM-Q2** Required or optional? Recommended: **required** for task, image and speech packages from the release that
  ships it, so "not declared" can never be read as "accepts everything".
- **SM-Q3** Text and embeddings families? Recommended: out of scope. Text families serve operator-chosen ids through
  routers; the mismatch measured on the server is a media-route problem.
- **RP-Q1** Should `task-xai-v2` declare `required_prefetch` because the host prefers to run every client URL through
  its fetch guard? Recommended: **no**. The package accepts URLs, so that is host policy; the host can require
  `fetch_images` on routes for its own reasons without a package declaration.
- **FC-Q1** The observation decoder refuses unknown fields (§5), so `category` needs task contract 9. Should only the
  packages that emit it declare 9, or every task package move? Recommended: emitting packages only (`task-veo-v2`,
  `task-minimax-v2` in this round).
- **FC-Q2** Leave `artifact_unavailable` to the host? Recommended: yes (§4).
- **FC-Q3** `upstream_timeout` as a category: no reference reads an explicit upstream timeout signal today, so it would
  ship unused. Recommended: keep the word so the vocabulary matches the host's, and set it only when a capture shows
  a signal.
