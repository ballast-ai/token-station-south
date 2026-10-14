# Image world step I3-3: `image-vertex` (Nano Banana on Vertex AI)

Status: accepted (lv reviewed the record on 2026-10-14, including the §8 decisions; VI-Q1 and VI-Q2 ruled on 2026-10-13
as recommended: the host re-reads a minted slot from round 2 on, and answers 400 for an operation the package does not
declare); implemented on branch `image-vertex` (see Implementation), not yet merged.

Date: 2026-10-13

Baseline: south `origin/main` = `2a95713` (release 0.59.0: I3-2, `image-gemini`, `contracts.media` 2). Host references
carry a `server:` prefix and refer to token-station-server `origin/dev-v2` = `e77eec98` (which contains the host's
multi-round dispatch, `f0e0c6ab`). `server:…/` abbreviates `server:gateway/src/modules/inference/`. Statements marked
**unverified** are conclusions from reading code or from what Google documents, not from running anything or
capturing traffic.

Parent record: [`2026-09-30-image-world.md`](2026-09-30-image-world.md) ("the record"): §13 (the Vertex row), §14
(I3-3), §18.3 (the value channel and the media worlds), §18.4 (credential recipes and Vertex), §18.6 (world
properties). Sibling records: [`2026-10-13-image-i3-2-gemini.md`](2026-10-13-image-i3-2-gemini.md) ("I3-2"), whose §7
plans this step and whose shared module `gemini_image` this package reuses, and
[`2026-10-12-image-i3-1-minimax-and-qwen.md`](2026-10-12-image-i3-1-minimax-and-qwen.md) ("I3-1"). Boundary record:
[`2026-09-30-host-zero-vendor-boundary.md`](2026-09-30-host-zero-vendor-boundary.md) §3 (credential recipes) and §13.8
(the value channel, exported attributes). Embeddings record §16 and the `embeddings-vertex` package are the Vertex
precedent for URL, location, project and recipe.

**Scope (lv, 2026-10-13 pre-approval).** I3-3 lets an image package declare a credential (recipe) section, reusing the
`vertex_sa` recipe of `embeddings-vertex`, and ships a Vertex image package. A difference from the native arm that
follows an established precedent (I3-1, I3-2, P22, P24 `embeddings-vertex`) is accepted without a new question; those
are listed in §7. Only new decisions are in §10.

## 1. What I3-3 delivers

| Item | Where | Change |
|---|---|---|
| `image-vertex` 1.0.0 | `components/image-vertex/`, `reference_vertex_image.rs`, `fixtures-image-vertex/` | Family `vertex-ai-image` (proposed, §8 decision 1); `generate` only (§3) |
| Shared Vertex location helpers | `crates/south-component-conformance/src/vertex_location.rs` (crate-private, new) | `origin_of`, `base_of`, the `declared` reader and the project rule, moved out of `reference_vertex_embeddings.rs` (behaviour-neutral for `embeddings-vertex`) |
| Shared Gemini image module | `gemini_image.rs` | Unchanged in behaviour; reused with `edit: false` (I3-2 §7) |
| Gate ① | `south-provider-api` | **No change**: the widening already shipped in 0.52.0 (§2) |
| Q47 | `south-component-conformance` minor | Every package takes a patch (§9) |

Image world (`image-adapter-v1`), contracts `{image: 1, media: 2}` (the shared module writes `data_url_payload` for a
reference image, GI-Q1), auth arm `bearer` on a `minted` slot, metering forms `tokens` and `images`, `repeat`, tier
dimension `resolution`.

**Models and endpoints in scope.** Only the Nano Banana models through Vertex `:generateContent`
(`gemini-3-pro-image`, `gemini-3.1-flash-image`, `gemini-3.1-flash-lite-image`; the catalog has an entry for each,
`catalogs/model-catalog.json:1364`, `:1408`, `:1456`; whether the lite id is offered on Vertex is **unverified**).
Imagen `:predict` is **out of scope**: Google shut Imagen down on Vertex on 2026-06-30, and the host deleted its
`:predict` arm on 2026-09-14 (`server:…/handler/images/gemini.rs:1-3`, `handlers.rs:343-349`). There is no native arm to
reproduce and no upstream to call, so this is not an open question.

## 2. Gate ①: the widening lv pre-approved has already shipped

