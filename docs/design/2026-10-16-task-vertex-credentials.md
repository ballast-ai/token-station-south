# Task world: the Vertex families declare their own credentials

Status: accepted (lv, 2026-10-16, relayed by the host session). Implemented; released in 0.71.0.

Date: 2026-10-16

Baseline: south `origin/main` = `60d85dd` (v0.70.0 plus records). Host references carry a `server:` prefix and refer
to token-station-server's `vertex_ai` provider type as the two records below describe it; nothing here was run against
an upstream.

Related: [Veo on Vertex AI](2026-10-15-veo-vertex-task-component.md) §2, [Omni task component](2026-10-01-omni-task-component.md)
§9, [image-vertex](2026-10-13-image-i3-3-vertex.md) (the recipe reused here).

## 1. What changed and why

Two task-world families relied on the host's `vertex_ai` provider type: `task-veo-v2`'s `vertex-veo-video` and
`task-gemini-omni-v2`'s `vertex-interactions-video`. For such a row the host minted the service-account token by row
type and derived the location prefix the component used as its base URL (Veo record §2; Omni record §9.3). The host is
deleting `vertex_ai`. Production is a fresh deployment (lv, 2026-10-16), so no existing row needs migrating.

Each package now declares a `credentials` section scoped to its Vertex family (`"families": ["vertex-veo-video"]`,
`"families": ["vertex-interactions-video"]`). Apart from `families` it is `image-vertex`'s section (fields
`service_account` and `project_id`, the import rules, slot `provider_api_key` minted by `vertex_sa`, the `vertex_sa`
recipe) with one exception (§2). The Gemini API families (`veo-video`, `gemini-interactions-video`) have no section and
keep the operator's static `x-goog-api-key`. The precedent for a recipe in the task world is `task-kling-v2`.

Manifest only: neither component's logic changes. Both already build every URL under the binding's endpoint
(`reference_veo_task_v2.rs` `under_base`, and `origin` of it for a verbatim `projects/…` poll;
`reference_gemini_omni_task_v2.rs` `{base}/interactions`), and both already present the slot as `bearer`.

## 2. No exported `project_id`

`image-vertex` exports `project_id` as an attribute. The task world has no value channel
(`TASK_WORLD_SCHEMA_V2.value_channel = false`), and gate ① refuses exported credential attributes in such a world
(`validate_component_values`: "credential attributes" is a provider-world declaration). The task sections therefore
declare the same recipe without its `attributes`. The `project_id` field stays, required and imported from the key
file, so one stored service-account credential serves `image-vertex`, `embeddings-vertex`, `speech-vertex`,
`provider-vertex` and both task families alike. Nothing is lost: the project reaches the component through the
endpoint (§3), not through `ProviderConfig.declared`, which no task host builds.

## 3. The endpoint carries the location prefix in full

The task world admits no `config_schema` and no endpoint template, so there are no `region` / `project` keys. The
operator writes the prefix the host used to derive as the component row's endpoint:

- `vertex-veo-video`: `https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}`, or
  `https://aiplatform.googleapis.com/v1/projects/{project}/locations/global` for `global` (Veo record §2). Submit is
  `{endpoint}/publishers/google/models/{model}:predictLongRunning`; a publisher-model operation name is polled under the
  same prefix (Q-VV1), any other `projects/…` name at `{origin}/v1/{name}`.
- `vertex-interactions-video`: the `v1beta1` prefix,
  `https://aiplatform.googleapis.com/v1beta1/projects/{project}/locations/global` (Omni record §9.2, §9.3: the only
  location sampled is `global`, and the Interactions API is under `v1beta1`, not `v1`). The component appends
  `/interactions` and `/interactions/{id}`. This is §9.3's stopgap made the only form.

A wrong prefix is the operator's error and surfaces as an upstream 404; the component cannot check it.

## 4. Trust

As with `image-vertex`, the recipe's token endpoint (`https://oauth2.googleapis.com/token`) is shown to the operator and
confirmed per package digest before the host mints with it (boundary record §3.4). A confirmation for another package's
digest does not carry over.

## 5. Gate ② and versions

Both fixture packs gain the five Vertex `credential.*` cases of `fixtures-image-vertex/`, byte for byte except that the
two minted expectations export no attributes (§2). `shipped_packages_v1` pins that each section applies to the Vertex
family only and equals `image-vertex`'s apart from `families` and the attribute, and that gate ① admits the manifest.

A new declaration is a minor version: `task-veo-v2` 0.36.4 → 0.37.0, `task-gemini-omni-v2` 0.1.8 → 0.2.0. No
`south_runtime` moves: `CredentialsV1.families` exists since 0.44.0, the recipe language used here was admitted at
0.57.0 (`embeddings-vertex` declares the identical recipe with 0.57.0), and the task-v2 world admits a `credentials`
section at 0.57.0 (`task-kling-v2` declares one with it); the packages keep 0.57.0 and 0.62.0. The change to the
conformance crate (reference metadata versions, fixtures) opens the 0.71.0 cycle's Q47 round:
`south-component-conformance` 0.69.0 → 0.70.0, and every other official package takes a patch
(`task_vertex_credentials_retires_every_published_070_package_identity`).

## 6. Host

After re-pinning, the host binds the two Vertex families on `component` rows whose endpoint is the prefix of §3 and
whose credential carries the recipe fields, and confirms the token endpoint for each package digest. Unverified: the
whole Vertex task path against a real account (both records list what a real-account test must settle).
