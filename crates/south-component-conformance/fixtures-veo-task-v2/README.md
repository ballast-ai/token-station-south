# Frozen samples: Google Veo video (the Gemini API and Vertex AI lines)

Expectations are hand-transcribed from token-station-server's native Veo arm
(`handler/video/gemini.rs` — `build_veo_submit_body`, `build_veo_instance`,
`encode_veo_image_part`; `durable.rs::parse_veo_create`, which feeds a receipt
straight through only when it carries no LRO name *and* is genuinely terminal;
`observe.rs::normalize_veo` for its three fallback keys and the RAI filter; and
the failure wording of `VideoTaskAdapter for Veo`). **Expectations are never
regenerated from the implementation under test.** Not captured live traffic, and
the base64 payloads are placeholder strings.

Where contract 6 lands in this family: the credential travels in the
`x-goog-api-key` header; every artifact is `fetch_with_credential: true` (the
rendered body still carries the upstream URI, so the host must rewrite it to its
own proxy path); `sampleCount` is `requested_outputs` and the number of delivered
samples is `usage.outputs`; a receipt is already terminal, so
`accepted-terminal`.

Differences from the native arm: an input image must already be a `data:` URI
(the host pre-fetches it), and a plain URL is refused with a 400 that says where
to go instead; a non-positive-integer duration is a 400, where the native arm
silently dropped it and reserved against the default 8 seconds; no declared
seconds are reported when the request carries no duration; and the `veo-video`
family refuses to poll a Vertex operation name (`projects/…`).

## The Vertex AI line (`vertex-veo-video`)

The `*vertex*` cases (and `prepare.unknown-family`, `observe.gemini-vertex-locator`)
belong to the family that design record
`docs/design/2026-10-15-veo-vertex-task-component.md` §4 adds, which reproduces the
native arm (lv 2026-10-15, VV-Q2 (b)). Expectations are hand-transcribed from
`durable.rs::veo_run` (the Vertex submit URL from `build_vertex_media_url`, the same
body as the Gemini line), `observe.rs::build_video_query_url` (a `projects/…` name
polled at `{vertex_api_base}/v1/{name}`), `observe.rs::normalize_veo` (the Gemini LRO
shape, which native reads for both rows) and the host test
`durable_vertex_veo_submits_with_the_sa_bearer_at_the_region_host` (the bearer on
submit and poll, the `projects/…` name stored verbatim, the `{"code": 3, "message":
"invalid prompt"}` terminal). The base URL is the location prefix
`{origin}/v1/projects/{p}/locations/{r}` the host binds; the credential is the
host-minted token, presented as a bearer.

What these samples do not prove: the success body's `generateVideoResponse` and the
sample URI are the native arm's assumption, not a capture. Google's documentation
describes `fetchPredictOperation` and `response.videos[]` instead (record §3,
**unverified**), and native never fetched a Vertex clip (its artifact fetch is
fail-closed). The clip URI is a placeholder under the bound endpoint. The operation
name carries a UUID-shaped id, which puts it above 128 bytes, the length a real
Vertex name reaches.

`created.vertex-name` changed with this family: a create response cannot say which
line answered, so a `projects/…` name is accepted, as native `parse_veo_create`
accepts any name. The Gemini family still refuses to poll it (`observe.vertex`).
`storageUri` (VV-Q3) is neither refused nor sent: native builds the upstream body
from the fields it knows (`prepare.vertex-storage-uri`).

`observe.vertex-project-number` and `observe.vertex-other-shape` (0.36.1, Q-VV1, lv
2026-10-15) are **not** native: native polls every name verbatim. A name of the shape
`projects/*/locations/*/publishers/google/models/*/operations/*` is polled under the
binding's own location prefix (the project number in the sample is replaced by the
bound project id), so a host's `authorize` sends it; any other `projects/…` shape is
polled verbatim, as before. Whether Vertex answers with a project number at all is
**unverified** (no capture; record "Q-VV1").

## Credential cases (`task-veo-v2` 0.37.0)

The package declares a `credentials` section scoped to `vertex-veo-video` (design record
`docs/design/2026-10-16-task-vertex-credentials.md`), so gate ② runs the five `credential.*`
cases of `fixtures-image-vertex/`, copied byte for byte except that the two minted
expectations export no attributes: the task world has no value channel, so the section
declares no `project_id` attribute (record §2). No case names a family. The fixture signer is
`FixtureSignerV1` (an FNV-1a stand-in, not a signature).
