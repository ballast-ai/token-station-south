# `image-openai-compatible`: the host's OpenAI-compatible image fallback as a package (D9 reconsidered)

Status: accepted under lv's 2026-10-16 ruling (below) and lv's 2026-10-15 directive (migrate by the host's native code,
speed first; where native behaviour is doubtful, reproduce it and register the doubt as **unverified**; do not wait for
real-key captures, do not query production). lv's 2026-10-13 pre-approvals for image packages apply: a difference that
follows a precedent is accepted (§5), and every open question carries a recommended default, which the implementation
implements (§8). **Step 1 implemented** (unreleased, 0.70.0 cycle): `image-openai-compatible` 1.0.0 with the
`openai-compatible`, `azure-openai-v1` and `azure-openai-dated` families; what landed, the interpretations and the
differences added while implementing are in §10. **Step 2 implemented** (unreleased, same cycle): the image world admits
the manifest `user_agent` (`south-provider-api` 0.60.0) and the same unpublished 1.0.0 gains `glm-coding`; see §11.

Date: 2026-10-16

Baseline: south `main` = `116df31` (release 0.69.0). Host references carry a `server:` prefix and refer to
token-station-server `origin/dev-v2` at `0f9eef6d`; `server:…/` abbreviates `server:gateway/src/modules/inference/`.
The host's S7 plan is read at `origin/s7-3-glm-kimi` = `e7531c4d`. Statements marked **unverified** are conclusions
from reading code and comments, not from running anything against an upstream or capturing traffic.

Predecessors: [`2026-09-30-image-world.md`](2026-09-30-image-world.md) ("the record"): §13 row "OpenAI-compatible
fallback (DI4)", Q12 (ruled 2026-10-08: one OpenAI-compatible component, differences as words), §9.1 (metering facts),
§10.2 (verify-then-settle). Sibling image records, treated as precedent: `image-azure` (P22 C1–C8 and its dual run C7;
reference `reference_azure_image.rs`), [`2026-10-12-image-i3-1-minimax-and-qwen.md`](2026-10-12-image-i3-1-minimax-and-qwen.md)
("I3-1"), [`2026-10-13-image-i3-5-ideogram.md`](2026-10-13-image-i3-5-ideogram.md) ("I3-5"),
[`2026-10-15-image-i3-7-xai.md`](2026-10-15-image-i3-7-xai.md) ("I3-7"). URL rule precedent: `speech-openai-compatible`
(`reference_openai_compatible_speech.rs:107-113`) and `embeddings-openai-compatible`.

## 1. Why

- **2026-10-08** (record DI4 / Q12): the fallback was to move into one OpenAI-compatible component; I3-1 left it out
  "pending the host's decision D9".
- **2026-10-13** (D9): lv ruled "keep the fallback in the host, pending; reconsider later".
- **2026-10-16** (D9 reconsidered): the host's S7 step S7-3 found that `glm`, `byteplus` and `deepinfra` cannot be
  deleted as provider types because their image models are served by the fallback (the host's S7 variant-retirement
  plan dated 2026-10-15 under `server:docs/product-review-v2/plans/`, §8, step S7-3, at `e7531c4d`). Once those rows become `component` rows, an image model
  without an image route is refused 400 before admission (`server:…/handler/images/handlers.rs:92-104`,
  `admit_component_row`). lv ruled: the fallback moves to a south package that reproduces the native arm; after it ships,
  the host deletes the arm and the `Glm` / `Byteplus` / `Deepinfra` variants. Speech's fallback is already covered by
  `speech-openai-compatible`.

## 2. Native behaviour (`server` `0f9eef6d`)

### 2.1 Who reaches the arm

`/v1/images/generations` (`handlers.rs:22`): after the component-route gate (`:157-186`) and the dedicated arms
(Vertex, Gemini, Bailian, MiniMax, Azure Foundry, Ideogram, Stability, Reve, `:355-636`), **every other provider type**
falls through to the fallback (`:637-737`). `images/observe.rs:94-117` lists them: `openai` (including rows named
`xai`), `azure_openai`, `deepinfra`, `byteplus` (Seedream; its capability tier `capabilities.rs:1452-1507`), `glm`,
`glm_coding`, and any type without an image surface that an operator gave an image model.
`/v1/images/edits` multipart (`handlers.rs:885`): the same types, except those refused by `edits_unsupported_reason`
(`:805-862`: Bailian, MiniMax, GMI, Ideogram, Stability, Vertex, and a row named `xai`), and except Gemini and Reve
(own arms, `:1155-1225`), reach a generic multipart pass-through (`:1227-1760`). JSON `/v1/images/edits` is xAI-only
(`:1849`); every other row gets 400. The held path (`Prefer: respond-async`) is wired for generation on `openai`,
`azure_openai`, `deepinfra`, `byteplus`, `glm`, `glm_coding` (`durable.rs:500`, `:591-605`), not for edits.

### 2.2 URL and auth (`engine/upstream.rs`)

| Type | Generation URL | Edit URL | Auth |
|---|---|---|---|
| `glm` | `{endpoint}/images/generations` (`/v1` stripped, `:436-439`); default endpoint `https://api.z.ai/api/paas/v4` (`infra/config/providers.rs:723`) | `{endpoint}/images/edits` | `Bearer` `oauth_token`, else `api_key` (`:120-123`, `:229-244`) |
| `byteplus` | `{endpoint}/api/v3/images/generations` (`:474-479`); default `https://ark.ap-southeast.bytepluses.com` (`providers.rs:728`) | `{endpoint}/api/v3/images/edits` | `Bearer` `api_key` (`:138-142`) |
| `deepinfra` | `{endpoint}/v1/openai/images/generations` (`:583-590`); default `https://api.deepinfra.com` (`providers.rs:729`) | `{endpoint}/v1/openai/images/edits` | `Bearer` `api_key` (`:66-71`) |
| `openai` (default arm) | `{endpoint}/v1/images/generations` (`:594`) | `{endpoint}/v1/images/edits` | `Bearer` `api_key` |
| `azure_openai` | `{endpoint}/openai/v1/…` or `{endpoint}/openai/deployments/{upstream}/…?api-version=` (`:548-581`) | same | `api-key` header |
| `glm_coding` | `{endpoint}/api/coding/paas/v4/images/generations` (`:440-451`) | same root | `Bearer` + `User-Agent: opencode/1.15.6` (`:153-160`) |