The record's §18.3 table says gate ① refuses `config_schema` and exported credential attributes outside the provider
and embeddings worlds, and §18.4's row says "what remains for Vertex images is the gate ① widening of §18.3". Both were
written against 0.50.0 and are **stale**: S-I-3 (0.52.0, #173) made the widening a world property, and the image world
has had it since its first release.

- `IMAGE_WORLD_SCHEMA` sets `value_channel: true` (`crates/south-provider-api/src/manifest.rs:412-424`; the field,
  `:130-135`). Gate ① then admits a family's `config_schema` (`manifest.rs:1061-1063`, refused only without the
  property) and holds its keys to the provider world's key rules (`:1114-1116`).
- Exported credential attributes are refused only in a world without the property
  (`crates/south-provider-api/src/values.rs:92-100`, comment names provider, embeddings and image).
- The `credentials` section itself (fields, import, slots, recipes) is validated in every world
  (`manifest.rs:919-921`); no world check applies to it. Task-v2 packages already declare recipes.
- A test admits exactly the Vertex shape in the image world: `image_with_values` is `embeddings_with_values` (a
  `config_schema` with `region`, a `credentials` section with a `minted` slot, an RS256 `jwt_sign` step, an
  `oauth2_token` step and an exported `project_id`) moved to `image-adapter-v1`, and
  `the_image_world_admits_config_keys_and_exported_attributes` passes gate ① with it
  (`crates/south-provider-api/tests/component_values_v1.rs:397-433`).
- The descriptor side: `admit_media_descriptor_auth` presents a `bearer` descriptor on a slot a recipe mints as Bearer
  (`crates/south-component-conformance/src/descriptor_auth.rs:200-203`, `:281-285`; test
  `a_media_bearer_on_a_minted_slot_is_admitted_without_the_bearer_arm`, `tests/descriptor_auth_v1.rs:575`).
- The suite: `south.image-component.v1` runs `credential_recipe_checks_v1` for any package with a credentials section
  (`image_suite.rs:329-331`) and `UndeclaredValuesIgnored` on every `prepare` case (`image_suite.rs:302`, `:422-450`).

**Consequences.** No `south-provider-api` change, no contract or world version change, nothing breaking for a host:
`south-provider-api` stays 0.53.0, and a host pinned to 0.52.0 or later already admits an image package with a recipe
at gate ① (its own image executor is a separate matter, §6). lv's pre-approval is satisfied without code. This record
proposes to correct §18.3 and §18.4 of the record in the implementation change (a revision note there, and the §18.4
"what remains" cell reading "nothing on the south side; met in 0.52.0").

## 3. Generation (`/v1/images/generations` → Vertex `:generateContent`)

Everything except the route is I3-2 §2 unchanged, because the native arm is one implementation for both families
(`server:…/handler/images/gemini.rs:10-22`; body builder shared, `:983-986` notes that `role: "user"` is required by
Vertex and is sent for both):

- **Request**: `POST {base_url}/v1/projects/{project}/locations/{region}/publishers/google/models/{model}:generateContent`
  (relative path in the media descriptor; each of `project`, `region` and `model` one percent-encoded segment, as
  `embeddings-vertex`, `reference_vertex_embeddings.rs:201-207`). Body, `image_url` → `inlineData` with
  `data_url_payload`, `aspect_ratio` / `resolution` → `imageConfig`, `n` → `repeat`, refusals before dispatch,
  immutable body paths and facts: I3-2 §2 and the shared module (`gemini_image.rs:276-291` and below).
- **Auth**: `{"arm": "bearer", "slot": <configured slot>}`. The host's recipe executor mints the value; the native arm
  presents the same thing (`Authorization: Bearer <access token>`, `server:…/engine/upstream.rs:312-314`, minted by
  `get_valid_vertex_token_for`, `server:…/engine/provider_pool.rs:549-558`). No other header (the native arm sends
  none, notably no `x-goog-user-project`).
- **Edit**: not served. The native entry refuses `/v1/images/edits` for Vertex (`server:…/handler/images/handlers.rs:
  840-846`), so the package passes `edit: false` to the shared module, declares capability `generate` only and
  `model-capabilities` lists operation `generate` and input role `image_url` only (`gemini_image.rs:81-118`). See VI-Q2
  for the status a client gets.

