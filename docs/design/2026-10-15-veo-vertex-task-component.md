# Task world: Veo on Vertex AI (a `vertex-veo-video` family in `task-veo-v2`)

Status: accepted, revised the same day (lv 2026-10-15: migrate by the existing code's logic, prioritising speed; §5).
Implemented ("Implementation"); released in 0.66.0.

Date: 2026-10-15

Baseline: south `origin/main` = `0caa22f` (release 0.65.0 plus the accepted `provider-vertex` record). Host references
carry a `server:` prefix and refer to token-station-server `origin/dev-v2` = `dcfaf26d`; `server:…/` abbreviates
`server:gateway/src/modules/inference/`. Statements marked **unverified** come from reading code or Google's
documentation, not from running anything or capturing traffic.

Related: `task-veo-v2` (the Gemini API Veo line; its reference header says Vertex operation names are refused because
"that line needs a minted service-account token and a region-derived endpoint, neither of which this component can
express"), [Omni record](2026-10-01-omni-task-component.md) (task contract 8, inline artifacts, and its
`vertex-interactions-video` family, the precedent for a Vertex task family), lv's L-Q2 ruling of 2026-10-14 (the
native Vertex Veo line goes offline with V1; this family re-launches it as a component family, §6).

## 1. Why

Veo was served natively on two rows: `gemini` (moved to `task-veo-v2`) and `vertex_ai`. lv's L-Q2 ruling
(2026-10-14) takes the native Vertex Veo line offline with V1, so Vertex Veo returns only as a component family. This
record designs it.

## 2. What the host already provides (read, not run)

The two things the 2026-09 note said a component could not express are now the host's job, as for Omni's Vertex family:

