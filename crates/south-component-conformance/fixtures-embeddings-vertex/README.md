# Frozen samples: Vertex AI embeddings (`:predict`)

Gate ② samples for `embeddings-vertex` (family `vertex-ai`), judged by `south.embeddings-component.v1`
(`docs/design/2026-09-30-embeddings-contract.md` §10, §16), including the package's `credential.*` cases.

Expectations are hand-transcribed from token-station-server's native Vertex embeddings arm, never regenerated from
the implementation under test:

- `vertex_embeddings_request` in `gateway/src/modules/inference/handler/embeddings.rs`: one text input only (more than
  one is "Vertex embeddings accept exactly one input per request (got N); send one request per text", a token-id
  input "Vertex embeddings take text input; token-id arrays are not supported", a text of whitespace only "'input'
  must be a non-empty string"); the body `{"instances": [{"content": text}]}` plus
  `parameters.outputDimensionality` when `dimensions` is set; the immutable field `instances`
  (`VERTEX_EMBEDDINGS_OWNED_FIELDS`).
- `build_vertex_media_url` and `vertex_host_for_region` in `gateway/src/modules/inference/engine/vertex.rs`:
  `https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}/publishers/google/models/{model}:predict`,
  with the unprefixed `https://aiplatform.googleapis.com` for `global`; the project is the row's override
  (`providers.group_id`, here the `project` config key) before the credential's `projectId` (here the exported
  `project_id`).
- `vertex_predict_to_openai`: usage is `embeddings.statistics.token_count` (or `tokenCount`) read as a float, refused
  when missing, negative or not a number, and rounded with `f64::round`, which rounds half away from zero: 2.5 is 3,
  where rounding half to even would give 2; 13.0 is 13. A single input reports `per_input_tokens` of one element.
- The Vertex SA mint (`VertexMint` and `sign_vertex_assertion` in
  `gateway/src/modules/inference/engine/token_refresh.rs`): an RS256 assertion with `iss` = the service account's
  `client_email`, `scope` = `https://www.googleapis.com/auth/cloud-platform`, `aud` = the token endpoint, `iat` = now
  and `exp` = now + 600, posted as a form (`grant_type` = `urn:ietf:params:oauth:grant-type:jwt-bearer`,
  `assertion`) to `https://oauth2.googleapis.com/token`; the token expires `expires_in` seconds later; a 4xx needs the
  operator, a 5xx is transient. The fixture signer is `FixtureSignerV1` (an FNV-1a stand-in, not a signature), so the
  JWT's last part is its eight bytes; the claims are in the interpreter's canonical order (`iss`, `aud`, `exp`, `iat`,
  then `scope`), where the host's struct writes `iss, scope, aud, iat, exp`: equal JSON, different bytes, as the
  boundary record §13.3 notes for every recipe.
- Error mapping: Vertex answers with the google.rpc status envelope, mapped as the Gemini reference maps it.

Request rows run with `provider_config.declared` holding `region` and the exported `project_id` (and `project` in
`request.project-override`), which is what a host builds per attempt (record §16). The base URL is the location's API
origin; `request.global-prefixed-host` and `request.region-mismatch` show a Vertex AI origin that is not the one the
location selects refused before admission, and `request.proxy-base` a base URL that is not a Vertex AI origin used as
given.

`response.count-mismatch` answers one input with two predictions. The component reports a count per prediction, so the
host's first consistency check, `per_input_tokens` against the input count, refuses it (`invalid_usage`) before the
vector count is compared; the native arm billed and returned only the first prediction.

Credential cases: `credential.clock.vertex-service-account` mints (`expires_in` 3599); `vertex-short-expiry` shows an
expiry of 120 s raised to the recipe's floor of 301 s, one past its 300 s refresh margin (the host refreshes 300 s
before expiry, `REFRESH_BUFFER_SECS`); `vertex-missing-project` is a configuration error before any request; the two
`on-status` cases are a 400 `invalid_grant` (`reauth_required`) and a 503 (`transient`).
