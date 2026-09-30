# Zero vendor logic in the host: revising the south boundary

Status: proposed — drafted for review by the host team (token-station-server P21), not accepted

Date: 2026-09-30

Predecessors: `2026-08-21-canonical-ir-inventory.md` (S0: D1 usage extraction belongs to the component, D2 chunks
are bytes), `2026-08-27-manifest-schema-beyond-one-world.md` (manifest validated per world; `host_signed` and
`emits`), `2026-09-10-released-component-artifacts.md` (release artifacts and `SHASUMS256.txt`),
`2026-09-20-task-adapter-v2-candidate.md` (task-v2 admits only bearer / header_secret);
`ARCHITECTURE.md` "What never enters South" (ruled 2026-09-08).

Origin: token-station-server plan P21 (`docs/product-review-v2/plans/2026-09-29-P21-*.md`)
(DP0 and DP1 decided; the decisions of P21 §8.3 "before stage 2 starts" — DP3, DP4, DP5, DP9, DE1, DE2 and
others — were approved as recommended on 2026-09-30, as relayed by the host team), P24 in the same directory
(DE1, DE2), and the red items measured in the P21 §5 S0 pilot. The embeddings contract is in
`2026-09-30-embeddings-contract.md`.

Rulings: on 2026-09-30 the host owner (lv) ruled on the questions tagged L in §16 — Q1, Q3, Q7, Q10 and Q12; each
ruling is recorded under its question. Q13 still awaits a measurement. Questions tagged S or K remain open for the
south and kernel maintainers.

Baseline: south `origin/main` = v0.42.0 (`3135e36`); kernel `f585bc83` (protocol 0.4.0 / kernel v0.3.0).
South line numbers refer to this baseline. Host line numbers refer to token-station-server `8b2a1976` and carry a
`server:` prefix; kernel line numbers carry `kernel:` and refer to `crates/protocol/src/`.

## 0. Summary

The host's standard, DP0: adding or removing a provider changes only south, with zero host changes. South's
boundary as it stands cannot deliver that — not because components cannot be written to do the translation, but
because south **leaves the host to know** a set of facts that differ per provider: how to mint credentials,
whether the auth arm in a descriptor counts, how the upstream frames its stream, whether usage can be trusted,
where the cap field is, whether a package works across versions, which packages exist to install. This record
gives a revision for each:

| # | Problem | South today | Proposal | Section |
|---|---|---|---|---|
| a | Credential minting | Minting, OAuth refresh and JWT signing are host code, "not a gap South intends to close" (ARCHITECTURE.md:117-126) | The component **describes** the recipe in the manifest with a closed step vocabulary; material, locks, CAS and storage stay in the host; Kling uses the same recipe mechanism rather than `HostSigned` | §3 |
| b | Auth arm | `auth_arms` only validates the vocabulary (manifest.rs:346-364); nothing checks the descriptor's `auth` against it | South provides a descriptor auth admission function; gate ② adds a matching check | §4 |
| c | Framing / signing | Components split SSE themselves; only AWS eventstream is split and re-encoded by the host (reference_bedrock_converse.rs:21-35); the host infers the signing scheme from the type | The manifest declares `stream_framing` per family and `signing.scheme` for `host_signed` (both closed sets); south provides one eventstream deframer | §5 |
| d | Usage | Already ruled to belong to the component (canonical-ir-inventory.md:187-203), but three reference implementations fill missing fields with 0 and gate ② does not require usage samples | Make the reference implementations strict; gate ② requires usage rows and a "delete the usage" mutation check; the manifest declares `usage_evidence`; the host does only out-of-bound and internal-consistency checks | §6 |
| e | Request facts / capabilities | The manifest has no location declarations; `model-capabilities` only echoes what the operator declared | The manifest declares the locations of cap, model and stream and the non-secret config keys; per-model differences go through dialect words; the model catalog becomes a south data artifact | §7 |
| f | Compatibility | The four tuple items must be exactly equal (manifest.rs:657-694) | `runtime_abi` epoch + range + explicit contract numbers; per-package admission report | §8 |
| g | Package discovery | A release carries only archives and `SHASUMS256.txt` (release.yml:98-106) | Publish a machine-readable index, `south-release-index.json` | §9 |
| — | Provider instances in closed vocabularies | Secret header names, query names and quota header names are enums compiled into the host | "Closed mechanisms, declared instances" | §10 |
| h | Follow-on components and non-chat operations | None | Scope and dependencies listed | §11 |

§12 lists what the synthetic unseen-provider guest (T21) must prove, §13 gives the phasing, and §16 lists the
questions that need a ruling from the south maintainers or lv; lv's rulings of 2026-09-30 are recorded there.

## 1. Problem

### 1.1 The standard

- **DP0** (lv, 2026-09-29): the host must contain no provider-specific logic; adding or removing a provider must
  not require modifying host modules.
- **J2**: install a provider component the host has never seen, and it runs end to end. **J3**: remove a
  provider's component and delete its catalog rows, and the host still compiles, tests and starts, with the other
  providers unaffected.
- **DP1**: accept the change in trust model that comes with moving usage parsing into components; the host does
  only out-of-bound and internal-consistency checks, and cannot catch under-reporting or in-bound deviation.

### 1.2 Red items from the S0 pilot that are rooted in south

The P21 §5 S0 pilot, using this repository's T03 guest, measured a set of red items. The ones below are rooted in
south's boundary or release artifacts, not only in the host:

| Red item | Host symptom | Root on the south side |
|---|---|---|
| J2b① auth | The manifest declares only `header_secret`, yet the upstream still receives Bearer | `auth_arms` is only a vocabulary (manifest.rs:234-238, 355-362); south has no public "descriptor auth → contract auth" function, and the raw prelude states that the host picks the auth arm (raw.rs:107), so the host builds a table by type (server:gateway/src/modules/inference/engine/south_adapter.rs:157) |
| J2b③ version gate | Dropping in a package declaring `south_runtime=0.42.1` stops the whole process from starting | Exact tuple equality (manifest.rs:657-694); the host aggregated per-package errors into a process failure |
| J2b④ fetch list | A component name outside the list gets `exit 2` | Releases have no machine-readable index (release.yml:60-133) |
| J2b⑤ cap contract | A nested cap field is refused | The manifest has no cap-location declaration; T03 itself states "Hosts admit only a closed set of cap paths" (crates/south-provider-runtime/tests/guests/t03-canary-provider/src/lib.rs:16-37) |
| J3① cannot remove | Deleting the package for a known dialect silently falls back to the built-in reference implementation | South describes the native reference implementations as a supported host path (2026-09-10-released-component-artifacts.md:152-154) |
| J3③ fails only at request time | Startup succeeds; the 400 arrives only at request time | South provides no per-package admission report, so the host's startup gate does not know which families nothing serves |

### 1.3 Two layers: the package layer and the link layer

South hands the host two kinds of things:

- **Package layer**: `manifest.json` + `component.wasm`, loaded at runtime (loader.rs:154-194). Adding or removing
  packages does not require rebuilding the host.
- **Link layer**: `south-contracts`, `south-core`, `south-provider-api`, `south-provider-runtime`,
  `south-component-conformance` (including the `sandbox` seam the host is using) and the kernel's
  `token-station-protocol` — compiled into the host binary, with versions pinned by the host's `Cargo.toml`.

DP0 holds only when "the new provider lands entirely in the package layer". Any change that needs the link layer
extended — even adding a single value to an enum — means the host re-pins, rebuilds and releases, and J2 is red at
that moment. P21 §1.3 lists "loading a newly released south component package" as not counting as modifying the
host, but does not say whether a link-layer upgrade counts; lv ruled on 2026-09-30 that it does (§16 Q1). Every
proposal in this record states whether a new
provider still touches the link layer afterwards; §10 deals specifically with the category most likely to trip on
this.

## 2. General rules

**R1 Closed mechanisms, declared instances.** Closed sets are used only to name **mechanisms**: auth arms, framing
forms, signing schemes, token-exchange step kinds, JWT algorithms — they correspond to public standards or to
south's own protocol (P21 §1.1, DP4, DP9). **Instances** specific to one provider — header names, query names,
endpoints, field pointers, dialect words — are declared by the component in the manifest, and south validates them
at gate ① against syntax and safety rules. A new mechanism is a south contract upgrade plus one host link-layer
upgrade (what P21 §1.4 calls "a new public-standard executor"); a new instance is just a new package.

**R2 The component declares, the host executes, and the host never chooses by provider identity.** The host writes
one generic executor per mechanism; which executor to use, and with what parameters, is decided only by the
manifest and the descriptor, never by `provider_type`, row name or domain.

**R3 Boundaries that do not move.** Components have no network and no filesystem (manifest.rs:331-344;
loader.rs:24-32); credential values never enter a component (provider-adapter.wit:56-60; the kernel's `Auth`
carries only a `SecretRef`, kernel:http.rs:150-166); the funds discipline — reservation, terminal-frame
withholding, settlement, `delivery_unknown` — stays in the host (canonical-ir-inventory.md:187-203); material whose
leak impact exceeds one key (service-account private keys, KEKs) and logic whose "wrong decision is money or an
unrecoverable credential" (rotation concurrency guards, CAS) stay in the host (ARCHITECTURE.md:117-123). What this
record moves out of the host is only **per-provider description** — not material, and not guards.

