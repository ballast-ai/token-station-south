# Token Station South

Token Station South is the host-neutral southbound provider execution boundary shared by the
Token Station community and enterprise hosts. "Southbound" means the path from a host to model
providers; routing, tenancy, billing, quotas, persistence, and user-facing behavior remain in each
host.

> [!WARNING]
> This repository ships host-neutral libraries, not a complete host product. Both community and
> enterprise hosts have verified provider-call v1 adapters in narrow scopes. Provider-stream and
> provider-quota-metadata verification are recorded independently per host in the compatibility
> manifest, and most host production traffic remains outside South.

## Repository boundaries

This repository owns provider-facing contracts, transports, provider component APIs and runtimes,
conformance fixtures, and migration test tooling. It must not depend on `token-station` or
`token-station-server`; both hosts consume this repository in one direction.

South does not read databases, environment variables, config files, keychains, or secret stores.
Hosts resolve credentials and inject capabilities explicitly. Provider components have no network,
filesystem, or secret access by default.

See [Architecture](ARCHITECTURE.md), [Compatibility](compatibility.json),
[Security](SECURITY.md), [Contributing](CONTRIBUTING.md), the
[repository bootstrap design](docs/design/2026-08-16-repository-bootstrap.md), and the
[minimal provider call design](docs/design/2026-08-16-minimal-provider-call.md), the
[streaming provider call design](docs/design/2026-08-17-streaming-provider-call.md), and the
[community compatibility release](docs/design/2026-08-17-community-host-compatibility-release.md),
the
[provider quota metadata design](docs/design/2026-08-17-provider-quota-response-metadata.md), the
[header-secret auth design](docs/design/2026-08-17-header-secret-auth.md), the
[controlled query design](docs/design/2026-08-18-controlled-query-support.md), the
[controlled user-agent design](docs/design/2026-08-20-controlled-user-agent.md), the
[host prelude design](docs/design/2026-08-20-host-prelude.md), the
[host-signed request finalizer design](docs/design/2026-08-20-host-signed-request-finalizer.md)
(shipped in 0.14.0; see its §8 for the one half deliberately left), the
[canonical IR inventory](docs/design/2026-08-21-canonical-ir-inventory.md), the
[provider-api promotion](docs/design/2026-08-21-provider-api-promotion.md), the
[component conformance gates](docs/design/2026-08-21-component-conformance.md), the
[provider runtime](docs/design/2026-08-21-provider-runtime.md), the
[OpenAI-compatible host parity](docs/design/2026-08-21-openai-compat-host-parity.md), the
[Anthropic provider component](docs/design/2026-08-22-anthropic-provider-component.md), the
[Gemini provider component](docs/design/2026-08-22-gemini-provider-component.md), the
[renderer refusal for unmappable blocks](docs/design/2026-08-23-renderer-refusal-for-unmappable-blocks.md),
the [task adapter vocabulary](docs/design/2026-08-27-task-adapter-vocabulary.md) (proposed), the
[manifest schema beyond one world](docs/design/2026-08-27-manifest-schema-beyond-one-world.md)
(proposed), and the
[released component artifacts](docs/design/2026-09-10-released-component-artifacts.md) (proposed).

## Implemented library slice

