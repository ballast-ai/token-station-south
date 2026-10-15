# Provider world: `provider-vertex` (Gemini models on Vertex AI, text)

Status: accepted (lv reviewed it on 2026-10-15 and ruled VX-Q1 (a) and VX-Q2 `vertex-ai` as recommended, §5).
Implemented (see Implementation).

Date: 2026-10-15

Baseline: south `origin/main` = `4bc59ac` (release 0.65.0). Host references carry a `server:` prefix and refer to
token-station-server `origin/dev-v2`; `server:…/` abbreviates `server:gateway/src/modules/inference/`. Statements marked
**unverified** come from reading code or Google's documentation, not from running anything or capturing traffic.

Related records: [`2026-10-14-generate-content-request-passthrough.md`](2026-10-14-generate-content-request-passthrough.md)
(D7, §1 and item 5: "a Vertex text family declares the same when it exists"), [boundary record](2026-09-30-host-zero-vendor-boundary.md)
§3 (credential recipes; "a package needing two different credential kinds for two families … is not expressible yet")
and §13.10 (SF27, Vertex's per-chunk `usageMetadata`), [I3-3](2026-10-13-image-i3-3-vertex.md) (the Vertex location,
project, base URL and credential precedents, V1–V7), embeddings record §16 (`embeddings-vertex`).

## 1. Why

The host's native `vertex_ai` text arm is the last native arm behind the Gemini `/v1beta` surface (speedup plan T5b). D7
made that surface accept `component` rows (host step 2, `server:c286be2d`) and left step 3 waiting for a south Vertex
text family. Without one, a Vertex text row cannot move to a component and the native arm cannot be deleted.

## 2. Decision: a new package, not a second family in `provider-gemini`

`provider-gemini` authenticates with a static secret header (`x-goog-api-key` on `provider_api_key`); Vertex needs a
`bearer` minted by the `vertex_sa` recipe and two configuration values. A package carries one `credentials` section for
the families it covers (boundary record §3), so the two cannot share a package without the "scoped sections" extension
that record defers. The image and embeddings worlds already split the same way (`image-gemini` / `image-vertex`,
`embeddings-gemini` / `embeddings-vertex`). So: a new package `provider-vertex`, world `provider-adapter-v2`, one family.

## 3. What the package does