**R4 Every new host execution mechanism gets a gate ③ host suite.** The same approach as header-auth and
controlled-query: the mechanism's fixtures are frozen in south, and only once both hosts pass them is the
mechanism marked `verified` under `host_capabilities` in `compatibility.json`. Otherwise "open-source and
closed-source hosts share southbound work" holds for only one host (P21 §7).

**R5 An absent new field must not silently change behavior.** The manifest is `deny_unknown_fields`
(manifest.rs:212-219), so a new field is itself a south-minor wire-format change
(2026-08-27-manifest-schema-beyond-one-world.md:147-160). When a new field is absent, the semantics must equal
today's behavior, or the package must be refused at load; there is no third outcome.

## 3. Credential minting: the component describes, the host executes (S3; problem a)

### 3.1 Today

- The 2026-09-08 ruling (ARCHITECTURE.md:117-126) keeps material and guards in the host with two tests, then
  concludes: "Minting, OAuth refresh, and request signing therefore remain host code by design … A host keeps a
  per-provider authentication layer above South, and that layer is not a gap South intends to close." The raw
  prelude takes the same line: dynamic auth completes before construction, and south receives only the finished
  product (raw.rs:9-12, 737-745). P21 §3.1 cites only raw.rs; what actually needs to change is that concluding
  sentence in ARCHITECTURE.
- The provider world's vocabulary has an `oauth` arm, meaning "the host exchanges a token by name before the funds
  marker; the component never sees the exchange" (manifest.rs:82-84); the kernel's `Auth::OAuth { secret, scopes }`
  means the same (kernel:http.rs:156-165). None of the thirteen packages declares it. It leaves all provider
  knowledge about the exchange in the host.
- task-v2 admits only bearer / header_secret (manifest.rs:141-148), and the loader refuses a task-v2 package that
  imports any `host` namespace (loader.rs:216-224).
- The host today: token exchange for the five families Codex, Claude Code, Copilot, Kiro and Vertex has converged
  on one skeleton plus one `MintStrategy` trait
  (server:gateway/src/modules/inference/engine/token_refresh.rs:387-476; the five implementations start at :120,
  :719, :914, :1279 and :1561; the seven skeleton steps are at :477-493). The trait methods are exactly "everything
  that differs for one family": endpoint and request, response parsing, clock convention, whether it rotates, what
  to do without refresh material, error classification, and Copilot's "404 is another kind of success". Kling's JWT
  sits outside the skeleton (server:gateway/src/modules/inference/engine/upstream.rs:435-460); on the task side
  there are also binding auth recipes named after vendors, `KlingJwt` / `VertexSa`
  (server:gateway/src/core/task_execution.rs:25-38).

### 3.2 Can Kling simply use the existing `HostSigned`?

P21 §3.1 and S3 list this as to be verified. Conclusion: **it does not need to, and it should not**.

1. Kling v2 is already on the **bearer arm** today. `task-kling-v2`'s `auth_arms` is `["bearer"]`
   (components/task-kling-v2/manifest.json); the reference implementation puts the operator's slot into the
   descriptor as `Auth::bearer` (reference_kling_task_v2.rs:325, 369), and the host mints a fresh HS256 JWT on each
   call through the binding recipe `KlingJwt` as that slot's value (server:gateway/src/core/task_execution.rs:31-33).
   The task-v2 candidate settled on exactly this at the time (2026-09-20-task-adapter-v2-candidate.md:32). The
   manifest.rs:128-132 text that P21 cites, "Kling's HS256 JWT … are both task-side families", hangs off the
   **task-adapter-v1** schema; it is a 2026-09-18 prediction (2026-09-18-task-adapter-world.md:178-181;
   2026-09-19-task-world-fit-survey.md:166-167) that the v2 candidate has superseded.
2. Mechanically, `HostSigned` could hold it: `SignedHeaderV1`'s documentation says explicitly that Kling's HS256
   can be one host-signed call (south-contracts/src/lib.rs:930-938), and `SIGNED_HEADER_NAMES` includes
   `authorization` (manifest.rs:97-98). But task-v2 does not admit this arm; using it would first require changing
   the task-v2 world schema.
3. More fundamentally: Kling's JWT contains only `iss` / `exp` / `nbf`
   (server:gateway/src/modules/inference/engine/upstream.rs:437-452) and is **independent of the request bytes**. It
   is a periodically minted bearer credential, not a signature over a formed request. `HostSigned` exists because
   "the signature covers the formed request, so it can only sit between assemble and send"
   (reference_bedrock_converse.rs:6-12); pushing Kling into it merely moves "how to mint" from one host branch into
   another host finalizer, and the J1 count does not change.

So Kling belongs to the recipes of §3.3 (a single `jwt_sign` step whose output is used directly as the bearer);
`HostSigned` stays reserved for schemes that genuinely cover request bytes (SigV4, §5.3).

### 3.3 Proposal: a `credentials` section in the manifest (credential recipe v1)

Four parts:

- **`fields`**: which fields this kind of credential consists of — name, whether it is secret, value syntax,
  whether it is required, media type (e.g. service-account JSON). The host renders the credential form, stores and
  redacts from this, replacing the per-family credential pages (the 260 lines of D2 in P21 Appendix B.3).
- **`import`**: an optional import mapping — which JSON Pointer in which kind of file lands in which field (Codex's
  `auth.json`, Claude Code's credential file, service-account JSON). The host no longer guesses the family from what
  a file looks like.
- **`slots`**: each slot in `permissions.secrets` is either `static` (the operator-entered value is used as is;
  this is what absence means, R5) or `minted` (minted by a recipe). The descriptor still names only the slot;
  **neither the IR nor the WIT changes**.
- **`recipes`**: minting recipes. Each recipe is a small state machine of at most 4 steps, with step kinds drawn
  from a closed set:

| Step kind | Standard | Covers |
|---|---|---|
| `oauth2_token` | RFC 6749 §6 (refresh_token), §4.4 | Codex, Claude Code, Kiro (social / IdC forms selected by field) |
| `oauth2_token` + `grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer` | RFC 7523 §2.1 | Vertex service account exchanged for an access token |
| `jwt_sign`, algorithms from the closed set HS256 / RS256 / ES256 | RFC 7519 / 7515 / 7518 | Kling (output used directly as the bearer); the Vertex assertion |
| `http_exchange` | Plain GET / POST, presenting a field or a previous step's output under the declared auth-scheme (RFC 7235 syntax) | Copilot's second hop (GitHub token → Copilot token) |
| `http_probe` | As above, but looks only at the status | Seat re-verification on Copilot's direct-use flow |

Each step may declare:

- **Endpoint**: only a constant in the manifest, or a template with restricted parameters (parameters take only
  non-secret fields and are validated against a closed syntax set, e.g. `aws_region`). The host is never taken from
  credential contents or operator extra configuration — the host's existing rule (the SSRF comment at
  server:…/token_refresh.rs:413-414) is promoted to contract.
- **Parameters**: `{"const": …}`, `{"field": name, "pointer": …}`, `{"output": "step.name"}`,
  `{"now_plus": seconds}`; encoding `form` / `json`; ordinary headers (the same safe-header rules as descriptors).
- **`on_status`**: a mapping from status code to `reauth_required` / `transient` / `{"goto": step}`; defaults are
  4xx→`reauth_required` and 5xx→`transient`. Copilot's "404 is another kind of success"
  (server:…/token_refresh.rs:1006-1041) is `"404": {"goto": "direct"}`.
- **`extract`**: extraction by JSON Pointer; a clock convention, one of four: `relative_seconds` /
  `epoch_seconds` / `epoch_millis` / `jwt_exp`; `jwt_claim` (decodes the payload only, no signature verification —
  e.g. Codex taking the account id from `id_token`). Each extracted value is marked `secret` or `export`.