## 4. Location, project, base URL and credentials

Copied from `embeddings-vertex` (manifest `components/embeddings-vertex/manifest.json`; reference
`reference_vertex_embeddings.rs:56-66`, `:124-160`, `:196-200`), as the record §18.3 / §18.4 prescribe:

- **`config_schema`** for the family: `region` (`aws_region` syntax, required, "The Vertex AI location, such as
  us-central1, or global.") and `project` (`gcp_project_id` syntax, optional override). Byte-identical keys, syntaxes
  and requiredness to `embeddings-vertex`, so that one provider row may route models to both packages (the host's R20
  requires every routed package to accept the row's `config_values` whole; P24 cutover manual §3).
- **Project**: the `project` config key when the operator set it, else the `project_id` the credential exports.
  Native order is the same: `providers.group_id` override first, else the credential's `projectId` extra
  (`server:…/engine/vertex.rs:199-232`). Cutover therefore writes `group_id` (when set) to `config_values.project`.
- **Region**: native reads the `providers.region` column and refuses a row without one (`vertex.rs:43-60`); the
  package reads `declared.region`. Cutover writes `region` to `config_values.region`.
- **Base URL**: native ignores the row's `endpoint` ("ignored" on Vertex rows) and derives the host from the region
  (`vertex.rs:35-37`, `:64-87`: `{region}-aiplatform.googleapis.com`, `aiplatform.googleapis.com` for `global`). The
  host's image binding has one rule, `row_endpoint` (`server:gateway/src/infra/config/sections.rs:1310-1315`;
  `server:…/handler/images/component/mod.rs:1305-1309`), so the row's endpoint must be the region's origin, and the
  package refuses a Vertex origin of another region (`base_of`, `reference_vertex_embeddings.rs:139-153`), exactly as
  `embeddings-vertex` (P24 manual, family table row `vertex-ai`). A non-Vertex base (proxy, test server) is used as
  given.
- **`credentials`**: byte-for-byte the `embeddings-vertex` section: fields `service_account` (secret, JSON) and
  `project_id` (non-secret), the `service-account-json` import, slot `provider_api_key` minted by `vertex_sa`
  (RS256 `jwt_sign` with scope `https://www.googleapis.com/auth/cloud-platform`, then `oauth2_token` to
  `https://oauth2.googleapis.com/token`, `refresh_margin_seconds` 300, `min_ttl_seconds` 301), attribute `project_id`
  exported. The scope covers `generateContent` (record §18.4). An identical section means one credential's
  `recipe_fields` serve both packages, and the two packages' minted tokens are interchangeable (same account, same
  scope; the host stores them in the shared `oauth_token` / `expires_at` columns, P24 manual §6).
