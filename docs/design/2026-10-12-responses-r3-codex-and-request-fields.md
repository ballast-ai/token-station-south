# Responses step R3: the `openai-codex` family, the four dropped request fields, and the tool-usage bucket

Status: accepted (lv, 2026-10-12: RQ3-Q1 to RQ3-Q8 ruled as recommended, §8). Implemented 2026-10-12 on branch
`responses-r3-codex`, merged as #186 and released in 0.56.0 (`2026-10-12-release-0.56.0.md`) before the §3.2 measurement, by lv's
ruling of 2026-10-12: no host row may be cut over to `openai-codex` until §3.2 is recorded (§9). lv approved in
principle on 2026-10-12 that R3 starts now and that the four request fields of record §4.6 be carried; the same day lv
had scheduled the IR tool-usage bucket (host SF23) with R3, and on the evidence of §5 re-ruled it out of R3 (RQ3-Q6).

Date: 2026-10-12

Baseline: south `origin/main` = `7ef9995` (release 0.55.0); kernel pin `c2581f37` (protocol 0.5.0, canonical IR 3).
Host references carry a `server:` prefix and refer to token-station-server `origin/dev-v2` as read on 2026-10-12
(`server:…/` abbreviates `server:gateway/src/modules/inference/engine/`). Statements marked **unverified** are
conclusions from reading code or published documentation, not from running anything or capturing traffic.

Parent record: [`2026-09-30-openai-responses-upstream-component.md`](2026-09-30-openai-responses-upstream-component.md)
(accepted; "the record"). It already designs the Codex family: D2, D8, D11, §3.3, §4.3, §6.4, §7.3–§7.4, §10.2–§10.3,
§13, and the rulings R-Q3, R-Q5, R-Q7, R-Q14. This document does not repeat that design. It (1) reconciles §3.4 and
§10.2 with the credential schema as it shipped (§2), (2) settles what R3 needs that the record leaves open (§2–§3),
(3) designs the four request fields as a co-traveller (§4), and (4) reports a finding on the tool-usage bucket (§5).
Other predecessors: [`2026-10-11-responses-reasoning-replay-and-passthrough-suite.md`](2026-10-11-responses-reasoning-replay-and-passthrough-suite.md)
(NC-1 and the pass-through suite, released in 0.55.0; its §2.5 covers Codex),
[`2026-09-30-host-zero-vendor-boundary.md`](2026-09-30-host-zero-vendor-boundary.md) (B4 recipes §13.3, Q14 attributes
§13.8, Q47 §13.12).

**Why now.** On the host, Codex is where the vendor-identifier count sits: of the host's 3,922 counted identifiers,
`codex` (52) and `openaicodex` (58) are about 110, roughly 95 of them in host code that retires with R3
(server:`scripts/arch/vendor_identifier_baseline.txt`). The OpenAI Responses rows (`openai-responses`) move almost
nothing (`openai` is exempt as a north protocol name), and the host's native Responses pass-through branch cannot be
deleted while Codex rows still use it.

## 1. What R3 delivers

| Item | Where | Change |
|---|---|---|
| `openai-codex` family | `provider-openai-responses` 1.1.0 → **1.2.0** | New family in the same package (record D2): URL, request rules, failure frames per the record; credentials and manifest per §2 |
| Codex fixture pack | `crates/south-component-conformance/fixtures-openai-codex/` | Gate ② pack derived from the host's pinned Codex behaviour (§3.3) |
| Production `codex` recipe | the package's `credentials` section | Promotes the test recipe of `tests/credential_recipe_v1.rs:181-226` with the differences of §2.2 |
| Four request fields | north codec + `provider-openai-responses` | §4 |
| Tool-usage bucket | — | §5: proposed **not** to be built for R3 |

## 2. The manifest and the credential section

### 2.1 Shape (reconciles record §3.4 and §10.2 with the shipped schema)

The record's §10.2 sketch has two slots (`provider_api_key` static and `codex_access_token` minted) and an `import`
keyed by file. Neither shape passes the shipped gate ①: every slot in `permissions.secrets` must have a minted or field
source in the section that applies to the family (`crates/south-provider-api/src/credentials.rs:616-625`), and
`import` is keyed by field (`ImportRuleV1`, :92-100). The pattern that ships is the one `provider-openai-compatible`
uses for `github-copilot`: one slot name, minted, in a `credentials` section scoped by `families`.