At recipe level: `present` (which output becomes the slot value), `rotates_refresh_material` (**required, no
default**), `refresh_margin_seconds`, `without_refresh_material` (`use_stored` / `fail`), `select` (choose a recipe
by a non-secret field, e.g. Kiro's `auth_method`). Making the clock convention and rotation "no default" copies the
host's lessons: getting either backwards raises no error — the former makes tokens never expire or always expire,
the latter wipes out the minting key (server:…/token_refresh.rs:399-437). Absence means gate ① refuses.

**Exported attributes.** Some minted outputs are not secret but the component needs them: Codex's account id (goes
into a request header), Vertex's project id (goes into the URL). The recipe marks such values `export`, and before
calling the component the host puts them into the reserved key `south_credential_attributes` of
`ProviderConfig.extensions` (the kernel's `ProviderConfig` has flattened `extensions`, kernel:provider.rs:334-335);
the component reads them as ordinary non-secret configuration. The IR does not change, and the component still sees
no credential value.

Sketch (field names are a draft):

```json
"credentials": {
  "schema": "south.credential-recipe.v1",
  "fields": {
    "service_account": { "secret": true, "required": true, "media": "application/json" }
  },
  "slots": { "provider_api_key": { "minted": "vertex_sa" } },
  "recipes": {
    "vertex_sa": {
      "steps": [
        { "id": "assertion", "kind": "jwt_sign", "alg": "RS256",
          "key":    { "field": "service_account", "pointer": "/private_key" },
          "claims": { "iss":   { "field": "service_account", "pointer": "/client_email" },
                      "scope": { "const": "https://www.googleapis.com/auth/cloud-platform" },
                      "aud":   { "const": "https://oauth2.googleapis.com/token" },
                      "iat":   { "now_plus": 0 }, "exp": { "now_plus": 3600 } } },
        { "id": "token", "kind": "oauth2_token", "encoding": "form",
          "endpoint": "https://oauth2.googleapis.com/token",
          "params": { "grant_type": { "const": "urn:ietf:params:oauth:grant-type:jwt-bearer" },
                      "assertion":  { "output": "assertion.jwt" } },
          "extract": { "access_token": { "pointer": "/access_token", "secret": true },
                       "expires_at":   { "relative_seconds": "/expires_in" } } }
      ],
      "present": "token.access_token",
      "rotates_refresh_material": false,
      "attributes": { "project_id": { "field": "service_account", "pointer": "/project_id", "export": true } }
    }
  }
}
```

Kling, by contrast, is a single `jwt_sign` step (HS256; key from `secret_key`; claims `iss`←`access_key`,
`exp`←now+1800, `nbf`←now−5), with `present` pointing at that step's JWT.

### 3.4 Why this does not violate the two 2026-09-08 tests

- **Leak impact**: private keys, service-account JSON and refresh tokens remain only in host storage and the host
  process; the component receives only slot names and the non-secret attributes marked `export`. South's crates do
  not touch these values either — the recipe is data, and the executor is in the host.
- **Money and unrecoverable credentials**: named locks, re-reading after taking the lock, CAS write-back, and CAS
  losers re-reading the authoritative new generation (the seven steps at server:…/token_refresh.rs:477-493) all
  stay in the host's generic executor. A recipe only answers "what the request looks like, where the token is in
  the response, whether it rotates", which is exactly what each family's `MintStrategy` implementation contains
  today — and the skeleton was never in those implementations to begin with.

So what needs revising is the **concluding sentence** of ARCHITECTURE.md:123-126, not the two tests: the
"per-provider authentication layer" changes from host code to data declared by the component, and "remain host
code" becomes "its **execution** and its **material** belong to the host".

### 3.5 Host counterpart

One generic recipe executor, replacing the five `MintStrategy` implementations, Kling's JWT minting and the
vendor-named recipes in task bindings; the skeleton is unchanged. The timing follows raw.rs:737-745: minting
completes before the funds marker, a failure is a pre-admission error that moves no money, and taking values after
resolution cannot fail. Token-exchange egress uses the same guard as webhooks (https only, pinned address, no
redirects, no system proxy — P21 §9 E-1), with a bounded response body.

### 3.6 Conformance

- **gate ①**: recipe structure validation — step kinds and algorithms are in the closed sets; endpoints are
  constants or restricted templates; both no-default fields are present; `present` is reachable; `goto` is acyclic
  with at most 4 steps; `export` may be attached only to non-secret values.
- **gate ②**: a new fixture family `credential.*`: given fake field values and fake responses, assert the rendered
  exchange request (method, URL, encoded body, headers) and the extraction results; it must include one rotation
  sample, one `on_status` transition sample and one clock-convention sample. Executing these fixtures needs a
  south-provided **reference recipe interpreter**, which runs only in tests and sees only fixture fake values; the
  production executor is the host's.
- **gate ③**: host suite `south.credential-recipe.v1`: a fake token endpoint, covering exchange failure, concurrent
  refresh (two requests hit the upstream only once), expiry, rotation write-back, CAS loser re-read, and no retry on
  `reauth_required`.

### 3.7 Versioning

A new optional manifest section: south minor (R5: absent means every slot is `static`, which equals today). Once
recipes land, the `oauth` arm and the kernel's `Auth::OAuth` become redundant: keep parsing them, mark them
deprecated, stop recommending them; removing them would have to go through the kernel chain and is not worth it.
Host link layer: the recipe executor is a one-time new generic mechanism (P21 §1.4); after that, an OAuth family
that fits within the step vocabulary is a package-layer change. **A new flow outside the vocabulary** (a sixth step
kind) is a south contract upgrade plus a host executor upgrade — this is the edge of DP0's coverage under this
proposal; see §16 Q3.

### 3.8 Coverage check

| Family | In the host today | Recipe expression |
|---|---|---|
| Codex | JSON refresh_token grant; falls back to decoding the JWT `exp` when `expires_at` is missing; takes the account id from `id_token`; rotates | `oauth2_token` + `jwt_exp` + `jwt_claim` (export); rotates = true |
| Claude Code | JSON refresh_token grant; millisecond clock; rotates | `oauth2_token` + `relative_seconds` (the stored convention is normalized by the host); rotates = true |
| Copilot | GET + `Authorization: token …` + editor headers; a 404 switches to the direct-use flow and re-verifies the seat; does not rotate | `http_exchange` + `on_status` 404→`goto` + `http_probe`; rotates = false; the editor headers belong to DP7 |
| Kiro | Social / IdC forms, endpoint built from a region template; rotates | `select` + two `oauth2_token` recipes; the endpoint template parameter uses the `aws_region` syntax |
| Vertex service account | RS256 assertion exchanged for a token; project id goes into the URL; does not rotate | The §3.3 sketch |
| Kling | HS256 JWT used directly as the bearer | A single `jwt_sign` step |

## 4. Making the descriptor's auth actually take effect (problem b)

### 4.1 Today

- The manifest's `auth_arms` means "the auth arms the component's descriptors may use" (manifest.rs:234-238);
  south only validates that it is within the world vocabulary (manifest.rs:355-362) and the coherence of
  `host_signed` (manifest.rs:376-399). Neither gate ② (suite.rs), nor the runtime, nor the sandbox seam checks the
  descriptor's `auth` against it — in production code outside manifest.rs, `auth_arms` appears only in a single
  comment (reference_bedrock_converse.rs:16).
- Components already choose how to present per dialect: the OpenAI-compatible reference implementation emits
  `Auth::bearer` for `openai-compatible` and `Auth::header("api-key")` for `azure-openai-v1`
  (reference.rs:640-648). The kernel's division of labor also says "how to present is the adapter's decision"
  (kernel:provider.rs:304-316).
- But south has no public function that turns IR `Auth` into the contract's `ProviderAuthV1`; the raw prelude's
  auth arm is chosen by the host (raw.rs:107, 198, 251). So the host's text path looks it up in a table by type (S0
  J2b① measured the component's declaration being ignored). The host's task path, on the other hand, already
  checks `descriptor.auth` against `auth_arms`
  (server:gateway/src/modules/inference/engine/south_task_component.rs:137-162) — the precedent is in the host, not
  in south.

### 4.2 Proposal

1. South provides
   `admit_descriptor_auth(manifest, config, descriptor) -> Result<AdmittedAuthV1, DescriptorAuthErrorV1>` in
   `south-component-conformance` (the in-repo sanctioned consumer of IR types,
   south-component-conformance/src/lib.rs:23-28; the host already calls components through its `sandbox` feature).
   Rules:
   - `Auth::Bearer` → the manifest must contain `bearer` → `RawAuthV1::Bearer`;
   - `Auth::Header { name }` → the manifest must contain `header_secret`, and `name` must be an admitted secret
     header (today the five of `SecretHeaderV1`; §10 proposes making them manifest-declared) →
     `RawAuthV1::HeaderSecret`;
   - `Auth::OAuth` → the slot must be `minted` in a §3 recipe, and is presented as bearer;
   - `None` → only when `ProviderConfig.auth` is also empty (the kernel's `ProviderConfig::authorize` already judges
     this pair, kernel:provider.rs:361-378);
   - the manifest is `host_signed` → the descriptor must carry no `auth`, and the signing scheme follows §5.3.
   Any mismatch is refused before admission, with zero upstream calls.
2. Both the host's text path and its task path switch to calling it; the host's per-type auth tables (P21 §2.5,
   "two to three tables from the same source") are deleted along with it.

### 4.3 Gap: the combined arm cannot be expressed by a component

The contract has `BearerAndHeaderSecret` (south-contracts/src/lib.rs:1331-1346, auth contract 4), which the host
uses to serve Gemini's OpenAI-compatible endpoint; but the manifest vocabulary has no corresponding arm
(manifest.rs:89), and the kernel's `Auth` has no corresponding variant (kernel:http.rs:150-166). `provider-gemini`'s
manifest declares only `header_secret`. Once the host presents strictly according to the descriptor, this path
loses any way to be expressed. Options:

- **A (recommended)**: go through the kernel chain to add `BearerAndHeader { name, secret }` to `Auth`, and add
  `bearer_and_header_secret` to the manifest vocabulary. P21 §7 has to go through the kernel chain anyway (`Usage`
  cache buckets); batch the two together.
- **B (interim)**: the manifest declares at family level that "this family's header_secret is also mirrored as
  Bearer". The kernel is left alone, but the same descriptor then means different things under different families;
  interim only.

### 4.4 Conformance and versioning

A new gate ② check, `DescriptorAuthWithinManifest`: run the §4.2 admission on the descriptor produced by every
request fixture. T21 adds a `rogue-arm` mode (§12). A new public function and one check: south minor. The four
reference implementations spot-checked all present according to their own `auth_arms`, so none of the thirteen
packages is expected to need changes; if a package turns red once the check lands, that is a package defect.

## 5. Declaring stream framing and signing schemes (problem c; DP4, DP9)

### 5.1 Today: eventstream is the only thing the host really deframes

- The components for the three SSE dialects split frames **themselves**: each reference implementation has an
  `sse_frame_boundary` that scans the bytes for delimiters (reference.rs:519, reference_anthropic.rs:448,
  reference_gemini.rs:331), and the host feeds upstream bytes unchanged to `parse-stream-chunk`. The WIT states that
  a chunk is not guaranteed to be a whole frame and that the component holds partial-frame state itself
  (provider-adapter.wit:118-125).
- The only thing the host deframes is AWS eventstream: the Converse reference implementation states "the host owns
  that layer"; the host deframes (CRC, 16 MiB bound) and then **re-encodes each event as one SSE frame** before
  feeding the component (reference_bedrock_converse.rs:21-35, 549-557). This is an agreement between one reference
  implementation and the host, not a south ruling: the WIT chose byte chunks precisely so that eventstream could go
  into the component (provider-adapter.wit:20-25), the plan line that S0 D2 cites is exactly "eventstream decode
  goes in the component" (canonical-ir-inventory.md:223-233), and framing syntax was left to gate ② fixtures (same
  record, :277-278).