- **Missing values**: a missing `region`, or neither `project` nor an exported `project_id`, is a 500 naming the key
  (`embeddings-vertex`'s `declared`, `reference_vertex_embeddings.rs:156-160`). The reference builds the route before
  calling the shared `prepare`, so a configuration error is reported before request refusals.

## 5. Response, metering and outcomes

I3-2 §4 unchanged (the shared `parse_response` and `render`): the elision path, Google's error envelope and the
`image-azure` status table, `promptFeedback.blockReason` → `rejected`, policy finish reasons → `unknown`, `thought` parts
skipped, `tokens` from `usageMetadata` with `total_input` / `total_output` declared and the modality split only when it
closes, `images_reported` null, the render over all rounds. The native arm reads the same fields for both families
(`gemini.rs:10-22`, "billing three segments zero divergence"). Two Vertex-only points, neither a decision:

- Vertex returns the same `google.rpc` error envelope as the Gemini API (**unverified** against a capture; the native
  arm parses both the same way).
- Vertex adds `usageMetadata.trafficType` (`ON_DEMAND` / `PROVISIONED_THROUGHPUT`, **unverified** that image models
  report it). Neither the native arm nor the package reads it, so a provisioned-throughput call is billed by tokens as
  native does.

Pricing follows I3-2: token rows bill `usageMetadata`; per-image rows wait for H5 and use the GI-Q3 tier rule
unchanged.

## 6. What the host must do

What dev-v2 already has (read, not run):

- **Recipe execution for image slots**: the image executor's seal takes the slot value through the shared
  `component_credentials::slot_value`, which resolves a `minted` slot with the recipe executor
  (`server:…/handler/images/component/mod.rs:1772-1790`; `server:…/engine/component_credentials.rs:47-97`), computes
  exported attributes (`component/mod.rs:1425-1432`; `component_credentials.rs:104-120`) and builds `declared` from the
  row's `config_values` and those attributes (`component/mod.rs:1433-1445`). Image recipe packages are listed for
  endpoint confirmation (`server:gateway/src/modules/admin_ui/handler/credential_recipes.rs:24-28`).
- **D9 BYOK guard**: a BYOK credential on a family whose package declares a credentials section is refused with 400
  before admission (`component/mod.rs:1398-1410`; `component_credentials.rs:33-41`).
- **Multi-round dispatch** (`repeat`, GI-Q2): `dispatch_rounds` on both paths (`component/mod.rs:725-782`).
- **R20 with image routes**: a row's `config_values` are checked against the packages its models route to, image
  routes included since I3-1 (`server:gateway/src/modules/catalog/routed_config_values.rs:26`).

What it still needs before a Vertex image row is cut over:

1. **Re-pin** to the release that ships `image-vertex` and add `media` 2 to the host range (the same item as I3-2 §5
   item 2; dev-v2 is still on `v0.58.0`, `server:Cargo.toml:117-134`, and asserts its media range equals
   `{MEDIA_CONTRACT_VERSION}`, `server:…/engine/south_component.rs:2174-2176`).
2. **Recipe fields on a native `vertex_ai` row.** The admin path that appends a package's credential fields to a
   native row's credential (P24 B1) only looks at the row's **embeddings** routes (`server:gateway/src/modules/admin_ui/
   handler/credentials/declared.rs:9-13`, `:298-312`, `provider_embeddings_route_bindings`). A row whose models route
   only to `image-vertex` cannot get `recipe_fields`, and `slot_value` then refuses with 400. The host should take
   image routes into the same rule (one declaration per row across both worlds, which the byte-identical section
   satisfies). Until then a row that also routes a model to `embeddings-vertex` works, because the stored fields are
   the same fields (**unverified** end to end).
3. **Endpoint confirmation** (R23) for the `image-vertex` digest: the confirmation is per package digest; a confirmed
   `embeddings-vertex` digest does not carry over to a package of another name (P24 manual §2, step 3).
4. **Row preparation**: `endpoint` set to the region's origin; `config_values.region` = `providers.region`;
   `config_values.project` = `providers.group_id` when set (§4).
5. **Per-round minting** (VI-Q1).
6. **Preflight and evidence**: the P22 image cutover preflight's family table has no Vertex row
   (`server:scripts/p22_image_cutover_preflight.py:124-131`); it needs one mapping `region` and `group_id` to the two
   config keys and checking the recipe fields and the endpoint confirmation, as the P24 preflight does for embeddings.
   The C10 real-key evidence rule (lv 2026-10-10) applies to this family as to the others.
7. **Binding**: family `vertex-ai-image`, `base_url = "row_endpoint"`, the `image-vertex` pin.

## 7. Differences that follow precedent (accepted under lv's 2026-10-13 pre-approval)

Every I3-2 §8 difference (P1 to P11) applies unchanged, because the request, response and pricing pieces are the same
module: P29 allow-list on non-2xx, 502 while funds are parked, declared buckets required, malformed counts `unknown`,
generic tier text, the tier word only when the client sent one, the host bound formula, the non-2xx outcome table, a
2xx `rejected` parked, no `size`-based tier, JSON above 32 MiB refused before admission. Vertex-specific ones:

- **V1 BYOK refused** (D9, lv 2026-10-09, embeddings precedent; P24 manual §4 row "BYOK"): native serves a BYOK Vertex
  credential through its fixed columns; the component path answers 400 before admission.
- **V2 Row endpoint must be the region's origin**, and a Vertex origin of another region is refused before admission;
  native ignores the endpoint (`embeddings-vertex`, P24 manual family table).
- **V3 Missing region or project**: native 400 (after admission, refunded, `vertex.rs:43-60`, `:214-226`); component 500
  naming the key, before admission (`embeddings-vertex` decision, its reference header; P24 manual §4 "row lacks
  region").
- **V4 Path segments percent-encoded**: identical for every `[A-Za-z0-9._-]` value; native interpolates them raw
  (`embeddings-vertex`).
- **V5 Retired ids** are not refused by the package (no clock); a routed retired id reaches Vertex and its 404 is
  released under P29 (I3-2 §5 item 5). Operators do not route retired ids.
- **V6 Recipe executor instead of the native token cache**: the token comes from the package's `vertex_sa` recipe, not
  from `get_valid_vertex_token_for`; both mint for the same account and scope and share the stored token (P24 B1/B2,
  embeddings precedent).
- **V7 Non-image ids** (LLMs, Veo, Imagen) on a routed Vertex model: native 400 after admission (`handlers.rs:
  378-391`); the component path refuses a model the package does not declare with 503 before admission, and the host
  lets only image-modality models be routed (the P22 preflight's `ROUTABLE_MODALITY`), so the case needs a misrouted
  row (P22 generic routing rule).

## 8. Fixtures, tests, wiring; decisions where the record is silent

As I3-2 §9, smaller because the shared module is already covered by `fixtures-image-gemini/`: a gate ② pack with
`capabilities` (models; wrong family), `prepare` (a plain generation and its exact URL and auth; `global` location;
a regional location; a proxy base; a Vertex base of another region refused; `project` override; exported
`project_id`; missing `region`; missing project; `image_url` data URL; `n` 10 → `repeat`; a multipart request refused,
since a host that skips the operation check must still get a refusal), `response` (one success with usage, one
non-2xx 401 and one 429, one blocked prompt, one missing declared bucket) and `render` (two rounds); the five
`credential.*` cases copied from `fixtures-embeddings-vertex/` (three clock cases, two on-status cases) with the family
renamed; suite test `vertex_image_suite`, sandbox parity `vertex_image_sandbox_parity`, the build script, the `ci.yml` /
`release.yml` enumerations, `shipped_packages_v1`, README and ARCHITECTURE. Fixture sources: the host's Vertex image
tests (`server:gateway/tests/images_providers.rs`, `images_sync_contract.rs`, `media_arms.rs`,
`byok_media_ownership.rs`, `model_request_extras_media.rs`, and `server:…/handler/images/tests/gemini.rs`), transcribed,
not back-derived.

Proposed decisions (not questions unless lv objects):

1. **Family name `vertex-ai-image`**, distinct from `embeddings-vertex`'s `vertex-ai` (each world's bindings name their
   own families, as `gemini-image` beside `gemini`), and naming the platform the way the embeddings family does.
2. **Package name `image-vertex`**, auth arms `["bearer"]`, capabilities `["generate"]`, permissions secrets
   `["provider_api_key"]`.
3. **`vertex_location.rs`**: the location helpers move out of `reference_vertex_embeddings.rs` so the two Vertex
   references cannot drift; `embeddings-vertex`'s behaviour and fixtures are unchanged (its package takes the Q47 patch
   anyway).
4. **Refusal texts**: the shared module's (the native arm's "Gemini Nano Banana …" texts, which the native arm also
   uses for Vertex), `embeddings-vertex`'s for configuration errors.
