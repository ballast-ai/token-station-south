# Task world: Veo on Vertex AI (`task-veo-vertex-v2`)

Status: accepted (lv reviewed it on 2026-10-15 and ruled VV-Q1 to VV-Q3 as recommended, §5). Not implemented.

Date: 2026-10-15

Baseline: south `origin/main` = `0caa22f` (release 0.65.0 plus the accepted `provider-vertex` record). Host references
carry a `server:` prefix and refer to token-station-server `origin/dev-v2` = `dcfaf26d`; `server:…/` abbreviates
`server:gateway/src/modules/inference/`. Statements marked **unverified** come from reading code or Google's
documentation, not from running anything or capturing traffic.

Related: `task-veo-v2` (the Gemini API Veo line; its reference header says Vertex operation names are refused because
"that line needs a minted service-account token and a region-derived endpoint, neither of which this component can
express"), [Omni record](2026-10-01-omni-task-component.md) (task contract 8, inline artifacts, and its
`vertex-interactions-video` family, the precedent for a Vertex task family), the host's deletion list D10 (Veo's native
arm is not deleted until south ships the Vertex task package).

## 1. Why

Veo is served natively on two rows: `gemini` (moved to `task-veo-v2`) and `vertex_ai`. D10 = B keeps the whole native
Veo arm until a Vertex task package exists. This record designs it.

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

## 4. The package (as proposed; the wire per VV-Q2)

- **Family** `vertex-veo-video`, world `task-adapter-v2`, `bearer` (host-minted), capabilities `submit`, `observe`,
  `render` (no `artifact_fetch` if the clip is inline).
- **Submit**: `POST {base}/publishers/google/models/{model}:predictLongRunning`, body built by `task-veo-v2`'s shared
  body builder (the native arm uses one body for both rows, `durable.rs:2761-2775`: `instances` / `parameters`). Remote
  first-frame images keep `task-veo-v2`'s rule (the host pre-fetches them to `data:` URIs).
- **Observe**: per VV-Q2 (a), `POST {base}/publishers/google/models/{model}:fetchPredictOperation` with
  `{"operationName": <the operation name>}`; done / error / pending as the LRO fields say.
- **Artifacts**: `bytesBase64Encoded` → task contract 8 `Inline` artifacts named by pointer into the host-elided view
  (`response_blob_paths` `/response/videos/*/bytesBase64Encoded`), as Omni's Vertex clip. `gcsUri` cannot be fetched by
  the host (no `gs://` second hop), so `storageUri` in the client body is refused before submit (VV-Q3).
- **Metering**: as `task-veo-v2`: requested seconds × delivered clips (Veo reports no duration; P13 S11 rule).
- **Contracts**: `{task: 8, media: 1}`.

## 5. Open questions for lv

**Ruled by lv on 2026-10-15: VV-Q1 (a) a new package `task-veo-vertex-v2`, VV-Q2 (a) the documented Vertex wire with
a real-account capture before cutover, VV-Q3 refuse `storageUri`.** The items below keep their text; each recommendation
is the ruling.

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

## 6. What the host must do before a cutover

1. Re-pin to the release that ships the package; the host range already lists task 8 (since 0.62.0 / 0.64.0 repins).
2. Inline artifacts for task contract 8 on this family (the host adopted them for Omni; R52 removed, `server:dcfaf26d`).
3. A binding for `vertex-veo-video` and `component_task` routes on Vertex Veo models.
4. The VV-Q2 capture (or the production query) before any route is written; the native drain preflight
   `native_task_drain_preflight.py` gains a `veo` Vertex row before deletion (D10).

## 7. Versioning (Q47)

A new reference in `south-component-conformance` and, if the Veo body builder is shared, a refactor of
`reference_veo_task_v2.rs` (behaviour unchanged): the conformance crate moves one step and every package published with
the latest release takes a patch. The new package declares the oldest runtime that admits task contract 8 and
`response_blob_paths` (0.62.0), confirmed by `check-declared-runtime`.

## Revision note

- 2026-10-15: first draft (proposed).
- 2026-10-15: lv accepted the record and ruled VV-Q1 to VV-Q3 as recommended (§5); status accepted.