- So P21 §3.1, citing reference_bedrock_converse.rs:22 as south's position that "the framing layer belongs to the
  host", overstates its scope: that holds only for eventstream.
- Signing: the `host_signed` arm declares only `emits` (manifest.rs:239-245); the contract deliberately does not
  know the scheme (south-contracts/src/lib.rs:930-938). "This package needs SigV4" can only be inferred by the host
  from the provider type.

### 5.2 Proposal: `stream_framing`

Declared per family in the manifest; absent = `bytes` (today's behavior for the three SSE dialects, R5):

| Value | What the host feeds to `parse-stream-chunk` | What the host must implement |
|---|---|---|
| `bytes` | Upstream bytes unchanged | Nothing |
| `aws-eventstream` | The canonical re-encoding of each message frame: `event: <:event-type>\ndata: <payload>\n\n`; a frame whose `:message-type` is `exception` is encoded as `event: exception:<:exception-type>` | One deframer (prelude, two CRC32s, frame-length bound) |

The message-frame encoding is byte-for-byte the same as the host's seam today (`parse_event` at
server:gateway/src/modules/inference/engine/south_component.rs:849-857), so the existing Converse fixtures do not
change. The exception-frame encoding is new: today the Converse stream parser ignores every unknown event, on the
grounds that "the host's own strict validator lives upstream of here" (reference_bedrock_converse.rs:736-739) — and
that upstream validator is precisely the dialect knowledge being moved out of the host. Instead, the host passes
exception frames through unchanged, the component maps them to `StreamEvent::Error` (kernel:stream.rs:99-101), and
fixtures pin this.

The `sse` / `ndjson` / `json` values in DP4's recommendation are not added: their splitting already lives in the
components and the host has nothing to do; putting them in the set would only give the host a branch that "picks,
by declaration, a decoder it does not actually use". If the host needs to know the upstream format for
diagnostics, a separate read-only informational field is enough.

**South provides one deframer.** A pure, bounded, fuzzed function in `south-core` (which already hosts the prelude
shared by both hosts). That way "each form is implemented once" holds for both hosts at the same time, and the
host's roughly 235 lines of deframing code (P21 §2.5) retire. A component may still declare `bytes` and split
eventstream itself — the WIT allows it and DP4 does not forbid it; what is forbidden is the host choosing a decoder
by provider identity. Kiro is also eventstream (P21 §2.5) and is covered by the same declaration.

### 5.3 Proposal: `signing` for `host_signed`

```json
"auth_arms": ["host_signed"],
"emits": ["authorization", "x-amz-date", "x-amz-content-sha256", "x-amz-security-token"],
"signing": { "scheme": "aws-sigv4", "service": "bedrock", "region": { "config": "region" } }
```

`scheme` is a closed set (today only `aws-sigv4`), `service` is component data, and `region` points at a
non-secret config key declared per §7.3. The host picks the finalizer by `scheme` — DP9: SigV4 is handled as a
public-standard executor, kept in the host and selected by declaration. The credential fields (access key, secret,
session token) are declared through §3.3's `fields`, and `aws_sigv4` among the host's credential kinds (P21 §2.6)
becomes a generic field set. The contract-level `SignedHeaderSetV1` stays scheme-agnostic and does not change.

### 5.4 Conformance and versioning