5. **No edit through the shared module**, though `:generateContent` on Vertex would accept the same edit body
   (**unverified**): adding a Vertex edit is a new surface with no native arm to reconcile against; it can be a later
   minor of the package with its own capture.

## 9. Versioning (Q47)

`south-component-conformance` 0.57.0 → 0.58.0 (the new reference, `vertex_location.rs`, suite rows); every component
lockfile and `fuzz/Cargo.lock` follow; every published package takes a patch and keeps its `contracts` and
`south_runtime`; `image-vertex` 1.0.0 declares `{image: 1, media: 2}` and declares the tree's release as its
`south_runtime` until the release commit moves both (I3-2 Implementation, departure 2). `south-contracts` (0.53.0) and
`south-provider-api` (0.53.0) are unchanged; no `compatibility.json` contract changes. Exact numbers are fixed at
implementation.

## 10. Open questions for lv

- **VI-Q1 A minted token across rounds.** The host seals one slot value and sends it on every round
  (`component/mod.rs:557-588` uses `sealed.secret`; `dispatch_rounds`, `:725-782`). A token is resolved with at least
  about 300 s left (`refresh_margin_seconds` 300, `min_ttl_seconds` 301; the exact guarantee of the host's cache is
  **unverified**), while a call may run up to ten rounds of up to 300 s each (`[server]
  upstream_non_streaming_timeout_secs`, default 300, `server:gateway/src/infra/config/mod.rs:373`). A token that expires
  mid-call makes a later round 401, which the host treats as a failure after served rounds: `unknown`, funds parked, 502
  (record §7). The native arm re-mints before every round after the first precisely for this
  (`server:…/handler/images/gemini.rs:149-155`, `:328`, `:355-372`). Options: (a) the host resolves a `minted` slot again
  before each round after the first (generic, keyed on the manifest's slot kind, not on Vertex; the cache makes it free
  while the token is fresh; a minting failure at round 2 or later is a post-dispatch failure and parks); (b) accept the
  risk and register it; (c) cap `n` on recipe rows. **Recommended: (a)**, as a cutover precondition for Vertex rows that
  serve `n > 1` (rows whose clients never send `n` may cut over before it, as GI-Q2 allowed).