- **Base URL**: for a `vertex_ai` row the host passes the location prefix
  `https://{region-host}/v1/projects/{project}/locations/{region}` as the binding's endpoint
  (`server:…/handler/video/durable.rs:2112-2127` `component_task_endpoint`, `server:…/engine/vertex.rs:244`
  `vertex_task_endpoint`; project from the row's override, else the selected credential).
- **Auth**: the host mints the Vertex access token and presents it as `bearer` (`server:…/handler/video/durable.rs:621-636`).

So the package needs no `credentials` section and no `config_schema` (the task worlds do not admit either), exactly as
`task-gemini-omni-v2`'s Vertex family.

## 3. The native arm and the documented wire disagree

The native arm treats Vertex Veo as the Gemini shape: it submits `…/publishers/google/models/{m}:predictLongRunning`,
polls `GET {base}/v1/{operation name}` (`server:…/handler/video/observe.rs:200-220`), and reads the clip from
`response.generateVideoResponse.generatedSamples[].video.uri` (`observe.rs:385-400`, "Vertex and Gemini share the LRO
shape"). Google's Vertex documentation describes a different poll and result (**unverified**): `POST
…/publishers/google/models/{m}:fetchPredictOperation` with `{"operationName": …}`, and `response.videos[]` carrying
either `bytesBase64Encoded` (inline) or `gcsUri` (when the request names `storageUri`). The repo has no capture of either.
If the documentation is right, the native Vertex Veo arm has never completed a task; if the native shape is right, the
documentation is wrong for this model line. Only a real-account capture settles it (VV-Q2).

The native arm also never fetched a Vertex clip (**unverified** doubt). Its artifact fetch refuses a Vertex row
fail-closed (`server:…/handler/video/transfer.rs:107-128`, host plan 59, step P2: no guessing before a real response sample), and its
only Vertex test pins a failure terminal, not a success. So no native baseline exists for the success shape or for the
fetch of `video.uri`.

## 4. The family (as ruled: the native arm's logic)

lv ruled on 2026-10-15 to migrate by the existing code's implementation and prioritise speed. The package therefore
reproduces the native arm (§3) rather than Google's documented wire; the documented shape stays an **unverified**
doubt, registered here and not blocking a cutover.

- **Where**: a second family `vertex-veo-video` in `task-veo-v2`, as `task-gemini-omni-v2` carries a Gemini and a
  Vertex family in one package. The native arm is one implementation for both rows, the clip is an upstream URI (no
  inline bytes, so task contract 7 suffices), and the Gemini family's request, observe, render and metering code is
  reused as is. The package's `auth_arms` gain `bearer`.
- **Submit**: `POST {base}/publishers/google/models/{model}:predictLongRunning`, the shared Veo body (native
  `durable.rs:2761-2775`), auth `bearer` (host-minted).
- **Observe**: `GET {base-origin}/v1/{operation name}` (the operation name carries `projects/…/locations/…`), as native
  `observe.rs:200-220`; the response is read as the Gemini LRO shape (`observe.rs:385-400`).
- **Artifacts**: `generateVideoResponse.generatedSamples[].video.uri`, `fetch_with_credential` with the family's
  `bearer`, as the Gemini family fetches with its key header. Submit and poll reproduce native; this fetch is **new**,
  since native refuses it (§3), so it and the result shape it reads have no native baseline (**unverified**).
- **Shared create parse**: a create response does not say which line answered, so the package accepts either line's
  operation name there, as native `parse_veo_create` accepts any name. In consequence the Gemini line's frozen case
  `created.vertex-name` changes from `unknown` to `accepted`; the Gemini family still refuses to poll a `projects/…`
  name.
- **Metering**: requested seconds × delivered clips, as the Gemini family (P13 S11).
- **Client parameters**: as native; `storageUri` is not refused (native passes the body through its builder).

## 5. Open questions for lv

**Ruled by lv on 2026-10-15, then revised the same day.** The first ruling took each recommendation. lv then ruled to
migrate by the existing code's logic, prioritising speed, without querying production. So: **VV-Q2 (b)**, the native
arm's Gemini-shaped wire (§4), its divergence from Google's documentation registered as unverified; and, derived from
that ruling (from the code, not from production data): **VV-Q1 (b)**, a family in `task-veo-v2` rather than a new
package, since an upstream URI needs no task contract 8 and the native arm is one implementation for both rows; and
**VV-Q3**, native behaviour (no refusal). The items below keep their original text for the record.

- **VV-Q1 Packaging.** (a) A new package `task-veo-vertex-v2`, or (b) a second family in `task-veo-v2`. A package
  declares one `contracts.task`; the Vertex clip needs contract 8 (inline bytes), while `task-veo-v2` is published at
  task 7, so (b) would make a task-7-only host refuse the Gemini Veo line too. **Recommended: (a)**, which also keeps
  `task-veo-v2`'s behaviour and identity line untouched.
- **VV-Q2 Which wire.** (a) Google's documented Vertex wire (`fetchPredictOperation`, `response.videos[]`), with a
  real-account capture as a cutover precondition; (b) the native arm's Gemini-shaped assumption, which a dual run would
  match but which may never have worked. **Recommended: (a)**, plus the capture before cutover; a dual run against the
  native arm is then expected to differ, and the record registers that difference instead of reproducing it. The host
  team should check whether any production `vertex_ai` Veo task ever reached `succeeded` (a read-only query on the
  task table) — if none did, the native arm can be deleted without a dual run.
- **VV-Q3 `storageUri` / `gcsUri`.** Refuse a client `storageUri` before submit (400), so the clip always comes inline;
  or accept it and add a `gs://` second hop to the host (new host capability). **Recommended: refuse.**

## 6. What the host must do before a re-launch

1. Re-pin to the release that ships the `task-veo-v2` with the new family.
2. Credential-gated artifact fetch with the family's `bearer` (the Gemini family already uses the gated fetch with its
   key header).
3. A binding for `vertex-veo-video` and `component_task` routes on Vertex Veo models.
4. Re-launch after the native line is offline (L-Q2, lv 2026-10-14, which overturns the earlier plan to keep the native
   arm until this package ships). The family's frozen fixtures are the baseline: there is no dual run against native
   and no drain-preflight deletion step. No production query (lv, 2026-10-15).

## 7. Versioning (Q47)

`task-veo-v2` gains a family and `bearer`, so it takes a minor version; `south-component-conformance` changes (the
reference gains the family) and moves one step, so every other package published with the latest release takes a patch.
`task-veo-v2` keeps task contract 7; its `south_runtime` stays the oldest runtime that admits the manifest, confirmed by
`check-declared-runtime`.

## Implementation

`task-veo-v2` 0.36.0 carries the family: `crates/south-component-conformance/src/reference_veo_task_v2.rs` (a `Family`
switch at the submit path, the observe URL, the credential and the locator route, as native branches only there), the
manifest (`providers` gains `vertex-veo-video`, `auth_arms` gains `bearer`, task contract 7 and `south_runtime`
unchanged), and Vertex rows in `fixtures-veo-task-v2/` hand-transcribed from `durable.rs::veo_run` /
`build_vertex_media_url`, `observe.rs::build_video_query_url` / `normalize_veo` and the host test
`durable_vertex_veo_submits_with_the_sa_bearer_at_the_region_host`.

**Interpretations.**

- *Observe origin.* Native polls `{vertex_api_base}/v1/{name}`, where `vertex_api_base` is scheme and host. The
  component takes the scheme and authority of the bound location prefix, which the host derives from the same
  `vertex_api_base`, so the URL is native's byte for byte, including the host's test override.
- *Locator route.* The Vertex family's locator route is `v1`, the Gemini family's stays `v1beta`; each family refuses
  the other's locator on observe and artifact.
- *Operation names.* A create response does not say which line answered, so `parse_submit_response` accepts either
  line's name, as native `parse_veo_create` accepts any name. On observe each family polls only its own shape: the
  Vertex family refuses a `models/…` name (native would poll it at `{origin}/v1beta/…`, outside the bound endpoint, which
  a host refuses to send); the Gemini family keeps its refusal of `projects/…`.
- *Name length.* A `projects/…` name is bounded by the wire's upstream-id limit (`MAX_ARTIFACT_REF_BYTES`), not by the
  128-byte task-id limit the Gemini names keep: a Vertex name with a UUID operation id is about 145 bytes. Native has no
  bound.
- *Project and location.* Superseded in 0.36.1 by Q-VV1 (below). As released in 0.36.0, the component did not compare
  the name's project and location with the binding's, as native does not; a name outside the bound location yielded a
  descriptor a host's `authorize` refuses, so the credential never left the location, but such a task could not be
  polled.
- *`storageUri` (VV-Q3).* Native `build_veo_submit_body` copies only the fields it knows, so a client `storageUri` is
  neither refused nor sent. An operator's request extra may still set `parameters.storageUri` (not an immutable path,
  as in native `VEO_VIDEO_OWNED`); the answer would then carry no `generatedSamples` and observe as `unknown`, as native.
