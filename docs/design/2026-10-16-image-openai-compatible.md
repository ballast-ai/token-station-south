# `image-openai-compatible`: the host's OpenAI-compatible image fallback as a package (D9 reconsidered)

Status: accepted under lv's 2026-10-16 ruling (below) and lv's 2026-10-15 directive (migrate by the host's native code,
speed first; where native behaviour is doubtful, reproduce it and register the doubt as **unverified**; do not wait for
real-key captures, do not query production). lv's 2026-10-13 pre-approvals for image packages apply: a difference that
follows a precedent is accepted (§5), and every open question carries a recommended default, which the implementation
implements (§8).

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
  pending its build). Until it ships, `glm-coding` image rows stay on the native arm.

Corrected on 2026-10-16 from the host's read-back: the first version put `azure-openai-dated` in step 2 behind a
`query_parameters` declaration it does not need.

Whether Z.ai's Coding Plan endpoint serves image generation at all is **unverified**; native forwards there, so the
family reproduces it.