`{endpoint}` has trailing `/` trimmed (`:431`). A credential with no usable secret is 400 in the prep step, refunded
(`openai_compat.rs:74-81`; the refund boundary is pinned at `:300-325`). The optional south binary / multipart arms (`south_binary.rs:163-240`,
`south_multipart_plan`) send the same wire and are off by default (kill switch); they do not change behaviour.

### 2.3 Generation request

- The host parses `n` strictly before anything else: absent / `null` → 1, an integer or integral float, 1..=10, else
  400 (`handlers.rs:133-150`). The capability-tier precheck runs on the raw body before admission (`:197-199`); for these
  types the generation input reader counts no input images (`precheck.rs:112-113`), so a Seedream catalog cap on
  reference images is not enforced here.
- Body: the client's JSON, with `model` replaced by the row's upstream id and operator request extras applied outside
  the owned set `model, n, size, image_size, quality, resolution, response_format` (`openai_compat.rs:48-57`,
  `execute.rs:32-40`). Every other field (`prompt`, `quality`, `style`, `image`, `watermark`, …) is forwarded unchanged.
  `content-type: application/json`; timeout `upstream_non_streaming_timeout` (`openai_compat.rs:167`).

### 2.4 Pricing, reservation, usage

- **Form**: token-priced iff the catalog row has any of the four image token rates, or `image_price == 0` and
  `output_price_per_million > 0` (`engine/token_counter/media.rs:1114-1122`, one predicate since P22-F1).
- **Reservation** (generation, `execute.rs:149-183`): token form → `calculate_gpt_image_token_cost` of {text = request
  body bytes, image = 0, output = `default_max_output_tokens × n`}; flat → `calculate_image_cost(n, size hint)`.
  Edits reserve the worst case `n = 10` (`handlers.rs:1108-1123`).
- **Size hint** (`media.rs:31-35`): `size`, else `image_size`, as a string (a non-string `size` gives no hint); it only
  matters when the row has `image_price_1k` / `_2k`: long edge from `WxH` / `W*H` / `W×H` or `Nk` (= N×1024), ≤ 1536
  is 1K, else 2K; no match → `image_price`, or the dearest tier when `image_price` is 0 (`media.rs:21-82`).
- **Settlement**: flat → `n × tier price` (the requested `n`, `openai_compat.rs:269`, `:282`; edit `handlers.rs:1715`),
  whatever the response holds. Token → `usage` parsed by `ImageTokenUsage::from_openai_usage`
  (`crates/gateway-provider-protocol/src/usage_types.rs:402-426`): text / image input only from
  `input_tokens_details.{text_tokens,image_tokens}`; cached from `input_tokens_details.cached_tokens`, else top-level
  `cached_tokens`; output `output_tokens`, else `output_tokens_details.{text,image}_tokens` summed; each read with
  `as_i64` (a float, even integral, reads as 0; a negative is kept). No bucket > 0 → 502, parked for manual review with
  evidence (`openai_compat.rs:220-261`; edit `handlers.rs:1656-1708`).

### 2.5 Response and errors

- **2xx**: the upstream bytes are returned unchanged, with the upstream status and `content-type: application/json`
  (`handlers.rs:731-737`, `:1753-1758`). `b64_json` or `url` is whatever the upstream answered (`response_format` is
  forwarded). The body is never inspected on a flat row: an empty `data`, or a non-JSON 2xx, is charged `n`.
- **≥ 400**: `UpstreamError {status, body}` (status passed through); funds by P29: allow-listed status or a
  provably-unsent transport failure releases, anything else parks (`handlers.rs:676-712`; edit `:1485-1508`).
- **Held** (generation only): 202 + task; the dispatch runs in the background (`durable.rs:1041-1105`); success stores
  every `data[]` entry (`b64_json` decoded and sniffed, falling back to `image/png`; `url` fetched through the guarded
  client) and settles `n` (`:1186-1201`, `:1303-1405`); any 4xx before output releases (`:1096-1098`); everything else
  parks.

### 2.6 Multipart edit

Native forwards the client's multipart bytes with the client's boundary, splicing only the `model` value
(`handlers.rs:1301-1314`), then appends or replaces operator extras outside `model, n, size, quality, width, height,
image, mask, response_format` (`:1351-1380`, `execute.rs:74-84`). After admission (refunded on failure): `n` a decimal
integer 1..=10, default 1; `stream` = `true` (any case) or `1` → 400 (`:1227-1285`). Inputs counted for the precheck:
parts named `image` / `image[]` and `mask` (`precheck.rs:143-154`). Billing as §2.4 with the multipart `size` as hint.

## 3. Family mapping

**One family, `openai-compatible`** (the name the provider, embeddings and speech worlds already use for the same wire
shape; OCI-Q2), with the **API-root rule** of `speech-openai-compatible`: the binding's `base_url` is the row endpoint;
if it carries a path, that path is the API root, if it is an origin the root is `v1`. The three URL shapes then come
from data, not from three families or config keys:

| Row (after S7-3 migration to a `component` row) | Endpoint the operator writes | Upstream path |
|---|---|---|
| GLM | `https://api.z.ai/api/paas/v4` (unchanged) | `…/api/paas/v4/images/generations` |
| BytePlus | `https://ark.ap-southeast.bytepluses.com/api/v3` | `…/api/v3/images/generations` |
| DeepInfra | `https://api.deepinfra.com/v1/openai` | `…/v1/openai/images/generations` |
| OpenAI (stays a native type; may route too) | `https://api.openai.com` | `…/v1/images/generations` |