- *Artifacts.* The clip is a `fetch_with_credential` URL artifact; the host fetches it with the submission's
  authentication, here the bearer. `build_artifact_request` returns none, as for the Gemini line.
- *Undeclared family.* A provider other than `veo-video` and `vertex-veo-video` is refused (the Omni package's rule);
  before, any provider was treated as the Gemini line.

**Contradictions (flagged).**

- *Native never fetched a Vertex clip.* `server:…/handler/video/transfer.rs:107-128` refuses the Vertex Veo artifact fetch
  ("not wired yet": the response shape was never verified). §4's "fetch with the family's bearer" is therefore not a
  transcription of native; it is the contract 6 rule the host has to implement (§6 item 2). Native's own tests pin only
  the failure terminal on Vertex, not a success.
- *One Gemini-line fixture changed.* `task-v2.created.vertex-name` expected `unknown` and now expects `accepted`: the
  create parser has no family to branch on, and accepting `projects/…` names there is what the Vertex family needs and
  what native does. Every other Gemini-line fixture is unchanged.

**Open questions.**

- *Project number or id.* Ruled by lv on 2026-10-15 (Q-VV1, "south rewrites the prefix"); see "Q-VV1" below. The
  question whether Vertex answers with the number at all stays open (**unverified**).
- *The clip URI.* Whether `video.uri` (if the native shape is right at all) lies under the bound location prefix, which
  a host's gated fetch may require, is unknown; the fixtures use a placeholder under it.

**Gate results** (local, on `origin/main` 0f80b91 plus this change, mirroring `ci.yml` with `set -o pipefail`): fmt,
clippy (workspace, all targets, all features, `-D warnings`), nextest with `PROPTEST_CASES=32` (2152 run, 2152 passed, 4
skipped), doc tests, `--no-default-features`, rustdoc `-D warnings`, fuzz `check --locked`, `check-boundaries` with its
self-test, the Python script tests, `check-language` (self-test, files, commits), `machete` (workspace and fuzz), every
`build-*-component.sh`, and `check-declared-runtime.sh --build` (`task-veo-v2` loads under its declared 0.57.0): all
pass. `cargo deny`, `cargo audit` and MSRV were not run locally. Mutations, each caught by the frozen pack and by
`veo_task_suite_v2`: the Vertex submit path without `publishers/google/` (four `prepare.vertex-*` cases), the Vertex
credential as the Gemini header instead of a bearer (the four `prepare.vertex-*` cases and `observe.vertex-operation`),
and the Vertex poll under the location prefix instead of the origin's `/v1/` (`observe.vertex-operation` and the
authorize tests).

### Q-VV1: the poll prefix (`task-veo-v2` 0.36.1)

**Ruling (lv 2026-10-15).** Vertex may return the `predictLongRunning` operation name with the project **number**
(`projects/123456789012/…`) while the binding's location prefix carries the project **id**. The poll URL built from the
name verbatim then leaves the bound endpoint, the host's `authorize` refuses it, and the task can never be polled.
Native polls the name verbatim and checks nothing. lv ruled that south rewrites the prefix; no capture is awaited.