- `south-contracts` defines bounded HTTP (the JSON POST request, the body-less GET request, and
  the multipart POST request, whose opaque bytes travel under a media type the contract renders
  from a validated boundary, and since HTTP contract 12 the SSML text POST request, whose bounded
  UTF-8 body travels under `application/ssml+xml`, a media type from a closed set),
  Bearer, sanctioned header-secret, combined Bearer-plus-header-secret, and package-declared
  header-secret authentication (auth contract 5, whose declared names join the reserved headers
  and leave the response transcript for that package's calls), stable
  error, byte-streaming, and closed provider quota metadata contracts, plus a buffered binary
  response beside the UTF-8 one, which keeps its guarantee unchanged — including reserved-header
  enforcement, redacted diagnostics, and the sanctioned controlled query and controlled
  user-agent request declarations.
- `south-core` binds a validated endpoint to one credential slot, resolves the host-owned secret,
  and applies cancellation and caller deadlines around prepared buffered and streaming JSON POST
  calls, buffered body-less GET calls, buffered multipart POST calls, and JSON POST, multipart
  POST and SSML text POST calls whose response is buffered as opaque bytes rather than proved to
  be UTF-8 (the last two since HTTP contract 12). Its
  `raw` module is the shared host prelude: a borrowed raw-call type, string-in contract parsing
  that names the failing field, zero-side-effect one-shot wrappers, and the pre-resolved and
  size-bounding credential resolver adapters both hosts previously hand-rolled — plus the
  host-signed twin of that raw call and its wrappers, which take a host finalizer in place of a
  credential resolver, the body-less GET twin a task poller hands over, and the multipart twin a
  transcription or image-edit path hands over.
- `south-host-grammars` holds pure, bounded wire grammars that only hosts call, so no
  component links it and a change to it leaves every package byte-identical. Today it holds the
  server-sent events decoder `decode_sse_v1` and its incremental form `SseDecoderV1`, with golden
  vectors (`crates/south-host-grammars/tests/vectors/sse-v1.json`) and a scheduled fuzz target:
  the media worlds build a component's view of an SSE body with it, and `north_passthrough`
  cuts a forwarded stream into whole frames with it. It has no dependencies; both rules are
  checked (boundary record §13.13). Unreleased; it ships in 0.53.0.
- `south-transport-reqwest` executes hardened buffered and byte-streaming JSON POST requests,
  buffered body-less GET requests, buffered multipart POST requests (emitting the media type
  the prepared request renders, and sharing the body's allocation rather than copying it), and
  SSML text POST requests under the `application/ssml+xml` media type the contract renders, and
  binary-response requests under their own larger body cap,
  applies the request's sanctioned user-agent declaration exactly once, applies every auth header
  the prepared request carries (one for the credential arms, the finalizer's diffed set for the
  host-signed arm), adds exactly `TRANSPORT_ADDED_HEADERS_V1` and nothing else, captures only the
  nine bounded quota metadata fields, redacts the request's declared secret headers from the
  response transcript, and keeps redirects, retries, compression, cookies, referer
  propagation, and implicit system proxies disabled. `TransportPairV1` builds the buffered and
  streaming transports from one timeout configuration.
- `south-provider-conformance` publishes immutable `south.provider-call.v1`,
  `south.provider-stream.v1`, `south.provider-quota-metadata.v1`, `south.header-auth.v1`,
  `south.controlled-query.v1`, `south.controlled-user-agent.v1`, `south.provider-get.v1`,
  `south.provider-multipart.v1`, `south.provider-binary.v1`, and `south.provider-media-binary.v1`
  fixtures, while `south-testkit`
  runs them against assembled host executors. Unreleased: `south.provider-media-binary.v1`,
  `south.provider-multipart.v1` and `south.provider-get.v1` each gain a wire claim,
  `wire_auth_exact` (mismatch category `WireAuth`): the transport's complete `auth_headers()` list
  must be exactly the one pair the suite's `…AuthArmV1::expected_wire_auth_header` builds from the
  row's declared arm, so an adapter that sends a header secret as Bearer fails the header-secret
  rows (two, one and one). Case counts and suite versions are unchanged; each suite's evidence
  constructor (`ProviderMediaBinaryEvidenceV1::new`, `ProviderMultipartEvidenceV1::new`,
  `ProviderGetEvidenceV1::new`) takes one more argument, last. **A host must re-verify all three
  suites after its adapter measures the new claim**: the `verified` status `compatibility.json`
  records for `provider_multipart` and `provider_get` was earned without it. It also carries the host-implemented
  `south.credential-recipe.v1` suite (gate ③ of credential recipes): a harness the host wraps around
  its own recipe executor and credential store, an in-process fake token endpoint, and the runner.
  Two more gate ③ suites are host-implemented the same way: `south.eventstream-framing.v1` (the host's
  framing executor: `bytes` versus `aws-eventstream`, canonical re-encoding, faults, the buffered path) and
  `south.request-signing.v1` (the declaration-selected SigV4 finalizer, verified by recomputing the signature).
  The media worlds' gate ③ suite `south.safe-fetch.v1` holds the host's artifact-URL fetch executor to the image
  record's §11 rules: its fixtures here describe a whole fake network (resolver answers, servers, a system proxy),
  and the `south-testkit` runner hands those fakes to the host's executor and judges what it resolved, where it
  connected, what it sent and what it returned, with a reference executor built on `ArtifactUrlV1::parse` and
  `is_forbidden_egress_address`.
- `south-provider-api` owns the v2 provider component ABI: the WIT package
  `token-station:adapter@2.0.0` (world `provider-adapter-v2`, JSON payloads named by
  canonical type, raw-bytes stream chunks) and the component `manifest.json` schema
  carrying the seven-field compatibility tuple the runtime handshake refuses on mismatch.
- Provider instances a package declares (B7a): a provider manifest may declare query parameters
  (a restricted name plus a closed value syntax), the response headers that feed the closed quota
  metadata fields, and a per-family user-agent. Gate ① validates them,
  `south_component_conformance::DeclaredInstancesV1` turns an admitted manifest into the contract
  types (`QueryParameterV1::Declared`, `ProviderQuotaHeaderMapV1`, `DeclaredUserAgentV1`), and
  `ReqwestTransportV1::with_quota_headers` captures a package's declared quota headers.
- The component value channel (Q14): a component reads `ProviderConfig.declared`, which holds its
  family's `config_schema` keys and the attributes the selected credential exports (a field, or the
  recipe a selector chose), and `ChatRequest.host_values`, which holds the host values the manifest
  declares in `host_values` (today only `attempt_id`). `ComponentManifestV1::declared_keys` and
  `declared_values` give a host the keys and the per-attempt map, and gate ② checks that a component
  ignores every undeclared key.
- `south-component-conformance` is gates ① and ② of the four-gate layering: package
  admission (manifest, reported identity, tuple handshake) and the
  `south.provider-component.v1` behavior suite (fixture-pinned translation, determinism,
  byte-level stream incrementality, endpoint confinement, error-catalog discipline),
  judged against a typed component seam and shipped with the native reference
  implementations of `provider-openai-compatible`, `provider-anthropic` and
  `provider-gemini`, each with its own frozen fixture pack — sharing one pack would freeze
  whichever dialect was written first. It is this repository's one sanctioned typed consumer of the Canonical IR, taken
  at a fixed kernel revision.

- `south-provider-runtime` executes provider components inside a wasmtime sandbox:
  gated loading (manifest, forbidden-import scan, reported identity), locked-down WASI
  (no preopens, no environment, no sockets/http by refusal), per-store memory limits,
  epoch call deadlines, boundary payload ceilings, one instance per stream — with a
  deliberately JSON-only API face, so the runtime never consumes the Canonical IR. The
  conformance crate's `sandbox` feature provides the typed seam over it, and `components/`
  packages each native reference as an official `wasm32-wasip2` component:
  `provider-openai-compatible` (`scripts/build-reference-component.sh`) covers the
  OpenAI-compatible and Azure dialects, `provider-anthropic`
  (`scripts/build-anthropic-component.sh`) covers Anthropic Messages, and
  `provider-gemini` (`scripts/build-gemini-component.sh`) covers Gemini
  `generateContent` — including its streaming operation, which the dialect selects with a
  different URL suffix rather than a body field. A component and its
  native reference are the same code, so sandbox parity is a property of construction that
  the parity tests then prove end to end.

The slice does not include a
synchronous transport, retries, fallback, routing, persistence, database access, or host adapters.
Passing a library conformance suite does not by itself verify a host integration; each verified
capability also requires review of the real host adapter wiring.

## Release notes

Each release has a design record; those records are the detail, and this table is
the index. A published tag does **not** mean either host has adopted the packages
it carries — adoption is recorded per host in `compatibility.json`.

| Release | What it added | Record |
|---|---|---|
| `0.29.0` | The `task-adapter-v2` world alongside provider-v2 and task-v1: `TaskLocatorV2` for bounded locators, `TaskUsageFactsV2` for coexisting usage, observation keeping queued/running and per-artifact id and duration, and rendering identity and time passed in explicitly by the host. Also the separate `TaskRequestEstimateV2`: the component states the final request's duration and protocol unit rate, the host chooses the estimate, and price, markup, missing-duration defaults and the funding transaction all stay with the host. | [candidate](docs/design/2026-09-20-task-adapter-v2-candidate.md), [request estimate](docs/design/2026-09-20-task-request-estimate.md), [release](docs/design/2026-09-20-release-0.29.0.md) |
| `0.30.0` | `task-minimax-v2` (Hailuo and H3), the HTTP9 `file_id` controlled query, and the Task 5 canonical request-input facts. | [component](docs/design/2026-09-20-minimax-v1-task-component.md), [H3 facts](docs/design/2026-09-20-minimax-h3-estimate-facts.md), [release](docs/design/2026-09-20-release-0.30.0-minimax.md) |
| `0.31.0` | `task-bailian-v2`, covering managed video; images and native pass-through are not migrated. Reuses the Task 5 / HTTP9 / Task V2 WIT with no new runtime capability. | [component](docs/design/2026-09-20-bailian-video-task-component.md), [release](docs/design/2026-09-20-release-0.31.0.md) |
| `0.32.0`–`0.36.0` | Runtime and `compatibility.south_runtime` only; package identities unchanged except where a component's own behaviour changed. | — |
| `0.37.0` | Responses in `south-north-codec`: request→IR, IR response→Responses, and IR stream events→SSE, plus the same-source JSON façade. The host passes identity, time, inbound tools and compatibility options and holds the per-stream state; admission, billing, the continuation cache and Native pass-through stay with the host. | [release](docs/design/2026-09-28-release-0.37.0.md), [design](docs/design/2026-09-28-responses-north-codec.md), [evidence](docs/design/2026-09-28-responses-north-codec-validation.md) |
| `0.39.0` | Responses reasoning replay on kernel v0.3.0 / protocol 0.4.0: a bounded `tsr.c1.` carrier preserves Claude thinking, signature, redacted thinking, and text/tool reference order. Only Anthropic Messages and Bedrock Converse models that explicitly declare `reasoning_replay.claude.v1` may receive it; OpenAI-compatible and Gemini refuse. `canonical_ir` ownership stays with the kernel, so South's `compatibility.json.contracts.canonical_ir` remains `null`. | [release](docs/design/2026-09-29-release-0.39.0.md) |
| `0.40.0` | IR usage follows the kernel partition contract: `provider-anthropic` (1.0.6) and `provider-bedrock-converse` (1.0.3) report the whole prompt as `input_tokens`, Converse accepts a `totalTokens` that counts the cache buckets (as AWS sends it), and the Anthropic north renderers subtract the buckets. `usage_ir_contract_v1` judges every reference against its provider's documented prompt formula. | — |
| `0.41.0` | Per-model Claude request dialect declared by the host in `supported_parameters` (`anthropic.*` words): `provider-anthropic` (1.0.7) and `provider-bedrock-converse` (1.0.4) drop sampling parameters the model rejects, refuse a forced tool choice it cannot honor, and carry the caller's reasoning effort as adaptive thinking or a bounded budget; declaring nothing keeps every request unchanged. | — |
| `0.42.0` | `anthropic.sampling.exclusive`: for a model that accepts `temperature` or `top_p` but not both (Opus 4.5 through Sonnet 4.6), `provider-anthropic` (1.0.8) and `provider-bedrock-converse` (1.0.5) keep `temperature` and drop `top_p` instead of sending a request the upstream rejects. | — |
| `0.43.0` | The host-zero-vendor-boundary phases B1–B4 and B7a: strict usage (missing or inconsistent usage is a protocol error; gate ② usage rows, `UsageNeverDefaulted`, `usage_evidence`); descriptor auth admission, per-family `request_facts`, `endpoint` / `config_schema`, an AWS eventstream deframer with canonical re-encoding, `stream_framing` and `signing`; the range handshake (`runtime_abi`, `HostRangeV1`), per-package isolation (`load_package_set`), a machine-readable release index with gate ② reports and a digest-stability check; credential recipe v1 (gate ①, reference interpreter, gate ③ suite `south.credential-recipe.v1`; `task-kling-v2` declares the Kling JWT); package-declared secret headers, query parameters, quota headers and user-agent. Contracts: auth 5, reserved header policy 2, HTTP 10, provider quota metadata 2. **Breaking for hosts on re-pin** (raw-call fields, `UserAgentV1`, non-`Copy` `QueryParameterV1`, new enum variants). | [boundary record](docs/design/2026-09-30-host-zero-vendor-boundary.md) §6.6, §13.1–§13.4 |
| `0.44.0` | The host-zero-vendor-boundary S3b prerequisites and the host's S3a feedback: a recipe's `present` takes ordered candidates, a step output or a declared secret field with its own validity (D1); a `credentials` section may be scoped to `families`, read through `credentials_for` (D2); a secret slot may name a declared field, and where a section applies every secret slot must name its source (D3, `stored_slot_value_v1`); optional `CredentialFieldV1.description` (D4) and an optional `default` on config keys; gate ③ gains a tenth case, `TransientFailureIsRetried` (suite still version 1); the Kiro draft uses the four-rule auth-method selector. `provider-openai-compatible` (2.2.0) adds the `github-copilot` family: chat at `https://api.{plan}.githubcopilot.com` with `plan` `individual` / `business` / `enterprise` (default `individual`), the six identification headers, the declared user-agent, and a family-scoped recipe that exchanges the GitHub token or, on 404, confirms the seat and presents the token for 3600 s. The other twelve packages take a patch bump with unchanged behavior. No contract number changes. **Breaking for hosts on re-pin** (`RecipeV1::present` is optional, `SlotV1::Field`, new fields on `CredentialsV1` / `CredentialFieldV1` / `ConfigKeyV1`, a new `CredentialRecipeCaseIdV1` variant). | [boundary record](docs/design/2026-09-30-host-zero-vendor-boundary.md) §13.5, §16 Q19–Q22 |
| `0.45.0` | The host's S2 / S4 prerequisites (SF10–SF19). A package's `south_runtime` is the oldest runtime it needs, not the release that carries it: this release re-stamps no manifest (every package keeps `0.44.0`), the digest-stability check also fails a changed `manifest.json` under an unchanged version, and `scripts/check-declared-runtime.sh` loads each package under exactly the runtime it declares. Gate ① requires every `signing.credentials` entry to name a declared secret field of the section that applies to each signed family, so a runtime of this release refuses `provider-bedrock-converse` 1.0.7. `provider-bedrock-converse` (1.0.8) declares its SigV4 credential fields and sends `accept` and `x-amzn-bedrock-accept` as the host's native arm does; the new `provider-bedrock-converse-bearer` (1.0.0, family `bedrock-bearer`, `bearer` arm) serves Bedrock API keys. Also: `SandboxedComponentV1::shared`, `ComponentManifestV1::endpoint_values`, gate ③ suites `south.eventstream-framing.v1` (nine cases) and `south.request-signing.v1` (seven cases), both `not_verified` for both hosts, and the unreleased `t21-unseen-eventstream` guest. The other twelve packages take a patch bump with unchanged behavior. No contract number changes; the kernel pin is unchanged. **Breaking for hosts on re-pin** (`SandboxedComponentV1::new` and `inner` are no longer `const fn`; the new gate ① refuses Converse 1.0.7). | [boundary record](docs/design/2026-09-30-host-zero-vendor-boundary.md) §13.6, §16 Q23–Q34 |
| `0.46.0` | The kernel re-pin to protocol 0.5.0 (mirror `v0.4.0`, `canonical_ir` 3), B7b, #138 and the Q14 value channel (#151, #152, #154). Every package declares `ir_schema_id` `token-station-protocol@0.5.0/v0.4.0` and `kernel_contracts.canonical_ir` 3, and `south_runtime` `0.46.0`: no earlier runtime records `canonical_ir` 3, so under the declared-runtime discipline 0.46.0 is the oldest runtime that admits them. A host recording `canonical_ir` 2 refuses every package, and a 0.46.0 host refuses packages built for 2 (one-way flag day). Protocol 0.5.0's `%2F` rule (D5) admits an encoded slash inside one path segment, for ARN model ids (#138: a model-with-a-slash row in the Converse, Converse-bearer and Gemini packs); the seven task-v2 packages that put a task id in one observe path segment refuse an id containing `/` before building a request. B7b: the provider-world auth arm `bearer_and_header_secret` (`AdmittedAuthV1::BearerAndHeaderSecret`, descriptor `Auth::BearerAndHeader` over a sanctioned header), and `provider-openai-compatible` (2.4.0) adds the `gemini-openai-compatible` family; Copilot `plan` is required with no default (#150). Five more never-credential header names are undeclarable as secret headers (Q40). Q14: `ProviderConfig.declared` carries a family's `config_schema` keys and exported credential attributes (`ComponentManifestV1::declared_keys` / `declared_values`; `AttributeV1` from a `field` with a declared syntax, or `selected_recipe`), and `ChatRequest.host_values` the closed vocabulary `attempt_id`; gate ② gains `undeclared_values_ignored`; gate ③ `south.credential-recipe.v1` gains the `Attributed` kind and two cases (twelve, suite still version 1), so `token-station-server`'s `credential_recipe` is `not_verified` until it runs them. The other thirteen packages take a patch bump. Contracts: `kernel_contracts.canonical_ir` 2 → 3; no south contract number changes. **Breaking for hosts on re-pin** (kernel pin and `HostRangeV1.kernel_contracts.canonical_ir` move together; new `DescriptorAuthErrorV1`, `ManifestErrorV1`, `CredentialRecipe*V1` and `CheckV1` variants; `AttributeV1.field` is optional; required `CredentialRecipeSessionV1::exported_attributes`). | [kernel re-pin](docs/design/2026-10-08-kernel-repin-protocol-0.5.0.md), [boundary record](docs/design/2026-09-30-host-zero-vendor-boundary.md) §13.7, §13.8, §16 Q35–Q45 |
| `0.47.0` | The `embeddings-adapter-v1` world. Embeddings contract 1 (text and token-id inputs only; media inputs are refused for a 400 before admission, a request view above 16 MiB is answered 413 before admission, and GLM, GLM Coding and Copilot are not migrated), suite `south.embeddings-component.v1`, the host functions `extract_vectors_v1`, `check_embeddings_response_v1` and `render_vectors_v1`, and the packages `embeddings-openai-compatible` (1.0.0, `openai-compatible` and `azure-openai-v1`) and `embeddings-gemini` (1.0.0), both `not_verified` for both hosts; the community host is asked for a synchronous implementation (E-Q5). The two packages declare `south_runtime` `0.47.0`, the first runtime that knows the world. The shared crates every guest links changed, so the other fourteen packages take a patch bump with unchanged behavior and keep `south_runtime` `0.46.0`. Contracts: `embeddings` 1 (new); `compatibility.json` `schema_version` 6. The kernel pin is unchanged. **Breaking for hosts on re-pin** (new `CheckV1` variants `LocatorResolves` and `NamedRowAssertion`, new `ManifestErrorV1::EmbedCapabilityRequired`). | [release](docs/design/2026-10-08-release-0.47.0.md), [embeddings record](docs/design/2026-09-30-embeddings-contract.md) |
| `0.48.0` | The embeddings value channel and `embeddings-vertex`. Gate ① admits a family's `config_schema` and exported credential attributes in the `embeddings-adapter-v1` world, the two sources of `ProviderConfig.declared`, under the provider world's rules; `host_values`, `endpoint` and the other provider-world declarations stay refused, and the task worlds are unchanged. `south.embeddings-component.v1` gains `undeclared_values_ignored` (suite still version 1). New package `embeddings-vertex` (1.0.0, `vertex-ai`, Vertex AI's `:predict`): one text input per request, `region` (required) and `project` (optional override) as config keys, a service-account credential recipe (RS256 `jwt_sign`, then an `oauth2_token` exchange at `https://oauth2.googleapis.com/token`) that exports `project_id`, and the location's API origin as `base_url`; it declares `south_runtime` `0.48.0`, the first runtime whose gate ① admits it, and is `not_verified` for both hosts. The shared crates every guest links changed, so the other sixteen packages take a patch bump with unchanged behavior and keep their `south_runtime`. No contract number changes; the kernel pin is unchanged. Not breaking for hosts on re-pin; a host that routes `embeddings-vertex` builds `declared` for this world as for the provider world (embeddings record §16). | [release](docs/design/2026-10-08-release-0.48.0.md), [embeddings record](docs/design/2026-09-30-embeddings-contract.md) §16 |
| `0.49.0` | Host feedback SF27: a Gemini stream's usage comes from its terminal chunk. `provider-gemini` (1.1.11) takes `Usage` only from the chunk in which a candidate carries `finishReason`, so Vertex AI native streams, whose intermediate chunks carry a `usageMetadata` without counts (`{"trafficType": "ON_DEMAND"}`), are no longer refused; an intermediate `usageMetadata` must be an object whose counts never decrease, and any frame after the terminal chunk is a protocol error. Two gate ② stream fixtures. The shared crates every guest links changed, so the other sixteen packages take a patch bump with unchanged behavior and keep their `south_runtime`. No contract number changes; the kernel pin is unchanged. Not breaking for hosts on re-pin. | [release](docs/design/2026-10-08-release-0.49.0.md), [boundary record](docs/design/2026-09-30-host-zero-vendor-boundary.md) §13.10 |
| `0.50.0` | The model catalog (B6-1, #161): `catalogs/model-catalog.json` ships as `model-catalog-v0.50.0.json`, is covered by `SHASUMS256.txt`, and is listed in `south-release-index.json` under `catalogs` with schema `south.model-catalog.v1` (image and video capabilities by upstream model id, read by `south_provider_runtime::ModelCatalogV1`); the first release whose `catalogs` is not `[]`. The first release under Q47 (#163): `south-contracts`, `south-provider-api` and `south-component-conformance` carry their own versions (still 0.49.0), so no package changes: all seventeen keep their version, `manifest.json`, `component.wasm` and `south_runtime`. No contract number changes; the kernel pin is unchanged. Not breaking for hosts on re-pin. | [release](docs/design/2026-10-08-release-0.50.0.md), [boundary record](docs/design/2026-09-30-host-zero-vendor-boundary.md) §13.11, §13.12 |
| `0.51.0` | Embeddings contract 2: media inputs inline and bounded, independent of `contracts.media` and the image world (E-Q8 and D3 superseded for media). `EmbeddingInputV1::Media`, `parse_embeddings_request_v2`, the `media` capability word (gate ① requires `contracts: {"embeddings": 2}` with it), the inline `embeddings.request.media` row and `MediaInputsFollowTheDeclaration`; contract 1 behavior and wire shapes unchanged. `embeddings-gemini` 1.1.0 carries Gemini's multimodal models (`south_runtime` 0.51.0). A host admitting a `media` package must give its embeddings runtime at least 192 MiB of guest memory (`RuntimeLimitsV1::for_embeddings_media()`; the default 64 MiB traps above 7.9 MiB of base64) and answer 413 above a 15 MiB request view (E-O2). `south-contracts`, `south-provider-api` and `south-component-conformance` take 0.50.0 under Q47, so the other sixteen packages take a patch bump. | [release](docs/design/2026-10-09-release-0.51.0.md), [record](docs/design/2026-09-30-embeddings-contract.md) §17 |
| `0.51.1` | Runtime fix for the host's gap #101: a guest trap no longer panics the calling tokio task, and a trapped component answers its next call. `south-provider-runtime` registers its own non-blocking `wasi:io` streams and poll functions after `add_to_linker_sync` (the synchronous WASI shims run `Handle::block_on` inside any tokio runtime, and a guest reaches them when it flushes its panic or allocation-failure message to stderr), and replaces the shared instance of a component at the start of the call that follows any guest error (wasmtime never re-enters a trapped instance). A guest that sleeps on a clock is now a trap at once instead of a blocked host thread. No package, contract, `south_runtime`, public API or guest-linked crate changes: Q47 does not apply, all eighteen packages keep their version and bytes. Not breaking for hosts on re-pin. | [release](docs/design/2026-10-09-release-0.51.1.md) |
| `0.52.0` | The image world, first batch: `contracts.media` v1 (the elider, the multipart parser and encoder, the closed transform vocabulary of both media worlds, the media request descriptor and its template expansion, the request and response views, the artifact URL grammar and the forbidden egress ranges, with golden vectors), `contracts.image` v1, HTTP contract 12 (`execute_multipart_binary_call_v1`, `TextPostRequestV1` and `execute_text_binary_call_v1`), the `image-adapter-v1` world with no host import, gate ② `south.image-component.v1` (five new rows), gate ③ `south.provider-media-binary.v1` and `south.safe-fetch.v1`, and the package `image-azure` (1.0.0, `azure-mai`, generation and edit), which declares `south_runtime` `0.52.0` and is `not_verified` for both hosts. `south-contracts`, `south-provider-api` and `south-component-conformance` move to 0.51.0, so every other package takes a patch bump with unchanged behavior and keeps its `south_runtime`. `compatibility.json` `schema_version` 7. **Breaking for hosts on re-pin** (`RequestBodyRefV1::Text`, `ManifestErrorV1::ImageOperationRequired`, new public fields on `WorldSchemaV1`, five `CheckV1` variants). | [release](docs/design/2026-10-10-release-0.52.0.md), [image world record](docs/design/2026-09-30-image-world.md) |
| `0.53.0` | `decode_sse_v1`, early and alone (Q-B6-6): the new host-only crate `south-host-grammars` with the server-sent events decoder `decode_sse_v1` and its incremental form `SseDecoderV1`, whose `position` cuts a stream into whole frames for `north_passthrough`; golden vectors, property tests and the `contract_parsers` fuzz target; `compatibility.json` lists the crate as `sse_decoder_v1`. No component links the crate (checked by `shipped_packages_v1` and `scripts/check-boundaries.sh`), so no guest-linked crate, package, `component.wasm`, contract number or `south_runtime` changes. All nineteen packages keep their version and bytes. Not breaking for hosts on re-pin; a host takes the crate as a git dependency at tag `v0.53.0`. | [release](docs/design/2026-10-10-release-0.53.0.md), [boundary record](docs/design/2026-09-30-host-zero-vendor-boundary.md) §13.13 |

## Component packages

Every official component is built by its own script, and each ships as the exact
directory shape the runtime loads — `manifest.json` beside `component.wasm`.

| Component | Family | Build | Status |
|---|---|---|---|
| `task-kling-v2` | kling | `bash scripts/build-kling-task-v2-component.sh` | released |
| `task-minimax-v2` | minimax | `bash scripts/build-minimax-task-v2-component.sh` | released |
| `task-bailian-v2` | bailian | `bash scripts/build-bailian-task-v2-component.sh` | released |
| `task-xai-v2` | `xai-video` | `bash scripts/build-xai-task-v2-component.sh` | contract 6 candidate, unreleased |
| `task-byteplus-v2` | `byteplus-video` | `bash scripts/build-byteplus-task-v2-component.sh` | contract 6 candidate, unreleased |
| `task-veo-v2` | `veo-video` | `bash scripts/build-veo-task-v2-component.sh` | contract 6 candidate, unreleased |
| `task-wan-image-v2` | `wan-image` | `bash scripts/build-wan-image-task-v2-component.sh` | contract 6 candidate, unreleased |
| `task-gmi-image-v2` | `gmi-image` | `bash scripts/build-gmi-image-task-v2-component.sh` | contract 6 candidate, unreleased |
| `embeddings-openai-compatible` | `openai-compatible`, `azure-openai-v1` | `bash scripts/build-embeddings-openai-compatible-component.sh` | embeddings contract 1, from 0.47.0 |
| `embeddings-gemini` | `gemini` | `bash scripts/build-embeddings-gemini-component.sh` | embeddings contract 2 (inline media inputs), from 0.51.0; contract 1 text before |
| `embeddings-vertex` | `vertex-ai` | `bash scripts/build-embeddings-vertex-component.sh` | embeddings contract 1, from 0.48.0 |
| `image-azure` | `azure-mai` | `bash scripts/build-image-azure-component.sh` | media contract 1 and image contract 1, from 0.52.0 |
| `provider-anthropic-bedrock-invoke` | `anthropic-bedrock-invoke` | `bash scripts/build-anthropic-bedrock-invoke-component.sh` | Bedrock InvokeModel (Anthropic), `host_signed` with `aws-sigv4`; unreleased (B6-2) |
| `provider-openai-responses` | `openai-responses` | `bash scripts/build-openai-responses-component.sh` | OpenAI Responses API, `bearer`; OpenAI's own wire only until another upstream ships a captured-traffic pack (R-Q16); unreleased (Responses R1) |

Each pack's `README.md` records how its expectations were derived and how the
component differs from the host's native arm — read that before changing a
fixture. The shared facts the contract-6 families rely on, and the artifact roles
contract 7 added, have their own records:
[contract 6 facts](docs/design/2026-09-27-task-contract-v6-facts.md),
[contract 7 artifact roles](docs/design/2026-09-28-task-contract-v7-artifact-role.md).

Every release also attaches `south-release-index.json` (schema `south.release-index.v1`): each
package's world, families, capabilities, auth arms, compatibility declaration and the digests of
its archive, `manifest.json` and `component.wasm`, plus the digest of the gate ② report
(`<package>-<tag>.gate2.json`) CI produced for that exact `component.wasm`, and under `catalogs`
the model catalog (`model-catalog-<tag>.json`, schema `south.model-catalog.v1`: image and video
capabilities by upstream model id, read by `south_provider_runtime::ModelCatalogV1`) with its digest.
`scripts/release_index.py` generates it from the archived manifests, and the release fails when a
package keeps its version but its `component.wasm` or `manifest.json` digest differs from the
previous release's index. A package's `south_runtime` is the oldest runtime it needs, not the
release that carries it, and `scripts/check-declared-runtime.sh` loads each archived package under
exactly that runtime before the release publishes (see CONTRIBUTING). The three crates every
component links (`south-contracts`, `south-provider-api`, `south-component-conformance`) carry
versions of their own rather than the workspace version, so a release that leaves them unchanged
re-identifies no package. See
[released component artifacts](docs/design/2026-09-10-released-component-artifacts.md) §8 and the
[boundary record](docs/design/2026-09-30-host-zero-vendor-boundary.md) §13.6 and §13.12.


## Local verification

```bash
rustup target add wasm32-wasip2  # once; guest components build as child cargo invocations
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
cargo test --workspace --doc --all-features
cargo test --workspace --no-default-features
cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
rustup run 1.96.0 cargo check --workspace --all-targets
scripts/check-boundaries.sh --self-test
scripts/check-boundaries.sh
python3 -m unittest discover -s scripts -p 'test_*.py'  # release index and digest-stability check
scripts/check-language.sh --self-test
scripts/check-language.sh
cargo deny check
cargo deny --manifest-path fuzz/Cargo.toml --config deny.toml --locked check
cargo audit
cargo audit --file fuzz/Cargo.lock
cargo machete
(cd fuzz && cargo machete)
```

All source code, documentation, diagnostics, logs, and commit messages in this repository are
written in English, and `scripts/check-language.sh` enforces it rather than leaving it to
recollection. `scripts/language-baseline.txt` records the CJK still left in the design records so
the gate blocks growth today instead of waiting for the backlog; translate, then run
`scripts/check-language.sh --regen`, which only ever lowers a count.