These are exactly the endpoints the S7 plan already prescribes for the same rows' text surfaces (§8 S7-3), so one
`component` row serves text (`provider-openai-compatible`), embeddings, speech and images with one endpoint. Binding:
`family = "openai-compatible"`, `base_url = "row_endpoint"`, auth `bearer` on `provider_api_key`. No config keys, no
`supported_parameters` words are needed (Q12 allowed them; nothing differs). The two Azure OpenAI families join this package in
step 1 and the GLM Coding Plan follows in step 2 (OCI-Q1, §9).

## 4. Mapping to the image contract

- **World** `image-adapter-v1`, contracts `{image: 1, media: 1}`, capabilities `generate`, `edit`, auth arm `bearer`.
- **`model-capabilities`**: declares every id the host passes (as `image-vertex` V7; the fallback serves any id).
  Operations `[generate, edit]`; input roles `image` / `image[]` → `input_image`, `mask` → `mask` (edit only);
  response formats `[url, b64_json]`; metering forms `[tokens, images]`; token buckets `[total_output]` (OCI-Q4);
  tier dimensions none; `repeat` false; request elision paths `/image`, `/image/*` (OCI-Q5).
- **`prepare`, generation**: path `{root}/images/generations`; JSON template = the client body with `model` set to
  `context.upstream_model`, every other node re-emitted unchanged (elided strings as `$south.ref` `as_is`).
  Facts: `operation` generate, `inputs` all 0 (native counts none, §2.3), `requested_outputs` = `n` (absent / `null`
  1; an integral number; anything else refused), `size` from the size hint of §2.4 (`WxH` / `*` / `×` → `{w, h}`;
  `Nk` → `{N×1024, N×1024}`, which gives the host's Q12 tier lookup the same long edge as native; unparseable → none),
  `tier` empty, no candidates, no `bounds`, `metering_forms` = `context.metering_required`.
  `immutable_body_paths` = §2.3's owned set.
- **`prepare`, edit**: multipart to `{root}/images/edits`; every part forwarded in order, file parts `as_is`, `model`'s
  value set to the upstream id (appended if absent); `n` decimal 1..=10 (refused otherwise), `stream` `true` / `1`
  refused; a text part above the 1 MiB elision threshold refused (as `image-azure`). Facts count `image` / `image[]`
  file parts and `mask`; size hint from the `size` part. Immutable part names = §2.6's owned set.
- **`parse-response`**: 2xx JSON object with a non-empty `data` array: each entry's non-empty `b64_json` → `inline`
  base64 artifact `image/png` (OCI-Q6); else a non-empty `url` → `url` artifact by pointer, no media type (host sniffs,
  I3-5 IG-Q2); an entry with neither, a missing / empty `data` or a non-JSON body → `unknown`. Metering:
  `images_reported` null (host counts delivered); `tokens` read from `usage` as §2.4 reads it, plus `total_input` from
  top-level `input_tokens` (OCI-Q3), integral floats accepted, negative or fractional → `unknown` when `tokens` is
  required, ignored otherwise; required and `output_tokens` (or its details) missing → `unknown`.
  Non-2xx: status code by `image_shared::status_code`; message from `error.message`, else `error` (string), else
  `message`, else `detail` (shapes **unverified**); 4xx other than 408 → `rejected`, else `unknown`.
- **`render`**: the I3-7 skeleton: the upstream body with every image field nulled, placeholders
  `{"$south.artifact": {"index": i, "as": "b64_json" | "url"}}` in the form the upstream answered; `created`, `usage`,
  `revised_prompt` and unknown fields pass through; skeleton budget 48 KiB (revised prompts dropped first).
- **Held**: no package work; the host's GatewayHeld path serves `images` and `tokens` forms (`durable.rs:473-483`).

## 5. Differences from native (D#)

Following precedent (accepted under the 2026-10-13 pre-approval):

| # | Difference | Precedent |
|---|---|---|
| D1 | Flat rows bill the **delivered** image count; native bills the requested `n` even when fewer, none, or a non-JSON 2xx came back (money: native overcharges) | I3-7 P1, record §9.1 |
| D2 | A 2xx without images, an entry without an image, or a non-JSON 2xx parks (`unknown`, 502); native passes the bytes through and charges `n` | I3-7 P3, XI-Q4 |
| D3 | Funds on held non-2xx follow the P29 allow-list (native held releases every 4xx, incl. 401) | P22 Q2, I3-1 IM-Q2 |
| D4 | Held `b64_json` storage uses the declared `image/png`; native sniffs (png / jpeg / webp / gif) | I3-1 IM-Q5 |
| D5 | Edit `n` / `stream` refusals and the >1 MiB text-part refusal happen before admission; native refuses after admission (refunded) | Azure C7 admission timing |
| D6 | Edit reservation from the facts' `n`, not native's `n = 10` worst case; token-form reservation per the host's §8 bound | Azure C7 "edit reservation" |
| D7 | Multipart re-encoded with the host's boundary; parts compared decoded | Azure C7 |
| D8 | Client status on success is the executor's 200, not the upstream's 2xx; error bodies are the host's envelope | I3-5 / RV-Q5 (b) |
| D9 | An integral float token count is accepted; native reads it as 0 (under-bill or a false 502) | I3-2 "integral float" |
| D10 | A negative / fractional token count on a token row parks; native kept a negative (under-bill) | `image-azure` header note |

New, registered:

| # | Difference | Kind |
|---|---|---|
| D11 | `usage.input_tokens` without `input_tokens_details` is billed as text input (host fold rule, #184 R1); native bills input as 0 (money: native under-bills token rows) | native defect, fixed (OCI-Q3) |
| D12 | Token rows: a 2xx `usage` with input but no output count parks; native settled any non-zero bucket | partial-report gap, record §9.1 |
| D13 | URL for misconfigured rows: an `openai`-type row whose endpoint has a path (native `{endpoint}/v1/…`) or a GLM row given an origin (native `{endpoint}/images/…`) composes differently under the API-root rule | data, preflight (H4) |
| D14 | GLM credentials holding only `oauth_token` are not read (component reads `api_key`); operators re-enter as `api_key` | S7 plan §8, P24 D1 |
| D15 | Held success that cannot store artifacts: the host executor verifies before settling and parks explicitly; native's error escaped `run_held_advance` and left the task to the orphan sweep (`durable.rs:1048-1057`) | native defect (correctness: funds held until lease expiry); fixed in the host on 2026-10-16 (parks at once) |

Unverified (reproduced, not blocking): **U1** GLM CogView answers `url` only and no `usage`; **U2** BytePlus Seedream's
`usage` is `{generated_images, output_tokens, total_tokens}` with no input count (hence OCI-Q4); **U3** DeepInfra's
default `response_format` is `b64_json`; **U4** none of the three hosts serves `/images/edits` (native forwards and
would get a 4xx, released by P29); **U5** error body shapes (§4); **U6** `image/png` for every `b64_json`.

## 6. Host prerequisites (H#)

- **H1 Re-pin and binding**: re-pin to the release carrying `image-openai-compatible`; one `[south.image_bindings.*]`
  with family `openai-compatible`, `base_url = "row_endpoint"`. No new `ImageBaseUrl` kind (OCI-Q7).
- **H2 Order of the cutover**: the row becomes a `component` row with the §3 endpoint and its image models get
  `component_image` routes in the same step (an unrouted image model on a `component` row is 400 from that moment).
  Routes can also be written first on the native GLM row (its endpoint is already the API root), not on native BytePlus
  or DeepInfra rows (origins).
- **H3 Credentials**: GLM `oauth_token`-only credentials re-entered as `api_key` (D14).
- **H4 Preflight**: `p22_image_cutover_preflight.py` family table gains `openai-compatible`: every enabled image model
  on a `glm` / `byteplus` / `deepinfra` row (and later `component` rows) routed; a routed row whose endpoint is an origin
  on a known BytePlus / DeepInfra host, or whose path ends in `/v1/images…`, FAILs (D13); token-form rows WARN with U2.
  The same table feeds the S7-3 `--stage s7-3` gate.
- **H5 Dual run**: a leg `component_image_dual_run_oci` (own `[[test]]`, ignored by default, both billing forms): a
  native `glm` / `byteplus` / `deepinfra` / `openai` row against a `component` row with the §3 endpoint, same mock and
  credential; compare upstream request (path, `Authorization`, JSON by value; edit parts decoded), northbound body,
  reservation, settlement and failure funds, sync and held; D1–D15 asserted; `unruled_` for off-table differences.
- **H6 Deleting**: after cutover, delete the `Glm` / `Byteplus` / `Deepinfra` variants (S7-3 template). The arm itself
  (`openai_compat.rs`, the generic edit pass-through, the `HeldArm::OpenaiCompat` lists) can go only when `openai` rows'
  image models are routed too and `azure_openai` / `glm_coding` image models are routed (needs OCI-Q1's later families)
  or retired; the name-`xai` branches go with H5-3.

## 7. Fixtures, tests, versioning

- Gate ② pack `fixtures-image-openai-compatible/`, transcribed from §2 (not back-derived): the four URL roots of §3 plus
  the two D13 shapes; body pass-through with `model` replaced, unknown fields and an elided data URL in `image` kept;
  `n` forms; the size-hint table (`WxH`, `*`, `×`, `1k`, `2K`, `image_size` fallback, non-string `size`); responses
  `b64_json` / `url` / mixed / both-in-one-entry / empty `data` / non-JSON; `usage` with details, top-level cached,
  top-level input only (D11), integral float, negative, output missing; error shapes per U5; multipart edit (order,
  `model` replaced and appended, `image[]`, `mask`, `stream`, bad `n`, an oversized text part).
- Suite test `openai_compatible_image_suite` (pack + named tests + mutations: the API-root rule, a dropped `total_input`
  fold, the delivered count, a nulled placeholder), sandbox parity `openai_compatible_image_sandbox_parity`, build
  script `scripts/build-image-openai-compatible-component.sh`, `ci.yml` / `release.yml` rows, `shipped_packages_v1`
  (official list; declares media 1, `bearer`, the runtime below), README and ARCHITECTURE.
- **Versioning**: `image-openai-compatible` 1.0.0. Q47: the 0.70.0 cycle's first PR moves `south-component-conformance`
  0.68.0 → 0.69.0 and patches every published package; if the `kimi-code` family PR opens the cycle first, this one
  joins it without a second bump. `south_runtime` 0.57.0, as `image-xai` and `image-reve` (the newest image packages
  that, like this one, need only media 1, `bearer` and `as_is`); 0.59.0 only if media 2 were needed, which it is not.

## 8. Open questions (defaults implemented)

- **OCI-Q1 Scope of families.** (a) `openai-compatible` only; (b) also `azure-openai-v1` / `azure-openai-dated` (as
  `speech-openai-compatible` 1.1) and `glm-coding`. **Ruled (b)** by lv on 2026-10-16 ("do it all at once"), so the
  host can delete the whole arm (H6). Delivered in two steps inside the 0.70.0 cycle (§9): step 1 is this package with
  `openai-compatible`, `azure-openai-v1` and `azure-openai-dated`; step 2 opens the image world to the manifest
  `user_agent` and adds `glm-coding`.
- **OCI-Q2 Family name.** (a) `openai-compatible` (embeddings / speech precedent); (b) `openai-compatible-image`.
  **Default (a).**
- **OCI-Q3 Top-level `input_tokens` with no breakdown.** (a) report `total_input` (host bills it; fixes a native
  under-bill, lv 2026-10-13 "fix defects on the way"); (b) reproduce native's 0. **Default (a)** (D11).
- **OCI-Q4 Required token buckets.** (a) `[total_output]`, input buckets reported when present (U2: Seedream reports no
  input); (b) `[total_input, total_output]` as `image-azure`, parking every Seedream token row. **Default (a).**
- **OCI-Q5 Request elision paths.** (a) `/image`, `/image/*` (keeps reference data URLs out of the sandbox); (b) none,
  threshold only. **Default (a).** Either way the bytes pass `as_is`.
- **OCI-Q6 `b64_json` media type.** (a) `image/png` (OpenAI's default; native held falls back to it); (b) read
  `output_format` like `image-azure`. **Default (a)**, plus (b)'s `output_format` reading when present (cheap, closer to
  the bytes).
- **OCI-Q7 Transitional `openai_compatible_path` for image bindings** (as embeddings / speech), so routes can be written
  on native BytePlus / DeepInfra rows before the row flip. **Default no**: S7-3 flips the rows anyway (H2).
- **OCI-Q8 Edits.** Declare `edit` for every id although U4 suggests these hosts have no edit endpoint. **Default yes**
  (native forwards; a 4xx is released by P29).

## 9. Step 2: Azure OpenAI and the GLM Coding Plan (OCI-Q1 (b))

**Implemented**: the Azure families in step 1 (§10), `glm-coding` and the image world's `user_agent` in step 2 (§11).

The image world admits none of the B7a instance declarations (`InstanceDeclarationsV1::None`,
`crates/south-provider-api/src/manifest.rs:159-176`), and the host's image sandbox parses descriptors with
`parse_prepared_image_call_v1`, which has no declared table (speech has a `_with_declared` form). What each remaining
family needs:

| Family | Native URL (fallback arm) | Needs |
|---|---|---|
| `azure-openai-v1` | `{endpoint}/openai/v1/images/{generations,edits}`, `api-key` | nothing new (`header_secret`) |
| `azure-openai-dated` | `{endpoint}/openai/deployments/{deployment}/images/…?api-version=…`, `api-key` | nothing new: `api-version` is the contract's sanctioned `QueryParameterV1::ApiVersion`, which a media descriptor carries without any declaration (`crates/south-contracts/src/media/descriptor.rs:208-215`), as `speech-openai-compatible` does on media 1 |
| `glm-coding` | `{endpoint}/api/coding/paas/v4/images/…`, Bearer, `User-Agent: opencode/1.15.6` | `user_agent`, today admitted only by the provider world |

So:
- **Step 1** carries `openai-compatible`, `azure-openai-v1` and `azure-openai-dated` (the two Azure families mirror
  `speech-openai-compatible` 1.1: `header_secret` `api-key`, the dated family's `api_version` config key defaulting to
  `2024-10-21`).
- **Step 2** widens the image world to admit the manifest `user_agent` (a `south-provider-api` minor; the package then
  declares a `south_runtime` that admits it, set in the release commit) and adds `glm-coding`, mirroring
  `provider-openai-compatible`'s `glm-coding`. `query_parameters` stays unadmitted: nothing here needs it.
- **Host prerequisite H7**: the host's image executor sends the manifest user-agent (written by the host on 2026-10-16,
  pending its build; exact requirements in §11). Until it ships, `glm-coding` image rows stay on the native arm.

Corrected on 2026-10-16 from the host's read-back: the first version put `azure-openai-dated` in step 2 behind a
`query_parameters` declaration it does not need.

Whether Z.ai's Coding Plan endpoint serves image generation at all is **unverified**; native forwards there, so the
family reproduces it.

## 10. Implementation (step 1)

**What landed.** The reference `crates/south-component-conformance/src/reference_openai_compatible_image.rs`, the guest
`components/image-openai-compatible/` (1.0.0; contracts `{image: 1, media: 1}`, `south_runtime` 0.57.0), the gate ②
pack `fixtures-image-openai-compatible/`, the suite test `tests/openai_compatible_image_suite.rs` (named behaviours and
a mutations module), the sandbox parity test `tests/openai_compatible_image_sandbox_parity.rs`, the build script
`scripts/build-image-openai-compatible-component.sh`, the `release.yml` rows (build, gate ② report, package; `ci.yml`
discovers build scripts since #244), the official list and an identity test in `tests/shipped_packages_v1.rs`, README and
ARCHITECTURE. The package joins the 0.70.0 cycle's Q47 round (the `kimi-code` family opened it); no other version moves.
The four Q47 round tests taken before this package compare against the official list less the packages no release up
to 0.69.0 published (`UNPUBLISHED_BY_069`), since a round cannot name a package that did not exist.

**Runtime.** 0.57.0, as §7 planned. Every declaration was admitted by the image world as it shipped in 0.52.0: both
auth arms (`bearer`, `header_secret`), the value channel (`config_schema`, as `image-minimax` 0.57.0 declares
`group_id`), and a media descriptor's sanctioned `api-version` query (`QueryParameterV1::ApiVersion`, parsed by 0.57.0's
`MediaRequestDescriptorV1::parse`); the `api_version_date` value syntax and a config key's `default` exist since 0.43.0.
So no image package needs anything newer for these families, and 0.57.0 is the oldest runtime an image package names
(`canonical_ir` 4).

**Interpretations (I#), each pinned by the pack or a named test.**

- **I1 Capabilities.** The contract has no per-operation input roles, so `mask` → `mask` is declared for every model
  (§4 said "edit only"); a generation never counts one (inputs are 0 there, §2.3).
- **I2 Order of refusals.** Configuration first (the family; on `azure-openai-dated` an undated `api_version` is a 500
  configuration error and an empty upstream model a capability refusal), then the host's metering requirement (only
  `tokens` and `images` can be reported; anything else, or none, is a capability refusal), then the request:
  `speech-openai-compatible`'s order. Within a generation only `n` is checked (native checks nothing else); within an
  edit an elided text part first (as `image-azure`), then `n`, then `stream` (native's order).
- **I3 Edit `n`.** The first text part named `n`, trimmed, parsed as a decimal `u32` (so `+3` is 3 and an empty value is
  refused, as native's `str::parse::<u32>`); absent is 1.
- **I4 `model` parts.** Every text part named `model` carries the upstream id (native rewrote only the first; a second
  `model` part would otherwise reach the upstream unchanged). `model` is appended when absent (the host requires one
  for routing anyway, native refused its absence with 400).
- **I5 Input counts.** Parts named `image` / `image[]` and `mask` are counted whatever their kind, as native's
  `multipart_edit_inputs` counts them; more than 64 of one role (the contract's per-role maximum) is refused, where
  native forwarded any number.
- **I6 Size fact.** `size`, else `image_size`, as a string (a present non-string or `null` `size` gives none, as native);
  `Nk` → `{N×1024, N×1024}`, `WxH` / `W*H` / `W×H` (any case, spaces trimmed) as written. A dimension of 0 or above the
  contract's 65,536 gives no `size` fact (the host then prices with `image_price`); native read `0x0` as a 1K long edge.
- **I7 Token buckets.** `total_output` is `output_tokens`, else the sum of `output_tokens_details.text_tokens` and
  `image_tokens` when either is present; `text_output` / `image_output` are never reported (native rolls output up).
  `cached_input` is `input_tokens_details.cached_tokens`, else the top-level `cached_tokens`. On an `images` row a
  malformed count drops the whole `tokens` fact (reported `null`); a well-formed usage is reported as evidence.
- **I8 Artifacts.** An empty `b64_json` string counts as absent, so an entry with an empty `b64_json` and a `url` is a
  `url` artifact; every `b64_json` of the upstream body is `null` in the render skeleton (the response view elides
  them), so an unused empty one renders as `null`.
- **I9 Error message.** `error.message`, else `error` as a string, else `message`, else `detail` (U5); the code follows
  the status only (no content-policy mapping, which native does not have either).

**Differences added while implementing** (beside D1 to D15):

| # | Difference | Kind |
|---|---|---|
| D16 | `azure-openai-dated`: the deployment is one escaped path segment (native sent it raw; a valid Azure deployment name is unchanged by the escape) | `speech-openai-compatible` precedent (speech Azure record D5) |
| D17 | `azure-openai-dated`: an empty upstream model is refused before admission (native would send `/openai/deployments//images/…`) | speech Azure record D4 |
| D18 | Every `model` text part of an edit carries the upstream id (I4); native rewrote only the first | correctness: what is billed is what is sent |
| D19 | An edit with more than 64 images (or masks) is refused before admission (I5); native forwarded it | contract maximum |

**Host prerequisites added.** H1 also covers the two Azure families: an `azure_openai` row whose effective version is
`v1` (any case) binds `azure-openai-v1` with `base_url = {endpoint}/openai/v1`, a dated row binds `azure-openai-dated`
with the row endpoint and `declared.api_version` = the row's version (omitted when unset), exactly as speech Azure
record H2 / H4 bind the same rows; a model whose `azure_api_version` differs from its row's moves to a row of its own
first (T9 H3). `azure_foundry` rows keep their own image arm (`image-azure`); they never reached the fallback.

**Native read against the record** (server `0f9eef6d`): no contradiction found in §2. Two nuances the record left
implicit: native's multipart edit refuses a body without a `model` part (400, before admission) and parses `n` with
`str::parse::<u32>` after trimming (so `+3` passes and an empty value is refused); both are reproduced or moot (I3, I4).

**Fixture reconciliation.** The pack's expectations were written from this record by an independent transcription
before the reference ran. On the first run the reference passed all 109 cases with no difference, so no row was
changed and no record correction was needed; the native arm was the authority for each row's input (§2) and the
record for each difference (§5, D16 to D19 above).

**Gates.** The suite test (15 tests, the pack and the mutations module), `shipped_packages_v1` (78) and the sandbox
parity test (5, gate ② over the built `component.wasm`, byte for byte against the reference) pass, with the README's
local verification list and the release-only checks. Source mutations of the reference, each restored from a backup,
went red: the API-root rule, the D11 fold, the D9 integral-float reading, the delivered count and the `api-version`
query (the PR lists them).

## 11. Implementation (step 2)

**Native read** (server `origin/dev-v2` = `daabf5f5`, `engine/upstream.rs`; unchanged from `0f9eef6d` on these lines):
`build_upstream_url`'s `GlmCoding` arm sends every path other than `/v1/messages` to
`{endpoint}/api/coding/paas/v4{path without its /v1}` with `{endpoint}` trailing-`/`-trimmed, so the image fallback's
`/v1/images/generations` and `/v1/images/edits` become `…/api/coding/paas/v4/images/{generations,edits}` whatever the
endpoint's own path; the type preset fills `https://api.z.ai` (`infra/config/providers.rs`, `GlmCoding`).
`apply_provider_auth`'s `GlmCoding` arm calls `require_api_key` (**`api_key` only**: unlike `Glm`, which prefers
`oauth_token` through `require_oauth_or_api_key`, so D14 does not apply to this family) and adds
`User-Agent: GLM_CODING_IMPERSONATION_UA` = `opencode/1.15.6`. Everything else (body, `n`, size hint, metering,
artifacts, held path) is the shared fallback arm of §2, so the family reuses `openai-compatible`'s behaviour. The image
handlers have no coding-plan quota alert (no `is_coding_plan` / subscription-quota read under `handler/images`), so the
image family needs no `subscription_quota_families` entry (the image world does not admit one either).

**Contract change: the image world admits `user_agent`.** `south-provider-api` 0.59.0 → **0.60.0** (minor):

- `InstanceDeclarationsV1` gains the value **`UserAgent`** (`user_agent` only), and `IMAGE_WORLD_SCHEMA` takes it; the
  other worlds keep their values (provider `All`, speech `QueryParameters`, task / task-v2 / embeddings `None`), so
  each refuses exactly what it refused before. `admits_quota_headers_and_user_agent` is split into
  `admits_quota_headers` (`All` only) and `admits_user_agent` (`UserAgent` or `All`); gate ①'s refusal order
  (`query_parameters`, `quota_headers`, `user_agent`) and error (`InstanceIsAProviderWorldDeclaration`) are unchanged.
  A fourth enum value, rather than three independent flags, keeps every world's posture one value that
  `every_world_states_its_host_import_and_admitted_declarations` pins; no world admits `quota_headers` without the other
  two, so the four values cover every posture in use.
- **Rules**: the image world's `user_agent` is checked by the same `validate_user_agents` as the provider world's: the
  family must be one the manifest declares, and the value must satisfy `is_user_agent_value` (1 to 256 bytes of
  printable ASCII, no leading or trailing space), the controlled user-agent grammar of
  `DeclaredUserAgentV1::from_manifest_value`. `quota_headers` and `query_parameters` stay refused in the image world.
- **Does it break hosts?** `InstanceDeclarationsV1` is **not** `#[non_exhaustive]`, so a host that matches it
  exhaustively must add the `UserAgent` arm on re-pin, and a host that calls the removed
  `admits_quota_headers_and_user_agent` must call the two new functions. token-station-server names neither outside
  comments (checked at `0f9eef6d`, at `daabf5f5` and at its H7 commit `8045ee94` on `host-fixes-069`), so its re-pin
  is not broken by the change. `ComponentManifestV1` and `ManifestErrorV1` are unchanged.
- **No other south change.** `user-agent` is a reserved header (`south-contracts` `RESERVED_HEADERS`), so a media
  descriptor can never carry it, and the image runtime, `parse_prepared_image_call_v1`, `SandboxedImageComponentV1` and
  the prepared-call types need no field or slot: the host reads the value from the manifest
  (`ComponentManifestV1::user_agent_for(family)`) exactly as the text path does (`DeclaredUserAgentV1`). The speech
  world's `_with_declared` descriptor form exists for declared **query parameters**, which the image world still does
  not admit. No WIT, IR, runtime ABI or `contracts.*` number moves.
- Gate ① tests: `provider_api_v2.rs` (`every_world_states_its_host_import_and_admitted_declarations` pins every
  world's value and each value against each declaration; `image_manifest_refuses_provider_only_declarations` no longer
  refuses `user_agent`; new `the_image_world_admits_user_agent_under_the_provider_rules`: a declared family is
  admitted, an undeclared family and values outside the grammar are refused, the 256-byte bound holds, `quota_headers`
  and `query_parameters` are still refused beside an admitted user-agent, and the task, embeddings and speech worlds
  still refuse `user_agent`) and `instance_declarations_v1.rs` (new `the_image_world_admits_user_agent_alone` over the
  shipped `image-openai-compatible` manifest).

**Package.** `image-openai-compatible` gains the family **`glm-coding`** (the provider world's name, `reference::
glm_coding::FAMILY`): the descriptor path is `api/coding/paas/v4/images/{generations,edits}` (`GLM_CODING_API_ROOT`)
below the binding's `base_url`, whatever its path (native appends the root to the endpoint as is), no query, `bearer`
on the configured slot; body, facts, immutable paths, response and render are `openai-compatible`'s. The manifest
declares `"user_agent": {"glm-coding": "opencode/1.15.6"}`, pinned by `glm_coding_declares_the_provider_world_user_agent`
to `reference::glm_coding::USER_AGENT` and to `provider-openai-compatible`'s manifest, and declared for no other family.
No config key. The wrong-family refusal names four families.

**Package version: stays 1.0.0.** No release published 1.0.0 (the 0.70.0 cycle is open), so the first published
identity simply carries the four families; a 1.1.0 would claim a published 1.0.0 without `glm-coding` that no host could
ever have pinned. The package is in the cycle's Q47 round already (`UNPUBLISHED_BY_069`).

**Runtime.** Every runtime before 0.70.0 parses `user_agent` (the field exists since 0.43.0) but its image world refuses
it at gate ① (`InstanceIsAProviderWorldDeclaration("user_agent")`), so a host on an older south fails closed and never
admits the package while dropping the user-agent. The package must therefore declare `south_runtime` **0.70.0**.
Following the 0.60.0 precedent (`2026-10-13-release-0.60.0.md` §3) and the 0.67.0 one (`provider-anthropic`'s
`immutable_request_headers`, `2026-10-15-immutable-request-headers.md` §5), this branch leaves the manifest at 0.57.0
and the **0.70.0 release commit raises it to 0.70.0**, together with `shipped_packages_v1::declared_runtime_now` (add
`"image-openai-compatible" => "0.70.0"`) and the literal in
`image_openai_compatible_declares_media_1_and_the_runtime_of_the_record`. Until that commit the release-only
declared-runtime check (`scripts/check-declared-runtime.sh`, which loads the package under the 0.57.0 tag's gate ①) is
**expected to refuse** this package; nothing in the regular gates runs it. That an older runtime refuses is read from
code (`validate_instances` with the image world's `None`), not run.

**Versions (Q47).** `south-provider-api` 0.59.0 → 0.60.0, with the requirement in `south-component-conformance`,
`south-provider-conformance`, `south-provider-runtime` and `south-testkit`, and the lockfile entry (name and version) in
every component's `Cargo.lock`, the synthetic-speech guest's and `fuzz/Cargo.lock` (as the 0.67.0 precedent did).
`south-component-conformance` stays 0.69.0 (the cycle's round already moved it; its source changed, which that unreleased
version covers) and `south-contracts` stays 0.56.0 (unchanged). No other package changes identity: they all took the
cycle's patch already, and a lockfile line for an unreleased `south-provider-api` is part of that same unreleased
identity. `compatibility.json` lists no crate versions; ARCHITECTURE's crate-version line now says 0.60.0.

**Fixtures and tests.** Gate ② rows for `glm-coding`, written from native's arms before the reference was extended:
capabilities (two ids), generation on the preset origin, on the origin with a trailing slash, on an endpoint with a path,
without an auth slot, a multipart edit, and a `url`-only response without `usage` (U1). Named suite tests
`glm_coding_posts_below_the_coding_plan_root` (paths for an origin, a trailing slash and a path-bearing endpoint; the
body, facts and owned set equal `openai-compatible`'s; no auth without a slot) and
`glm_coding_declares_the_provider_world_user_agent`; the family joins `the_manifest_declares_the_four_families`,
`every_configured_id_is_declared_on_every_family` and `the_auth_arm_follows_the_family`; a mutation
`GlmCodingWithoutItsRoot` (the family routed as `openai-compatible` with an origin) must fail the pack.

**Fixture reconciliation and gates (phase 2).** The reference matched every new `glm-coding` row on its first run,
so no fixture row was changed and no record correction was needed; the native arm stayed the authority for each row's
input. One existing gate ① test, `component_values_v1::the_image_world_holds_its_values_to_the_provider_rules`, pinned
the old refusal of an image-world `user_agent`; it now pins its admission and the refusal of `quota_headers` (a
behaviour change this step makes on purpose). Gates on the branch: Clippy (all targets, all features, `-D warnings`),
`cargo nextest run --workspace --all-features` (2296 passed, 4 skipped), doc tests, the no-default-features check and
`south-component-conformance` tests (839 passed), the fuzz check, rustdoc, the boundary, language and MSRV scripts, the
script unit tests (83), `cargo deny` and `cargo audit` (workspace and fuzz), `cargo machete`; the suite (17), the
sandbox parity test (5, gate ② over the rebuilt `component.wasm`, byte for byte) and the pack (115 cases). The Q47
check `check_crate_versions.py --tag v0.70.0` reports `south-provider-api` 0.59.0 → 0.60.0 and
`south-component-conformance` 0.68.0 → 0.69.0. `check-declared-runtime.sh --build` refuses exactly one package,
`image-openai-compatible` under 0.57.0 ("user_agent is a provider-world declaration"), as planned above; with the
package's `south_runtime` set to this tree's version (restored afterwards) every package loads. Mutations, each
restored from a backup, went red: in the reference, the Coding Plan root replaced by the API-root rule, `glm-coding`
authenticated with the Azure `api-key`, and the family left unrecognised; in the contract, the image world back to
`None`, the image world admitting all three declarations, and the speech world's value admitting `user_agent`.

**Differences.** None new for `glm-coding` beyond `openai-compatible`'s D1 to D12, D15 and D18, D19 (the shared
behaviour): the URL is native's for every endpoint (no D13 analogue: native and the package both append the fixed
root), the credential read is native's (`api_key` only, no D14 analogue), and the user-agent is native's byte for byte
once the host sends the manifest value. Whether Z.ai's Coding Plan endpoint serves image generation stays **unverified**
(§9).

**Host prerequisites for `glm-coding` (H7 and its neighbours).** In addition to H1 to H6:

- **H7 Controlled user-agent on image calls.** When the host seals an image call for a row whose binding family is
  `glm-coding` (in general: any family for which the package's manifest declares a user-agent), it takes
  `ComponentManifestV1::user_agent_for(family)`, converts it with
  `south_contracts::DeclaredUserAgentV1::from_manifest_value` (the grammar gate ① already checked), and sets the
  controlled user-agent slot of **both** the JSON (generation) and the multipart (edit) calls, on the synchronous and
  the held paths; a family without a declaration sends no slot (the call is unchanged); a value the conversion refuses
  is a 503 before admission, never a silently dropped header. The slot replaces the HTTP client's default
  `user-agent`, and no operator request extra may set `user-agent` on these calls. Written by the host as `8045ee94`
  ("H7", `declared_user_agent_for` in `handler/images/component/mod.rs`, both call shapes) and on the host's `dev-v2` at
  `51013ebc`; by the host's read-back the synchronous and the held paths both go through the one sealing step, so
  both carry the slot. It can be exercised end to end only against a south that admits the declaration (this step),
  i.e. after the re-pin.
- **H1 for `glm-coding`**: re-pin to the release carrying `south-provider-api` 0.60.0 (0.70.0; an older south refuses
  the package at gate ①). One `[south.image_bindings.*]` for family `glm-coding` with `base_url = "row_endpoint"`, on a
  `component` row whose endpoint is native's (`https://api.z.ai`, the same endpoint the text family
  `provider-openai-compatible` `glm-coding` uses, so one row serves both); auth `bearer` on `provider_api_key` filled
  from the credential's `api_key` (native never read `oauth_token` on this type).
- **H4 for `glm-coding`**: the preflight family table gains `glm-coding` (replacing native `glm_coding` image models):
  every enabled image model on a `glm_coding` row routed; the routed row's credentials carry `api_key`; the package's
  `user_agent_for("glm-coding")` equals `opencode/1.15.6` (FAIL otherwise).
- **H5 for `glm-coding`**: the dual run adds a native `glm_coding` row against a `component` row with family
  `glm-coding`, same endpoint and credential: path `…/api/coding/paas/v4/images/{generations,edits}`,
  `Authorization: Bearer`, and **`user-agent: opencode/1.15.6` compared as a literal on both arms**, sync and held, plus
  an edit.
- **H6**: the `glm_coding` image arm's share of the fallback can go once `glm_coding` image models are routed; the arm
  itself still waits for the `openai` and `azure_openai` rows (§6).