- **VI-Q2 `/v1/images/edits` on a routed Vertex model.** Native answers 400 with a pointer to generations, after
  admission with a refund (`handlers.rs:840-846`, `:1197-1199`). The executor refuses an operation the package does not
  declare with 503 before admission (`component/mod.rs:1468-1472`), before `prepare` runs, so the package's own 400
  (`gemini_image.rs:286-288`) is never reached on that host. Funds are zero on both sides; the status class changes
  from client error to service error, which retrying clients treat differently. `image-qwen` already meets the same
  path for its edits (it declares no edit operation), without a recorded ruling. Options: (a) accept 503 and register
  it; (b) the host answers 400 for an operation the package does not declare (a generic change, all image families);
  (c) the package declares `edit` and refuses in `prepare` with 400 (a capability claim that is false, which the host
  also uses for discovery). **Recommended: (b)**, a one-line host change that keeps the native status for Vertex and
  fixes Qwen at the same time; (a) if lv prefers not to touch the executor now.

## Implementation (2026-10-14)

What landed, on branch `image-vertex` (rebased on `origin/main` = `bcfcd45`, after task contract 8, #205; the latest release is 0.61.0):

- **`image-vertex` 1.0.0**: manifest (family `vertex-ai-image`, `image-adapter-v1`, capabilities `["generate"]`,
  `auth_arms` `["bearer"]`, `permissions.secrets` `["provider_api_key"]`, `contracts` `{image: 1, media: 2}`; the
  `config_schema` entry and the `credentials` section copied byte for byte from `embeddings-vertex`, only the family key
  differs), the guest crate (a thin shell like `image-gemini`), the reference `reference_vertex_image.rs` (the route and
  the auth; everything else is `gemini_image` with `edit: false`), the fixture pack `fixtures-image-vertex/` (20 image
  cases and the five copied `credential.*` cases, with a README), the suite test `vertex_image_suite` (the pack plus
  seven named tests, two of them mutations), the sandbox parity test `vertex_image_sandbox_parity`, the build script and
  the enumerations (`ci.yml` rust-cache, `release.yml` build / gate ② / package lists, `shipped_packages_v1`: 28 official
  components, the media 2 branch of the range-handshake test and a test of its own), README and ARCHITECTURE.
- **`vertex_location.rs`** (crate-private): the region and project readers, `base_of` and `origin_of`, and
  `publisher_model_path`, which both Vertex references call (`…:predict` and `…:generateContent`). The read order
  (region, project, then the base URL) is the one `embeddings-vertex` had; its pack passes unchanged and none of its
  fixtures moved.

Failing first: with the reference written and only three expectations written by hand (`prepare.generate`,
`response.usage`, `response.prompt-blocked`), the pack test failed on the 17 cases still without an expectation and passed
the three hand-written ones and the five credential cases. The other expectations were then taken from the reference and
reviewed row by row against §3 to §5. Mutations: the shared module called with `edit: true` turns
`edit_is_not_served` and the pack test red (the `capabilities.models` and `prepare.multipart-refused` rows); calling the
shared `prepare` before building the route turns `a_configuration_error_is_a_500_before_any_request_refusal` red; a
component that drops the `project` override fails the `prepare.project-override` row; one that accepts any Vertex AI base
URL fails the `region-mismatch` and `global-prefixed-host` configuration rows.

Versions (§9 is stale on numbers): since the 0.61.0 release `south-component-conformance` has already moved twice for
unreleased work, 0.59.0 → 0.60.0 for the Kiro step (#204) and 0.60.0 → 0.61.0 for task contract 8 (#205), and every
published package already took its patch there. The new reference and `vertex_location.rs` change the crate again, but no
release has carried 0.61.0, so it is not bumped again and no other package is re-patched; only `image-vertex` (1.0.0,
with its own lockfile) is added. `south-contracts` (0.54.0) and `south-provider-api` (0.55.0) are as #205 left them; no
`compatibility.json` change. `image-vertex` declares this tree's release, 0.61.0, as its `south_runtime` (§9, the I3-2
precedent); `scripts/check-declared-runtime.sh --build` judges it with this tree, and the release commit moves the
declaration with the workspace version.

Departures and interpretations:

1. **The configuration refusals are not in the gate ② pack.** §8 lists `prepare` rows for a Vertex base of another
   region, a missing `region` and a missing project, and §4 / V3 make each a 500 naming the value, as `embeddings-vertex`
   answers. The image suite's `pre_dispatch_refusal` row (image record §12.1) fails any `prepare` refusal that is not
   `invalid_request` or `capability`, so those three rows cannot be pack cases without either changing their status or
   widening a gate ② row every image package is judged by. Neither is this step's to decide: the behaviour stays as §4
   says, and the rows (with the prefixed `global` host as a fourth) are pinned by name in `vertex_image_suite`
   (`CONFIGURATION_REFUSALS`, including the order before a request refusal) and run through the guest in
   `vertex_image_sandbox_parity`. If lv prefers them in the pack, the choices are `capability` 400 (diverging from
   `embeddings-vertex`) or a suite row that admits `internal` for a configuration error.
2. **Two rows the suite requires beyond §8's list.** The suite needs a succeeding response under each declared metering
   form and a `missing` row for each declared token bucket, so the pack adds `prepare.images-form` / `response.images-form`
   (a per-image call with `resolution` `2k`, the `images` sample) and `response.missing-prompt-tokens` beside §8's one
   missing bucket (`response.missing-candidates-tokens`).
3. **The render's two rounds are the same answer twice.** The suite requires a render case's rounds to come from one
   prepare case, and §8's only tokens-form success is `response.usage`, so `render.two-rounds` names it twice (a call
   with `n` 2 whose two rounds answer alike).