**Request and response: `provider-gemini`'s, shared.** The IR → `contents` / `generationConfig` / `tools` body, the
stream parser (with SF27's terminal-chunk usage rule, which was found on Vertex), the response reader, refusals and
outcomes are one implementation for both packages: the Gemini reference is refactored so its body, parse and stream
halves are reused, as `gemini_image` is shared by `image-gemini` and `image-vertex`. Only the route, the auth and the
values differ:

- **Route**: `POST {base}/v1/projects/{project}/locations/{region}/publishers/google/models/{model}:generateContent`,
  or `:streamGenerateContent?alt=sse` when streaming; each of `project`, `region`, `model` one percent-encoded segment
  (`vertex_location::publisher_model_path`, as `image-vertex`). Native builds the same shape
  (`server:…/engine/vertex.rs:147` `build_vertex_url`, and `text_admission.rs:839-858`).
- **Auth**: `{"arm": "bearer", "slot": "provider_api_key"}`, the slot minted by `vertex_sa`. No other header.
- **Values**: the family's `config_schema` (`region` required, `project` optional) and the `credentials` section copied
  byte for byte from `image-vertex` / `embeddings-vertex`, so one provider row and one credential's recipe fields serve
  all three Vertex packages. Project and region resolve, and the base URL is refused for another region's Vertex host,
  exactly as in I3-3 §4 (`vertex_location`).
- **Declarations**: `north_request_passthrough` `{"<family>": "generate_content"}` (D7) and `north_passthrough` as
  `provider-gemini` declares them; `request_facts` with the output cap at `/generationConfig/maxOutputTokens` and
  `stream: "url"`; `model.url` matches the `…/models/{model}:` suffix of the Vertex path (**to confirm** against
  gate ①'s pattern rule when implemented).
- **Capabilities**: `chat`, `stream`, `tool_call`, as `provider-gemini`. No `count_tokens` (D7 Q4: countTokens on a
  component row stays 400).

## 4. Differences from the native arm that follow precedent (accepted under lv's 2026-10-13 pre-approval)

- **P1 BYOK refused** (D9; I3-3 V1, embeddings precedent).
- **P2 Row endpoint must be the region's origin**, another region's Vertex host refused before admission; native ignores
  the endpoint and derives the host from the region (I3-3 V2).
- **P3 Missing region or project** is a 500 naming the key before admission; native answers 400 (I3-3 V3).
- **P4 Path segments percent-encoded** (I3-3 V4).
- **P5 The token comes from the recipe executor**, not the native token cache (I3-3 V6).
- **P6 Every `provider-gemini` difference already registered for component Gemini rows** applies unchanged, since the
  request and response code is shared (the D7 dual-run differences: non-stream `accept`, `x-goog-api-client` not
  forwarded, component non-2xx bodies as is).

## 5. Open questions for lv

**Ruled by lv on 2026-10-15: VX-Q1 (a), `generateContent` for every surface, and VX-Q2, the family is `vertex-ai`.**
The items below keep their text; each recommendation is the ruling. `<family>` in §3 reads `vertex-ai`.

- **VX-Q1 Which upstream wire serves the chat, messages and responses surfaces.** Native serves `/v1beta` through
  `generateContent`, but serves `/v1/chat/completions` and `/v1/messages` on a Vertex row through Vertex's
  **OpenAI-compatible** endpoint `…/endpoints/openapi/chat/completions` (`server:…/engine/vertex.rs:162-180`,
  `text_admission.rs:870-878`; the code marks the endpoint shape "not verified by a live leg", and Google requires a
  `google/` prefix on the model there, which the row's `upstream_model` must carry). A component translates one IR to
  one wire. Options: (a) `generateContent` for every surface, as `provider-gemini` does: one model id form (no prefix),
  D7 served, SF27 and the Gemini reference reused; the chat surfaces' upstream request changes from OpenAI-shaped to
  Gemini-shaped, registered as a difference, and rows whose `upstream_model` carries `google/` must drop it at cutover;
  (b) the OpenAI-compatible endpoint for the IR surfaces: nearer native on chat, but `/v1beta` could not be served by
  the same package (D7 needs `generateContent`), the model id form would differ between surfaces, and the native shape
  is unverified. **Recommended: (a).**
- **VX-Q2 The family name.** `vertex-ai` (the platform name, as `embeddings-vertex` uses in its world; provider-world
  families are named by platform, like `gemini`) or `vertex-ai-text` (distinct from the other worlds' families, as
  I3-3 chose `vertex-ai-image`). Family names live in each world's own bindings, so they cannot collide. **Recommended:
  `vertex-ai`**, matching how the native dialect is named and how an operator reads a `component` row's
  `south_dialect`.

## 6. What the host must do before a cutover

1. Re-pin to the release that ships `provider-vertex`.
2. D7 step 3: let the `/v1beta` entry route a `component` row of this family (the package declares
   `north_request_passthrough`); today only `gemini` is wired.
3. Component rows for Vertex: `south_dialect` = the family; recipe fields through the existing `component` row path
   (P21 S3 C5); endpoint confirmation (R23) for this package's digest (a confirmation of `image-vertex` or
   `embeddings-vertex` does not carry over); `config_values.region` = `providers.region`, `config_values.project` =
   `providers.group_id` when set; the row endpoint = the region's origin.
4. Per VX-Q1 (a): rows whose `upstream_model` carries `google/` drop the prefix; the cutover preflight lists them.
5. IR billing: Vertex rows are already billed from the Gemini IR on the native arm (the host maps `vertex_ai` to the
   Gemini dialect, `server:…/engine/south_component.rs:655-658`), so a component row bills the same way.
6. A dual run against the native arm on all three IR surfaces and `/v1beta`, stream and non-stream, with the P1–P6 and
   VX-Q1 differences asserted; then a cutover preflight and the C10-style real-key evidence before production.

## 7. Versioning (Q47)

`south-component-conformance` changes (the Gemini reference is split for sharing, a new reference), so it moves one
step past 0.64.0 and every package published with 0.65.0 takes a patch with unchanged behaviour. `provider-vertex`
1.0.0 declares the oldest runtime that admits its declarations (`north_request_passthrough` needs 0.60.0, the minted
slot and `vertex_sa` are older), to be confirmed by `check-declared-runtime`. No contract change.

## Implementation

### What landed

- **The shared wire** (§3): `provider-gemini`'s request body, response reader, stream parser (with SF27) and refusals
  moved, unchanged, from `reference_gemini.rs` into the crate-private module `gemini_text`. The module exposes
  `serves` (the family check), `refuse_foreign_reasoning_replay`, `require_model`, `operation` (the `:method` suffix and
  query), `descriptor` (the JSON `POST` with the body and no auth), `parse_response`, `map_provider_error` and
  `stream_parser`; the parser's unit tests moved with it. `reference_gemini.rs` keeps the family `gemini`, the
  `v1beta/models/{model}:{method}` route and the `x-goog-api-key` header, and calls the shared checks in the order it
  always had (Claude replay, `OpenAI` replay, family, model). Its fixed `config.provider != "gemini"` became
  `gemini_text::serves(config, "gemini")`, with the same message. `fixtures-gemini/` and the Gemini tests are unchanged.
- **`reference_vertex.rs`**: family `vertex-ai`; the family check first, then
  `vertex_location::publisher_model_path` (region, then project, then the base URL check), then the request refusals,
  so a configuration error is a 500 naming the key before any request refusal (P3, as `image-vertex`); the URL is
  `{base}/{path}` plus `?alt=sse` when streaming; `Auth::bearer` on the configured slot.
- **`components/provider-vertex`** 1.0.0: as §3 lists. `config_schema["vertex-ai"]` and `credentials` are
  `image-vertex`'s byte for byte (a test compares the manifest text). `south_runtime` 0.60.0.
- **`fixtures-vertex/`**: fixtures-gemini's rows with the route, auth and values adapted; the host's Vertex
  thinking-exhausted request and its non-streaming and streaming answers; the location rows; `image-vertex`'s five
  credential cases. Its README records where each expectation came from.
- **Tests**: `tests/vertex_conformance_v1.rs` (gates ① and ②, the decided rows, the byte equality, the configuration
  refusals by name, the family check in both directions, the shared code over every `fixtures-gemini/` case, two fault
  components) and `tests/vertex_sandbox_parity_v1.rs` (the guest passes gate ② byte for byte, its report equals the
  native one, and the configuration refusals are byte-identical across the ABI); build script; `ci.yml` / `release.yml`
  / `shipped_packages_v1` (38 official components).

### Interpretations

1. **`request_facts.model.url` is `/publishers/google/models/{model}:`.** Gate ① admits a template that starts with
   `/`, holds exactly one `{model}` and no other brace, `?`, `#` or space; gate ② requires the URL path to *contain* the
   template with the model encoded as one segment. `provider-gemini`'s `/models/{model}:` would also pass (it is a
   suffix of the Vertex path); the longer form pins the publisher segment, which is the part of the Vertex path that
   names the model.
2. **The region mismatch is a pack row as well as a named refusal.** Unlike the image suite, the provider suite admits
   any error envelope as a request case's expectation, so
   `provider.request.another-regions-vertex-host-is-refused` sits in the pack. The other configuration refusals (missing
   region, missing project, the prefixed `global` host) and the mismatch are pinned by name in the suite test against a
   request the package would otherwise refuse, and through the guest in the parity test.
3. **Mirrored rows are adapted by rule.** `fixtures-gemini/`'s request rows were adapted mechanically (row, URL, auth)
   rather than re-derived; the shared-code test holds both references to the same output over every Gemini case, so a
   mirrored row cannot hide a difference. Gemini's `provider.request.chat` is `provider.request.system-and-sampling`
   here, because `provider.request.chat` is the host-transcribed request.
4. **No `endpoint` declaration.** The origin follows the region, so the package declares none, as `image-vertex`.

### Contradiction flagged

- **§5 VX-Q1 overstates what changes.** The record says native serves `/v1/chat/completions` and `/v1/messages` on a
  Vertex row through the OpenAI-compatible endpoint. The host's own tests show `/v1/chat/completions` on a `vertex_ai`
  row reaching `…/publishers/google/models/gemini-2.5-flash:generateContent` with the Bearer token
  (`server:gateway/tests/gemini_thinking_exhausted.rs`, `vertex_chat_nonstream_thinking_exhausted_returns_length_and_settles`;
  the `(VertexAi, GeminiGenerate)` arm of `text_admission.rs`), and the Responses surface falls back through chat
  (`sender/responses.rs`). Only `/v1/messages` uses `…/endpoints/openapi/chat/completions`
  (`server:gateway/tests/messages_providers.rs`). Under VX-Q1 (a) the registered difference is therefore the messages
  surface's upstream wire (and the `google/` prefix on rows that serve it), not the chat surface's. The ruling is
  unaffected; §6 item 4's preflight should list only rows that carry `google/`.

### Open questions

- None blocking. The record's §6 host steps are unchanged; the dual run (§6 item 6) should cover `/v1/messages` against
  the OpenAI-compatible native arm as the one surface whose upstream wire changes.

### Gate results

Run locally on the final tree (macOS, toolchain 1.96.0, root lockfile with wasmtime 49.0.3 and rustls 0.23.46), mirroring
`ci.yml` with `set -o pipefail`:

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass (the first run flagged `unnecessary_wraps` on the moved `map_provider_error`, which never fails; it now returns the envelope and both references wrap it) |
| `PROPTEST_CASES=32 cargo nextest run --workspace --all-features` | 2146 passed, 4 skipped (12 in `vertex_conformance_v1`, 5 in `vertex_sandbox_parity_v1`; every Gemini test unchanged and passing) |
| `cargo test --workspace --doc --all-features` | 3 passed |
| `cargo test --workspace --no-default-features` | 1975 passed |
| `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --all-features` | pass |
| `cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked` | pass |
| `scripts/check-boundaries.sh --self-test` and `scripts/check-boundaries.sh` | pass |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | 72 passed |
| `scripts/check-language.sh` (self-test, files, commits since `origin/main`) | pass |
| `cargo machete` (root and `fuzz/`) | pass |
| `cargo deny check` and `cargo audit` (root and `fuzz/`) | pass |
| every `scripts/build-*-component.sh`, then `scripts/check-declared-runtime.sh --build` | pass: `provider-vertex` loads under the v0.60.0 runtime; with its `south_runtime` set to 0.59.0 the v0.59.0 runtime refuses it (`unknown variant generate_content`), so 0.60.0 is the oldest runtime that admits it |

Mutations (each applied to the source, the suite test run, the source restored from a copy):

| Mutation | Red tests in `vertex_conformance_v1` |
|---|---|
| `provider-vertex`'s family check removed | `each_package_refuses_the_other_family` |
| `provider-gemini`'s family check removed | `each_package_refuses_the_other_family` |
| another region's Vertex host accepted (`base_of` check disabled) | `gate_two_passes_over_the_shipped_pack` (the region-mismatch row), `a_configuration_error_is_a_500_before_any_request_refusal` |
| the exported `project_id` read before the `project` override | `gate_two_passes_over_the_shipped_pack` (the project-override row), `a_configuration_error_is_a_500_before_any_request_refusal`, `accepting_a_vertex_base_of_another_region_fails_its_rows` |
| the request refusals run before the location | `a_configuration_error_is_a_500_before_any_request_refusal`, `accepting_a_vertex_base_of_another_region_fails_its_rows` |

### Versions (Q47 against v0.65.0)

`south-component-conformance` 0.64.0 → 0.65.0 (the shared module and the new reference). Every package published with
0.65.0 (all 37) takes a patch with unchanged behaviour and keeps its `south_runtime` and its contracts; the reference
`VERSION` constants move with them, and `provider_vertex_retires_every_published_065_package_identity` pins each new
identity. `south-contracts` (0.56.0) and `south-provider-api` (0.58.0) are unchanged. `provider-vertex` ships at 1.0.0
with `south_runtime` 0.60.0.

## Revision note

- 2026-10-15: first draft (proposed).
- 2026-10-15: lv accepted the record and ruled VX-Q1 (a) and VX-Q2 `vertex-ai` as recommended; status accepted.
- 2026-10-15: implemented; Implementation section added (what landed, interpretations, the VX-Q1 contradiction, gate
  results, versions).
