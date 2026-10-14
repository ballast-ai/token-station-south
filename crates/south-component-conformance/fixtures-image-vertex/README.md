# `fixtures-image-vertex`

Gate ② pack (`south.image-component.v1`) of `image-vertex`, image world step I3-3
(`docs/design/2026-10-13-image-i3-3-vertex.md` §3 to §5, §8), including the package's `credential.*` cases.

The request, response and render pieces are the shared Gemini image module, which `fixtures-image-gemini/` already
covers row by row; this pack covers what is Vertex's own (the route, the location rules, the bearer auth on the minted
slot, generation only) and one sample of each shared behaviour the record names. The inputs were transcribed from the
server repository's Vertex image tests, read at `origin/dev-v2`: `gateway/tests/images_providers.rs`
(`vertex_nano_banana_generates_at_the_region_host_with_the_sa_bearer_and_bills_per_image`: region `us-central1`,
credential project `test-project`, model `gemini-3.1-flash-image`, prompt `a cat`, the `Bearer` access token and no
`x-goog-api-key`, the answer `aGk=` with one prompt and one candidate token;
`vertex_image_edit_is_refused_explicitly_before_any_upstream_call`) and
`gateway/src/modules/inference/handler/images/tests/gemini.rs`
(`the_vertex_nano_banana_url_targets_generatecontent_on_publishers_google`: `publishers/google/models/…:generateContent`
below the region's origin, the credential's `proj-a` and the `override-project` override that wins over it). The
`prepare.generate`, `response.usage` and `response.prompt-blocked` expectations were written by hand before the
reference ran; the others were produced by the reference and reviewed against the record row by row.

Rows: a plain generation with its exact path and `bearer` auth on `provider_api_key` (`prepare.generate`); the `global`
location on the unprefixed origin; a regional location (`europe-west4`); a proxy base used as given; the `project`
override over the exported `project_id`; the exported `project_id` alone; a reference image as a `data_url_payload`
reference; `n` 10 as `repeat`; a multipart (edit) request refused with 400, since a host that skips the operation check
must still get a refusal; `resolution` on a per-image call (the `images` metering sample, with the tier word). Responses:
the host test's success with usage, a 401 and a 429 in Google's error envelope, a blocked prompt (`rejected`), each
declared token bucket missing (`missing` rows), and a per-image success (Vertex's `usageMetadata.trafficType` is read by
neither the native arm nor the package). Render: two rounds of one call.

Not in this pack: the configuration refusals (a missing `region`, a missing project, a Vertex AI base URL of another
region, the prefixed `global` host). They are 500s naming the value, as `embeddings-vertex` answers (record §4, V3), and
the suite's `pre_dispatch_refusal` row admits only `invalid_request` and `capability` from `prepare`, so they are pinned
by name in `tests/vertex_image_suite.rs` (`CONFIGURATION_REFUSALS`) and through the guest in
`tests/vertex_image_sandbox_parity.rs`.

Credential cases: the five `credential.*` cases of `fixtures-embeddings-vertex/`, copied byte for byte (the two packages
declare the same `credentials` section, so the same samples judge both; no case names a family, so nothing needed
renaming): `credential.clock.vertex-service-account` mints (`expires_in` 3599); `vertex-short-expiry` shows an expiry of
120 s raised to the recipe's floor of 301 s; `vertex-missing-project` is a configuration error before any request; the
two `on-status` cases are a 400 `invalid_grant` (`reauth_required`) and a 503 (`transient`). The fixture signer is
`FixtureSignerV1` (an FNV-1a stand-in, not a signature).

**Unverified**: that Vertex answers with the same `google.rpc` error envelope as the Gemini API (the 401 and 429 bodies
here are Google's documented shape, not a capture), and that image models report `trafficType`.