- Deframer: property tests and a fuzz target (CONTRIBUTING's requirement for untrusted parsers); fixtures include
  frames split across chunks, CRC errors, oversized frames and exception frames.
- gate ②: stream fixtures for `aws-eventstream` families are written in the canonical re-encoding (consistent with
  today's Converse fixtures); gate ① validates that `signing` and `emits` are compatible (`aws-sigv4` requires at
  least `authorization`, `x-amz-date` and `x-amz-content-sha256`).
- New manifest fields: south minor. `provider-bedrock-converse` needs `stream_framing` and `signing` added, with an
  identity bump. The deframer is a new link-layer API, recorded under `host_capabilities` once the host adopts it.

## 6. Usage belongs to the component (problem d; DP1)

### 6.1 Today: the ruling exists, the implementation has not caught up

- **Already ruled**: S0 D1 (2026-08-21) — "per-dialect evidence extraction moves into the component; the funds
  discipline stays host", with a commitment that gate ② would machine-judge "exact" using adversarial fixtures
  (duplicates, reordering, zero-wiping, missing terminal frame) (canonical-ir-inventory.md:187-203). The WIT is
  written accordingly: the usage from `parse-response` is funds evidence, "a 2xx whose body cannot yield exact usage
  is an error, never a zero" (provider-adapter.wit:110-116); streaming usage is "emitted exactly as often as the
  dialect reports usage" (provider-adapter.wit:122-124). P21 §3.1 takes the streaming record
  (2026-08-17-streaming-provider-call.md:16-23) as south's position, but that record predates S0, covers only the
  transport layer, and has been superseded by D1. **For south, DP1 is not a boundary revision; it is finishing
  something already ruled.**
- **What has not been done**:
  1. The reference implementations do not follow the WIT. OpenAI-compatible (reference.rs:343-358, call site
     :728), Anthropic (reference_anthropic.rs:381-389, 422-424) and Gemini (reference_gemini.rs:292-301) all
     `unwrap_or(0)` missing fields, and a 2xx with no usage object yields all-zero usage. Only Converse is strict:
     three items are required, and `totalTokens` must equal the sum of the buckets, otherwise it is a protocol error
     (reference_bedrock_converse.rs:470-510). This is why the host still keeps its strict `usage_evidence` (P21
     Appendix B.2: "1,208 lines; south already has a lenient version").
  2. gate ② does not enforce it. `Coverage` only requires at least one row per fixture family (suite.rs:327-341;
     fixture.rs:164-169). The OpenAI package does have S0 rows (`stream.usage-terminal`, `stream.duplicate-usage`,
     `stream.missing-terminal`, `stream.no-usage`, `response.cached-usage`), but only in-repo tests assert them by
     name (tests/component_conformance_v1.rs:108-124); a third-party package passes without them.
  3. The existing usage judge covers only the in-repo reference implementations: `tests/usage_ir_contract_v1.rs`
     derives its expectations from provider documentation, specifically to guard against "fixtures written from the
     reference implementation" (that file, :1-30). This is exactly the layer the DP1 trust chain needs, but it is
     not in gate ②.
  4. The IR cannot tell "the upstream reported 0" from "the upstream reported nothing": `ChatResponse.usage` is not
     optional and defaults to all zeros (kernel:chat.rs:271-281; kernel:usage.rs:10-19). For streaming, "no Usage
     event" tells them apart (the `provider.stream.no-usage` fixture pins exactly this shape); for non-streaming,
     only a component error can.
  5. The reasoning-token convention is not frozen. The kernel says `reasoning_tokens` is a subset of
     `output_tokens` for every modeled provider (kernel:usage.rs:54-56), and v0.40.0 aligned only the cache-bucket
     convention (ARCHITECTURE.md:182-186). The Gemini reference implementation maps `candidatesTokenCount` to output
     and `thoughtsTokenCount` to reasoning (reference_gemini.rs:292-301), yet in its own fixture the two are 5 and 40
     (fixtures-gemini/provider.response.thought-parts-and-token-buckets.input.json) — reasoning larger than output,
     contradicting "subset". Per the composition of `totalTokenCount` that Google publishes, thoughts are not
     included in candidates (to be rechecked against the documentation).
- P21 §3.2 S5 says the conformance tests contain "not a single usage sample", which is inaccurate: there are some,
  and there is a documentation-derived judge; what is missing is **enforcement** and **strictness**.

### 6.2 Proposal

1. **Make the reference implementations strict**, with the same convention as Converse: a non-streaming 2xx that
   lacks the usage object or a count this dialect requires → `provider_protocol_error`; a verifiable relationship
   given by the upstream (e.g. OpenAI's `total_tokens == prompt_tokens + completion_tokens`) that does not hold →
   protocol error.
2. **gate ② requires usage rows by name** (provider world): `response.usage`, `response.missing-usage` (expects a
   protocol error), `response.cached-usage` (partition convention), `stream.usage-terminal`, `stream.no-usage`.
   Families declaring `usage_evidence: absent` (item 4 below) instead require a sample showing they "never produce
   Usage".
3. **An automatic mutation check, `UsageNeverDefaulted`**: response fixtures carry `usage_pointer` metadata (where
   the usage object sits in the upstream body); the suite deletes that object and calls again, requiring the
   component to report an error rather than produce zeros — the same technique as `unknown_field_tolerance`
   (suite.rs:387-420). The three lenient reference implementations would turn red on this today, which is exactly
   what it is meant to prove.
4. **A manifest `usage_evidence`** (per family): `reported` (default) | `absent`. `absent` means the upstream never
   reports tokens — e.g. Kiro, which lv ruled on 2026-09-22 is billed by estimate and labeled truthfully
   (server:crates/gateway-provider-protocol/src/usage_types.rs:131-147). For `absent` families the host uses a
   provider-agnostic estimator and writes `tokens_estimated = 1`, `quantity_estimated = 1`, no longer judging by
   family name.
5. **Documentation-derived judges become part of south's release discipline**: every provider package south
   publishes must have a `usage_ir_contract`-style judge (expectations derived from provider documentation, not from
   the reference implementation). It does not go into gate ② — a third-party package author cannot vouch for
   documentation semantics on south's behalf — it is south's commitment for its own released packages.
6. **Freeze the reasoning convention**: following how v0.40.0 handled the cache buckets, write "`reasoning_tokens`
   ⊂ `output_tokens`" into the IR usage contract and add reasoning rows to the `usage_ir_contract`-style judges; if
   the documentation recheck confirms that thoughts are not within candidates, change the Gemini reference
   implementation to `output_tokens = candidatesTokenCount + thoughtsTokenCount`. This changes amounts charged by
   output and must be confirmed by a host dual run (§16 Q13).

### 6.3 The host's generic checks and the undetectable zone

The host performs two kinds of checks at the IR layer, independent of dialect; a hit sends the call to manual
review instead of settling it as a success:

- **Out-of-bound checks**: `output_tokens ≤ authorized output cap × number of choices`;
  `reasoning_tokens ≤ output_tokens` (must wait for §6.2 item 6 to land, otherwise today's Gemini mapping raises
  false positives); `input_tokens ≤ g(request)`, where the host computes g from the outbound descriptor body's byte
  count plus a fixed allowance per media part (images referenced by URL are few bytes but many tokens, so counting
  bytes alone raises false positives); settled amount ≤ reservation (exists today).
- **Internal-consistency checks**: `cache_read_tokens + cache_write_tokens ≤ input_tokens` (the kernel partition
  contract, kernel:usage.rs:52-61); `cache_write_5m_tokens + cache_write_1h_tokens ≤ cache_write_tokens`; exactly
  one streaming terminal state and no `Usage` after `Done`; a `reported` family with no `Usage` at all cannot be
  settled.

**The undetectable zone, written into acceptance: under-reporting, and over-reporting or deviation that falls
within bounds, cannot be detected by the host.** The only thing the host can compare against is an upper bound it
can compute itself; any number below that bound is equally credible to the host. Trust comes from only three
places: pinned package digests (§9), gate ② usage samples and documentation-derived judges (§6.2 items 2, 3 and
5), and dual-run reconciliation against the native arm before cutover (host P21 S5). If the "lower-bound signal"
that P21 S5 envisions is to be established, it is host policy; south neither provides nor blocks it.

Unchanged: `Usage::absorb`'s folding semantics (kernel:usage.rs:63-85); S0 D3, "do not add provenance to
`StreamEvent`" (canonical-ir-inventory.md:235-243); the derivation rules for thinking markers (same record,
:215-221).

### 6.4 Versioning

Making the reference implementations strict is a behavior change: for a host that links the reference
implementations directly (the community host), responses previously treated as zero-usage successes become
protocol errors, which needs the community host's confirmation (§16 Q9). Three package identities bump. The new
gate ② check and by-name enforcement: south minor; third-party packages need to add fixtures. Normalization of
quota headers (`ProviderQuotaMetadataFieldV1` enumerates header names per provider,
south-contracts/src/lib.rs:1686-1706) is covered in §10 and §11.

## 7. Request facts, non-secret configuration and capability metadata (problem e)

### 7.1 Today

- In its sealing phase the host recognizes only a few closed locations: the cap in top-level `max_tokens` /
  `max_completion_tokens`, `model` at top level, `stream` at top level; T03's documentation calls these "Three
  fields that are not free" (t03-canary-provider/src/lib.rs:16-37). Real dialects do not look like this: Gemini's
  cap is at `generationConfig.maxOutputTokens` (reference_gemini.rs:224), Converse's at `inferenceConfig.maxTokens`
  (reference_bedrock_converse.rs:343), and both put model and stream in the URL. So the host wrote a location table
  keyed by `provider_type` (server:gateway/src/modules/inference/engine/text_admission.rs:1030 onward). The manifest
  has no place to declare any of this.
- Non-secret configuration: the kernel's `ProviderConfig` has only `provider` / `base_url` / `auth` / `models` and
  the flattened `extensions` (kernel:provider.rs:318-336). Vertex's project, Bedrock's region and Azure's
  api-version are assembled by the host per type (P21 §2.5; S1, "take region / project from the URL template";
  `form_preset`).
- Capabilities: the provider world has `model-capabilities` (provider-adapter.wit:89-98), but all four reference
  implementations merely echo `config.models` (reference.rs:600-607, reference_anthropic.rs:701-708,
  reference_gemini.rs:454-459, reference_bedrock_converse.rs:791-798). When the host calls it, what comes back is
  what the operator filled in. P21 S6's premise — that the text side can "call the existing `model-capabilities`
  directly" to replace `capabilities.rs` — does not hold: there is no model catalog in the components.
- Per-model request differences already have a channel: a model declares dialect words in
  `supported_parameters`, and the component interprets them (the six `anthropic.*` words of
  2026-09-29-claude-model-dialect.md).

### 7.2 Proposal: `request_facts` (per family)

```json
"request_facts": {
  "gemini": { "output_cap": ["/generationConfig/maxOutputTokens"], "model": "url", "stream": "url" }
}
```

- `output_cap`: JSON Pointers into the descriptor body where the component may write the cap (at most 4). The
  host's generic seal check: the host always writes the authorized cap into the IR's `sampling.max_output_tokens`;
  in the body the component produces, **exactly one** declared location has a value and it equals that cap, and the
  other declared locations are absent. This replaces the per-type location table and also removes T03's constraint
  2.
- `model`: `{"body": "/model"}` or `"url"`; `stream`: `{"body": "/stream"}` or `"url"`. For the body form the host
  checks value by value; the `url` form relies on endpoint confinement (`ProviderConfig::authorize`) and gate ②
  checks.
- Absent = today's three top-level fields (R5).

### 7.3 Proposal: `config_schema` (per family)

Declares the non-secret keys the component reads from `ProviderConfig.extensions`: name, value syntax (from a
closed syntax set, e.g. `aws_region`, `gcp_project_id`, `api_version_date`, `digits`, `token`, `enum[…]`), whether
required, and a one-line description. The host renders the operator form from this and validates the values before
handing them to the component, replacing `form_preset` and the per-type "required extra fields". The URL is built
by the component from `base_url` and these keys — Gemini and Converse already build it themselves today (the
comments at reference_gemini.rs:483 onward and reference_bedrock_converse.rs:825 onward); the host does no URL
templating.

### 7.4 Per-model differences: dialect words

Model-level differences such as "is the cap field called `max_tokens` or `max_completion_tokens`", "Responses
only" or "supports countTokens" are all expressed as `supported_parameters` dialect words and interpreted by the
component — the same mechanism as the Claude dialect words. A new word is a package-layer change; the host only
passes the words declared on the model row to the component unchanged.

### 7.5 Model catalog

The roughly 691 lines of built-in capability profiles in `capabilities.rs` (P21 Appendix B.2) are **data**, not
translation logic. Three possible homes:

- **A Built into the component**: `model-capabilities` returns the merge of a built-in catalog and the operator's
  declarations. Every new model means a package release, and the package digest changes with the data.
- **B Catalog data published by south (recommended)**: JSON in `south.model-catalog.v1` format, published per
  family and listed in the §9 index; the host loads it as data, with operator rows overriding it;
  `model-capabilities` stays as a hook for "supplementing from the upstream".
- **C Pure operator data**: south defines only the vocabulary (capability fields, dialect words), and each host's
  operators maintain the catalog.

B lets the two hosts share the catalog but turns "keeping up with providers' new models" into a south maintenance
burden; C costs south the least, but each host maintains its own copy, which contradicts DP0's rationale (sharing
southbound work). See §16 Q7. Prices do not enter south (ARCHITECTURE.md:108-112).

### 7.6 Conformance and versioning

A new gate ② check, `RequestFactsHonoured`: every request fixture asserts that the IR cap appears in exactly one
declared location, and that body-form model and stream agree with the IR; fixtures missing a required
`config_schema` key expect a capability error. New manifest fields: south minor; the catalog format is a new
release artifact (§9).

