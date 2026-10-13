# Image world step I3-5: `image-ideogram` (Ideogram 4.0)

Status: proposed (2026-10-13). Under lv's pre-approval (2026-10-13), I3-5 may be built directly from the image world
record once this design is reviewed; IG-Q1 to IG-Q3 (§10) are the only new decisions.

Date: 2026-10-13

Baseline: south `origin/main` = `93246d5` (release 0.60.0: I3-4 `image-stability`, D7). Host references carry a
`server:` prefix and refer to token-station-server at `38dcb6d1` (the checkout of branch `h3b`, which contains the
host's generic image executor, its safe fetch executor and the VI-Q2 / SI-Q4 400). `server:…/` abbreviates
`server:gateway/src/modules/inference/`. Statements marked **unverified** are conclusions from reading code, from the
host's own comments about Ideogram's documentation, or from what Ideogram documents, not from running anything or
capturing traffic. Nothing in this record was built or run.

Parent record: [`2026-09-30-image-world.md`](2026-09-30-image-world.md) ("the record"): §7 (allowed artifact
deliveries, multipart immutable names), §9.1 / §9.2 (metering, the Ideogram all-filtered outcome), §10.1 (`url`
artifacts are pointers), §10.2 (verify-then-settle, DI6 = A, written with Ideogram as the example), §10.3 (held),
§11 (safe fetch), §13 (the Ideogram row), §14 item 6 and §19 (I3-5: "needs the safe fetch executor in the host";
acceptance adds three download-failure fixtures). Sibling records, treated as precedent:
[`2026-10-12-image-i3-1-minimax-and-qwen.md`](2026-10-12-image-i3-1-minimax-and-qwen.md) ("I3-1", the shipped `url`
artifact families), [`2026-10-13-image-i3-2-gemini.md`](2026-10-13-image-i3-2-gemini.md) ("I3-2") and
[`2026-10-13-image-i3-4-stability.md`](2026-10-13-image-i3-4-stability.md) ("I3-4", the shipped template for a
multipart descriptor built from a JSON client body).

**Pre-approval (lv, 2026-10-13).** Differences from the native arm that follow an established precedent (I3-1, I3-2,
I3-4, P22 Azure, or a ruling in the record) are accepted without a new question and are listed in §7; only new
decisions are in §10.

## 0. Prerequisites the record named: all exist

§14 item 6 says I3-5 "needs the safe fetch executor (including the new §11 item 3 rows) and §10.2". Each has shipped;
**no contract change is needed for this step**:

| Record names | Where it is now |
|---|---|
| The multipart encoder (§6.7) and a multipart POST with a byte-buffered response (HTTP contract 12) | Shipped and used by `image-stability` 1.0.0 for a generate request with text parts only (I3-4 §0). Text parts carry no `Content-Type` line (`crates/south-contracts/src/media/multipart.rs:336-345`), as `reqwest`'s `Form::text` writes them |
| `ArtifactUrlV1::parse` and `is_forbidden_egress_address` (§11 D8b, including the v4-compatible, NAT64, 6to4 and `fec0::/10` rows) | `crates/south-contracts/src/media/egress.rs`; host suite `south.safe-fetch.v1` in `crates/south-testkit/src/safe_fetch.rs` |
| The host's safe fetch executor | `server:…/engine/safe_fetch.rs` (P22 C6): policy separated from ports, built on the two south functions (`:29`), no proxy and no redirects (`:317-319`), only `accept` sent; gate ③ `south.safe-fetch.v1` driven against the same `safe_fetch` in `server:gateway/tests/south_adoption.rs:2003-2070` |
| `url` → `b64_json` delivery through the safe fetch, verify-then-settle (§10.2) | `server:…/handler/images/component/deliver.rs:1-17`, `fetch_url_artifact` at `:111`; the sync path at `component/mod.rs:517-522`, the held path at `:1186`; total timeout 30 s (`mod.rs:97`), byte limit `MAX_BINARY_RESPONSE_BODY_BYTES` (`:664-669`) |
| Held path for routed models (§10.3) | `server:…/handler/images/durable.rs`, `run_component_held_advance`; artifacts stored only after verification |

Nothing in south needs to change besides the new package, its reference and its fixtures.

## 1. What I3-5 delivers

| Item | Where | Change |
|---|---|---|
| `image-ideogram` 1.0.0 | `components/image-ideogram/`, `crates/south-component-conformance/src/reference_ideogram_image.rs`, `fixtures-image-ideogram/` | Family `ideogram-image` (proposed); `generate` only (§2) |
| Q47 | `south-component-conformance` minor | Every published package takes a patch (§9) |

Image world (`image-adapter-v1`), contracts `{image: 1, media: 1}`, `south_runtime` the oldest runtime that admits these
declarations (expected 0.57.0, as `image-azure`, which already uses `header_secret` `api-key` in this world),
`header_secret` `api-key` on `provider_api_key` (the native `Api-Key` header, `server:…/engine/upstream.rs:202-207`;
header names are case-insensitive, record §4), metering form `images` only, `repeat` 1, response form `json`,
artifacts `url` pointers.

## 2. Scope: the three models the native arm serves, generation only

The native arm (`server:…/handler/images/ideogram.rs`, 609 production lines) serves `/v1/images/generations` only,
through one upstream path, `POST {endpoint}/v1/ideogram-v4/generate` (`ideogram.rs:285-289`; the host's default URL
composition, `server:…/engine/upstream.rs:425-430`, `{base}{api_path}`). The upstream id pins the
`rendering_speed`, which is the price tier (`ideogram.rs:56-73`; a row per speed, `image_price` $0.03 / $0.06 / $0.10,
`server:gateway/KNOWLEDGE.md:2468-2473`, `:2489-2495`):

| Upstream id | `rendering_speed` |
|---|---|
| `ideogram-v4-turbo` | `TURBO` |
| `ideogram-v4` | `DEFAULT` |
| `ideogram-v4-quality` | `QUALITY` |

`FLASH` is excluded natively ("the upstream currently rejects it with a 400", `ideogram.rs:60-61`). The model catalog
already carries the three ids with `n.max` 1 and nothing else (`catalogs/model-catalog.json`, the `ideogram-v4` entry;
the host's built-in twin `server:…/engine/capabilities.rs:1407-1418` declares no size, because the arm snaps any `WxH`
to its own list). `/v1/images/edits` is refused natively for Ideogram with 400 (`server:…/handler/images/handlers.rs:
827-834`: v4 has no edit surface; the v3 Edit / Remix endpoints are not wired); a routed model whose package declares
only `generate` gets the host's 400 too (`server:…/handler/images/component/mod.rs:15`, `:1547`, VI-Q2 / SI-Q4).
Ideogram's v3 endpoints (edit, remix, reframe, replace background, upscale, describe), `json_prompt` and
`enable_copyright_detection` are not served by the native arm and are out of scope. **Recommended (IG-Q1): exactly
these three ids, generation only**, as native.

## 3. Request (`prepare`)

Reproduces `ideogram_prepare` (`ideogram.rs:206-314`), `ideogram_v4_speed` (`:62-73`), `ideogram_map_size`
(`:85-120`) and `build_ideogram_v4_form` (`:125-137`):

- **Descriptor**: `POST v1/ideogram-v4/generate`, auth `{"arm": "header_secret", "header": "api-key", "slot":
  "provider_api_key"}`, body **multipart** with text parts only, no `content-type` (the encoder writes it), no
  `accept` (native sends none).
- **Text parts, in the native order**: `text_prompt` = the client's `prompt` as sent (not trimmed); `rendering_speed`
  from the upstream id (table above); `resolution` only when one is derived from `size` (below). Nothing else: the
  host's pinned test asserts that `model`, `n`, `num_images`, `prompt`, `size` and `response_format` never reach the
  form (`server:gateway/tests/images_ideogram_stability.rs:233-244`, `:296-303`).
- **`resolution`** from `size` (`ideogram.rs:274-283`, `:85-120`): absent or `null` → none; a non-string → 400; the
  string is trimmed; empty or `auto` (any case) → none; an exact member of the 38-entry list
  (`ideogram.rs:15-54`, compared after trimming, case-sensitive) → itself; else `W` `x` `H` split at the **first
  lower-case `x` only** (native does not lower-case; `1024X1024` is refused, unlike Stability), each side trimmed and
  parsed as a positive `u32` (Rust's `u32` parser accepts a leading `+`) → the member nearest by
  `|ln(rw/rh) − ln(w/h)|`, the earlier member on a tie (so a non-member `1:1` such as `1000x1000` becomes `2048x2048`
  and `1024x1024`, itself a member, passes through, `server:…/handler/images/tests/ideogram.rs:32-47`); anything else →
  400 with the native text, echoing the trimmed value. The distance is computed in `f64` with `ln`, as
  `image-stability` does (`reference_stability_image.rs:402-415`); the sandbox parity test pins agreement on every
  pack case.
- **Refusals before dispatch** (`invalid_request`, native texts, in the native order after the upstream id):
  `prompt` missing ("Missing 'prompt' field"), not a string ("'prompt' must be a string (got …)", `null` included),
  blank after trimming ("'prompt' must not be empty") (`:226-236`); `n` other than 1 ("Ideogram v4 generates exactly
  one image per request; 'n' must be 1 (got N)", `:241-247`); `response_format` (`:249-272`): absent, `null`, `"url"`,
  `"b64_json"`, an object whose `type` is `"url"`, `"b64_json"`, absent or not a string (→ `url`) are accepted, an
  object with any other string `type` and any other value (another string, a number, an array) are refused with the
  native texts; `size` as above; an elided `prompt`, `response_format` or `size` (above 1 MiB) is refused with
  `image-azure`'s "'{field}' is longer than the gateway accepts" (a text part has no blob form, `multipart.rs:122-128`).
- **Dropped**: every other field (`image_url`, `quality`, `style`, `seed`, `negative_prompt`, `user`, `stream`, …), as
  native. No request elision paths are declared: the native arm reads no input image.
- **Immutable part names**: `text_prompt`, `rendering_speed`, `resolution`, `num_images` (= `IDEOGRAM_IMAGE_OWNED`,
  `server:…/handler/images/execute.rs:68-70`; the host's save-time rule marks the arm `form`,
  `server:…/handler/images.rs:59-60`), with the multipart meaning of record §7 (P22 Q3); `num_images` is a part the
  component never writes, so immutable means "stays absent".
- **Facts**: `operation` `generate`; no `inputs`; `requested_outputs` 1; `size` per IG-Q3; no tier (the speed is fixed
  by the row, record §8); `metering_forms` `[images]`; no `bounds`; `repeat` 1; `response_body_form` `json`; no response
  elision paths (Ideogram answers with URLs, and the fallback threshold covers any oversized string).
- **`state`**: `{"format": "url" | "b64_json"}`, the delivery the client asked for, read by `render`.

## 4. Response (`parse-response`) and render

Reproduces `ideogram_dispatch` and `ideogram_image_result_to_openai` (`ideogram.rs:321-389`, `:146-170`):

- **2xx**: JSON `{"created": "<ISO 8601>", "data": [{"url", "prompt", "resolution", "is_image_safe", "seed"}]}`
  (`KNOWLEDGE.md:2479-2483`; shape **unverified** against a capture). For each entry of `data`, in order: skip it when
  `is_image_safe` is `false`; skip it when `url` is missing, not a string or empty (Ideogram's per-image unsafe
  signal, `ideogram.rs:139-145`); otherwise it becomes a `url` artifact `{"pointer": "/data/<i>/url", "media_type":
  IG-Q2}`. At least one artifact → `succeeded`, `metering` `images` with `images_reported` `null` (Ideogram reports no
  count, and `data`'s length includes filtered entries; the host settles the delivered count, as native
  `ideogram.rs:365-388`). `data` a non-empty array and every entry skipped → **`rejected`**, envelope
  `content_policy`, `http_status` 422, the native message "Ideogram filtered all generated images as unsafe; nothing
  was billed" (record §9.2: "the upstream says nothing was produced"). `data` missing, not an array or empty →
  `unknown` (native 502 "returned no images", `:369-375`). A 2xx that is not a JSON object → `unknown` (native 502
  parse failure, `:354-358`).
- **`extras`**: `{"revised_prompts": [...]}`, one entry per artifact: the entry's `prompt` when it is a string,
  else `null` (Ideogram rewrites prompts; native surfaces it as `revised_prompt`, `ideogram.rs:162-163`). A prompt
  the host elided (above 1 MiB) becomes `null`: a response-side placeholder cannot reach the template. `image-azure`
  carries its render input in `extras` the same way (`reference_azure_image.rs:558-571`). `seed` and `resolution`
  are not reported (native drops them).
- **Non-2xx**: error envelope with code by status as `image-azure` (`image_shared::status_code`), `provider_message`
  from the body's `error` / `message` / `detail` string or `error.message`, capped at 256 characters (Ideogram's error
  shape **unverified**); outcome `rejected` for a 4xx other than 408, else `unknown` (`image_shared::non_2xx`). A 422
  is Ideogram's unsafe-prompt answer (`KNOWLEDGE.md:2484-2488`, **unverified**) but is also a validation status, so it
  keeps the status table's `invalid_request`.
- **Render**: `{"created": <host>, "data": [{<"url" | "b64_json">: {"$south.artifact": {"index": i, "as": <state
  format>}}, "revised_prompt": <extras>}]}` — the native envelope (`ideogram.rs:163-169`, `:596`). With `url` the host
  passes the upstream URL through (validated by `ArtifactUrlV1`, not fetched); with `b64_json` the host fetches it
  through the safe fetch before settling (record §7 table, §10.2). `model-capabilities` declares
  `response_formats` `[url, b64_json]`.

## 5. What the host must do

1. **Nothing contract-side.** A host that admits `image-azure` (header secret `api-key`, media 1, HTTP contract 12)
   admits this package. Re-pin to the release that carries `image-ideogram`.
2. **Binding**: family `ideogram-image`, `base_url` = the provider row's endpoint (`https://api.ideogram.ai` by
   default, `server:gateway/src/infra/config/providers.rs:740`), slot `provider_api_key`.
3. **Metering form `images`**: native bills `calculate_image_cost(model, delivered, image_size_hint(body))`
   (`ideogram.rs:384`; flat `image_price` per speed row). The executor's single pricing-form predicate
   (`server:…/engine/token_counter/media.rs:1114-1134`) yields `images` for these rows; a token-priced Ideogram row is
   refused 503 before admission (native: 400 at `handlers.rs:550-561` when `image_price <= 0`, otherwise it bills per
   image). Unpriced rows: settled on the host by its enable gate (I3-4 §5 item 4, release 0.60.0 §3): a model with no image
   price cannot be enabled (A10, `server:gateway/src/modules/catalog/service.rs` `admin_set_model_enabled`), so a routed
   unpriced row never reaches the component path.
4. **A test seam for the `b64_json` dual run and the record's three fixtures.** The executor hard-wires
   `SystemSafeFetchPorts` (`component/mod.rs:522`, `:1186`) and the development escape hatch
   `asset_fetch_unsafe_allow_private_destinations` does not reach it (`safe_fetch.rs:19-21`), so a local mock upstream
   (127.0.0.1, an arbitrary port) can never be fetched through the real handler. The native side of the dual run uses
   that hatch (`images_ideogram_stability.rs:266-267`). The deliver-level tests cover timeout, empty body and forbidden
   destinations (`deliver.rs:743-775`), but record §10.2 asks for the **ledger state** through the handler (download
   404 / timeout / empty body → `delivery_unknown`, no `finalize`, 502). The host needs a test-only way to inject
   ports into the executor (for example behind a feature gated like `credential-recipe-test-hooks`); this is the first
   family whose delivery depends on the second hop (MiniMax's base64 comes from the upstream, Qwen answers only `url`).
5. **Media type of stored `url` artifacts** (IG-Q2): the held path stores an undeclared `url` artifact as
   `application/octet-stream` (`component/mod.rs:450-457`, `:551`); the native held path stores the CDN's
   `content-type`, falling back to `image/png` (`durable.rs:1360-1365`).
6. **Cutover preflight**: `scripts/p22_image_cutover_preflight.py` checks families against a table (today
   `azure-mai`, `minimax-image`, `bailian-qwen-image`; a family outside it is a WARN). It needs an `ideogram-image` row
   (package name, the native `ideogram` row type it replaces, the C10 real-key capture items: a `b64_json` download and
   an unsafe-prompt answer).
7. **Dual run and cutover**: as I3-1 (decoded part lists compared, record §14); sync and held paths switch together; the
   native held arm (`durable.rs:425`, `:488`, `:533`, `:649`) is deleted only after in-flight held rows are 0. Fixture
   sources: `server:…/handler/images/tests/ideogram.rs`, `server:gateway/tests/images_ideogram_stability.rs:135-370`,
   `server:gateway/tests/byok_media_ownership.rs:1026-1135` (the user's key in `Api-Key`, never Bearer), and the
   native held wiring test `durable.rs:1440-1450`.

## 6. Host defects found on the way (read, not run)

1. **The sync `b64_json` path charges for an image it could not deliver**: `finalize` runs before the download
   (`ideogram.rs:469-487`), so a download failure answers 502 and the charge stands (pinned as today's behaviour by
   `images_ideogram_stability.rs:307-338`). Known since the record (§10.2) and fixed by DI6 = A on the component path.
2. **The held path parks a result that proves nothing was produced**: the all-filtered 422 is built after a 2xx
   without setting the rejection probe (`ideogram.rs:340`, `:376-380`), so `run_held_advance` parks it
   (`durable.rs:1084-1092`). Known (record §9.2); the component path parks it too today (§7 P10).
3. **The tier is priced from the client's raw `size` or `image_size`** (`image_size_hint`, `media.rs:31-35`), not the
   resolution Ideogram receives: `1000x1000` is priced as a 1K row tier while `2048x2048` is rendered, and an
   `image_size` that is never forwarded is priced too. Only rows with `image_price_1k` / `_2k` are affected; the shipped
   Ideogram rows are flat. Same class as I3-1 §6 item 2 and I3-4 §6 item 3.
4. **The `image_price <= 0` gate** rejects rows priced only by tier (`handlers.rs:550`), as I3-1 §6 item 1.
5. **The size parser is case-sensitive** (`ideogram.rs:93`, `split_once('x')`): `1024X1024` is refused while
   Stability's accepts it. Reproduced, not fixed (the package follows native).
6. **`KNOWLEDGE.md` is stale**: it says `1024x1024` becomes `2048x2048` and that the list has 23 values
   (`KNOWLEDGE.md:2462`, `:2474-2478`); since the 38-entry list (2026-09-12) `1024x1024` passes through
   (`tests/ideogram.rs:32-37`).
7. **Undeclared `url` artifacts are stored as `application/octet-stream` on the component held path** (§5 item 5).
   This already affects `image-minimax` and `image-qwen`, whose URL artifacts declare no media type
   (`reference_minimax_image.rs:337-340`): a held MiniMax or Qwen task's artifact is served with a generic type, where
   the native held path served the CDN's (whether the artifact endpoint serves the stored type is **unverified**).

Per lv's I3-2 ruling, native-arm defects (items 1 to 6) are registered here and leave with the arm; item 7 is a
component-path gap for the host (IG-Q2).

## 7. Differences that follow precedent (accepted under lv's 2026-10-13 pre-approval)

- **P1 Funds on non-2xx**: the P29 allow-list (400, 403, 404, 410, 429; `server:modules/billing/zero_usage_release.rs:
  111-119`) on both paths (P22 Q2, I3-1 IM-Q2). Native sync parks every post-dispatch failure (`ideogram.rs:457-465`);
  native held releases any 4xx (`ideogram.rs:340`, `durable.rs:1088-1090`). Ideogram's unsafe-prompt answer is 422,
  outside the allow-list, so on the held path it moves from released to parked; on the sync path it stays parked.
- **P2 Client status while funds are parked**: 502 (I3-1 IM-Q6). Native answers the upstream status (for example 422)
  on the sync path while it parks, and its own 422 for an all-filtered result (`ideogram.rs:376-380`).
- **P3 Admission timing**: every §3 refusal is before admission; native refuses after admission and cancels
  (`ideogram.rs:420-432`). `n` is refused before admission on both (native through the catalog precheck,
  `handlers.rs:192-194`, `capabilities.rs:1411-1418`). Same as the Azure / I3-1 timing ruling.
- **P4 Generic refusal texts**: an undeclared upstream id → 503 before admission (native 400 with its SKU list,
  `ideogram.rs:67-71`); `/v1/images/edits` → the host's 400 text (native `handlers.rs:831-834`); tier and capability
  texts are the host's (record Q10).
- **P5 Unforwarded fields are not priced**: `image_size` is never sent to Ideogram and gives no fact (I3-1 §5 item 3,
  I3-2 P10, I3-4 P5; host defect §6 item 3). The forwarded `size` is IG-Q3.
- **P6 Non-2xx outcome table**: `rejected` for 4xx other than 408, else `unknown` (`image-azure`, I3-1 decision 3).
- **P7 Elided text fields** (above 1 MiB) are refused before dispatch (`image-azure`, I3-2 decision 2, I3-4 P7);
  native would send them.
- **P8 Verify-then-settle on `b64_json`** (record §10.2, DI6 = A, lv 2026-10-08): a failed download parks the funds
  (`delivery_unknown`, no `finalize`, 502) instead of charging; the fetch limit is 64 MiB
  (`MAX_BINARY_RESPONSE_BODY_BYTES`, record §11 item 6) instead of native's 50 MiB (`ideogram.rs:177`); `accept` is
  `image/*` unless IG-Q2 declares a type (native `image/*`, `:528`); the timeout is 30 s on both.
- **P9 `url` artifacts are validated** (record §10.1, as I3-1's MiniMax and Qwen): a URL that fails `ArtifactUrlV1`
  (not `https`, userinfo, no host, above 8 KiB, a forbidden literal address) is undeliverable and parks; native passes
  any non-empty string through on the `url` path.
- **P10 A 2xx `rejected` is parked, not released** on the host (`component/mod.rs:1032-1038`; I3-2 P9): the all-filtered
  result parks on both paths, as native does on both, so the funds agree; only the client status differs (P2). The
  record §14 "cannot prove" item ("today parked, afterwards released") therefore stays "parked" until the host adopts
  the record §9.2 release rule; the package returns `rejected` either way.
- **P11 No development escape hatch on the second hop** (record §11: production must not enable it): with
  `asset_fetch_unsafe_allow_private_destinations` native fetches `http` and private addresses
  (`images_ideogram_stability.rs:266-267`); the component path never does (`safe_fetch.rs:19-21`). The host executor
  fetches only ports 443 and 8443 (`safe_fetch.rs:447`, test `only_ports_443_and_8443_are_fetched`).
- **P12 Token-priced rows are refused 503** before admission (§5 item 3; the I3-4 §5 item 3 analogue).

## 8. Fixtures, tests, wiring; decisions where the record is silent

As I3-1 §4 / I3-4 §8: a gate ② pack covering each of the three ids (`rendering_speed`), the `size` table of §3
(absent, `null`, empty, `auto`, `AUTO`, a member, a member with surrounding spaces, `1024x1024`, `1536x512`,
`1000x1000`, `1792x1024`, `1024x1792`, `1536x1024`, `+5x5`, and the refusals `1024X1024`, `1024`, `axb`, `0x1024`,
`1024x0`, `-5x100`, a number), every prompt and `response_format` refusal and acceptance (including `{"type": 5}` →
`url`), `n` 2, dropped fields (`image_url`, `quality`, `seed`), every §4 outcome (one safe entry, a missing `prompt`, an
unsafe entry with a URL, an empty / `null` / non-string `url`, a safe and an unsafe entry together, all filtered,
empty `data`, missing `data`, a non-JSON 2xx, 401, 422, 429, 500), and both renders (`url`, `b64_json`). Suite test
`ideogram_image_suite`, sandbox parity `ideogram_image_sandbox_parity`, build script
`scripts/build-image-ideogram-component.sh`, the `ci.yml` / `release.yml` enumerations, `shipped_packages_v1`, README
and ARCHITECTURE. Fixtures are transcribed from the host files of §5 item 7, not back-derived. The three download
failures and the held all-filtered case are host fixtures (§5 item 4); the package's part ends at the template.

Proposed decisions (not questions unless lv objects):

1. `model-capabilities` declares the configured models among the three ids, operation `generate`, no `input_roles`,
   `response_formats` `[url, b64_json]`, `metering_forms` `[images]`, no token buckets, no tier dimensions, `repeat`
   false, no request elision paths.
2. Every safe entry becomes an artifact, as native; `n` = 1 keeps that one in practice (v4 has no `num_images`). An
   answer with more safe entries than reserved is left to the host's settlement ≤ reservation guard.
3. `response_format` is carried in `state`, not re-read from the request in `render` (render receives no request).
4. A refusal text that echoes a value names its type when it holds an elided string, as I3-4 interpretation 3.
5. `n` = 1 as native and as SI-Q5 ruled for Stability; `repeat` is not used.

## 9. Versioning (Q47)

`south-component-conformance` 0.58.0 → 0.59.0 (new reference); `south-contracts` and `south-provider-api`
unchanged; every component lockfile and `fuzz/Cargo.lock` follow; every published package takes a patch and keeps its
`south_runtime` and `contracts`; `image-ideogram` starts at 1.0.0 with `{image: 1, media: 1}`. Exact numbers are fixed
at implementation.

## 10. Open questions for lv

- **IG-Q1 Scope.** The three ids the native arm maps, `generate` only; `FLASH`, the v3 endpoints, `json_prompt` and
  `enable_copyright_detection` wait for a later package version with a catalog entry (an operator can already send
  `enable_copyright_detection` as a request extra; it is not immutable). **Recommended: yes.**
- **IG-Q2 Media type of Ideogram's URL artifacts.** The package has to say what it knows about the bytes behind the
  URL, and the held path stores whatever it declares (§5 item 5). Options: (a) declare `image/png` (the URLs in every
  host fixture end in `.png`; Ideogram's output format is **unverified**), which also turns `accept` into
  `image/png`, and a CDN that negotiates strictly could refuse a non-PNG object; (b) declare nothing, as `image-minimax`
  and `image-qwen` do, and the host stores undeclared `url` artifacts with the type it sniffs from the bytes
  (`sniff_image_mime`, already neutral, `server:…/handler/images/media_type.rs`), falling back to
  `application/octet-stream` — record §10.1 allows the host to sniff as a generic check, and this also fixes MiniMax
  and Qwen held artifacts (§6 item 7); (c) declare nothing and accept `application/octet-stream`. **Recommended: (b)**,
  a small generic host change before any `url` family's held path is cut over; (a) if lv prefers no host change, after
  a C10 capture shows Ideogram's content type.
- **IG-Q3 The `size` fact.** Unlike MiniMax and Stability, Ideogram receives a size, but not the client's: the arm
  snaps it to its own list (`1000x1000` → `2048x2048`). The host prices per-image tiers from `facts.size` (P22 Q12,
  `component/checks.rs:89-95`, `mod.rs:924`). Options: (a) report the forwarded `resolution` as the `size` fact
  when one is sent, none otherwise (the price follows what is rendered); (b) report the client's parsed `size` (the
  native pricing input, host defect §6 item 3); (c) never report it (base price always). The shipped Ideogram rows are
  flat per speed, so none of the three changes a bill today. **Recommended: (a)**, registered as a difference from
  native for rows with tier prices.

## Revision note

- 2026-10-13: first draft (proposed).