```text
providers:            ["openai-responses", "openai-codex"]
auth_arms:            ["bearer"]
request_facts:
  openai-codex:       output_cap [] ; model /model ; stream /stream      (record §4.3, R-Q5)
immutable_body_paths:
  openai-codex:       store, stream, instructions, temperature, max_output_tokens   (record §4.3)
north_passthrough:
  openai-codex:       responses
credentials:          families ["openai-codex"]; fields access_token (secret), refresh_token (secret),
                      account_id (non-secret, syntax token); require_one_of [[access_token, refresh_token]];
                      import from the Codex CLI's auth.json; slot provider_api_key minted by recipe "codex"
```

The `openai-responses` family is unchanged: its `provider_api_key` stays a static field (the section does not apply to
it).

### 2.2 The recipe

Start from the test recipe `codex()` (`tests/credential_recipe_v1.rs:181-226`), which already passes gate ① and the
reference interpreter, with three changes:

1. **`scope` is sent.** The host sends `scope: "openid profile email offline_access"` on every refresh
   (server:…/token_refresh.rs:180-191, pinned by `codex_mint_request_preserves_original_scope`); the test recipe omits
   it. Record §10.3 lists it as a constant recipe parameter. **Unverified** whether the endpoint requires it; keeping
   it is the conservative choice.
2. **`import`** of `auth.json`: `access_token` ← `/tokens/access_token`, `refresh_token` ← `/tokens/refresh_token`,
   `account_id` ← `/tokens/account_id` (the host's importer, server:`gateway/src/modules/admin_ui/handler/credentials/mod.rs:200-217`).
   The importer ignores `tokens.id_token`; so does this.
3. **`account_id` syntax** `token` (RFC 9110 token, as the record's sketch has) instead of the test's
   `printable_ascii: 128`; it is written into a request header. **RQ3-Q2** asks lv to confirm.

Kept from the test recipe: `oauth2_token` step with JSON encoding; `rotates_refresh_material: true` with write-back of
a rotated refresh token; `without_refresh_material: use_stored`; `fixed_validity_seconds: 3000` (the host ignores
`expires_in` and stores now + 3000 s, server:…/token_refresh.rs:206-215); the account from the `id_token` claim
`https://api.openai.com/auth`.`chatgpt_account_id`, `must_equal_field: account_id`; the attribute
`account_id` exported and **persisted** (RQ3-Q1).

### 2.3 Request headers

`authorization: Bearer <minted>` (auth arm) and `chatgpt-account-id: <account_id>` as an ordinary descriptor header,
only when the attribute is present (record §10.3; the attribute arrives in `ProviderConfig.declared`, boundary §13.8).
`accept: text/event-stream` as the host sends for a streaming upstream. The host sends no other identifying header
(no `user-agent`, `originator`, `OpenAI-Beta` or `session_id`; server:…/south_adapter.rs:279-288, :364). If the R-Q5
measurement (§3.2) shows the backend needs one, it is a constant descriptor header outside `RESERVED_HEADERS`.

## 3. Request rules, responses, fixtures, measurement

### 3.1 Behaviour

The record's §4.3, §6.4 and §7 apply unchanged: `stream: true` forced and `stream=false` refused at build time with zero
upstream calls (R-Q5); `input` always an item list; `instructions` defaulted to `You are a helpful assistant.`; no
`max_output_tokens`, no `temperature`; `store: false`; the three failure-frame shapes recognised before the evidence
rules; strict `reported` usage; non-zero `tool_usage` refused (R-Q3, §5). NC-1 applies unchanged (NC-1 record §2.5):
a Codex model may declare `reasoning_replay.openai.v1` as operator data, and then gets `include`; per the host's
standing rule no model declares the word before a real multi-turn replay capture.

### 3.2 Measurement before cutover (R-Q5, host Q-B6-8)

On a real Codex account in staging (an operator action; the host plan assigns it to ops): (a) whether the backend
accepts `max_output_tokens` (R-Q5; if it does, the family gains a cap and the empty-cap settlement rule stops applying);
(b) whether it requires any header beyond §2.3; (c) a reasoning replay round trip and the item sizes against NC-1's
NP-Q11 bounds; (d) whether `scope` is needed on refresh. Each result is recorded in this document before the package
is released; (a) and (b) can change the package, so the release waits for them.

### 3.3 Fixtures

A new pack `fixtures-openai-codex/` derived from the host's pinned Codex behaviour (server:`gateway/tests/south_adoption.rs`
`codex_stream_routes_south_without_account_id`, `codex_never_forwards_a_client_store_true`,
`codex_nonstream_routes_south_with_account_id_and_relays_verbatim` — the last as a refusal row, since R-Q5 refuses
non-streaming) and the record's §14 Codex rows: request (string input lifted, defaults, store, stream, account header
present / absent, non-stream refusal), stream (text, tool call, reasoning with encrypted content, the three failure
shapes, truncation), usage (`UsageNeverDefaulted` on stream fixtures, moved here from R1 by record §17.4), and
`response.nonzero-tool-usage`. Recipe fixtures run in the existing gate ③ suite `south.credential-recipe.v1`, whose
`Rotating` kind is already Codex-shaped (`crates/south-provider-conformance/src/credential_recipe.rs:73-79`); the
production recipe joins its pack.