## 8. Compatibility range and per-package isolation (problem f; DP5)

### 8.1 Today

- The tuple's four host-side values (IR, kernel version, kernel revision, south runtime) must each be exactly equal
  (manifest.rs:657-694); the loader calls this before reading the wasm (loader.rs:165-168, 188-191).
- The result is that "the runtime and the thirteen packages must be upgraded in one batch" (ARCHITECTURE.md:205-207):
  every release re-stamps `south_runtime` on every package, even when the wasm did not change. Task contract 6 even
  used this as its reason for not giving new keys default values (2026-09-27-task-contract-v6-facts.md:22-23).
- Loading itself is **per package**: `read_package` / `parse_package` return one `LoadErrorV1` per directory
  (loader.rs:154-194). S0 J2b③'s "one package takes down the whole text surface" is the host's choice to aggregate
  per-package errors into a process failure, not the shape of south's API.

### 8.2 Three places where DP5's wording needs refining

DP5: "the same world api_version, the same runtime major version, and not lower than the minimum version the host
requires".

1. **"Major version" degenerates under 0.x**: the runtime is 0.42.0. Comparing semver major versions, all of 0.x is
   the same, which amounts to no gate at all; comparing by Cargo caret rules (under 0.x the minor acts as the
   major), every release changes the "major version", which amounts to today. Neither is right.
2. **Only a lower bound**: the version a component declares must also be no higher than the runtime the host links
   — a newer component may use manifest fields (`deny_unknown_fields`) or ABI semantics that an older runtime does
   not understand.
3. **Contract numbers**: the task JSON contract number (7 today, south-contracts/src/task.rs:38) has always relied
   on "exact runtime equality forces a re-stamp" to stay consistent. Once the range is relaxed, which contract
   version a component speaks must be declared explicitly.

### 8.3 Proposed rules

| Tuple item | Today | Proposal |
|---|---|---|
| world / WIT package / suite name | Exact (`validate`) | Unchanged |
| `ir_schema_id` (protocol crate version) | Exact | Component ≤ host, and on the same compatibility line (same minor under 0.x); the kernel promises additions only within a line |
| `kernel_version` / `kernel_revision` | Exact | Recorded for provenance only; not part of the decision |
| `south_runtime` | Exact | `host.minimum ≤ component declaration ≤ host.runtime` |
| New `runtime_abi` (integer epoch) | — | Equal; incremented only on incompatible changes to loader, sandbox or WIT semantics; recorded in `compatibility.json` |
| New `contracts` (e.g. `{"task": 7}`) | Implicit | Declared by the component; the host accepts a set; south's codecs can decode every version in the set |

`runtime_abi` is the operational stand-in for DP5's "runtime major version" under 0.x; when south reaches 1.0 it can
merge with the major version.

### 8.4 API

Add `compatibility_admits(manifest, &HostRangeV1) -> Result<(), CompatibilityMismatchV2>`, with
`HostRangeV1 { runtime_abi, south_runtime_min, south_runtime, ir_line, contracts }`; the loader switches to taking
`HostRangeV1`. `compatibility_matches` is kept for one version and marked deprecated.

### 8.5 Per-package isolation

Add `load_package_set(runtime, root, &HostRangeV1) -> PackageSetReportV1`: walk the directory (or follow the §9
index); each package independently goes through gate ①, the range handshake, the import scan and the identity
probe; return `admitted` and `refused { package, reason }` sorted by package name, never failing as a whole because
one package failed. Accompanying rules, written into ARCHITECTURE:

- A refused package makes only the families it declares unavailable; startup continues, and the readiness probe
  reports truthfully.
- **No fallback to native reference implementations** (J3①): the reference implementations are gate ②'s judges
  and an optional native engine for the community host, not stand-ins for missing or refused packages.
- If two admitted packages in the same world declare the same family, both are unavailable and the operator is
  required to pin one by digest (the task side already selects packages by pin today); **ties are never broken by
  load order**.

### 8.6 Changes to release discipline

- No more re-stamping every package on every release; if a package's content is unchanged, its identity and digest
  are unchanged.
- Contract changes must be additive (new keys have defaults, old shapes still decode), or the codecs must also
  accept the older versions in the declared set. "Reject when the new key is missing; it will be re-stamped anyway"
  (2026-09-27-task-contract-v6-facts.md:22-23) no longer holds.
- Incrementing `runtime_abi` is a breaking event and requires a design record.

### 8.7 Conformance and versioning

`crates/south-contracts/tests/compatibility_manifest.rs` and `crates/south-provider-api/tests/provider_api_v2.rs`
gain range cases (upper and lower bounds, a different epoch, a different IR line, a contract not in the set); T21's
skew package (§12) gets only itself refused. South minor; the manifest's `compatibility` gains `runtime_abi` and
`contracts`.

## 9. A machine-readable release index (problem g)

### 9.1 Today

Each tag publishes 13 `<component>-vX.Y.Z.tar.gz` archives and one `SHASUMS256.txt` (release.yml:60-106,
131-133). That is enough to enumerate archives and verify integrity, but it carries no world, family, component
version or compatibility declaration, and the digests cover the tar.gz rather than the `manifest.json` /
`component.wasm` inside; for the host to learn which package serves which family, it can only download and unpack
everything. The host's fetch script therefore hard-codes 13 names and exits with `exit 2` on anything outside the
list (server:scripts/fetch_south_components.sh:60-70). The answer to P21 S2's "is the existing artifact checksum
file enough (to be verified)": enough for verification, not enough for discovery.

### 9.2 Proposal

Every release attaches `south-release-index.json`, generated mechanically by release.yml from each package's
manifest (never hand-written) and listed in `SHASUMS256.txt`:

```json
{
  "schema": "south.release-index.v1",
  "south_release": "0.43.0",
  "runtime_abi": 1,
  "packages": [{
    "name": "provider-gemini", "version": "1.1.5",
    "world": "provider-adapter-v2", "wit_package": "token-station:adapter@2.0.0",
    "providers": ["gemini"], "capabilities": ["chat", "json_schema", "stream", "tool_call"],
    "auth_arms": ["header_secret"], "stream_framing": { "gemini": "bytes" },
    "compatibility": { "south_runtime": "0.43.0", "runtime_abi": 1,
                       "ir_schema_id": "token-station-protocol@0.4.0/v0.3.0", "contracts": {} },
    "archive": "provider-gemini-v0.43.0.tar.gz",
    "archive_sha256": "…", "manifest_sha256": "…", "component_sha256": "…"
  }],
  "catalogs": [{ "family": "gemini", "file": "catalog-gemini-v0.43.0.json", "sha256": "…" }]
}
```

The host's fetch script and startup gate discover packages through the index and verify them by digest; operators
still pin packages by digest. The index's `providers` can also serve as a lower-bound source for the J1 vocabulary
(P21 S0 has already pointed out that family names are not vendor names and cannot replace the vendor list).

### 9.3 Trust and versioning

Like the archives, the index has only checksums and no signature (released-artifacts §6 lists signing as the next
slice). While only south's first-party packages are installed, the root of trust is the tag and the CI build;
before **third-party packages** are loaded through the index, signing is a prerequisite (§16 Q11). This is a
release-behavior change (CONTRIBUTING requires a design record — this one) and does not touch contract numbers.

## 10. Provider instances in closed vocabularies

The link-layer problem of §1.3 is sharpest here. The following sets are compiled into the host, yet their values
belong to specific providers:

| Set | Location | Values |
|---|---|---|
| `SecretHeaderV1` | south-contracts/src/lib.rs:877-923 | `api-key` (Azure, Ideogram), `x-api-key` (Anthropic), `x-goog-api-key`, `xi-api-key` (ElevenLabs), `ocp-apim-subscription-key` (Azure Speech) |
| `QueryParameterV1` | south-contracts/src/lib.rs:1051-1095 | `api-version`, `alt`, `GroupId` / `task_id` / `file_id` (MiniMax) |
| `ProviderQuotaMetadataFieldV1` | south-contracts/src/lib.rs:1686-1706 | OpenAI-style and Anthropic-style rate-limit headers |
| `CREDENTIAL_HEADERS` | kernel:lib.rs:86-96 | Corresponds to `SecretHeaderV1`, plus `authorization`, `cookie` and others; both construction and deserialization of `Auth::header` consult it (kernel:http.rs:182-188, 209-218) |

As soon as a new provider uses a secret header name or query name that is not in these tables, south (and even the
kernel) must release and the host must re-pin and rebuild — J2 is red. These sets were closed for real security
reasons: a secret header must also be on the reserved-header denylist so that it cannot be smuggled through the
ordinary header channel (lib.rs:877-883); the query is the part of a request most often logged
(lib.rs:1051-1062). Per R1, separate the mechanism from the instances:

- **Secret headers**: the manifest declares `secret_headers` (name syntax restricted; no hop-by-hop headers, no
  `host`, no framing headers); for that package's requests the runtime merges these names into the reserved set
  (the ordinary header channel refuses them), and transcripts and logs always redact them. What is closed is the
  "secret header" mechanism and its safety rules, not the list.
