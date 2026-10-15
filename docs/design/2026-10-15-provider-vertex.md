# Provider world: `provider-vertex` (Gemini models on Vertex AI, text)

Status: proposed (2026-10-15), for lv's review. Not implemented.

Date: 2026-10-15

Baseline: south `origin/main` = `4bc59ac` (release 0.65.0). Host references carry a `server:` prefix and refer to
token-station-server `origin/dev-v2`; `server:…/` abbreviates `server:gateway/src/modules/inference/`. Statements marked
**unverified** come from reading code or Google's documentation, not from running anything or capturing traffic.

Related records: [`2026-10-14-generate-content-request-passthrough.md`](2026-10-14-generate-content-request-passthrough.md)
(D7, §1 and item 5: "a Vertex text family declares the same when it exists"), [boundary record](2026-09-30-host-zero-vendor-boundary.md)
§3 (credential recipes; "a package needing two different credential kinds for two families … is not expressible yet")
and §13.10 (SF27, Vertex's per-chunk `usageMetadata`), [I3-3](2026-10-13-image-i3-3-vertex.md) (the Vertex location,
project, base URL and credential precedents, V1–V7), embeddings record §16 (`embeddings-vertex`).

## 1. Why

The host's native `vertex_ai` text arm is the last native arm behind the Gemini `/v1beta` surface (speedup plan T5b). D7
made that surface accept `component` rows (host step 2, `server:c286be2d`) and left step 3 waiting for a south Vertex
text family. Without one, a Vertex text row cannot move to a component and the native arm cannot be deleted.

## 2. Decision: a new package, not a second family in `provider-gemini`

`provider-gemini` authenticates with a static secret header (`x-goog-api-key` on `provider_api_key`); Vertex needs a
`bearer` minted by the `vertex_sa` recipe and two configuration values. A package carries one `credentials` section for
the families it covers (boundary record §3), so the two cannot share a package without the "scoped sections" extension
that record defers. The image and embeddings worlds already split the same way (`image-gemini` / `image-vertex`,
`embeddings-gemini` / `embeddings-vertex`). So: a new package `provider-vertex`, world `provider-adapter-v2`, one family.

## 3. What the package does

