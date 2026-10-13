# Image world step I3-2: `image-gemini` (Gemini API, token pricing first)

Status: accepted (lv, 2026-10-13: GI-Q1 to GI-Q7 ruled as recommended, §11). Implemented on branch `image-i3-2` (see
Implementation); not released.

Date: 2026-10-13

Baseline: south `origin/main` = `c176494` (I3-1 merged, #190; the latest release is still 0.57.0 at `116cf13`). Host
references carry a `server:` prefix and refer to token-station-server `origin/dev-v2` = `06da99c4` unless marked
**H5**, which refers to the unmerged branch `h5-tier-price-table` at `37fa0ca3` (the tier price table, H5-1 / H5-2).
`server:…/` abbreviates `server:gateway/src/modules/inference/`. Statements marked **unverified** are conclusions from
reading code or from what Google documents, not from running anything or capturing traffic.

Parent record: [`2026-09-30-image-world.md`](2026-09-30-image-world.md) ("the record"): §7 (`repeat`), §8 (tier words,
the tier refusal rule), §9.1 (declared buckets), §9.3 (pricing form), §13 (the Gemini row), §14 (I3-2), Q8 / R-1 / R-2 /
Q16 (rulings), §18.4 (Vertex recipe, for I3-3). Sibling record:
[`2026-10-12-image-i3-1-minimax-and-qwen.md`](2026-10-12-image-i3-1-minimax-and-qwen.md) ("I3-1"), whose rulings
this record treats as precedent.

**Scope (lv, 2026-10-13 pre-approval).** I3-2 delivers Gemini **token** pricing and its cutover first. Per-image rows
("Nano Banana tier" prices) wait for the host's H5 tier price table (model-row JSON
`tier_prices.image = {defaults: {resolution: "1K"}, rows: [{tier: {resolution: "1K"}, output_each, input_each?}]}`,
tier words in R-2 exact case). The package is designed so that its token metering is complete now and its tier fact
(`resolution`) lets H5 per-image rows be used later **without a package change** (§6, answer: achievable, with GI-Q3).
Vertex is I3-3 and is not in this package; §7 lists what makes I3-3 a thin second package. Only the Gemini API family
(`generativelanguage.googleapis.com`, `x-goog-api-key`) is designed here.

lv also pre-approved (2026-10-13) that a difference following an established precedent (I3-1 or P22) is accepted
without a new question; those are listed in §8 as "follows precedent, accepted", not in §11.

## 1. What I3-2 delivers

| Item | Where | Change |
|---|---|---|
| `image-gemini` 1.0.0 | `components/image-gemini/`, `reference_gemini_image.rs`, `fixtures-image-gemini/` | Family `gemini-image` (proposed); `generate` and `edit` (§2, §3) |
| Shared Gemini image module | `crates/south-component-conformance/src/gemini_image.rs` (crate-private) | Body builder, response reader, usage reader, render; reused by I3-3 (§7) |
| One transform word (GI-Q1) | `south-contracts::media` | `contracts.media` 1 → 2: `data_url_payload` (a base64 data URL to its base64 payload, as text) |
| Q47 | `south-contracts` and `south-component-conformance` minors | Every package takes a new version (§10) |

Image world (`image-adapter-v1`), contracts `{image: 1, media: 2}` (`media: 1` if GI-Q1 is answered the other way),
`header_secret` `x-goog-api-key` on `provider_api_key`, metering forms `tokens` and `images`, `repeat` (one image per
upstream call), tier dimension `resolution`.

## 2. Generation (`/v1/images/generations` → `:generateContent`)

Reproduces the host's native arm (`server:…/handler/images/gemini.rs:54-481`, body builder `:937-1021`):

- **Request**: `POST {base_url}/v1beta/models/{upstream_model}:generateContent` (`gemini.rs:227-231`), JSON body
  `{"contents": [{"role": "user", "parts": [<inlineData>?, {"text": prompt}]}], "generationConfig":
  {"responseModalities": ["IMAGE"], "imageConfig": {…}?}}` (`gemini.rs:983-990`, `:1015-1021`). `role` is always
  sent (the native arm sends it for both families, `:983-986`). The image part precedes the text part (`:953-970`).