- **Query parameters**: the manifest declares `query_parameters`, each choosing one entry from a closed set of
  **value syntaxes** (`digits`, `token`, `enum[…]`, `date`), with restricted parameter-name syntax. `ProviderAuthV1`
  remains the only channel through which secrets go on the wire; query values never come from credential
  resolution — that rule does not change.
- **Quota headers**: the component normalizes its dialect's rate-limit headers in `parse-response` /
  `map-provider-error` and hands them over (a south-local response extension); which headers the transport captures
  is declared by the package (P21 S5).
- **Kernel catalog**: the `Auth::header` check needs the kernel to open up to "a declared set supplied by the
  caller", or to be performed instead by the §4.2 descriptor auth admission; this goes through the kernel chain,
  merged with §4.3.

Without this step, the accurate statement of DP0 is: "If a new provider uses only existing instances, the host
needs zero changes; otherwise the host must bump its south pin." lv ruled on 2026-09-30 (§16 Q1) that this
statement is not acceptable: bumping the pin counts as modifying the host. This step is therefore a necessary
condition of DP0, not an improvement.

## 11. Follow-on components and non-chat operations (problem h)

| Work item | Scope | Depends on |
|---|---|---|
| OpenAI Responses upstream dialect component (Codex and others) | New provider package, family `openai-responses`: IR → Responses request (instructions, input items, tools, reasoning effort, `store`); Responses response and `response.*` stream events → IR; strict usage (`input_tokens`, `input_tokens_details.cached_tokens`, `output_tokens`, `output_tokens_details.reasoning_tokens`, terminal frame `response.completed`). `south-north-codec` already has a Responses northbound mapping (2026-09-28-responses-north-codec.md); the direction is opposite, so wire types can be shared but the mapping cannot. "Responses only" uses a model dialect word (§7.4); client-identification request headers the backend requires belong to DP7 | §3 (Codex recipe), §6, §7 |
| Kiro component (DP6, migration recommended) | New provider package: conversationState request shape, `aws-eventstream` (§5.2), `usage_evidence: absent` (§6.2), social / IdC recipes (§3.8). The host's three-hop translation and the Kiro part of the leaf crate retire with it (P21 §2.5) | §3, §5, §6 |
| The Anthropic variant of Bedrock InvokeModel | `provider-anthropic` adds a family (e.g. `anthropic-bedrock-invoke`): the body carries `anthropic_version` and the model is in the URL; the stream is eventstream, and the `bytes` in the payload is base64-wrapped Anthropic event JSON; signing is `aws-sigv4` | §5 |
| Non-chat operations such as model listing | The kernel has `ProviderApi::Models` (kernel:provider.rs:137-143); the provider world has no corresponding function. A WIT world cannot have optional exports, so adding a function to v2 makes a new world; recommended is a separate small world (e.g. `provider-catalog-v1`: build a list request, parse a list response), used by the host for health probing and model discovery, replacing the per-type probe fallback table (P21 Appendix B.3) | §8 (multiple worlds coexisting) |
| Quota header normalization | §10 | — |
| IR explicit / implicit cache buckets | Kernel chain (P21 §7) | kernel |
| Task estimate in "whole-order milliunits" | An additive change to the task contract (P21 S5) | §8.6 |

## 12. The synthetic unseen-provider guest (T21)

Location and form follow T03: `crates/south-provider-runtime/tests/guests/t21-unseen-provider/`, not published with
releases; a host script outside the commit gate builds it from a local south checkout. T03 is not extended
directly: the host's T03 canary has already pinned its wire format and its "three fields that are not free" as
acceptance facts (t03-canary-provider/src/lib.rs:16-37), and changing it would change the meaning of existing
acceptance. What T21 must prove, each item mapping to a host red item:

1. The family name is `t21-unseen-wire`, with a wire format unlike every known dialect (as with T03: a known parser
   handed it can only fail).
2. **Auth**: the manifest declares only `header_secret`, and the descriptor presents an admitted secret header;
   another model mode uses a `minted` slot — a single-step `jwt_sign` (HS256) recipe, with the fake upstream
   verifying the signature against a known key. → J2b①, §3, §4.
3. The **cap** is written at the nested location `/t21/limits/max_out`, declared by `request_facts`; the model is in
   the URL. → J2b⑤, §7.2.
4. It needs one non-secret config key (declared by `config_schema`, syntax `token`) and uses it to build the URL.
   → §7.3.
5. **Streaming** in two model modes: `bytes` (reusing T03's non-SSE line format) and `aws-eventstream` (the fake
   upstream sends real eventstream frames). → §5.
6. **Usage** has its own shape, including cache buckets; the fixtures carry every required usage row and
   `usage_pointer`; there is also a family with `usage_evidence: absent`. → §6.
7. A two-model **catalog** ships with the package, one model carrying a new dialect word; the host's admission
   results change when the catalog changes. → §7.5, P21 S6 acceptance.
8. **Compatibility**: `south_runtime` takes an older value within the range and loads normally; a separate skew
   package with a different `runtime_abi` is also prepared, and only it is refused, with other packages and the
   known dialects unaffected. → J2b③, §8.
9. It appears in a test **release index**, and the fetch script needs no list. → J2b④, §9.
10. **Rogue modes** (triggered by model name, as T03 does): T03's four, plus `rogue-arm` (the descriptor's auth arm
    is not in the manifest), `rogue-cap` (the cap is written at an undeclared location) and `rogue-zero-usage` (a
    2xx without usage that still produces zeros); plus a recipe whose endpoint points at an undeclared host, as a
    gate ① counterexample. The host must refuse T03's four, `rogue-arm` and `rogue-cap` with zero upstream calls;
    `rogue-zero-usage` must turn red at gate ②.

The J3 counterpart: delete the T21 package but keep its catalog rows → startup reports the family unavailable,
requests fail fast as "not served", and no built-in implementation takes over (§8.5).

**What T21 cannot prove**: that a component reports usage faithfully according to the upstream documentation (the
undetectable zone of §6.3), or anything outside the link layer.

## 13. Phasing