**Request and response: `provider-gemini`'s, shared.** The IR → `contents` / `generationConfig` / `tools` body, the
stream parser (with SF27's terminal-chunk usage rule, which was found on Vertex), the response reader, refusals and
outcomes are one implementation for both packages: the Gemini reference is refactored so its body, parse and stream
halves are reused, as `gemini_image` is shared by `image-gemini` and `image-vertex`. Only the route, the auth and the
values differ:

- **Route**: `POST {base}/v1/projects/{project}/locations/{region}/publishers/google/models/{model}:generateContent`,
  or `:streamGenerateContent?alt=sse` when streaming; each of `project`, `region`, `model` one percent-encoded segment
  (`vertex_location::publisher_model_path`, as `image-vertex`). Native builds the same shape
  (`server:…/engine/vertex.rs:147` `build_vertex_url`, and `text_admission.rs:839-858`).
- **Auth**: `{"arm": "bearer", "slot": "provider_api_key"}`, the slot minted by `vertex_sa`. No other header.
- **Values**: the family's `config_schema` (`region` required, `project` optional) and the `credentials` section copied
  byte for byte from `image-vertex` / `embeddings-vertex`, so one provider row and one credential's recipe fields serve
  all three Vertex packages. Project and region resolve, and the base URL is refused for another region's Vertex host,
  exactly as in I3-3 §4 (`vertex_location`).
- **Declarations**: `north_request_passthrough` `{"<family>": "generate_content"}` (D7) and `north_passthrough` as
  `provider-gemini` declares them; `request_facts` with the output cap at `/generationConfig/maxOutputTokens` and
  `stream: "url"`; `model.url` matches the `…/models/{model}:` suffix of the Vertex path (**to confirm** against
  gate ①'s pattern rule when implemented).
- **Capabilities**: `chat`, `stream`, `tool_call`, as `provider-gemini`. No `count_tokens` (D7 Q4: countTokens on a
  component row stays 400).

## 4. Differences from the native arm that follow precedent (accepted under lv's 2026-10-13 pre-approval)

- **P1 BYOK refused** (D9; I3-3 V1, embeddings precedent).
- **P2 Row endpoint must be the region's origin**, another region's Vertex host refused before admission; native ignores
  the endpoint and derives the host from the region (I3-3 V2).
- **P3 Missing region or project** is a 500 naming the key before admission; native answers 400 (I3-3 V3).
- **P4 Path segments percent-encoded** (I3-3 V4).
- **P5 The token comes from the recipe executor**, not the native token cache (I3-3 V6).
- **P6 Every `provider-gemini` difference already registered for component Gemini rows** applies unchanged, since the
  request and response code is shared (the D7 dual-run differences: non-stream `accept`, `x-goog-api-client` not
  forwarded, component non-2xx bodies as is).

## 5. Open questions for lv

- **VX-Q1 Which upstream wire serves the chat, messages and responses surfaces.** Native serves `/v1beta` through
  `generateContent`, but serves `/v1/chat/completions` and `/v1/messages` on a Vertex row through Vertex's
  **OpenAI-compatible** endpoint `…/endpoints/openapi/chat/completions` (`server:…/engine/vertex.rs:162-180`,
  `text_admission.rs:870-878`; the code marks the endpoint shape "not verified by a live leg", and Google requires a
  `google/` prefix on the model there, which the row's `upstream_model` must carry). A component translates one IR to
  one wire. Options: (a) `generateContent` for every surface, as `provider-gemini` does: one model id form (no prefix),
  D7 served, SF27 and the Gemini reference reused; the chat surfaces' upstream request changes from OpenAI-shaped to
  Gemini-shaped, registered as a difference, and rows whose `upstream_model` carries `google/` must drop it at cutover;
  (b) the OpenAI-compatible endpoint for the IR surfaces: nearer native on chat, but `/v1beta` could not be served by
  the same package (D7 needs `generateContent`), the model id form would differ between surfaces, and the native shape
  is unverified. **Recommended: (a).**
- **VX-Q2 The family name.** `vertex-ai` (the platform name, as `embeddings-vertex` uses in its world; provider-world
  families are named by platform, like `gemini`) or `vertex-ai-text` (distinct from the other worlds' families, as
  I3-3 chose `vertex-ai-image`). Family names live in each world's own bindings, so they cannot collide. **Recommended:
  `vertex-ai`**, matching how the native dialect is named and how an operator reads a `component` row's
  `south_dialect`.

## 6. What the host must do before a cutover

1. Re-pin to the release that ships `provider-vertex`.
2. D7 step 3: let the `/v1beta` entry route a `component` row of this family (the package declares
   `north_request_passthrough`); today only `gemini` is wired.
3. Component rows for Vertex: `south_dialect` = the family; recipe fields through the existing `component` row path
   (P21 S3 C5); endpoint confirmation (R23) for this package's digest (a confirmation of `image-vertex` or
   `embeddings-vertex` does not carry over); `config_values.region` = `providers.region`, `config_values.project` =
   `providers.group_id` when set; the row endpoint = the region's origin.
4. Per VX-Q1 (a): rows whose `upstream_model` carries `google/` drop the prefix; the cutover preflight lists them.
5. IR billing: Vertex rows are already billed from the Gemini IR on the native arm (the host maps `vertex_ai` to the
   Gemini dialect, `server:…/engine/south_component.rs:655-658`), so a component row bills the same way.
6. A dual run against the native arm on all three IR surfaces and `/v1beta`, stream and non-stream, with the P1–P6 and
   VX-Q1 differences asserted; then a cutover preflight and the C10-style real-key evidence before production.

## 7. Versioning (Q47)

`south-component-conformance` changes (the Gemini reference is split for sharing, a new reference), so it moves one
step past 0.64.0 and every package published with 0.65.0 takes a patch with unchanged behaviour. `provider-vertex`
1.0.0 declares the oldest runtime that admits its declarations (`north_request_passthrough` needs 0.60.0, the minted
slot and `vertex_sa` are older), to be confirmed by `check-declared-runtime`. No contract change.

## Revision note

- 2026-10-15: first draft (proposed).