- **`image_url`** (reference image): absent or `null` → none; a string → one `inlineData` part
  `{"mimeType": <the data URL's media type>, "data": <its base64 payload>}`; any other type → 400 with the native text
  (`gemini.rs:953-969`). Only a base64 data URL is accepted; an `http(s)` URL, a missing `;base64,` or an empty media
  type are refused before dispatch (`gemini.rs:1273-1299`). The package declares the request elision path `/image_url`,
  so the value is always a blob: the component reads the prefix from the placeholder's 64-byte `head`
  (`crates/south-contracts/src/media/mod.rs:89`; record §6.2) and writes
  `{"$south.ref": {"blob": …, "transform": "data_url_payload"}}` for `data` — which needs GI-Q1. Base64 syntax errors
  in the payload are then refused by the host's template expansion before admission (`server:…/handler/images/
  component/mod.rs:1096-1115`, **unverified** that every native syntax refusal of `gemini.rs:1329-1348` is matched by
  the canonical check of `crates/south-contracts/src/media/transform.rs:115-116`).
- **`aspect_ratio` / `resolution`**: each a string or absent (any other type → 400, `gemini.rs:972-981`); non-empty after
  trimming → `imageConfig.aspectRatio` / `imageConfig.imageSize`, the resolution **upper-cased** as the native arm does
  because Google rejects `1k` (`gemini.rs:998-1013`). Every other request field (`size`, `quality`, `style`,
  `response_format`, `stream`, …) is dropped, as native (§9 decision 2 for `response_format`).
- **`n`**: 1..=10 (`gemini.rs:1259-1267`; the host entry checks the same range, `server:…/handler/images/handlers.rs:
  129-145`). Not sent upstream: the descriptor is sent `repeat = n` times (record §7; the native loop,
  `gemini.rs:329`). `requested_outputs = n` (one image per round × `repeat`). **Host prerequisite** (§5 item 1, GI-Q2).
- **Refusals before dispatch** (`prepare`, `invalid_request`): `prompt` missing or not a string (`gemini.rs:940-943`;
  an elided prompt uses `image-azure`'s "longer than the gateway accepts"); `n` outside 1..=10 or not whole; the
  `image_url`, `aspect_ratio` and `resolution` type and shape refusals above.
- **Immutable body paths**: `contents`, `generationConfig.imageConfig`, `generationConfig.candidateCount`,
  `generationConfig.responseModalities` (= the host's `GEMINI_IMAGE_OWNED`, `server:…/handler/images/execute.rs:54-59`).
- **Facts**: `operation` `generate`; `inputs.reference_image` = 1 when `image_url` is a string (the native precheck's
  role, `server:…/handler/images/precheck.rs:108-111`; the catalog limits `reference_image` to 1 for all three ids,
  `catalogs/model-catalog.json` entries `gemini-3-pro-image`, `gemini-3.1-flash-image`, `gemini-3.1-flash-lite-image`);
  `requested_outputs = n`; **no `size`** (Gemini has none; §8 P10); `tier.resolution` per §6; `metering_forms` = the
  host's `metering_required`; no `bounds`.

## 3. Edit (`/v1/images/edits`, multipart → `:generateContent`)

Reproduces `server:…/handler/images/gemini.rs:503-918` (`build_gemini_generatecontent_image_edit_body`, `:1364-1392`):

- **Request**: the same URL and a **JSON** descriptor built from the multipart view: `contents[0].parts =
  [{"inlineData": {"mimeType": <media type>, "data": {"$south.ref": {"blob": <the first "image" file part>,
  "transform": "base64"}}}}, {"text": prompt}]` and the same `generationConfig`. `base64` on a file part's bytes is the
  record's intended use (record §6.4). Only the first part named `image` is forwarded (the native arm reads one,
  `gemini.rs:535-547`, `:603-610`); `image[]` is not read.
- **Media type of the input image**: the native arm sniffs magic bytes and falls back to `image/png`
  (`gemini.rs:612-622`, `:1370-1373`). A component never sees file bytes (a file part carries only name, file name,
  media type and length, `crates/south-contracts/src/media/multipart.rs:79-92`), so the package uses the part's own
  media type when it starts with `image/` and `image/png` otherwise (**GI-Q6**).
- **Text fields**: `prompt` (trimmed, as native `:599-601`; missing → 400), `n` (absent or empty → 1; not a
  non-negative integer → 400; outside 1..=10 → 400, `:576-594`), `aspect_ratio` and `resolution` (trimmed,
  `:623-630`).
- **Refusals before dispatch**: a `mask` part (`:549-555`, native text kept); `stream` equal to `true` or `1`,
  case-insensitively (`:557-571`); missing `prompt`; missing `image` part (`:603-609`); a bad `n`; an elided text field
  (as `image-azure`).
- **Facts**: `operation` `edit`; `inputs.reference_image` = the number of `image` parts (the native precheck counts
  them all, `precheck.rs:133-142`, and the catalog caps it at 1); the rest as §2.
- **Size**: an input image above about 24 MiB expands past the 32 MiB JSON limit and is refused before admission
  (413, record §6.3a); the native arm sends it. Follows the record (§8 P11).

## 4. Response, metering and outcomes (both operations, per round)

- **Elision**: response path `/candidates/*/content/parts/*/inlineData/data` (the record's own example, §6.2).
- **Non-2xx** → error envelope from Google's `{"error": {"code", "message", "status"}}` (`provider_message` = `message`
  capped at 256 characters), code by status as `image-azure` (401 / 403 auth, 429 rate limit, 5xx unavailable, else
  invalid request); outcome `rejected` for a 4xx other than 408, else `unknown` (`image_shared::non_2xx`,
  `crates/south-component-conformance/src/image_shared.rs:158-168`).
- **2xx with `promptFeedback.blockReason`** and no image → `rejected`, `content_policy` (**GI-Q5**).
- **2xx, a candidate whose `finishReason` is a safety or policy reason** (`SAFETY`, `IMAGE_SAFETY`,
  `PROHIBITED_CONTENT`, `BLOCKLIST`, `SPII`, `IMAGE_PROHIBITED_CONTENT`, `NO_IMAGE`, … — the exact list is
  **unverified** against a capture) and no image → `unknown` with a `content_policy` envelope (**GI-Q5**).
- **Artifacts**: every `candidates[].content.parts[]` with a non-empty `inlineData.data` blob, in order, flattened
  across candidates, is an `inline` base64 artifact (native `transform_gemini_generatecontent_to_openai_images`,
  `gemini.rs:1406-1457`). Its `media_type` is the part's `inlineData.mimeType` when that starts with `image/`, else
  `image/png` (the native arm drops it, record §10.1). Parts carrying `"thought": true` are skipped (**GI-Q7**). Zero
  artifacts (no `candidates`, or no image part) → `unknown` (native: 502 and parked, `:1409-1418`, `:1443-1451`).
- **Metering, `tokens`** (`usageMetadata`):
  - **Declared buckets** (required on a 2xx, record Q16): `total_input` = `promptTokenCount`, `total_output` =
    `candidatesTokenCount`. Missing `usageMetadata`, or either count missing → `unknown`. A negative or fractional
    count → `unknown`. Same shape as `image-azure` (`reference_azure_image.rs:468`, `:538`).
  - **Breakdown, reported when present** (null otherwise): `text_input` / `image_input` = the `TEXT` / `IMAGE` entries
    of `promptTokensDetails`; `text_output` / `image_output` = the `TEXT` / `IMAGE` entries of
    `candidatesTokensDetails`; `cached_input` = `cachedContentTokenCount`. Each pair is reported only when its two
    entries sum to the matching total, so the host's consistency check (`server:…/handler/images/component/checks.rs:
    145-196`) never parks a response for a breakdown Google does not promise to close (**GI-Q4**).
  - `thoughtsTokenCount` is not reported (no bucket for it; **GI-Q4**).
- **Metering, `images`**: no required fact; `images_reported` is `null` (Gemini reports no image count), so the host
  settles the delivered count (`checks.rs:241-271`).
- **Render**: `{"created": <host>, "data": [{"b64_json": <artifact i>, "revised_prompt": null}, …]}` over **all
  rounds**, artifact indices counted across rounds in order (the suite's convention, `image_suite.rs:229-259`); as
  native (`gemini.rs:446-450`, `:1434-1437`).

## 5. What the host must do

1. **`repeat` (GI-Q2).** The executor refuses `repeat != 1` before admission (`server:…/handler/images/component/
   mod.rs:1296-1301`) and its held path assumes one round (`mod.rs:436`). For `n > 1` the host needs sequential rounds
   on both paths with the record §7 aggregation (round 1 `rejected` → the call is `rejected`; any later non-success →
   `unknown`; first-round P29 classification only, as native `gemini.rs:108-115`, `:411-421`), metering summed over
   rounds, `render` given every round's outcome, and artifact delivery across rounds. The §9.4 input bounds must
   scale with the rounds: every round resends the prompt and the reference image, but `HostBounds` derives
   `text_input` and `image_input` from the request alone (`checks.rs:43-50`), so a summed `n = 10` call would exceed
   them and park. Until this lands, a routed Gemini model answers `n > 1` with 503 before admission.
2. **`contracts.media` 2** (GI-Q1): re-pin to the release that carries the new transform word.
3. **Per-image rows**: H5 (unmerged, **H5** `component/tiers.rs`) plus GI-Q3; nothing for token rows.
4. **The pricing-form predicate.** The native arm decides "token-priced" with `nano_banana_token_priced`
   (`gemini.rs:1027-1034`), the executor with `image_model_token_priced` (`server:…/engine/token_counter/media.rs:
   1114-1122`, via `image_metering_required`, `:1128-1134`), which also counts a row with `image_price == 0` and a
   positive `output_price_per_million`. A Gemini row in that shape changes from per-image to per-token at cutover.
   The record §9.3 already leaves this to the host; it should check its Gemini rows before cutover.
5. **Retired and non-image ids.** The native entry refuses non-image and retired ids before dispatch
   (`handlers.rs:394-430`, `gemini.rs:1121-1183`). The component has no clock, so a date-driven retirement list cannot
   live in it; a routed retired id reaches the upstream, whose 404 is released under P29. Operators should not route
   retired ids (the routing write is theirs).
6. **Binding**: family `gemini-image`, `base_url` = the provider row's endpoint, slot `provider_api_key`.
7. **Catalog**: entries exist for the three current ids (§2); a new id needs one before it is routed (record §18.5).

## 6. Token pricing now, per-image tier rows later: is it achievable without a package change?

**Yes, with GI-Q3.** The difficulty is on the host side, read from code:

- dev-v2 refuses any call whose facts carry a tier word, whatever the pricing form (503,
  `component/mod.rs:1302-1310`).
- H5 replaces that with `tiers::plan` (**H5** `component/tiers.rs:81-154`, called from **H5** `component/mod.rs`
  before the pricing-form check). On a row **without** a tier table, a word other than the catalog default is refused
  with 400 (`tiers.rs:113-132`) — also on a token-priced row, where today's native arm serves 2K and 4K (the native
  flat-tier guard skips token rows, `precheck.rs:39-46`). On a row **with** a table, a token-priced row gets 503
  (the host's H5 plan, §3.2 executor rules, **H5** `docs/product-review-v2/plans/2026-10-13-H5-*.md`:72: "a
  token-priced model reporting tier words → 503").

So a package that always reported `resolution` would break 2K / 4K on every token-priced row. The proposed rule
(GI-Q3): **report `tier.resolution` only when the client sent a non-empty `resolution` and the host's
`metering_required` contains `images`.** The record lets the component use `metering_required` to decide what to
report (§9.3, last paragraph). Then:

| Row | dev-v2 today | With H5 merged |
|---|---|---|
| Token-priced | no tier word → served; 2K / 4K billed by `usageMetadata` | same |
| Per-image, no tier table | any `resolution` → 503 (unchanged until H5) | `1K` (catalog default) served at `image_price`; `2K` / `4K` → 400 (= the native flat-tier guard, `precheck.rs:31-62`, with generic text, Q10) |
| Per-image, tier table | → 503 | priced by the row's tier; reserve the dearest candidate (none here); settle at list price |

The word is the upstream's spelling (`1K`, `2K`, `4K`; the package upper-cases as it does for `imageSize`), matching
the catalog's words (`params.resolution.options`) and H5's exact match (R-2). No `tier_candidates` and no
`upstream_cost` are reported: Gemini serves the requested tier and quotes no price. An unsized request reports no
word, so H5 prices it at the row's `image_price`, not the table's `defaults` row (`plan` returns `None` for an empty
tier, `tiers.rs:87-89`); H5 already records that such rows need a positive `image_price` (H5 commit `d70d9b8c`), which
should equal the 1K price. If lv prefers the table's default row to price unsized requests, that is a host change in
`plan`, not a package change.

## 7. Making I3-3 (Vertex) a thin second package

The native arm already shares everything but URL and auth between the two families (`gemini.rs:12-22`, `:219-232`).
The proposal follows I3-1's `bailian_image.rs` pattern: a crate-private `gemini_image.rs` holding the body builder,
the edit body, the response reader (artifacts, finish reasons, usage), the render and the refusal texts, with the
family-specific half (path, auth arm, `edit` support) passed in. `reference_gemini_image.rs` is then path
`v1beta/models/{m}:generateContent` + `header_secret`; the Vertex reference is path
`v1/projects/{project}/locations/{region}/publishers/google/models/{m}:generateContent` + a `minted` `bearer` slot with
the `vertex_sa` recipe copied from `embeddings-vertex` (record §18.4), and `generate` only (the host refuses Vertex
edits, `handlers.rs:840-846`). The body already carries `role: "user"`, which Vertex requires (`gemini.rs:983-986`).
GI-Q1's transform word serves Vertex unchanged.

## 8. Differences that follow precedent (accepted under lv's 2026-10-13 pre-approval)

- **P1 Funds on non-2xx**: the P29 allow-list on both paths (P22 Q2, I3-1 IM-Q2). The native **sync** Gemini arm
  already does this on the first round (`gemini.rs:108-115`); the native **held** arm releases any first-round 4xx,
  the component held path only the allow-list (`component/mod.rs:436-443`).
- **P2 Client status while funds are parked**: 502 (I3-1 IM-Q6). Native: 502 for no image / no usage; 500 for a
  2xx body that is not JSON (`gemini.rs:427-428`), which becomes 502.
- **P3 Declared buckets** (record Q16): a 2xx missing `promptTokenCount` or `candidatesTokenCount` is `unknown`; the
  native arm settles any non-zero bucket (`gemini.rs:1040-1063`, `has_usage`), so a missing output count settles low
  today. Accepted by its own fixture, not the dual run (record §14).
- **P4 Malformed counts**: negative or fractional → `unknown`; native clamps to 0 (`media.rs:187-190`) (`image-azure`
  precedent).
- **P5 Tier refusal text** is the host's generic text (record Q10).
- **P6 `resolution` fact only when the client sent one** (I3-1 Qwen `size` decision, I3-1 §3).
- **P7 Reservation**: the host bound formula (one allowance per media part) replaces the native reservation (P22's
  registered "token reservation" difference).
- **P8 Non-2xx outcome table**: `rejected` for 4xx other than 408, else `unknown` (`image-azure`, I3-1 decision 3).
- **P9 2xx `rejected` is parked, not released**, on the host (`component/mod.rs:824-831`), so GI-Q5's choice changes
  labels and future policy, not today's funds.
- **P10 Per-image rows are not priced from an unforwarded `size`**: the native generation arm prices `image_price_1k`
  / `_2k` from the client's `size` (`gemini.rs:455`, `media.rs:31-51`) though Gemini never receives it, while its edit
  arm passes `None` (`gemini.rs:864-867`); the component reports no `size` (I3-1 §5 item 3, MiniMax).
- **P11 JSON expansion above 32 MiB** is refused before admission (record §6.3a; P22 C7).

## 9. Fixtures, tests, wiring; decisions where the record is silent

As I3-1 §4: a gate ② pack (`capabilities`, `prepare`, `response`, `render` rows) covering every refusal of §2 and §3,
`image_url` absent / data URL / `http` URL / wrong type / missing `;base64,`, `resolution` lower-case → `1K`,
`aspect_ratio`, `resolution` with `images` required and with `tokens` required (GI-Q3), `n` 1 and 10 (`repeat`),
an edit with JPEG and with an unlabelled part (GI-Q6), `mask` and `stream` refusals, every outcome of §4 (each safety
path, a `thought` part, zero images, a non-JSON 2xx, each declared bucket missing — the `missing_meter_is_not_zero`
row — a breakdown that does not close, a negative count), a three-round render; a suite test `gemini_image_suite`,
sandbox parity `gemini_image_sandbox_parity`, the build script, the `ci.yml` / `release.yml` enumerations,
`shipped_packages_v1`, README and ARCHITECTURE notes. Fixture sources: the host's pinned behaviour in
`server:…/handler/images/tests/gemini.rs`, `server:gateway/tests/images_providers.rs`, `images_sync_contract.rs`,
`media_arms.rs`, `images_held_failure.rs` and `byok_media_ownership.rs`, transcribed, not back-derived.

Proposed decisions (the record is silent; not questions unless lv objects):

1. Models: `model-capabilities` declares every configured model (as `image-azure`), operations `generate` and `edit`,
   `input_roles` `image_url` → `reference_image` and `image` → `reference_image`, `response_formats` `[b64_json]`,
   `metering_forms` `[tokens, images]`, `token_buckets` `[total_input, total_output]`, `tier_dimensions`
   `[resolution]`, `repeat` true, `request_elision_paths` `[/image_url]`.
2. `response_format` is ignored and the answer is always `b64_json`, as native (the native arm never reads it); a
   client asking for `url` gets `b64_json` (the `image-azure` refusal is not copied, because native Gemini does not
   refuse).
3. Refusal texts: the native ones where §2 / §3 cite them, `image-azure`'s for elided fields.
4. No `content-type` header is set by the package (the host's JSON executor sets it, as `image-azure`, I3-1 decision 7).

## 10. Versioning (Q47)

With GI-Q1 as recommended: `south-contracts` 0.52.0 → 0.53.0 (`data_url_payload`, `MEDIA_CONTRACT_VERSION` 1 → 2,
golden vectors, fuzz target), `south-component-conformance` 0.56.0 → 0.57.0 (shared module, reference, suite rows);
every component lockfile and `fuzz/Cargo.lock` follow; every published package takes a patch and keeps its
`contracts.media` and `south_runtime`; `image-gemini` 1.0.0 declares `{image: 1, media: 2}` and the `south_runtime`
of the release that ships the word; `compatibility.json` `contracts.media` 2. Exact numbers are fixed at
implementation (I3-1's §7 numbers moved once already). Without GI-Q1: no `south-contracts` change and
`image-gemini` declares `media: 1`.

## 11. Open questions for lv (ruled 2026-10-13)

lv ruled all seven as recommended: GI-Q1 adds the transform word `data_url_payload` (`contracts.media` 1 to 2,
`south-contracts` 0.52.0 to 0.53.0); GI-Q2 the package emits `repeat = n` and the host's multi-round dispatch is a
precondition of cutover, so no Gemini row is cut over before it lands; GI-Q3 to GI-Q7 as recommended. lv also ruled
that the native-arm defects this record found (read, not run) are not fixed in the native arm: they are registered
here and leave with the arm after cutover.

- **GI-Q1 Reference images on generation need a transform the contract lacks.** A JSON request's blob is the client's
  string, so for `image_url` it is the whole data URL text (`elide.rs:318-329`). Gemini wants only the base64 payload.
  `base64` would encode the data URL text itself, `from_data_url` yields bytes and a JSON template accepts only UTF-8
  text (`descriptor.rs:525-530`), and a reference takes one transform. Options: (a) add one word `data_url_payload`
  (a base64 data URL to its canonical base64 payload, as text) — a `contracts.media` 2 change, as Q3 requires for any
  new word; (b) ship generation without reference images (refuse `image_url` in `prepare`, a registered regression)
  and add the word later; (c) leave `/image_url` undeclared so values under 1 MiB stay inline and the component
  splits them itself, refusing larger ones (a regression for ordinary photos). **Recommended: (a)**, in this step; I3-3
  needs the same word, and the Reve bridge (I3-6) probably does (**unverified**).
- **GI-Q2 `n > 1`.** Gemini returns one image per call (`gemini.rs:43-49`), and the record's answer is `repeat`
  (§7). The host executor does not run repeats yet (§5 item 1). Options: (a) the package emits `repeat = n` now and
  the host implements repeats (sync and held) as a cutover prerequisite; (b) cut over with `n > 1` answered 503 until
  then (a registered regression; native serves 1..=10); (c) `generationConfig.candidateCount` (the host lists it as
  immutable precisely because it multiplies images, `execute.rs:52-59`; whether Nano Banana honours it is
  **unverified**). **Recommended: (a)**, with no Gemini row cut over before the host's repeat lands, unless lv accepts
  (b) for rows whose clients never send `n`.
- **GI-Q3 Tier word only on per-image calls.** Report `tier.resolution` only when `metering_required` contains
  `images` (§6). Alternative: always report it and change the host so tier words are ignored on token-priced rows
  (H5 `plan` and its 503). **Recommended: the package rule** — no host change, H5's ruling stands, and per-image rows
  work later without a package change.
- **GI-Q4 Output tokens.** (i) The executor bills `total_output` = `candidatesTokenCount` (`media.rs:1157-1160`); the
  native arm bills the `IMAGE` entry of `candidatesTokensDetails` when it is non-zero and drops any `TEXT` output
  tokens (`media.rs:169-175`). With `responseModalities: ["IMAGE"]` no text output is expected (**unverified**), so
  the two agree in practice; when text tokens do appear, the component bills them at the image output rate (the host
  has one image output rate, `media.rs:306-322`). (ii) `thoughtsTokenCount` is ignored by both; Google bills thinking
  as output on its thinking models, possibly including `gemini-3-pro-image` (**unverified**), and the host has no
  text-output rate for image rows to price it at. **Recommended:** accept (i) as a registered difference; for (ii),
  follow native now and ask the host to capture a `gemini-3-pro-image` response before deciding whether a bucket and
  a host rate are needed.
- **GI-Q5 Safety blocks in a 2xx.** `promptFeedback.blockReason` (refused before generation) → `rejected`; a
  candidate safety `finishReason` with no image (possibly after generation, possibly charged) → `unknown`, as I3-1's
  MiniMax 1026 (IM-Q4). Today the host parks both (§8 P9) and answers 502; native parks both too (no image → 502,
  `gemini.rs:1443-1451`). Whether Google charges for a blocked prompt is **unverified**. **Recommended: as stated.**
- **GI-Q6 Input image media type on edits.** The package uses the multipart part's media type (`image/*`) or
  `image/png`; native sniffs bytes. A JPEG uploaded as `application/octet-stream` would be labelled `image/png`
  (whether Gemini tolerates a wrong label is **unverified**). Alternative: a host-supplied sniffed type in the
  multipart view (a contract change). **Recommended: the package rule, registered**, plus one capture with a
  mislabelled JPEG before cutover.
- **GI-Q7 Thought parts.** Skip `inlineData` parts marked `"thought": true` (interim images; native would deliver
  them). They appear only if a request asks for thoughts, which operator request extras can do
  (`generationConfig.thinkingConfig` is not immutable). **Recommended: skip.**

## Implementation (2026-10-13)

What landed, on branch `image-i3-2` (on `origin/main` = `57c4ae8`, release 0.58.0):

- **GI-Q1, `contracts.media` 2.** `MediaTransformV1::DataUrlPayload` (`"data_url_payload"`) reads a base64 `data:` URL as
  `from_data_url` does and yields the canonical base64 of its bytes as text (`data_url_payload_v1`); it refuses exactly
  what `from_data_url` refuses (no `data:` scheme, no comma, no `;base64` marker, a media type outside the grammar,
  non-canonical base64, non-UTF-8 input). Unit tests, two property tests (round trip with `data_url` and `base64`; the
  same decisions as `from_data_url`), a template expansion test, golden vectors in `tests/vectors/media-v1.json` (two
  outputs, four refusals, one template; the file's `version` is now 2) and the `media_parsers` fuzz word list.
  `compatibility.json` records `contracts.media` 2. `MediaTransformV1::minimum_contract_version` and
  `MediaRequestDescriptorV1::minimum_contract_version` say which media contract a word or a descriptor needs.
- **`image-gemini` 1.0.0**: manifest, guest crate (a thin shell like `image-minimax`), reference
  `reference_gemini_image.rs` over the crate-private module `gemini_image.rs` (§7: body builders, response reader,
  usage reader, render, refusal texts; the family passes its path, its auth member and whether it edits), fixture pack
  `fixtures-image-gemini/` (90 cases, with a README), suite test `gemini_image_suite` (the pack plus eleven named
  behaviour tests), sandbox parity test `gemini_image_sandbox_parity`, build script, and the enumerations (`ci.yml`
  rust-cache, `release.yml` build / gate ② / package lists, `shipped_packages_v1` official list, the image-world contract
  check and a test that a media 1 host refuses the package while still admitting `image-azure`, README and
  ARCHITECTURE).

Failing first: with the reference written and only three expectations written by hand
(`prepare.generate-image-url-png`, `prepare.generate-resolution-images-form`, `response.prompt-blocked`), the pack test
failed on the 87 cases still without an expectation and passed the three hand-written ones. The other expectations were
then taken from the reference and reviewed row by row against §2 to §4. Each of six mutations turns a named test red,
and the pack test with it: `data_url_payload` written as `as_is`
(`the_reference_image_is_sent_as_its_base64_payload`), the tier word reported whatever the metering form
(`the_tier_word_exists_only_on_per_image_calls_with_a_resolution`), a blocked prompt `unknown`
(`a_blocked_prompt_is_rejected_and_a_policy_stop_is_unknown`), `thought` images delivered
(`thought_images_are_not_delivered`), the modality split reported without the closing check
(`the_modality_split_is_reported_only_when_it_adds_up`) and `repeat` fixed at 1 (`n_is_repeat_and_never_sent_upstream`).

Versions (§10):

| Item | Before | After |
|---|---|---|
| `south-contracts` | 0.52.0 | 0.53.0 |
| `south-component-conformance` | 0.56.0 | 0.57.0 |
| `image-gemini` | — | 1.0.0 (`contracts` `{image: 1, media: 2}`) |
| `image-azure` / `image-minimax` / `image-qwen` | 1.0.5 / 1.0.0 / 1.0.0 | 1.0.6 / 1.0.1 / 1.0.1 |
| `provider-openai-compatible` | 2.6.1 | 2.6.2 |
| `provider-anthropic`, `provider-anthropic-bedrock-invoke`, `provider-bedrock-converse`, `provider-bedrock-converse-bearer` | 1.1.1 | 1.1.2 |
| `provider-gemini`, `provider-openai-responses` | 1.3.1 | 1.3.2 |
| `embeddings-openai-compatible` / `embeddings-gemini` / `embeddings-vertex` | 1.0.9 / 1.1.6 / 1.0.8 | 1.0.10 / 1.1.7 / 1.0.9 |
| `task-kling` / `task-kling-v2` | 1.0.18 / 0.32.16 | 1.0.19 / 0.32.17 |
| `task-minimax-v2`, `task-bailian-v2` | 0.31.15 | 0.31.16 |
| `task-xai-v2`, `task-veo-v2`, `task-wan-image-v2`, `task-gmi-image-v2` | 0.35.15 | 0.35.16 |
| `task-byteplus-v2` | 0.36.15 | 0.36.16 |

`south-provider-api` is unchanged (0.53.0). Every published package keeps `south_runtime` 0.57.0 and its `contracts`
(`shipped_packages_v1::i3_2_retires_every_published_058_package_identity`); the I3-1 retirement test no longer pins
I3-1's identities. The workspace version and tags are unchanged.

Departures from §10, each following the embeddings contract 2 precedent (0.51.0):

1. `MEDIA_CONTRACT_VERSION` keeps its value 1 and a new `MEDIA_CONTRACT_VERSION_V2` is 2, with
   `MEDIA_CONTRACT_VERSIONS` = `[1, 2]` for a host's range; §10 says the constant itself moves to 2. Every published
   image package declares media 1, and token-station-server builds its media range from that constant (its
   `south_component.rs` test asserts the range equals `{MEDIA_CONTRACT_VERSION}`): moving the constant would make a
   host that re-pins without other changes refuse every image package. With the precedent, a host keeps admitting
   them and adds 2 to its range to admit `image-gemini` (§5 item 2). The test hosts (`host_range` in the conformance
   and runtime crates, `declared_runtime_v1`) decode every media contract up to the one `compatibility.json` records,
   as they do for embeddings.
2. `image-gemini` declares this tree's release, 0.58.0, as its `south_runtime` until the release commit, which moves
   the workspace version and the declaration together to the release that ships the word (as `embeddings-gemini`
   1.1.0 declared 0.50.0 until the 0.51.0 release commit). `scripts/check-declared-runtime.sh --build` passes on this
   branch only because a package declaring the workspace version is judged by this tree; the published 0.58.0
   runtime decodes media 1 only and would refuse it, so the release commit must not ship the declaration unchanged.

Decisions where the record is silent:

1. **`image_url` from the placeholder head.** The 64-byte `head` must start with `data:` (case-sensitive, as native)
   and contain `;base64,`; the `mimeType` sent is the text between them, verbatim, as native. A data URL whose media
   type and parameters push `;base64,` past the head is refused with the native "must use base64 encoding" text (no
   image media type comes close). A value a non-conforming host did not elide is read the native way, with its
   syntax-only base64 check, and inlined.
2. **Refusal texts**: the native ones, except that a non-string `image_url` is named by its JSON type rather than
   echoed (an array of data URLs would otherwise be copied into the error), an elided `prompt`, `aspect_ratio`,
   `resolution` or edit text field uses `image-azure`'s "longer than the gateway accepts", and an `n` that is not a
   whole number on generation uses the host entry's text.
3. **A resolution outside the tier grammar on a per-image call** (`images` required) is refused before dispatch: no
   tier row could price it, and the native per-image guard refuses any resolution its row does not price. On a
   token-priced call it is sent as is, as native.
4. **Edit parts**: only file parts named `image` count (a text part named `image` does not; `image[]` is not read);
   `prompt`, `n`, `stream`, `aspect_ratio` and `resolution` are read from text parts; the part's media type is used
   when it starts with `image/` (either case) and is a valid media type (GI-Q6).
5. **Artifacts**: `inlineData.mimeType` is used when it starts with `image/` and is a valid media type; an empty
   `data` is skipped; an image present wins over a block or finish reason.
6. **GI-Q5 details**: a blocked prompt is `rejected` with a `content_policy` 400 envelope whose `provider_message` is
   the `blockReason`; a policy stop is `unknown` with the same envelope naming the finish reason. The finish reasons
   are the record's seven plus `RECITATION` and `IMAGE_RECITATION` (still **unverified**); any other image-less 2xx is
   `unknown` without an envelope.
7. **Usage**: as `image-azure`, a missing declared bucket is `unknown` only when `tokens` is required (a per-image call
   succeeds without usage), and a malformed count anywhere, the modality details included, is `unknown`; a details
   entry without `tokenCount` counts 0 (proto3 omits zeros); a details list with non-zero tokens of another modality
   reports no split; `cachedContentTokenCount` is `cached_input`.
8. **Non-2xx**: the code follows the HTTP status (`image-azure`'s table); Google's `error.status` word is not read.

## Revision note

- 2026-10-13: first draft (proposed).
- 2026-10-13: lv ruled GI-Q1 to GI-Q7 as recommended and chose not to fix the native-arm defects in the native arm;
  the status is accepted.
- 2026-10-13: implemented on branch `image-i3-2` ("Implementation"): `MEDIA_CONTRACT_VERSION` keeps 1 with a new `_V2`
  and `MEDIA_CONTRACT_VERSIONS` (embeddings precedent), and `image-gemini` declares 0.58.0 until the release commit.