4. **"With the family renamed"** (§8) had nothing to rename: the five `credential.*` cases name no family, so they are
   byte-identical copies.
5. **The plain generation and `exported-project` rows.** The plain generation already reads the exported `project_id`
   (`test-project`, from the host's test); `prepare.exported-project` uses the other id the host's tests use (`proj-a`)
   so the row shows that value reaching the path; `prepare.project-override` sends both and the override wins.
6. **The multipart refusal text** is the shared module's ("this family does not edit images", 400), per decision 4; the
   native arm's own text ("not supported for Vertex AI models") is not reproduced. On a host with VI-Q2 the executor
   answers before `prepare` runs, so the text is only seen by a host that skips the operation check.

## Corrections to the parent record (to land with the implementation)

- §18.3, first row, "Fact" column: gate ① no longer refuses `config_schema` and credential attributes in the image
  world; since 0.52.0 they are governed by `WorldSchemaV1::value_channel`, which the image world sets (§2 here).
- §18.4, first row, "Effect": "What remains for Vertex images is the gate ① widening of §18.3" reads "nothing on the
  south side; the widening shipped in 0.52.0 (S-I-3)". The host-side items are §6 here.
- §18.4, second row: `fixtures-embeddings-vertex/` holds five `credential.*` cases (ten files), not four files.

## Revision note

- 2026-10-13: first draft (proposed).
- 2026-10-13: lv ruled VI-Q1 (the host re-reads a minted slot from round 2 on; a precondition for `n > 1` Vertex rows) and
  VI-Q2 (option (b): the host answers 400 for an operation the package does not declare) as recommended; the rest of
  the record awaits lv's review.
- 2026-10-14: lv reviewed and accepted the record, including the §8 decisions; status accepted. Implemented on branch
  `image-vertex` (Implementation section, six departures or interpretations); the corrections to the parent record are
  applied there.