### 3.4 Host defects that retire with the native arm (for the host's dual run)

Read on the host, not run. Each is a difference the host's replay-style dual run should register rather than treat as a
component fault: non-streaming Codex requests are sent unnormalised (`max_output_tokens`, `temperature`, string
`input`, no forced stream; server:…/text_admission/sender/responses.rs:508-528, record §13.5 #5); a refreshed
`account_id` is not persisted (server:…/token_refresh.rs:368-373); every non-2xx from the token endpoint, 5xx included,
becomes an operator error (:224-232; the recipe maps 5xx to transient, record §10.2); the token endpoint can be
redirected by credential extras in production (:180-181; recipes have no such seam, by design); in-band failure frames
are marked `delivery_unknown` and forwarded verbatim (responses.rs:1993-2003; the component path renders the host's
fixed-message event, record D7).

## 4. The four request fields (`metadata`, `user`, `prompt_cache_key`, `truncation`)

### 4.1 The gap

The Responses north codec reads `model`, `instructions`, `input`, `tools`, `tool_choice`, `text.format`,
`max_output_tokens`, `temperature`, `top_p`, `stop`, `stream`, `reasoning` and `parallel_tool_calls`
(`crates/south-north-codec/src/responses/request.rs:17-117`); every other key is dropped, including these four
(record §4.6). The Chat Completions codec is different: it copies every top-level key it does not type into
`ChatRequest.extensions` (`crates/south-north-codec/src/request.rs:29-41, 117-124`), so on the Chat surface `user`,
`metadata` and `prompt_cache_key` reach the IR as untyped extensions, and then no component sends them either
(`reference_openai_responses.rs:339-415`, the OpenAI-compatible reference `reference.rs:273-340`). The kernel
`ChatRequest` has no typed field for any of them (`token-station-protocol` `chat.rs:197-231`). `prompt_cache_key`
matters most: it steers OpenAI's prompt-cache routing, so dropping it lowers cache hits and raises cost (**unverified**
by measurement; from OpenAI's published semantics).

### 4.2 Two ways to carry them

- **(A) Typed fields in the kernel.** `ChatRequest` gains `user`, `metadata`, `prompt_cache_key` (and a Responses-only
  `truncation`, which does not belong in a protocol-neutral IR). Cost: the kernel chain (upstream protocol release,
  mirror tag, south re-pin), `canonical_ir` 3 → 4 if the kernel bumps it, every manifest's compatibility tuple changes,
  every package takes a new version, and every host re-pins in lockstep because `compatibility_admits` compares
  `kernel_contracts` exactly (kernel re-pin record `2026-10-08-kernel-repin-protocol-0.5.0.md` §2, §6).
- **(B) Named request extensions, under the same D5 exception NC-1 used.** The Responses codec writes the four values
  as request-level extension keys; `provider-openai-responses` maps them back onto the body. No kernel change, no
  handshake break. D5 forbids a component to act on an `Extensions` key; NC-1 (NP-Q1, ruled 2026-10-12) and the Claude
  carrier are the argued precedents, and record R-Q15 already collects these keys for promotion to typed fields in the
  next kernel-chain change.

**Recommended: (B)** (**RQ3-Q3**). The value is in `prompt_cache_key` reaching OpenAI soon; (A) buys type safety at
the price of a full handshake break, and (B)'s keys are promoted by R-Q15 whenever the kernel chain next moves.

### 4.3 Design under (B)

- **Key names reuse the Chat codec's.** The Chat codec already writes `user`, `metadata` and `prompt_cache_key` as raw
  top-level extension keys; the Responses codec writes the same three names, so one vocabulary covers both north
  surfaces, plus `truncation` (Responses only). **RQ3-Q4.**
- **Validation in the codec** (fail closed, `invalid_request`): `user` a string ≤ 512 bytes; `prompt_cache_key` a
  string ≤ 512 bytes; `metadata` an object of at most 16 string-valued entries, keys ≤ 64 and values ≤ 512 characters
  (OpenAI's published limits, **unverified** against the live API); `truncation` one of `auto`, `disabled`. `null` is
  the same as absent. The bounds keep an untrusted client from widening the upstream body beyond what OpenAI accepts.
- **`provider-openai-responses`** writes them onto the body for the `openai-responses` family. For `openai-codex` it
  writes none of them until §3.2 measures whether the Codex backend accepts them (**RQ3-Q5**).
- **Not in this change:** the OpenAI-compatible component sending the Chat-side `user` / `metadata` /
  `prompt_cache_key`. It is the same gap on the Chat surface but changes another package's wire for every
  `openai-compatible` upstream, several of which reject unknown fields (**unverified** per upstream); a separate record.
- **Host side:** none required; the codec runs in the host and the keys travel in `ChatRequest.extensions`. The host
  must keep these four names out of any operator-extras or reserved-key stripping that would remove client data
  (**unverified** whether any such stripping touches them today; server's R2 plan §5 lists them only as losses).

## 5. The tool-usage bucket (host SF23): a finding

lv ruled on 2026-10-12 to schedule the IR tool-usage bucket with R3. The ruling followed the host's Q-S5-2 (lv,
2026-10-07) and Q-B6-7 (lv, 2026-10-09): keep the host's wire parsing of Responses usage until the IR has a tool-usage
bucket, because refusing non-zero `image_gen` usage (R-Q3) "would make every Codex request that uses the image tool
fail" (the host's P21 S5 implementation plan, server:`docs/product-review-v2/plans/2026-10-07-P21-S5-*.md`, lines 158 and 165).

**On the host today no such request succeeds either.** Hosted tools, `image_generation` included, are refused at
admission on the Responses surface: "OpenAI Responses text billing allows only flattened client functions"
(server:…/token_counter/authorize.rs:564-584; a hosted `tool_choice` likewise at :663; tests at :1168-1196, :1421-1460), and
the native Responses usage parser refuses any output item other than `message`, `function_call` and `reasoning`
(its error reads, in the host's language, "Responses output item not admitted by cost admission"; server:`crates/gateway-provider-protocol/src/usage_evidence.rs:887-910`), which is
where an image call's `image_generation_call` item would land. The host's image-tool pricing
(server:…/token_counter/media.rs:214-278) is therefore reachable only from tests; record §13.5 #7 suspected this. Read,
not run: no capture of a Codex response that used the image tool exists.

So R-Q3's refusal changes no outcome a client gets today: both arms end a response carrying image-tool usage in review
rather than a charge. Building the bucket is a new product capability (billing Codex's hosted image tool), not a
precondition for retiring the native arm. A bucket is a kernel change of the (A) kind in §4.2, with the same handshake
break.

**Recommended (RQ3-Q6): take the bucket out of R3.** Keep R-Q3; the host's E1-8 / E1-9 deletions (Responses usage
parsing, image-tool pricing) proceed with the cutover; reopen the bucket only when hosted tools are to be admitted and
sold, together with whatever admission change that needs.

## 6. Versioning and release (Q47)

| Item | Change | Why |
|---|---|---|
| `south-component-conformance` | 0.53.0 → **0.54.0** | Codex reference behaviour, recipe, fixtures, vocabulary |
| `south-provider-api` | unchanged, unless the manifest needs a schema change (none expected; §2.1 uses shipped forms) | — |
| `south-contracts` | unchanged | — |
| `south-north-codec` | workspace version | §4 |
| `provider-openai-responses` | 1.1.0 → **1.2.0** | New family and new body fields |
| every other package | patch | Q47 (the conformance crate is in every `component.wasm`) |

Proposed for **0.56.0**, released once §3.2 (a) and (b) are recorded. The four request fields (§4) do not depend on
the measurement and could ship earlier on their own if lv prefers (**RQ3-Q7**); that would be one more full patch wave.

## 7. Test plan

- Gate ① accepts the §2.1 manifest; negative rows: the record's two-slot sketch, a file-keyed `import`, `account_id`
  without a syntax.
- Gate ② over `fixtures-openai-codex/` against the sandboxed component, byte for byte; the existing
  `fixtures-openai-responses/` pack unchanged except §4 rows.
- Gate ③ `south.credential-recipe.v1` with the production recipe: refresh, rotation and write-back, fixed window,
  `must_equal_field` conflict, missing refresh material, 4xx `reauth_required`, 5xx transient.
- Codec: the four fields round-trip into extensions; each bound refused; `null` equals absent; a request without them
  unchanged byte for byte (golden).
- `shipped_packages_v1`: every 0.55.0 identity retires; the Responses package declares the runtime that first admits
  it (unchanged 0.54.0, since no new manifest field is used).
- Mutations: drop the account header, drop `store`, let `stream=false` through, map 5xx to `reauth_required`, drop
  `prompt_cache_key` from the body — each must fail a named test.

## 8. Open questions for lv

- **RQ3-Q1** Persist the refreshed `account_id` (`persist: true`). **Recommended: yes** — the host does not persist it
  today (§3.4), so a credential imported without an account id sends no account header until the next refresh.
  **Ruled (lv, 2026-10-12): as recommended.**
- **RQ3-Q2** `account_id` value syntax `token` (RFC 9110). **Recommended: yes** — it goes into a header; the test's
  `printable_ascii: 128` would admit spaces.
  **Ruled (lv, 2026-10-12): as recommended.**
- **RQ3-Q3** Carry the four fields as named request extensions under the NC-1 D5 exception (§4.2 B), not as typed kernel
  fields. **Recommended: yes**; R-Q15 promotes them later.
  **Ruled (lv, 2026-10-12): as recommended.**
- **RQ3-Q4** Reuse the Chat codec's raw names (`user`, `metadata`, `prompt_cache_key`) plus `truncation`.
  **Recommended: yes** — one vocabulary for both north surfaces.
  **Ruled (lv, 2026-10-12): as recommended.**
- **RQ3-Q5** `openai-codex` sends none of the four until §3.2 measures the backend. **Recommended: yes.**
  **Ruled (lv, 2026-10-12): as recommended.**
- **RQ3-Q6** Take the IR tool-usage bucket out of R3 and keep R-Q3 (§5). **Recommended: yes** — this re-opens lv's
  2026-10-12 ruling on new evidence; if lv keeps the ruling, the bucket is a kernel-chain change of its own, and R3's
  release waits for it.
  **Ruled (lv, 2026-10-12): as recommended.**
- **RQ3-Q7** Ship §4 with R3 in 0.56.0 rather than in a release of its own. **Recommended: with R3** — one patch wave
  instead of two; the measurement gates only the Codex family, and if it stalls, §4 ships alone then.
  **Ruled (lv, 2026-10-12): as recommended.**
- **RQ3-Q8** Keep `scope` in the recipe (§2.2 item 1). **Recommended: yes**, until §3.2 (d) shows it is not needed.
  **Ruled (lv, 2026-10-12): as recommended.**
- **RQ3-Q9** (raised by the implementation, §9 item 8) A 429 from the token endpoint is `transient`, as for
  `github-copilot`, instead of the vocabulary default `reauth_required`. **Recommended: yes.**
  **Ruled (lv, 2026-10-12): as recommended.**
- **RQ3-Q10** (§9 item 2) The recipe's fixtures run in gate ②'s `CredentialRecipeMatch`, not in the host-implemented
  gate ③ suite. **Recommended: yes.**
  **Ruled (lv, 2026-10-12): as recommended.**

## 9. Implementation (2026-10-12)

Implemented on branch `responses-r3-codex` after the rulings of §8; merged as #186; released in 0.56.0. The §3.2 measurement is
**pending** (an operator action on a real Codex account): until (a) and (b) are recorded here the release waits
(§6), and (c) and (d) are recorded when measured. Commits, in order: the north codec (§4); the conformance crate,
the package and the Q47 bumps; these notes.

### 9.1 What landed

- **North codec (§4.3).** `chat_request_from_responses` validates `user`, `metadata`, `prompt_cache_key` and
  `truncation` and writes them into `ChatRequest.extensions` under those names, the three Chat names the Chat codec
  already copies (RQ3-Q4). Bounds, fail closed with `invalid_request`: `user` and `prompt_cache_key` strings of at
  most 512 bytes; `metadata` an object of at most 16 string-valued entries, keys at most 64 and values at most 512
  characters; `truncation` `auto` or `disabled`. `null` is absent. New public constants
  `RESPONSES_REQUEST_FIELD_MAX_BYTES`, `RESPONSES_METADATA_MAX_ENTRIES`, `RESPONSES_METADATA_KEY_MAX_CHARS`,
  `RESPONSES_METADATA_VALUE_MAX_CHARS`. A golden test pins the IR of a request without the four fields to the bytes
  the codec produced at `7ef9995`.
- **Package `provider-openai-responses` 1.2.0 (§2).** Providers `openai-responses` and `openai-codex`;
  `request_facts.openai-codex` with `output_cap: []`, model `/model`, stream `/stream`; `immutable_body_paths`
  `store`, `stream`, `instructions`, `temperature`, `max_output_tokens`; `north_passthrough: responses`; a
  `credentials` section with `families: ["openai-codex"]`, the three fields, `require_one_of`, `import` from
  `codex-auth-json` (`/tokens/access_token`, `/tokens/refresh_token`, `/tokens/account_id`), and slot
  `provider_api_key` minted by recipe `codex`. The recipe is the test recipe `codex()` with exactly the three §2.2
  changes and item 8 below (`openai_codex_conformance_v1::the_recipe_is_the_test_recipe_with_the_r3_changes` pins
  the difference). `south_runtime` stays 0.54.0: gate ① at `v0.54.0` admits every form used.
- **Reference (§3.1).** For `openai-codex`: `{base_url}/codex/responses`, `accept: text/event-stream`,
  `chatgpt-account-id` from `ProviderConfig.declared["account_id"]` only, `stream: true`, a non-streaming request a
  400 capability error at build time, `instructions` defaulted, no `temperature`, no `max_output_tokens`, none of the
  four client fields (RQ3-Q5), `store: false`, `input` always an item list, NC-1 unchanged (a declaring model gets
  `include`). The response, stream, failure-frame and usage rules are the family-neutral ones (the response-side
  functions take no configuration). For `openai-responses`: the four fields from their extension keys, held to the
  codec's bounds (§9.2 item 5).
- **Gate ② (record §12.2, moved here by record §17.4).** A stream case may carry
  `provider.stream.<case>.meta.json` = `{"usage_chunk": <index>, "usage_pointer": "/…"}`; `UsageNeverDefaulted`
  deletes the pointer from that chunk's `data:` JSON and requires a provider protocol error with no usage event.
  `CaseV1` gains `usage_chunk`. Existing packs are unchanged.
- **Fixtures (§3.3).** `fixtures-openai-codex/`: 6 request rows, 9 stream rows (5 with the usage sidecar), 10
  credential rows for the production recipe; sources in its README. `fixtures-openai-responses/` gains
  `request.request-fields` (codec-derived), `request.refused-request-field-out-of-bounds` and
  `request.refused-unknown-truncation`; every other row is unchanged. The documentation-derived usage judge covers
  the new pack.
- **Tests.** `openai_codex_conformance_v1` (14 tests), new tests in `openai_responses_conformance_v1` and
  `openai_responses_vocabulary_v1`, `south-north-codec`'s `responses_request_fields` (6 tests), and
  `shipped_packages_v1::r3_retires_every_published_055_package_identity`.

### 9.2 Decisions taken where the record is silent

1. **Two directories, one gate ② pack.** The manifest's `conformance.fixtures` still names
   `fixtures-openai-responses/`. The response-side functions take no configuration, so the Codex directory carries no
   response, error or capabilities rows, and the manifest's recipe owes credential rows that only the Codex directory
   carries: neither passes alone (`the_responses_directory_alone_does_not_pass_gate_two`). Every gate ② run of the
   package — native, sandbox parity, and so the release's one gate ② report per package — loads both
   (`tests/support/openai_responses_packs.rs`). Every Codex row is named `codex-*`, so no name repeats.
2. **Where the recipe's fixtures run.** §3.3 and §7 place them in "the gate ③ suite `south.credential-recipe.v1`".
   They run in gate ②'s `CredentialRecipeMatch` (south's reference interpreter, the south half of that schema), as
   `github-copilot`'s and Kling's do, and cover every §7 item: refresh with `scope`, rotation and write-back, the
   fixed window, the `must_equal_field` conflict, missing refresh material (`use_stored`, and neither token), 4xx
   `reauth_required`, 5xx transient. The host-implemented gate ③ suite is unchanged: its recipes reach only the
   `.invalid` fake endpoint (its constant's contract), its cases are numbered host evidence in `compatibility.json`
   (one host records `credential_recipe` verified with 12 cases), and its `Rotating` kind already exercises the host
   invariants this recipe relies on. Adding the production recipe there would change that evidence, which §6 does not
   list. **Ruled (lv, 2026-10-12, RQ3-Q10): gate ② as implemented; §3.3 and §7 read accordingly.**
3. **`response.nonzero-tool-usage` (§3.3)** is the stream row `stream.codex-nonzero-tool-usage`: Codex only
   streams, and the name would repeat the Responses pack's response row.
4. **The stream sidecar** is opt-in per row; it is not made mandatory for the `stream.usage-terminal` row of every
   reported package, which would change every pack. The chunk must hold exactly one `data:` line; the check passes on a
   provider protocol error returned by the parser or emitted as an `error` event with no `usage` event.
5. **The component re-checks the four fields.** The Chat codec copies `user`, `metadata` and `prompt_cache_key`
   verbatim, so a Chat request routed to an `openai-responses` row now sends them when they are in bounds and is
   refused (400 `invalid_request`) when they are not; before 1.2.0 they were dropped silently. A value the Responses
   codec admits always passes. `null` is absent here too.
6. **No refresh margin is declared**, as in the test recipe: the host's default (30 s) applies. The native arm's 300 s
   (record §10.2) would need `min_ttl_seconds` above it under gate ① (boundary record §13.3).
7. **No import `seed`** (record §10.2's sketch had one; §2.2 does not): an imported access token is used as stored
   only without a refresh token (`use_stored`); with one, the first request refreshes.
8. **429 from the token endpoint is `transient`.** The vocabulary default is `reauth_required` (record §10.2, §7 here);
   `github-copilot` declares 429 `transient` so a rate limit does not latch a working credential. **Ruled (lv,
   2026-10-12, RQ3-Q9): the same for Codex** — the refresh step declares `on_status: {"429": "transient"}`, and the
   row `credential.on-status.codex-rate-limited` expects `transient`. The host's native arm maps every non-2xx to an
   operator error (§3.4), so this is one more registered difference for the host's dual run.
9. **Import file kind** `codex-auth-json`; the three fields carry operator-form descriptions.
10. **A failure frame whose `response` object has no `id`** (the shape of the host's unit test
    `codex_response_failed_keeps_the_upstream_error_message`) is a provider protocol error, not the mapped failure:
    the parser binds a present `response` object's id (record §6.4, rules 1 and 4 still apply). The Codex row carries
    an id. A host dual run may see the difference on such a frame.
11. **Fixture values**: model `gpt-5-codex`, base URL `https://chatgpt.com/backend-api`, account `acct-1`; all
    illustrative, none captured.

### 9.3 Failing-first evidence

- With only the four bound constants added, 4 of the 6 new codec tests failed (the fields were dropped and no bound
  was enforced); the golden test passed before and after.
- Against the 1.1.0 reference (plus a `CODEX_FAMILY` placeholder so the tests compile), 8 of the 41 tests in the
  Codex, Responses-conformance and vocabulary binaries failed, among them `gate_two_passes_over_the_shipped_pack`,
  `codex_requests_never_carry_what_the_family_removes`, `the_account_header_comes_from_the_declared_attribute_only`
  and `only_the_named_extension_keys_change_the_body`.
- Mutations (§7), each applied alone and then restored:

  | Mutation | Red tests (besides `gate_two_passes_over_the_shipped_pack`, red in every row but the last) |
  |---|---|
  | Drop the account header | `the_account_header_comes_from_the_declared_attribute_only` |
  | Drop `store` | `codex_requests_never_carry_what_the_family_removes`, `every_request_is_stateless_whatever_the_ir_carries` |
  | Let `stream=false` through | `a_non_streaming_codex_request_is_refused_at_build_time` |
  | Map 5xx to `reauth_required` (`on_status`) | `the_credential_rows_show_the_r3_test_plan`, `the_recipe_is_the_test_recipe_with_the_r3_changes` |
  | Drop `prompt_cache_key` from the body | `only_the_named_extension_keys_change_the_body`, `the_client_fields_are_held_to_the_codec_bounds` |
  | Skip the stream form of `UsageNeverDefaulted` | `every_codex_stream_row_that_reports_usage_runs_usage_never_defaulted`, `the_stream_usage_check_bites_on_a_component_that_defaults_missing_usage`, `a_stream_sidecar_that_names_nothing_fails_the_check` |

### 9.4 Versions (Q47)

| Item | Before | After |
|---|---|---|
| `south-component-conformance` | 0.53.0 | 0.54.0 |
| `south-contracts`, `south-provider-api` | 0.52.0, 0.52.0 | unchanged |
| `south-north-codec` | workspace (0.55.0) | workspace, unchanged until the release |
| `provider-openai-responses` | 1.1.0 | 1.2.0 |
| `provider-openai-compatible` | 2.5.0 | 2.5.1 |
| `provider-gemini` | 1.2.0 | 1.2.1 |
| `provider-anthropic` | 1.0.19 | 1.0.20 |
| `provider-anthropic-bedrock-invoke` | 1.0.3 | 1.0.4 |
| `provider-bedrock-converse` | 1.0.16 | 1.0.17 |
| `provider-bedrock-converse-bearer` | 1.0.8 | 1.0.9 |
| `task-kling` | 1.0.15 | 1.0.16 |
| `task-kling-v2` | 0.32.13 | 0.32.14 |
| `task-minimax-v2`, `task-bailian-v2` | 0.31.12 | 0.31.13 |
| `task-xai-v2`, `task-veo-v2`, `task-wan-image-v2`, `task-gmi-image-v2` | 0.35.12 | 0.35.13 |
| `task-byteplus-v2` | 0.36.12 | 0.36.13 |
| `embeddings-openai-compatible` | 1.0.6 | 1.0.7 |
| `embeddings-gemini` | 1.1.3 | 1.1.4 |
| `embeddings-vertex` | 1.0.5 | 1.0.6 |
| `image-azure` | 1.0.2 | 1.0.3 |

No package's `south_runtime` moves (`shipped_packages_v1::r3_retires_every_published_055_package_identity`;
`provider-openai-responses` keeps 0.54.0, and `scripts/check-declared-runtime.sh --build` loads it under that tag).
The workspace version and `compatibility.json` are not touched; this is not a release.

## Revision note

- 2026-10-12: first draft (proposed).
- 2026-10-12: lv ruled RQ3-Q1 to RQ3-Q8 as recommended (RQ3-Q6 re-rules the same day's scheduling of the tool-usage
  bucket with R3); the status is accepted.
- 2026-10-12: implemented (§9, new); the status line notes it. The §3.2 measurement is still pending.
- 2026-10-12: released in 0.56.0 before the §3.2 measurement (lv): rows are not cut over to `openai-codex` until it is
  recorded; a package change it requires ships as a patch release.
- 2026-10-12: lv ruled the two implementation questions of §9: RQ3-Q9, a 429 from the token endpoint is `transient` for
  Codex (item 8; package changed); RQ3-Q10, the recipe's fixtures run in gate ② (item 2). Release order (lv,
  2026-10-12): this step ships first as 0.56.0; KC1 (canonical IR 4) follows as 0.57.0, rebased onto it.
