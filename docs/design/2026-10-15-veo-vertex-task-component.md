# Task world: Veo on Vertex AI (a `vertex-veo-video` family in `task-veo-v2`)

Status: accepted, revised the same day (lv 2026-10-15: migrate by the existing code's logic, prioritising speed; §5).
Not implemented.

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
  `bearer`, as the Gemini family fetches with its key header.
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

## 6. What the host must do before a cutover

1. Re-pin to the release that ships the `task-veo-v2` with the new family.
2. Credential-gated artifact fetch with the family's `bearer` (the Gemini family already uses the gated fetch with its
   key header).
3. A binding for `vertex-veo-video` and `component_task` routes on Vertex Veo models.
4. A dual run against the native arm, which the family reproduces (VV-Q2 (b)), on submit, observe and the gated
   fetch; then the native drain preflight `native_task_drain_preflight.py` gains a `veo` Vertex row before deletion
   (D10). No production query (lv, 2026-10-15).

## 7. Versioning (Q47)

`task-veo-v2` gains a family and `bearer`, so it takes a minor version; `south-component-conformance` changes (the
reference gains the family) and moves one step, so every other package published with the latest release takes a patch.
`task-veo-v2` keeps task contract 7; its `south_runtime` stays the oldest runtime that admits the manifest, confirmed by
`check-declared-runtime`.

## Revision note

- 2026-10-15: first draft (proposed).
- 2026-10-15: lv accepted the record and ruled VV-Q1 to VV-Q3 as recommended (§5); status accepted.
- 2026-10-15: revised: lv ruled to migrate by the existing code's logic and prioritise speed. VV-Q2 (b), and from it
  VV-Q1 (b) (a family in `task-veo-v2`) and VV-Q3 (native behaviour); §4 rewritten. The decision whether the native
  Vertex arm gets a dual run or a deletion is derived from the code, not from production data.
