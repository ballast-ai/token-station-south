# B7b in south: the combined auth arm and the `gemini-openai-compatible` family

Status: implemented on branch `p21-b7b`; not released. The release that carries it is a separate step.

Date: 2026-10-08

Predecessors: `2026-09-30-host-zero-vendor-boundary.md` (§4.3 and Q4, the combined arm; §10 and §13 B7b),
`2026-09-08-bearer-with-header-secret-auth.md` (the contract's `BearerAndHeaderSecret` arm, auth contract 4),
`2026-10-08-kernel-repin-protocol-0.5.0.md` (the re-pin to protocol 0.5.0, which this change builds on). Kernel side:
the token-station design record `2026-10-03-kernel-b7b-protocol-0.5.0.md`, "Remaining work" item 3.

Host context: token-station-server P21 handoff §4.5, step 2 of 5. Step 3 (#138, gate ② ARN cases), step 4 (Q14) and
step 5 (the minor release and the host re-pin) are not part of this change.

## 1. What B7b asks of south, and what this step does

Protocol 0.5.0 closes four kernel gaps. Each has a different owner on the south side:

| Kernel change | South work | Where |
|---|---|---|
| `Auth::BearerAndHeader` | A manifest word for the arm, admission of the arm, and a family that presents it | This change |
| Open credential header names | Replace `until_b7b_no_component_descriptor_can_name_a_declared_header` | Done in the re-pin (step 1, #151) |
| `Usage::explicit_cache_read_tokens` | None now. No reference reports the bucket, and both hosts keep pricing `cache_read_tokens` until a price table needs the split | Open; see §7 |
| `ProviderConfig::declared`, `ChatRequest::host_values` | A component channel for declared and host-minted values | Q14 (step 4) |

## 2. Manifest vocabulary

`PROVIDER_AUTH_ARMS` gains `bearer_and_header_secret`: the one resolved secret travels twice, as
`Authorization: Bearer <secret>` and verbatim in one sanctioned provider header.

- **Worlds.** The provider world admits it. The task-v1 world shares the provider vocabulary by construction (the
  2026-09-18 task-adapter-world record, D4), so it admits it too. The task-v2 world stays closed to `bearer` and
  `header_secret`.
- **Coherence.** The arm stands on its own. Declaring it does not admit `bearer` or `header_secret`, and declaring
  those two does not admit it. `host_signed` still stands alone. `secret_headers` still needs `header_secret`, because
  the combined arm never presents a declared header (§3).

## 3. Admission

`admit_descriptor_auth` admits `Auth::BearerAndHeader { name, secret }` when both of these hold:

1. The manifest declares `bearer_and_header_secret`. Otherwise it refuses with `BearerAndHeaderNotDeclared`, as
   before.
2. `name` is a sanctioned secret header, compared without case. Otherwise it refuses with `HeaderNotSanctioned`. A
   name the manifest declares in `secret_headers` is refused too, because the contract's
   `ProviderAuthV1::BearerAndHeaderSecret` is closed over `SecretHeaderV1` (combined-arm record, D1). Extending it to
   declared names would be an auth contract change. No provider needs it today.

The result is the new `AdmittedAuthV1::BearerAndHeaderSecret(SecretHeaderV1)`, which a host maps onto
`RawAuthV1::BearerAndHeaderSecret`. The kernel's own checks still run first: `ProviderConfig::authorize` binds the
slot and the endpoint, and `Auth::bearer_and_header` refuses `authorization` as the header name.

## 4. The `gemini-openai-compatible` family

`provider-openai-compatible` serves a fourth family, `gemini-openai-compatible`. It targets Gemini's
OpenAI-compatible surface, which accepts a key only when it arrives in both places.

- **Request.** `build-http-request` posts to `base_url` resolved for Chat Completions, so an operator who sets
  `https://generativelanguage.googleapis.com/v1beta/openai` gets `.../v1beta/openai/chat/completions`. The body is the
  `openai-compatible` family's body. The descriptor's auth is `Auth::BearerAndHeader { name: "x-goog-api-key" }` on
  the configured slot, or no auth when the upstream has no slot.
- **Declarations.** The family uses the top-level request facts. It declares no endpoint template, config schema,
  user-agent or credential recipe, so it is configured like `openai-compatible` and `azure-openai-v1`. A fixed
  endpoint template for Google's host is possible later. It would refuse operator proxies, so it is left to the owner
  (§7).
- **Response and usage.** These are unchanged. The response-side functions cannot tell families apart (R6), so the
  family uses the OpenAI dialect's strict usage reader: `prompt_tokens`, `completion_tokens` and `total_tokens` are
  required, and a total that is not their sum (allowing for orchestration input) is a protocol error. The host
  settles this surface today with the same rule (its OpenAI Chat usage evidence). If Gemini reports thinking tokens
  outside `completion_tokens` but inside `total_tokens`, the response fails. It is never under-counted. This step did
  not measure the surface against a live upstream.
- **Gate ②.** A new request fixture, `provider.request.gemini-openai-compatible`, pins the URL, the body and the
  combined arm. `DescriptorAuthWithinManifest` refuses it under a manifest without `bearer_and_header_secret`.

## 5. Versions and compatibility

- **`provider-openai-compatible` stays 2.3.0.** It was published as 2.2.1 in v0.45.0. #150 moved it to 2.3.0, and that
  version has not been released, so the new family and arm ride the same minor step. No other package changes.
- **`south_runtime` stays 0.44.0** (lv's 2026-10-08 decision in the re-pin record §5). Under the declared-runtime
  discipline this package now also relies on a manifest word that a v0.44.0 or v0.45.0 runtime's gate ① refuses
  (`AuthArmIsNotInTheWorldVocabulary`). The step-5 move of every package to 0.46.0 covers this.
  `scripts/check-declared-runtime.sh --build` still fails until then, as it did after the re-pin.
- **No contract number changes.** Auth contract 5 already carries `BearerAndHeaderSecret`, and the
  `south.header-auth.v1` case `BufferedBearerAndHeaderSecretSuccess` already pins its wire. `compatibility.json` is
  unchanged.

## 6. Hosts

- **Breaking for a host that matches `AdmittedAuthV1` exhaustively:** it gains `BearerAndHeaderSecret`.
- To serve the surface through the component, a host configures a row of family `gemini-openai-compatible` and
  presents strictly from the admitted arm. The host's own table entry for Gemini's `/openai/` shape can then retire
  with the rest of P21 §2.5.

## 7. Open for the owner

1. **Explicit cache reads.** `provider-openai-compatible` reads Bailian's cache counts into `cache_read_tokens`
   (`cached_tokens`) and `cache_write_tokens` (`cache_creation_input_tokens`). It does not report
   `explicit_cache_read_tokens`.
   Reporting that bucket changes funds evidence, needs a usage judge case and must match a host price table. It is left
   for a decision with the host's pricing work.
2. **An endpoint template for `gemini-openai-compatible`.** One would confine the credential to Google's host but
   refuse operator proxies.
3. **A usage judge case for Gemini's OpenAI-compatible usage.** It needs Google's documentation or captured traffic
   showing how `completion_tokens` and `total_tokens` treat thinking tokens.
4. **The combined arm in the task-v1 world.** It follows D4 here. A task-v1 package that declares it relies on the
   host's task path mapping the new admitted arm.