**Rule.** On observe, the Vertex family matches the operation name segment by segment against
`projects/{p}/locations/{r}/publishers/google/models/{m}/operations/{id}` (exactly ten segments, the literals
`projects`, `locations`, `publishers`, `google`, `models` and `operations` in place; every segment already passed the
existing name check: non-empty, not `.` or `..`, ASCII alphanumerics and `-_.` only). When it matches, `{p}` and `{r}`
are dropped and the poll URL is `{base}/publishers/google/models/{m}/operations/{id}`, where `{base}` is the binding's
location prefix, the same prefix the submit URL is built under. For a name that repeats the bound project and location
this is native's URL byte for byte (`observe.vertex-operation` unchanged). The stored task id (the upstream name) is
not rewritten; only the poll URL changes. A different location in the name is replaced as well, since the ruling names
the whole `projects/*/locations/*` prefix.

**Any other shape.** A `projects/…` name that passes the existing name check but not the ten-segment shape (a generic
`projects/{p}/locations/{r}/operations/{id}`, a missing location, a trailing segment, a publisher other than `google`)
is polled verbatim at `{origin}/v1/{name}`, as in 0.36.0 and as native: the existing observe code refuses only names that
fail its character and family checks, and polls every name it accepts, so a pass-through keeps that style and leaves the
backstop where it was, the host's `authorize`, which still refuses such a name when it leaves the bound endpoint. A
name that fails the existing check is still refused with `invalid Veo operation name` (400), unchanged.

**Unverified (registered, not blocking).** Whether Vertex names the operation by project number, by project id, or as
the request named it, is not known: the repo holds no capture of a Vertex Veo create response, and no real-key capture
is awaited (lv 2026-10-15). If Vertex always echoes the binding's form, the rewrite never changes a URL. If the poll
under the rewritten prefix were refused upstream for an operation created under the number (not expected, since the
number and the id name the same project), the task would observe `unknown` and stall as it would have under the host's
refusal. This doubt sits beside VV-Q2's (whether the poll is `GET …/operations/{id}` at all, or
`fetchPredictOperation`), which a real-account capture settles together.

**Tests.** `veo_task_suite_v2`: `a_project_number_operation_name_is_polled_under_the_binding_prefix` (number, and a
different location, rewritten; sendable), `a_matching_operation_name_polls_the_native_url_unchanged`,
`an_operation_name_of_another_shape_is_polled_verbatim_or_refused` (four other shapes polled verbatim and refused by
`authorize`; the same shape under the bound project sendable; three malformed names refused). Frozen rows
`task-v2.observe.vertex-project-number` and `task-v2.observe.vertex-other-shape` carry both outcomes through the
native and the sandboxed builds.

**Version (Q47).** A behaviour fix in an existing family, with no new family, auth arm, declaration or contract, so
`task-veo-v2` takes a patch: 0.36.0 (published with 0.66.0) → **0.36.1**; `south_runtime` 0.57.0 and task contract 7
unchanged. The reference and the fixtures live in `south-component-conformance`, which therefore must move past 0.65.0
before the next release (`check_crate_versions.py`); that round, and the patch it gives every other package, is left to
the cycle's conformance round on another branch (not part of this change). `task-veo-v2` already carries a new
identity in this cycle and needs no further step there.

## Revision note

- 2026-10-15: first draft (proposed).
- 2026-10-15: lv accepted the record and ruled VV-Q1 to VV-Q3 as recommended (§5); status accepted.
- 2026-10-15: revised: lv ruled to migrate by the existing code's logic and prioritise speed. VV-Q2 (b), and from it
  VV-Q1 (b) (a family in `task-veo-v2`) and VV-Q3 (native behaviour); §4 rewritten. The decision whether the native
  Vertex arm gets a dual run or a deletion is derived from the code, not from production data.
- 2026-10-15: implemented in `task-veo-v2` 0.36.0 ("Implementation"); unreleased.
- 2026-10-15: L-Q2 (lv 2026-10-14) applied: the native Vertex Veo line goes offline with V1, so §6 is a re-launch with
  the frozen fixtures as baseline (no dual run, no drain-preflight deletion); the reliance on the deletion list is struck.
  §3 and §4 register the gated clip fetch as new and unverified, and §4 the shared create parse's consequence for
  `created.vertex-name`.
- 2026-10-15 (after 0.66.0): Q-VV1 (lv): the Vertex family polls a publisher-model operation name under the binding's
  own location prefix; registered as unverified ("Q-VV1"); `task-veo-v2` 0.36.1; unreleased.
- 2026-10-16 (after 0.70.0): the host deletes `vertex_ai`, so §2 no longer holds: the package declares the `vertex_sa`
  recipe for `vertex-veo-video` and the operator writes the location prefix in full as the component row's endpoint
  (record [`2026-10-16-task-vertex-credentials.md`](2026-10-16-task-vertex-credentials.md), lv 2026-10-16; fresh
  production deployment, no row migration); `task-veo-v2` 0.37.0; unreleased (0.71.0).
