# Image world step I3-1: `image-minimax` and `image-qwen`

Status: accepted (lv, 2026-10-12: IM-Q1 to IM-Q7 ruled as recommended, §8). Implemented on branch `image-i3-1` (see
"Implementation"); not released. lv chose on 2026-10-12 that the synchronous image
packages (the image world record's I3 batches) are the next lane, and that each batch's design is reviewed before it is
built.

Date: 2026-10-12

Baseline: south `origin/main` = `116cf13` (release 0.57.0, canonical IR 4). Host references carry a `server:` prefix and
refer to token-station-server `origin/dev-v2` `baa957d6` (`server:…/` abbreviates
`server:gateway/src/modules/inference/`). Statements marked **unverified** are conclusions from reading code, not from
running anything or capturing traffic.

Parent record: [`2026-09-30-image-world.md`](2026-09-30-image-world.md) ("the record"): §13 (the MiniMax and Bailian
Qwen-Image rows), §14 / §19 (I3-1), Q7 (funds during the dual run), Q9 (the "≤ delivered + 1" rule stays in the host),
Q15 (Bailian artifacts are `url` pointers). The shipped template is `image-azure` (`components/image-azure/`,
`reference_azure_image.rs`, `fixtures-image-azure/`).

**Scope.** I3-1 in the record also lists the OpenAI-compatible fallback (DI4). It is left out: whether it becomes a
component at all is the host's open decision D9 (its native arm names no vendor). The host's Bailian **Wan** arm is
already a task-world package (`task-wan-image-v2`) and is not touched; nor is Bailian's native image surface (P25).

## 1. What I3-1 delivers

| Item | Where | Change |
|---|---|---|
| `image-minimax` 1.0.0 | `components/image-minimax/`, `reference_minimax_image.rs`, `fixtures-image-minimax/` | Family `minimax-image`; generation only (§2) |
| `image-qwen` 1.0.0 | `components/image-qwen/`, `reference_qwen_image.rs`, `fixtures-image-qwen/` | Family `bailian-qwen-image`; generation, including the edit models that edit through generation (§3) |
| Q47 | `south-component-conformance` minor | Every package takes a new version |

Both packages are image world (`image-adapter-v1`), contracts `{image: 1, media: 1}`, `south_runtime` the oldest runtime
that admits their declarations (expected 0.57.0, as `image-azure`), `generate` capability only, JSON request,
`bearer` auth on `provider_api_key`, metering form `images`.

## 2. `image-minimax`

Reproduces the host's native arm (server:`…/handler/images/minimax.rs`, 374 production lines):

- **Request**: `POST {base_url}/v1/image_generation`, `content-type: application/json`. Body: `model` (the upstream id),
  `prompt`; `n` only when the client sent it (as an integer); `aspect_ratio`, `width`, `height`, `seed`,
  `prompt_optimizer` copied verbatim when present; `response_format` mapped `b64_json → base64` (other strings as sent);
  `subject_reference` forwarded verbatim, else `image_url` (string or array of strings) wrapped as
  `[{"type":"character","image_file": …}]`; every other field (including `size`) dropped (minimax.rs:45-120).
- **Refusals before dispatch** (`prepare`, all `invalid_request`): `prompt` missing or not a string; prompt over 1500
  characters; `n` outside 1..=9 or not a whole number; `image_url` of the wrong type. Texts follow the native ones
  (minimax.rs:13-29, 229-241).
- **`GroupId`**: when configured, `?GroupId=<value>` is appended (server:`…/engine/upstream.rs:511-536`). The package
  declares a config key `group_id` (non-secret, optional, syntax `digits`, the HTTP contract's grammar for this query
  name) and adds the query only when it is set (**IM-Q3**).
- **Response** (`parse-response`): HTTP 200 with a non-zero `base_resp.status_code` is a failure detected in the body
  (record §7): mapped to an error envelope whose `http_status` follows the host's table
  (server:`…/minimax_status.rs:43-61`: 1004 / 2049 → 401, 1026 / 2013 → 400, 1002 → 429, 1008 / 2153 → 503, else 502);
  outcome `rejected` for the codes that mean "refused before output" (1004, 2049, 2013, 1002, 1008, 2153) and
  **`unknown` for 1026** ("output violation", possibly charged; **IM-Q4**). A missing `base_resp` is tolerated. Success:
  each non-empty `data.image_urls[]` becomes a `url` pointer artifact and each non-empty `data.image_base64[]` an inline
  base64 artifact (media type `image/png` declared, **IM-Q5**), URLs first; zero artifacts → `unknown` (the host returns
  502 and parks, as today). Evidence `images_reported = metadata.success_count`.
- **Elision**: response `/data/image_base64/*`; request `subject_reference/*/image_file` and `image_url` (data URLs).
- **Immutable paths**: `model`, `n`, `width`, `height`, `aspect_ratio`, `response_format` (= the host's
  `MINIMAX_IMAGE_OWNED`, server:`…/images/execute.rs:42-49`).
- **Render**: `{"created": <host>, "data": [{"url" | "b64_json": …, "revised_prompt": null}]}`, as native.
- **Facts**: `requested_outputs = n` (1 when absent); `inputs.reference_image` = the number of reference images; no
  `size` fact (MiniMax has no size; §5 item 3).

## 3. `image-qwen`

Reproduces the host's native sync arm (server:`…/handler/images/bailian.rs:333-570`):

- **Request**: `POST {base_url}/api/v1/services/aigc/multimodal-generation/generation`, `content-type:
  application/json`, no `X-DashScope-Async`. Body `{"model", "input": {"messages": [{"role": "user", "content":
  [{"image": url}…, {"text": prompt}]}]}, "parameters": {…}}`: image parts from `image_url` (string or array) before the
  text part; `n` only when sent; `size` only when sent, `WxH → W*H` (other strings as sent); `watermark`, `bbox_list`,
  `seed`, `prompt_extend`, `negative_prompt` only when sent (bailian.rs:117-196). The helpers are the ones
  `reference_wan_image_task_v2.rs` already has (`wan_size`, the `image_url` parts, the pass-through list); they move to a
  shared module rather than being copied.
- **Refusals before dispatch**: `prompt` missing or not a string; `n` outside 1..=6 or not whole ("'n' must be in 1..=6
  for Bailian Qwen-Image models (got N)"); `image_url` of the wrong type.
- **Response**: non-2xx → error envelope from DashScope's `code` / `message` (capped), outcome `rejected` for 4xx other
  than 408, else `unknown` (as `image-azure`); 2xx: every non-empty `output.choices[].message.content[].image` becomes a
  `url` pointer artifact (record Q15), flattened across choices; a 2xx body carrying a DashScope `code` is a failure
  (`unknown`; the native arm does not detect it, §6 item 4); zero artifacts → `unknown`. Evidence
  `images_reported = usage.image_count`.
- **Immutable paths**: `model`, `input`, `parameters.n`, `parameters.size` (= `BAILIAN_IMAGE_OWNED`).
- **Facts**: `requested_outputs = n` (1 when absent); `inputs.input_image` = the number of `image_url` entries;
  **`size` only when the client sent one** (parsed from `WxH` / `W*H`) — the host prices tiers from the requested size,
  and reporting the upstream's default (1664×928) would move an unsized request into the 2K tier (§5 item 3).

## 4. Fixtures, tests, wiring

Each package: a gate ② pack with capabilities / prepare / response / render rows covering every refusal above, both
artifact forms, every `base_resp` code class (MiniMax), the 2xx-with-`code` case (Qwen), `GroupId` present / absent
(MiniMax), the edit-through-generation request (Qwen, `qwen-image-edit-plus` with three images), and `images_reported`
equal to, one above, and far above the delivered count. A suite test per package, a sandbox parity test, the build
script, the `ci.yml` / `release.yml` enumerations, `shipped_packages_v1` (official list and the runtime declaration),
`compatibility.json` `media_component_capabilities`, README and ARCHITECTURE notes. The host's pinned behaviour
(server:`…/images/tests/minimax.rs`, `…/images/tests/bailian.rs`, `tests/images_providers.rs`,
`tests/byok_media_ownership.rs`, `tests/south_adoption.rs` MiniMax image-01) is the fixture source.

## 5. What the host must do (for its own plan; listed so the cutover is not blocked by surprises)

1. **The "≤ delivered + 1" rule** (record Q9, §9.4 item 3). The host's image executor settles the delivered count and
   only logs `images_reported` (server:`…/handler/images/component/checks.rs:234-250`). Both native arms settle
   `images_reported` when it is above 0 and at most delivered + 1. Unless the host adopts the rule, the dual run
   differs whenever the upstream reports one more image than it delivered (**IM-Q1**).
2. **Funds on non-2xx**: the executor releases the P29 allow-list and parks the rest; the native sync arms park every
   failure, and MiniMax's held arm releases any 4xx. The host ruled for Azure (P22 Q2) that synchronous 4xx follow the
   P29 allow-list; the same applies here as a registered difference (**IM-Q2**).
3. **Size tiers**: the host prices tiers from the request's size. The packages report a `size` fact only when the client
   sent one (Qwen) or never (MiniMax, which has no size). The native MiniMax arm prices a client `size` it never forwards
   (host defect, §6 item 2); the component path does not.
4. **Config values on an image route**: `group_id` reaches the component as a declared config value from the row's
   `config_values` (server:`…/component/mod.rs:1192-1212`). Cutover writes `config_values.group_id` on the MiniMax row.
5. **Client status of a body-detected failure**: the executor answers 502 for any `rejected` / `unknown` 2xx outcome
   while the funds are parked; the native MiniMax arm answers the mapped status (401 / 400 / 429 / 503). Registered as a
   difference (**IM-Q6**, re-ruled 2026-10-13: keep 502).
6. **Prefer**: native Qwen ignores `Prefer: respond-async` (no held arm); a routed Qwen model gets the generic held arm
   (202). A deliberate improvement, registered.

## 6. Host defects found on the way (read, not run)

1. The `image_price <= 0` gate rejects rows priced only by tier (`image_price_1k` / `_2k`) on both arms
   (server:`…/handlers.rs:457, 488-514`), though the cost function would price them.
2. MiniMax prices the tier from the client's `size`, which it never forwards (server:`…/token_counter/media.rs:31-35`).
3. MiniMax can settle more than it reserved (`n` absent → reserve 1; delivered 1, `success_count` 2 → bill 2).
4. Qwen does not detect a 2xx body carrying a DashScope `code`.
5. `qwen-image-3.0-pro`, the id every host test uses, has no capability entry, so nothing is prechecked for it.
6. The edits refusal for Bailian names only Wan.

None blocks the packages; each is either fixed by the component path or listed for the host.

## 7. Versioning (Q47)

`south-component-conformance` 0.54.0 → 0.55.0 (new references, shared Bailian helpers); every published package takes a
patch; `image-minimax` and `image-qwen` start at 1.0.0. Proposed to ship in the next release after review.

## 8. Open questions for lv

- **IM-Q1** Should the host's image executor adopt Q9's "settle `images_reported` when 0 < reported ≤ delivered + 1"?
  **Recommended: yes, with the reservation as the cap** — it is what both native arms do and what Q9 ruled, and capping
  at the reservation removes host defect §6 item 3. Otherwise register the difference.
  **Ruled (lv, 2026-10-12): as recommended.**
- **IM-Q2** Funds on non-2xx follow the P29 allow-list (as P22 Q2 for Azure), registered as a difference from the native
  sync arms. **Recommended: yes.**
  **Ruled (lv, 2026-10-12): as recommended.**
- **IM-Q3** `GroupId` as a declared config key `group_id`, syntax digits. **Recommended: yes** (MiniMax group ids are
  numeric, **unverified** for every account; the native arm percent-encodes any string).
  **Ruled (lv, 2026-10-12): as recommended.**
- **IM-Q4** MiniMax `base_resp` 1026 ("output violation") is `unknown` (parked), not `rejected`. **Recommended: yes** —
  it may be charged.
  **Ruled (lv, 2026-10-12): as recommended.**
- **IM-Q5** MiniMax inline base64 artifacts declare `image/png`. **Recommended: yes** (**unverified**: MiniMax's
  default output format; the host's held path sniffs today). Alternative: `image/jpeg`; a capture settles it.
  **Ruled (lv, 2026-10-12): as recommended.**
- **IM-Q6** A body-detected failure's client status: the host carries the component's `http_status` (a small host
  change) rather than answering 502. ~~Recommended: carry it~~ **Re-ruled (lv, 2026-10-13): keep 502, registered as a
  difference.** The host's executor answers 502 on purpose while the funds sit in review
  (server:`…/handler/images/component/mod.rs:965-972`): a 4xx would tell the client "refused, not charged" and invite a
  retry while the money may still be charged. The native MiniMax arm returns the mapped 401 / 400 / 429 / 503 while it
  too parks the funds; the component path does not reproduce that.
- **IM-Q7** Two packages, not one "bailian + minimax" package. **Recommended: two** — different families, auth hosts
  and error vocabularies; one package per family matches `image-azure`.
  **Ruled (lv, 2026-10-12): as recommended.**

## Implementation (2026-10-12)

What landed, on branch `image-i3-1`:

- `image-minimax` 1.0.0 and `image-qwen` 1.0.0: manifests, guest crates (thin shells like `image-azure`), references
  `reference_minimax_image.rs` and `reference_qwen_image.rs`, fixture packs `fixtures-image-minimax/` and
  `fixtures-image-qwen/` (each with a README), suite tests `minimax_image_suite` and `qwen_image_suite`, sandbox parity
  tests `minimax_image_sandbox_parity` and `qwen_image_sandbox_parity`, build scripts, and the enumerations
  (`ci.yml` rust-cache, `release.yml` build / gate ② / package lists, `shipped_packages_v1` official list and a test
  pinning the two packages' runtime and declared keys, README and ARCHITECTURE).
- The Bailian helpers (`wan_size`, the `image_url` parts, the pass-through list, the whole-count reader and where the
  result's images sit) moved from `reference_wan_image_task_v2.rs` to the crate-private module `bailian_image.rs`,
  which both packages use; `task-wan-image-v2`'s pack passes unchanged. Helpers the two new references share (placeholder
  to `$south.ref`, status-to-code table, refusal shapes) are in the crate-private module `image_shared.rs`.
- Q47 (§7): `south-component-conformance` was already 0.55.0 at 0.57.0 (KC1 took 0.54.0 → 0.55.0), so this step
  moves it **0.55.0 → 0.56.0**, not the 0.54.0 → 0.55.0 written in §7. The fuzz requirement, every component lockfile
  and `fuzz/Cargo.lock` follow; every published package takes a patch and keeps `south_runtime` 0.57.0
  (`shipped_packages_v1::i3_1_retires_every_published_057_package_identity`; the KC1 test no longer pins KC1's
  identities, as KC1 did to R3's).
- `compatibility.json` needs no change: `media_component_capabilities` records the image contract per host
  (`not_verified` for both), not per package.

Failing first: with the references written and only three expectations written by hand (`prepare.group-id-sent` and
`response.base-resp-1026` for MiniMax, `prepare.text-defaults` for Qwen), both pack tests failed on the 74 cases still
without an expectation and passed the three hand-written ones. The other expectations were then taken from the
reference and reviewed row by row against §2 and §3. Each of six mutations turns a named test red: dropping `GroupId`
(`the_group_id_is_sent_only_when_configured`), 1026 as `rejected` (`an_output_violation_is_unknown`,
`a_body_detected_failure_carries_the_hosts_status`), forwarding `size` (`the_size_is_neither_forwarded_nor_a_fact`), a
`size` fact when none was sent (`the_size_fact_exists_only_when_the_client_sent_a_size`), an `X-DashScope-Async` header
(`the_request_is_synchronous`) and ignoring a 2xx `code` (`a_2xx_carrying_a_code_is_unknown`); the pack test goes red
for each as well.

Versions:

| Item | Before | After |
|---|---|---|
| `south-component-conformance` | 0.55.0 | 0.56.0 |
| `image-minimax` / `image-qwen` | — | 1.0.0 / 1.0.0 (`south_runtime` 0.57.0) |
| `image-azure` | 1.0.4 | 1.0.5 |
| `provider-openai-compatible` | 2.6.0 | 2.6.1 |
| `provider-anthropic`, `provider-anthropic-bedrock-invoke`, `provider-bedrock-converse`, `provider-bedrock-converse-bearer` | 1.1.0 | 1.1.1 |
| `provider-gemini`, `provider-openai-responses` | 1.3.0 | 1.3.1 |
| `embeddings-openai-compatible` / `embeddings-gemini` / `embeddings-vertex` | 1.0.8 / 1.1.5 / 1.0.7 | 1.0.9 / 1.1.6 / 1.0.8 |
| `task-kling` / `task-kling-v2` | 1.0.17 / 0.32.15 | 1.0.18 / 0.32.16 |
| `task-minimax-v2`, `task-bailian-v2` | 0.31.14 | 0.31.15 |
| `task-xai-v2`, `task-veo-v2`, `task-wan-image-v2`, `task-gmi-image-v2` | 0.35.14 | 0.35.15 |
| `task-byteplus-v2` | 0.36.14 | 0.36.15 |

The workspace version, tags and `south_runtime` of the published packages are unchanged.

Decisions where the record is silent:

1. `image_url` on MiniMax is wrapped once: `[{"type": "character", "image_file": <the value>}]`, the value being the
   client's string or array as sent (elided entries as `$south.ref`). `inputs.reference_image` counts the strings;
   with `subject_reference` present it counts that array's entries (1 for a non-array value); a `null`
   `subject_reference` counts as absent.
2. Native refusal texts were not available; the packages use the record's wording for `n` ("'n' must be in 1..=9 for
   MiniMax image models (got N)", "… 1..=6 for Bailian Qwen-Image …"), `image-azure`'s "Missing 'prompt' field" and
   "'prompt' is longer than the gateway accepts" (an elided prompt), and the Wan package's `image_url` texts. The dual
   run compares status and timing, not text.
3. MiniMax non-2xx answers follow `image-azure`'s status table (`rejected` for a 4xx other than 408, else `unknown`);
   a `base_resp.status_code` that is not an integer is `unknown`; a code the host table does not name is `unknown`
   with a 502 envelope. `base_resp.status_msg` (capped at 256 characters) is the envelope's `provider_message`.
4. `metadata.success_count` is read as a whole number or a string of digits (MiniMax writes strings); anything else is
   `null` evidence. Neither package applies "≤ delivered + 1": Q9 keeps it in the host (IM-Q1).
5. Request elision: MiniMax declares `/subject_reference/*/image_file`, `/image_url` and `/image_url/*` (§2); Qwen
   declares `/image_url` and `/image_url/*` as well, so data URLs never enter the sandbox. Any forwarded value that
   reaches `prepare` elided (declared or above the fallback threshold) is written back as an `as_is` `$south.ref`.
6. Qwen declares `response_formats: [url]` and renders every artifact as `url`, as the native arm answers; MiniMax
   declares `b64_json` and `url` and renders each artifact in its own form.
7. Neither package sets `content-type`; like `image-azure`, it relies on the host's JSON body executor.
8. Qwen's DashScope errors: a `code` containing `DataInspection` is `content_policy`, anything else follows the
   status; `provider_message` is `code: message`, capped at 256 characters. A 2xx carrying a `code` is `unknown` with
   a 502 `provider_protocol_error` envelope.
9. Qwen's `size` fact is read from `WxH`, `WXH` or `W*H` with both edges 1 to 65,536; any other value is forwarded as
   sent and gives no fact.

## Revision note

- 2026-10-12: first draft (proposed).
- 2026-10-12: lv ruled IM-Q1 to IM-Q7 as recommended; the status is accepted. The host takes IM-Q1 (settle
  `images_reported` when 0 < reported ≤ delivered + 1, capped at the reservation) as a host change.
- 2026-10-13: lv re-ruled IM-Q6 on the host executor's documented reason: keep 502 while funds are parked, registered as
  a difference.
- 2026-10-12: implemented on branch `image-i3-1` ("Implementation"); §7's conformance version corrected to
  0.55.0 → 0.56.0.