| Phase | South delivers | Unlocks in the host | Red items flipped | Release |
|---|---|---|---|---|
| B0 | This record ruled on by the south maintainers; existing text revised per §14 | — | — | Docs |
| B1 | Usage strictness: the three reference implementations, gate ② by-name enforcement, `UsageNeverDefaulted`, `usage_evidence`, documentation judges in the release discipline, the reasoning convention (once Q13 is settled) | P21 S5 (and T-7) | Prerequisite for S5 acceptance | minor; three package identities bump |
| B2 | Descriptor auth admission and `DescriptorAuthWithinManifest`; `request_facts`, `config_schema`, `stream_framing`, `signing`; the south-provided eventstream deframer | P21 S1, S4 | J2b① (static slots), ⑤ (J2b② is purely host-side: descriptors carry the URL today, and gate ②'s `EndpointConfinement` already guards it) | minor |
| B3 | `runtime_abi` and ranges, `load_package_set`, release index | P21 S2 | J2b③④, J3③ | minor + release behavior |
| B4 | Credential recipe v1: manifest section, reference interpreter, gate ② fixtures, gate ③ host suite | P21 S3; P22 Vertex, P23 Vertex TTS | J2b① (minting part) | minor |
| B5 | T21 guest, gaining modes phase by phase alongside B1–B4 | J2 standing pilot | All of J2b | Not published |
| B6 | Responses upstream, Kiro, InvokeModel-Anthropic, catalog data, catalog world | P21 S6, S7 | J1 keeps falling | minor each |
| B7 | §10 instance declaration; kernel chain (`Auth` combined arm, credential header catalog, cache buckets) | P21 S7 | New instances no longer touch the link layer | minor + kernel |

B1, B2 and B3 are independent of one another and can proceed in parallel; B4 depends on B2's descriptor auth
admission; B6 depends on B1–B4. B7 comes last only because it goes through the kernel chain; after the Q1 ruling
it is required for DP0 and is not optional.

## 14. Existing text to revise in step

- ARCHITECTURE.md:117-126: change the concluding sentence to "execution and material belong to the host;
  per-provider description belongs to the component" (§3.4).
- ARCHITECTURE.md:205-207: "the runtime and the thirteen packages must be upgraded in one batch" lapses with §8.
- The Kling comment at manifest.rs:128-132: point it to task-v2's bearer approach and §3.
- reference_bedrock_converse.rs:21-35, 549-557: restate as "this family declares `stream_framing: aws-eventstream`".
- 2026-09-10-released-component-artifacts.md:152-154: native reference implementations are not stand-ins for
  missing packages (§8.5).
- The rationale at 2026-09-27-task-contract-v6-facts.md:22-23 lapses with §8.6; later contracts follow the additive
  rule.

## 15. Rejected alternatives

- **Kling via `HostSigned`** (§3.2): the provider knowledge merely moves to another place in the host.
- **Token exchange as a WIT function, with component code generating the exchange request**: the component would
  either touch secret values such as refresh tokens, or need a "placeholder substitution + response redaction"
  protocol to keep secrets out of the component; a data recipe can be validated as a whole at gate ① and read line
  by line in review, and its expressiveness suffices for the six known families. The cost is that a flow outside the
  vocabulary requires a contract upgrade (§3.7).
- **The host choosing an `sse` / `ndjson` decoder by declaration**: the components already split these two
  themselves, so the host would gain a useless branch (§5.2).
- **Keeping the exact tuple and automating "re-stamp everything"**: the collateral damage of J2b③ would not go away
  — any package from a different batch still would not load.
- **Compiling the capability catalog into each package's wasm** (§7.5 A): models move far faster than dialects, so
  package digests would change frequently because of data, diluting the point of pinning digests.
- **The host inferring the credential kind from what files such as `auth.json` look like**: judging by what a file
  looks like is judging by provider; declare it through the manifest's `import` instead.

## 16. Open questions

Tags: S = south maintainers, L = lv, K = kernel.

- **Q1 (L) Link layer**: does DP0 count "bumping the south / kernel pin" as modifying the host? If it does, §10's
  instance declaration is a necessary condition for DP0, not an improvement.
  **Ruled (lv, 2026-09-30): it counts.** A new provider that needs a secret header name, query name or quota header
  outside the compiled-in sets would otherwise force both hosts to re-pin, rebuild and release. §10 is a necessary
  condition of DP0 and phase B7 is required.
- **Q2 (S)** Accept revising the concluding sentence of ARCHITECTURE.md:117-126 (§3.4).
- **Q3 (S, L)** Credential recipes as a closed data vocabulary (recommended) or as WIT functions; and accept the DP0
  boundary that "a new flow outside the vocabulary requires a contract upgrade".
  **Ruled for the host side (lv, 2026-09-30): a closed data vocabulary; the contract-upgrade boundary is accepted.**
  The south maintainers' half remains open.
- **Q4 (S, K)** The combined arm: add a variant to the kernel's `Auth` (recommended), or an interim manifest
  family-level mirror (§4.3).
- **Q5 (S)** Refining DP5: a `runtime_abi` epoch (recommended) or south going straight to 1.0; how the IR
  compatibility line is determined; contracts made additive, or multi-version decoding (§8.2, §8.6).
- **Q6 (S)** Does the eventstream deframer go into `south-core` (recommended), or does each host implement its own
  (§5.2)?
- **Q7 (L, S)** Model catalog: a south data artifact (recommended), built into the components, or pure operator
  data; if it belongs to south, who keeps up with providers' new models (§7.5)?
  **Ruled for the host side (lv, 2026-09-30): a south data artifact.** Who maintains it remains open for the south
  maintainers.
- **Q8 (S)** Do the native reference implementations remain a supported production engine? If so, J3 needs the
  host to disable fallback explicitly (§8.5).
- **Q9 (S, community host)** Reference-implementation strictness is a behavior change for the community host
  (§6.4); in addition, ARCHITECTURE.md:114-115 requires a metering vocabulary to have "a second consumer in sight" —
  both `usage_evidence` and the recipes need the community host to confirm its intent to adopt them (P21 §7
  recommends implementing in step).
- **Q10 (L)** DP7: does south take in Copilot's editor headers, Claude Code's impersonation headers and Codex's
  client-identification headers? They would appear in both recipes and components.
  **Ruled (lv, 2026-09-30): south takes them in; the host keeps no special case.** Headers on the inference request
  are written by the component; headers on the exchange request are written by the credential recipe; names outside
  the compiled-in sets are declared per §10. The per-provider lists are specified in the component records
  (`2026-09-30-kiro-provider-component.md`, `2026-09-30-openai-responses-upstream-component.md`; Claude Code and
  Copilot to follow).
- **Q11 (S)** Must artifact signing be completed before third-party packages are loaded through the index (§9.3)?
- **Q12 (L)** Is the estimate for `usage_evidence: absent` made by the host's generic estimator (this record's
  recommendation, by characters), or reported by the component as an estimate when it builds the request (the
  embeddings record takes the latter; see `2026-09-30-embeddings-contract.md` §7)? Chat goes through the kernel IR,
  which has no place for a component estimate; the embeddings contract is south-local and can hold one. Is it
  acceptable for the two worlds to follow different conventions?
  **Ruled (lv, 2026-09-30): the two conventions may coexist.** On the chat family the estimate for
  `usage_evidence: absent` is the host's generic estimator; in south-local contracts that can hold one, the component
  reports the estimate at build time. Both are labeled as estimates in the ledger.
- **Q13 (S, L)** The reasoning-token convention (§6.1 item 5, §6.2 item 6): confirm whether Gemini's
  `thoughtsTokenCount` lies outside `candidatesTokenCount`; if it does, the Gemini reference implementation's
  `output_tokens` must change, and the host's amounts charged by output change with it, which needs lv's
  confirmation and a dual run.

## 17. Amendments found while drafting the component records (2026-09-30)

Three follow-on records were drafted after this one — `2026-09-30-kiro-provider-component.md`,
`2026-09-30-openai-responses-upstream-component.md` and `2026-09-30-north-codec-render-gaps.md` (follow-on PR).
Working two real components through the proposals above showed where this record is wrong or incomplete. The
sections above are left as written so the review thread stays readable; each item below says what changes and
where the argument is. Items marked *verified* were re-checked against code by the host team; the rest rest on the
component records' own citations.

| # | Section | What this record says | What changes | Argued in |
|---|---|---|---|---|
| A1 | §7.4, §11 | A "Responses only" model is expressed by a model dialect word | It is expressed by the provider row belonging to a family of a separate package. A dialect word can change the request shape inside one package's wire, but `parse-response`, `parse-stream-chunk` and `map-provider-error` receive no provider config (`provider-adapter.wit:116-132`, *verified*), so every family in a package must share one response wire | Responses record §3.1, §3.2 |
| A2 | §11 | Wire types can be shared with `south-north-codec`; only the mapping cannot | `south-north-codec` has no Responses wire types — its Responses module works on `serde_json::Value` (*verified*: only options / context / frame / state structs exist). What can be shared is the event vocabulary, extension key names, fixtures and the round-trip judge | Responses record §9 |
| A3 | §7.2, §6.3 | `request_facts` has exactly one location carrying the output cap, and a stream switch in the body or the URL | Both must admit "none". Kiro has neither a cap field nor a stream switch; the Codex family has no cap field. §6.3's check `output_tokens ≤ cap` assumes the cap was sent upstream and needs a stated rule for families that cannot send it | Kiro record §8.2 (P-1), §8.3 (P-2); Responses record §4.3 |
| A4 | §10 | The closed sets with provider instances are secret headers, query parameters, quota headers and the kernel's credential header catalog | Add the `user-agent` value. `ControlledUserAgentV1` takes a `&'static str` that "must exist in host program text" (`south-contracts/src/lib.rs:1233-1246`, *verified*). After the rulings on Q1 (a pin bump counts) and Q10 (south takes in client-identification headers) a new provider's user-agent cannot stay a host literal; it has to be declared per family and validated by gate ① against the existing value grammar. This reopens a deliberate safety property of that type and needs the maintainers' ruling | Kiro record §3.3 (P-3), §4.6; Responses record §10.4 |
| A5 | §3.3, §3.8 | Kiro's two forms are `oauth2_token` recipes; `select` picks by one non-secret field | Kiro's exchanges are not RFC 6749 shaped: both send camelCase JSON, and the social form sends only `{refreshToken}` with no grant type (`server:…/token_refresh.rs:1254-1268`, *verified*). Either `oauth2_token` stops prescribing RFC parameter names, or these are `http_exchange` steps. `select` needs ordered rules that may test the presence of several fields | Kiro record §7.2, §7.3 (P-6) |
| A6 | §3.3 | The recipe sketch covers expiry from the response or the JWT | Missing: a default and clamp when the response gives no expiry; a fixed validity window after refresh (the host uses 50 minutes for Codex); what `jwt_exp` does with a non-JWT token; exported attributes that persist across requests and fall back to a stored field; import with several candidate pointers; which field a rotated token is written back to | Kiro record §7.3; Responses record §10.2 |
| A7 | §5.2 | `stream_framing` defines what `parse-stream-chunk` is fed | It must also cover upstreams that answer a non-stream request with an eventstream body (Kiro): the host deframes the whole body and hands `parse-response` the same canonical re-encoding | Kiro record §5.4 (P-5) |
| A8 | §6.2 item 4 | `usage_evidence: absent` — the component emits no usage | It does not say what `parse-response` returns: `usage` is not optional in the IR and the WIT text says it is never zero. State that for an `absent` family the field is zero and the host must not read it, and check it with a conformance property | Kiro record §6.1 |
| A9 | §6.2 item 5 | Usage criteria are derived from the provider's official documentation | Kiro has no public documentation. For such providers the criterion is captured traffic archived with the fixtures, plus the property that an `absent` family never emits usage | Kiro record §11.2 |
| A10 | §7.3 | Non-secret config values choose from the closed value syntaxes | None of them admits an ARN (`:` and `/`), which Kiro's profile needs | Kiro record §7.5 |
| A11 | §6.1, §11 | `response.completed` is the terminal frame of the Responses wire | `response.incomplete` also carries usage and is rendered as a terminal by `south-north-codec` itself; whether it is complete evidence is an open question in the Responses record | Responses record §6.3 |
| A12 | §11 | The component maps `store` from the IR | The IR and `south-north-codec` carry neither `store` nor `previous_response_id` nor `include`; the component always sends a stateless request, and refusing (rather than dropping) a client's `previous_response_id` needs the codec to carry it | Responses record §4.4 |
