# Image world step I3-4: `image-stability` (Stability AI Stable Image)

Status: accepted (2026-10-13). SI-Q4 ruled by lv (with I3-3's VI-Q2): the host answers 400 for an operation the package
does not declare. SI-Q1, SI-Q2, SI-Q3 and SI-Q5 follow the recommendations, under lv's pre-approval to build I3-4
directly per the image world record (each keeps the native arm's behaviour). Implemented in #197 (see Implementation); released in 0.60.0
([release record](2026-10-13-release-0.60.0.md)).

Date: 2026-10-13

Baseline: south `origin/main` = `2a95713` (release 0.59.0: I3-2 `image-gemini`, `contracts.media` 2). Host references
carry a `server:` prefix and refer to token-station-server `origin/dev-v2` = `e77eec98` (which contains the host's
multi-round executor, `f0e0c6ab`). `server:…/` abbreviates `server:gateway/src/modules/inference/`. Statements marked
**unverified** are conclusions from reading code or from what Stability documents, not from running anything or
capturing traffic. Nothing in this record was built or run.

Parent record: [`2026-09-30-image-world.md`](2026-09-30-image-world.md) ("the record"): §6.3 / §6.3a / §6.7 (descriptor,
transport, encoder), §7 (`repeat`, allowed deliveries), §10.1 (artifact forms), §13 (the Stability row), §14 item 5
(I3-4), §18.1 (the HTTP contract number). Sibling records, treated as precedent:
[`2026-10-12-image-i3-1-minimax-and-qwen.md`](2026-10-12-image-i3-1-minimax-and-qwen.md) ("I3-1") and
[`2026-10-13-image-i3-2-gemini.md`](2026-10-13-image-i3-2-gemini.md) ("I3-2"); the shipped template for a multipart
descriptor is `image-azure`'s edit (`crates/south-component-conformance/src/reference_azure_image.rs`).

**Pre-approval (lv, 2026-10-13).** I3-4 may be built directly from the record once this design is reviewed.
Differences from the native arm that follow an established precedent (I3-1, I3-2, P22 Azure) are accepted without a new
question and are listed in §7; only new decisions are in §10.

## 0. Prerequisites the record named: both exist

§14 item 5 says I3-4 "needs the encoder (§6.7) and HTTP contract 10". Both have shipped; **no contract change is
needed for this step**:

| Record names | Where it is now |
|---|---|
| The multipart encoder (§6.7, Q1) | `encode_multipart_v1`, `crates/south-contracts/src/media/multipart.rs:345` (S-I-1a, 0.52.0). The host already calls it for `image-azure` edits (`server:…/handler/images/component/mod.rs:1696-1732`, `encode_parts` at `:1818`) |
| "HTTP contract 10" (multipart POST with a byte-buffered response) | Renumbered **12** by §18.1; `HTTP_CONTRACT_VERSION = 12` (`crates/south-contracts/src/lib.rs:122`, `compatibility.json` `contracts.http` 12); entry point `execute_multipart_binary_call_v1` (`crates/south-core/src/lib.rs:950`); the host's executor reads every round as bytes through it (`component/mod.rs:556-580`) |
| `from_data_url` (a client data URL to file bytes) | `contracts.media` 1, `crates/south-contracts/src/media/transform.rs:122` |
| A multipart descriptor on a **generate** call | Nothing ties the body form to the operation: the host seals whatever `MediaBodyV1` the descriptor carries (`component/mod.rs:1696`) and decides the operation from the endpoint only (`:12`) |

"Binary response" in the record's Stability row means the transport reads the body as bytes (rule 1 of §6.3a), not
that Stability is asked for `image/*`; the response form is a separate choice (§4, SI-Q2).

## 1. What I3-4 delivers

| Item | Where | Change |
|---|---|---|
| `image-stability` 1.0.0 | `components/image-stability/`, `crates/south-component-conformance/src/reference_stability_image.rs`, `fixtures-image-stability/` | Family `stability-image` (proposed); `generate` only (§2) |
| Q47 | `south-component-conformance` minor | Every published package takes a patch (§9) |

Image world (`image-adapter-v1`), contracts `{image: 1, media: 1}` (no word past media 1 is used), `south_runtime` the
oldest runtime that admits these declarations (expected 0.57.0, as `image-minimax`), `bearer` on `provider_api_key`
(the native arm's plain Bearer, `server:…/engine/upstream.rs:64-72`), metering form `images` only, `repeat` 1,
response form `json`.

## 2. Scope: the five models the native arm serves, generation only

The native arm (`server:…/handler/images/stability.rs`, 803 production lines) serves `/v1/images/generations` only and
classifies the upstream id into three endpoints (`stability.rs:41-48`, `:95-117`):

| Upstream id | Path | Extra form fields | Source image |
|---|---|---|---|
| `stable-image-ultra` | `v2beta/stable-image/generate/ultra` | — | yes (image-to-image) |
| `stable-image-core` | `v2beta/stable-image/generate/core` | — | **no** (`stability.rs:443-449`) |
| `sd3.5-large`, `sd3.5-large-turbo`, `sd3.5-medium` | `v2beta/stable-image/generate/sd3` | `model` = the id, `mode`, `cfg_scale` | yes |

The model catalog already carries these five ids (`catalogs/model-catalog.json`: `aspect_ratio` options and default
`1:1`, `n.max` 1, `media.roles.input_image.max` 1 for all but Core). `/v1/images/edits` is refused natively for
Stability (`server:…/handler/images/handlers.rs:835-839`); Stability's own edit, upscale and control endpoints are not
served by the host and are out of scope. **Recommended (SI-Q1): exactly these five ids, generation only**, as native.

## 3. Request (`prepare`)

Reproduces `build_stability_generation_request` (`stability.rs:423-546`) and `stability_prepare` (`:603-664`):

- **Descriptor**: `POST v2beta/stable-image/generate/{ultra|core|sd3}`, header `accept: application/json`
  (`stability.rs:653`; `accept` is not a reserved header, `crates/south-contracts/src/lib.rs:326-350`), auth
  `{"arm": "bearer", "slot": "provider_api_key"}`, body **multipart**, no `content-type` (the encoder writes it).
- **Text parts, in the native order** (`stability.rs:488-539`): `prompt`; `output_format` (the client's, else `png`);
  `negative_prompt`, `seed`, `style_preset` when sent; with a source image: `strength`, then (SD3 only) `mode` =
  `image-to-image`; without one: `aspect_ratio` when derived; SD3 only: `model`, then `mode` = `text-to-image` when there
  is no source image, then `cfg_scale` when sent.
- **File part** (last, `stability.rs:78-90`): `{"name": "image", "blob": <the data URL's blob>, "transform":
  "from_data_url", "filename": "source.<png|jpg|webp>", "media_type": <normalised type>}`. The source is `image_url` or
  `image`, each a string or `{"url": <string>}` (`stability.rs:229-269`). Declared request elision paths: `/image_url`,
  `/image_url/url`, `/image`, `/image/url`, so the data URL never enters the sandbox; the component reads its prefix from
  the placeholder's 64-byte `head` (record §6.2).
- **`aspect_ratio`**: the client's `aspect_ratio` (one of the nine words, `stability.rs:8-10`) wins; else `size`: absent,
  `null`, empty or `auto` → none; one of the nine words → that word; `WxH` (case-insensitive `x`, positive) → the
  nearest word by log ratio (`stability.rs:149-227`); anything else → 400. Only on text-to-image; on Ultra
  image-to-image the native arm neither validates nor forwards `size` / `aspect_ratio`, and SD3 image-to-image refuses
  them (`stability.rs:513-525`). Both reproduced.
- **Numbers**: `seed` a whole number 0..=4294967294 (`:370-386`); `strength` 0..=1, required with a source image
  (`:506-512`); `cfg_scale` 1..=10, SD3 only, refused on Ultra and Core (`:527-539`). A number is sent as `serde_json`
  renders the parsed value (`v.to_string()`, `:367`), not as the view's source text (decision 3).
- **Refusals before dispatch** (`invalid_request`, native texts): `prompt` missing, not a string, or blank after
  trimming (`:430-440`); `response_format` `url` / `{"type": "url"}` / any other type (`:119-147`, the record §7
  "refuse in `prepare`" rule); `n` other than 1 (`:611-616`); both `image_url` and `image`; a source of the wrong
  shape; a source that is not `data:`, lacks `;base64,`, has an empty or unsupported media type (only `image/png`,
  `image/jpeg`, `image/jpg` → `image/jpeg`, `image/webp`), an empty payload, or a payload above 10 MiB decoded
  (`:271-330`; size rule in decision 2); a source image on Core; every `mode` refusal (`:451-486`); `output_format`
  outside `png`, `jpeg`, `webp`; `style_preset` outside the seventeen words (`:12-30`); the range refusals above; an
  elided `prompt` or `negative_prompt` (`image-azure`'s "longer than the gateway accepts"). Base64 syntax errors in the
  payload are refused by the host's expansion before admission with its generic text (`from_data_url` refuses
  non-canonical base64, as the native `STANDARD` engine does; **unverified** that the two agree on every input).
- **Dropped**: every other field (`quality`, `style`, `user`, `stream`, …) and Ultra's `mode` hint, as native.
- **Immutable part names**: `model`, `aspect_ratio`, `output_format` (= `STABILITY_IMAGE_OWNED`,
  `server:…/handler/images/execute.rs:71-72`), with the multipart meaning of record §7 (P22 Q3).
- **Facts**: `operation` `generate`; `inputs.input_image` = 1 with a source image (the native precheck's role,
  `server:…/handler/images/precheck.rs:106-107`); `requested_outputs` 1; **no `size`** (§7 P5); no tier; `metering_forms`
  `[images]`; no `bounds`; `repeat` 1; `response_body_form` `json`; response elision path `/image`.

## 4. Response (`parse-response`) and render

Reproduces `stability_dispatch` and `stability_image_result_to_openai` (`stability.rs:548-582`, `:667-710`):

- **2xx**: JSON `{"image": <base64>, "finish_reason", "seed"}`. `image` missing or empty → `unknown` (native 502,
  parked). `finish_reason` missing → `unknown` (native 502). `SUCCESS` → `succeeded`. `CONTENT_FILTERED` →
  `succeeded`, the image delivered and billed, as native (**SI-Q3**). Any other word → `unknown`. A 2xx that is not
  JSON → the host cannot build the view → `unknown` (native 502 parse failure; host behaviour **unverified**).
- **Artifact**: one `inline` base64 artifact at `/image`, `media_type` from the `output_format` the component sent
  (`image/png`, `image/jpeg`, `image/webp`), kept in `state` (the native arm drops it; record §10.1).
- **Metering**: `images`, `images_reported` `null` (Stability reports no count; the host settles the delivered count,
  1). `seed` is not reported (native drops it).
- **Non-2xx**: error envelope from Stability's `{"id", "name", "errors": [...]}` (`provider_message` = `name: errors`
  joined, capped at 256 characters; shape **unverified** against a capture); code by status as `image-azure`; `name`
  `content_moderation` (Stability's 403, **unverified**) → `content_policy`; outcome `rejected` for a 4xx other than
  408, else `unknown` (`image_shared::non_2xx`).
- **Render**: `{"created": <host>, "data": [{"b64_json": <artifact 0>, "revised_prompt": null}]}` (`stability.rs:575-581`).

## 5. What the host must do

1. **Nothing contract-side.** No `south-contracts` change; a host that admits `image-minimax` (media 1, HTTP
   contract 12) admits this package. Re-pin to the release that carries `image-stability`.
2. **Binding**: family `stability-image`, `base_url` = the provider row's endpoint (`https://api.stability.ai` by default,
   `server:gateway/src/infra/config/providers.rs:743`), slot `provider_api_key`.
3. **Metering form `images`** (asked to check): the executor refuses `credits` and `requests` before admission
   (`component/mod.rs:1473-1482`) and its form predicate never yields them (`server:…/engine/token_counter/media.rs:
   1125-1134`). Native bills `calculate_image_cost(model, 1, size)` at the row's `image_price`
   (`stability.rs:705-707`; "flat credit-derived `image_price`", `handlers.rs:578-580`). Stability's response carries no
   credit count, so `credits` would have no evidence anyway. **`images` is the only form**; the package does not declare
   `tokens`, so a token-priced Stability row is refused 503 before admission (native: 400 at `handlers.rs:582-593`).
4. **Unpriced rows**: native refuses `image_price <= 0` with 400 (`handlers.rs:582-593`). I found no `image_price`
   check in the component path (`component/mod.rs`), so whether a routed unpriced row is refused (the 2026-09-30 P8
   "unpriced is refused" ruling) is **unverified**; the host should confirm before cutover.
5. **Edits on a generate-only model** (SI-Q4): a routed model answers `/v1/images/edits` with 503 "the package does not
   declare the Edit operation" (`component/mod.rs:1468-1472`); native answers 400 with a pointer to generation.
6. **Dual run and cutover**: as I3-1 (decoded part lists compared, record §14); sync and held paths switch together; the
   native held arm (`server:…/handler/images/durable.rs:426`, `:544-554`) is deleted only after in-flight held rows are
   0. Fixture sources: `server:…/handler/images/tests/stability.rs`, `server:gateway/tests/images_ideogram_stability.rs:
   370-540`, `model_request_extras_media.rs:877-880`, `byok_media_ownership.rs`.

## 6. Host defects found on the way (read, not run)

1. `mode` = `text-to-image` together with a source image is refused with "must be `text-to-image` or `image-to-image`
   (got 'text-to-image')" (`stability.rs:462-476`), a misleading text.
2. Ultra image-to-image silently drops an invalid `aspect_ratio` / `size` (never validated, `:506-525`).
3. The per-image tier is priced from the client's `size` (`image_size_hint`, `media.rs:31-35`), which Stability never
   receives (it gets an aspect ratio), as I3-1 §6 item 2 and I3-2 P10.
4. The `image_price <= 0` gate rejects rows priced only by tier (`handlers.rs:582`), as I3-1 §6 item 1.

None blocks the package. Per lv's I3-2 ruling, native-arm defects are registered here and leave with the arm.

## 7. Differences that follow precedent (accepted under lv's 2026-10-13 pre-approval)

- **P1 Funds on non-2xx**: the P29 allow-list (P22 Q2, I3-1 IM-Q2). Native sync parks every post-dispatch failure
  (`delivery_unknown`, `stability.rs:766-772`), 4xx included.
- **P2 Client status while funds are parked**: 502 for a failure found inside a 2xx body (I3-1 IM-Q6). Native also
  answers 502 for its 2xx failures. A non-2xx answer the host parks (401, 5xx) keeps the upstream status on both arms;
  IM-Q6 covers only 2xx bodies (corrected 2026-10-13 from the host dual run, see the revision note).
- **P3 Admission timing**: every §3 refusal is before admission; native refuses after admission and cancels
  (`stability.rs:730-742`; the host's `stability_url_format_and_multi_image_…` test pins zero funds either way). Same as
  the Azure / I3-1 timing ruling.
- **P4 Generic refusal texts**: base64 syntax refused by host expansion; an undeclared upstream id → 503 before admission
  (native 400 with its SKU list, `stability.rs:110-115`); tier and capability texts are the host's (record Q10).
- **P5 No `size` fact**: per-image rows are priced at `image_price`, not a tier chosen by an unforwarded `size`
  (I3-1 §5 item 3, I3-2 P10; host defect §6 item 3).
- **P6 Non-2xx outcome table**: `rejected` for 4xx other than 408, else `unknown` (`image-azure`, I3-1 decision 3).
- **P7 Elided text fields** (above 1 MiB) are refused before dispatch (`image-azure`, I3-2 decision 2); native would
  send them and Stability would refuse them.
- **P8 Artifact media type** is declared (I3-2 §4, record §10.1); native drops it.

## 8. Fixtures, tests, wiring; decisions where the record is silent

As I3-1 §4 / I3-2 §9: a gate ② pack covering each of the five ids (path, `model` / `mode` parts), text-to-image and
image-to-image (Ultra, SD3; `image_url` string and `{"url"}`, `image` string), every refusal of §3 (including Core with
an image, both fields, 10 MiB edge, each `mode` combination), `size` → nearest ratio, `aspect_ratio` precedence,
`output_format` jpeg and webp, every §4 outcome (each `finish_reason`, missing image, missing reason, non-JSON 2xx,
403 `content_moderation`, 401, 429, 500), a request-extras row on an immutable name; suite test
`stability_image_suite`, sandbox parity `stability_image_sandbox_parity`, build script, the `ci.yml` / `release.yml`
enumerations, `shipped_packages_v1`, README and ARCHITECTURE. Fixtures are transcribed from the host files of §5 item 6,
not back-derived.

Proposed decisions (not questions unless lv objects):

1. `model-capabilities` declares the five ids, operation `generate`, `input_roles` `image_url` and `image` →
   `input_image`, `response_formats` `[b64_json]`, `metering_forms` `[images]`, no token buckets, no tier dimensions,
   `repeat` false, the four request elision paths.
2. **10 MiB decoded**: from the placeholder's `bytes` the component knows the payload length P (bytes after
   `;base64,`); it refuses when P = 0 or P > 13,981,016 (the largest canonical length whose decoded size can be
   10 MiB). A payload of exactly that length decoding to 10,485,761 or 10,485,762 bytes passes where native refuses; the
   component cannot see the padding. Stability's own limit then applies.
3. Numbers are rendered as `serde_json` renders the parsed value (`0.50` → `0.5`), as native.
4. `response_format` `b64_json` or `{"type": "b64_json"}` or absent is accepted and ignored; `url` refused (native).
5. Media type normalisation and file name as native (`image/jpg` → `image/jpeg`, `source.jpg`).

## 9. Versioning (Q47)

`south-component-conformance` 0.57.0 → 0.58.0 (new reference); `south-contracts` and `south-provider-api` unchanged;
every component lockfile and `fuzz/Cargo.lock` follow; every published package takes a patch and keeps its
`south_runtime` and `contracts`; `image-stability` starts at 1.0.0 with `{image: 1, media: 1}`. Exact numbers are fixed
at implementation.

## 10. Open questions for lv

- **SI-Q1 Scope.** The five ids the native arm classifies, `generate` only; Stability's other endpoints (edit,
  upscale, control) and any newer id wait for a later package version with a catalog entry. **Recommended: yes.**
- **SI-Q2 Response form.** Ask for JSON (`accept: application/json`, base64 `image`, `response_body_form` `json`) as
  native, or ask for `image/*` (`binary` form, a `body` artifact, `finish-reason` / `seed` from response headers, about
  25% smaller; header names **unverified**). The dual run compares upstream headers, so `image/*` would be a registered
  difference. **Recommended: JSON, as native.**
- **SI-Q3 `CONTENT_FILTERED`.** `succeeded` (deliver the filtered image and bill one image, as native and as the host's
  pinned test `stability_content_filtered_finish_is_delivered_and_billed_as_one_image` expects), or `charged_failure`
  (the host parks it today, `component/mod.rs:945`, and the client gets 502). Whether Stability charges for a filtered
  result is **unverified**; the native comment says it does. **Recommended: `succeeded`.**
- **SI-Q4 Edits on a generate-only routed model.** The host answers 503 before admission; native 400. It applies to
  every generate-only package (MiniMax, Qwen, and Vertex in I3-3), not only this one. **Recommended: a small host change
  — 400 `invalid_request` when the package declares the model but not the operation**; otherwise register it for each
  generate-only family.
- **SI-Q5 `n > 1`.** Native refuses `n != 1` and the catalog says `n.max` 1, so the host's `repeat` is not used.
  `repeat = n` would be possible, but a client `seed` would make every round identical and it needs a catalog change.
  **Recommended: keep `n` = 1.**

## Implementation (2026-10-13)

What landed on branch `image-i3-4` (on `origin/main` = `2a95713`, release 0.59.0):

- **`image-stability` 1.0.0**: manifest (family `stability-image`, `bearer` on `provider_api_key`, `generate`,
  `contracts` `{image: 1, media: 1}`, `south_runtime` 0.57.0, no config keys); guest crate (a thin shell like
  `image-minimax`); reference `crates/south-component-conformance/src/reference_stability_image.rs`, which follows §2–§4
  and the native order of checks (`n`, then the upstream id, `response_format`, `prompt`, the source image, Core with an
  image, `mode`, then each field in part order); the fixture pack `fixtures-image-stability/` (105 cases, with a
  README); the suite test `stability_image_suite` (the pack, eleven named behaviour tests and a mutations module); the
  sandbox parity test `stability_image_sandbox_parity`; the build script `scripts/build-image-stability-component.sh`;
  and the enumerations (`ci.yml` rust-cache, the `release.yml` build / gate ② / package lists, the conformance crate's
  `[[test]]` entry, the `shipped_packages_v1` official list and a manifest test, README and ARCHITECTURE).
- No contract changes. `south-contracts` and `south-provider-api` are unchanged.

Failing first: with the reference written and only three expectations written by hand
(`prepare.sd3-large-turbo-image-url-to-image`, `prepare.core-with-a-source-image`, `response.content-filtered`), the
pack's other 102 expectations were `null` and failed; the three hand-written ones passed. The other expectations were
then taken from the reference and reviewed row by row against §2–§4 and the native source (`server:…/handler/images/
stability.rs`). The mutations module checks that each of five broken components (a retriable 401, a placeholder in
`extras`, an inline image delivered as `url`, an `internal` refusal, `CONTENT_FILTERED` as `unknown`) turns the matching
suite row red.

Versions (§9, Q47): `south-component-conformance` 0.57.0 → 0.58.0, with `fuzz/Cargo.toml` and `fuzz/Cargo.lock` and every
component lockfile following. Every package published with 0.59.0 takes a patch with unchanged behaviour and keeps its
`south_runtime` and its `contracts` (`shipped_packages_v1::i3_4_retires_every_published_059_package_identity`):

| Package | 0.59.0 | Now |
|---|---|---|
| `provider-openai-compatible` | 2.6.2 | 2.6.3 |
| `provider-anthropic`, `provider-anthropic-bedrock-invoke`, `provider-bedrock-converse`, `provider-bedrock-converse-bearer` | 1.1.2 | 1.1.3 |
| `provider-gemini`, `provider-openai-responses` | 1.3.2 | 1.3.3 |
| `task-kling` / `task-kling-v2` | 1.0.19 / 0.32.17 | 1.0.20 / 0.32.18 |
| `task-minimax-v2`, `task-bailian-v2` | 0.31.16 | 0.31.17 |
| `task-xai-v2`, `task-veo-v2`, `task-wan-image-v2`, `task-gmi-image-v2` | 0.35.16 | 0.35.17 |
| `task-byteplus-v2` | 0.36.16 | 0.36.17 |
| `embeddings-openai-compatible` / `embeddings-gemini` / `embeddings-vertex` | 1.0.10 / 1.1.7 / 1.0.9 | 1.0.11 / 1.1.8 / 1.0.10 |
| `image-azure` / `image-minimax` / `image-qwen` / `image-gemini` | 1.0.6 / 1.0.1 / 1.0.1 / 1.0.0 | 1.0.7 / 1.0.2 / 1.0.2 / 1.0.1 |
| `image-stability` | — | 1.0.0 |

The I3-2 retirement test no longer pins I3-2's identities. `image-gemini`'s test now pins its `south_runtime` as the
literal 0.59.0 (it was published with that runtime) instead of the tree's release, so the next release bump does not
turn it red. The workspace version and tags are unchanged; that is the release step.

Implementation interpretations (where the record is silent):

1. **`provider_message`** of a non-2xx is `"{name}: {errors joined with "; "}"`, or `name` alone, or the joined errors
   alone, capped at 256 characters (§4 says "joined" without a separator). `content_moderation` maps to
   `content_policy` on any non-2xx status, not only 403.
2. **Elided text fields** other than `prompt` and `negative_prompt` (`mode`, `output_format`, `style_preset`,
   `aspect_ratio`, `size`) are refused with `image-azure`'s generic "'{field}' is longer than the gateway accepts".
   Natively each would fail its type or word check with a different text; both are 400 before dispatch.
3. **Echoed values**: a refusal text that echoes a request value (as native) names the value's type instead when it
   holds an elided string ("a string longer than the gateway accepts", "an array", "an object"), so a placeholder never
   reaches a message. A source given as an array is named "an array" in the shape refusal, following I3-2's decision 2.
4. **The 10 MiB refusal text** reads "image is more than 10485760 bytes after base64 decode; maximum is 10485760
   bytes", since the exact decoded size cannot be known from the payload length (decision 2).
5. **The non-`data:` preview** is taken from the placeholder's 64-byte head (native: the URL's first 80 characters), so
   a URL longer than 64 bytes is echoed shorter. As in I3-2 decision 1, a data URL whose `;base64,` marker lies past
   the head is refused with the native "must use base64 encoding" text.
6. **A source string the host did not elide** (all four source paths are declared, so a conforming host always elides
   them) is refused `internal`: a multipart file part must name a blob, and there is no inline form for it.
7. **An undeclared upstream id** reaching `prepare` is refused with the native text as `invalid_request`;
   `model-capabilities` declares only the configured models among the five ids, so the host refuses such an id first
   (P4).
8. **"A request-extras row on an immutable name"** (§8): the gate ② pack cannot express operator request extras, so it
   is covered by every prepared expectation carrying `immutable_body_paths` `["model", "aspect_ratio",
   "output_format"]` and by the named test `the_immutable_parts_are_the_native_owned_fields`.

Differences from native the record does not list: none found beyond items 2 to 6 above. One is noted for the host's
dual run: a number such as `strength: 1.0` is sent as `1.0` (as `serde_json` renders a parsed float), which is also
what the native `v.to_string()` sends.

## Revision note

- 2026-10-13: first draft (proposed).
- 2026-10-13: accepted; SI-Q4 ruled by lv, the other questions taken as recommended under the pre-approval.
- 2026-10-13: implemented on branch `image-i3-4` (Implementation).
- 2026-10-13: §7 P2 corrected after the host dual run (`server:gateway/tests/component_image_dual_run_i34_i35.rs`):
  the host answers 502 only for failures found inside a 2xx body (IM-Q6); a parked non-2xx keeps the upstream status,
  as native does and as the I3-1 dual run pins.
