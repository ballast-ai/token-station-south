# Zero vendor logic in the host: revising the south boundary

Status: proposed — drafted for review by the host team (token-station-server P21), not accepted

Date: 2026-09-30

Revised: 2026-10-01 after independent review (see the revision note at the end).

Predecessors: `2026-08-21-canonical-ir-inventory.md` (S0: D1 usage extraction belongs to the component, D2 chunks
are bytes, §6 the `ProviderConfig` policy fence, D5 `extensions` are data),
`2026-08-27-manifest-schema-beyond-one-world.md` (manifest validated per world; `host_signed` and `emits`),
`2026-09-10-released-component-artifacts.md` (release artifacts and `SHASUMS256.txt`),
`2026-09-20-task-adapter-v2-candidate.md` (task-v2 admits only bearer / header_secret),
`2026-08-20-controlled-user-agent.md` (the `'static` user-agent ruling); `ARCHITECTURE.md` "What never enters
South" (ruled 2026-09-08).

Origin: token-station-server plan P21 (`docs/product-review-v2/plans/2026-09-29-P21-*.md`)
(DP0 and DP1 decided; the decisions of P21 §8.3 "before stage 2 starts" — DP3, DP4, DP5, DP9, DE1, DE2 and
others — were approved as recommended on 2026-09-30, as relayed by the host team), P24 in the same directory
(DE1, DE2), and the red items measured in the P21 §5 S0 pilot. The embeddings contract is in
`2026-09-30-embeddings-contract.md`.

Rulings: on 2026-09-30 the host owner (lv) ruled on the questions tagged L in §16 — Q1, Q3, Q7, Q10 and Q12; each
ruling is recorded under its question. On 2026-10-01 lv ruled the host side of Q18, and Q13 was answered by
measurement (§16 Q13). Questions tagged S or K remain open for the south and kernel maintainers. Two rulings made in
sibling records bind this one and are quoted where they apply: Responses R-Q5 / Kiro K-Q1 (families that cannot send
the output cap, §6.3) and image Q7 (`rejected` releases the reservation, §6.4).

Baseline: south `origin/main` = v0.42.0 (`3135e36`); kernel `f585bc83` (protocol 0.4.0 / kernel v0.3.0); host
`a82c852b`. South line numbers refer to this baseline. Host line numbers refer to token-station-server `a82c852b`
and carry a `server:` prefix; kernel line numbers carry `kernel:` and refer to `crates/protocol/src/` at `f585bc83`.

## 0. Summary

The host's standard, DP0: adding or removing a provider changes only south, with zero host changes. South's
boundary as it stands cannot deliver that — not because components cannot be written to do the translation, but
because south **leaves the host to know** a set of facts that differ per provider: how to mint credentials,
whether the auth arm in a descriptor counts, how the upstream frames its stream, whether usage can be trusted,
where the cap field is, where the endpoint lives, whether a package works across versions, which packages exist to
install. This record gives a revision for each:

| # | Problem | South today | Proposal | Section |
|---|---|---|---|---|
| a | Credential minting | Minting, OAuth refresh and JWT signing are host code, "not a gap South intends to close" (ARCHITECTURE.md:117-126) | The component **describes** the recipe in the manifest with a closed step vocabulary; the recipe is untrusted input, so its endpoints are operator-confirmed and third-party recipes wait for package signing; material, locks, CAS and a no-wipe invariant stay in the host; Kling uses the same recipe mechanism rather than `HostSigned` | §3 |
| b | Auth arm | `auth_arms` only validates the vocabulary (manifest.rs:346-364); nothing checks the descriptor's `auth` against it | South provides a descriptor auth admission function; gate ② adds a matching check; the host must pass the slot to the component | §4 |
| c | Framing / signing | Components split SSE themselves; only AWS eventstream is split and re-encoded by the host (reference_bedrock_converse.rs:21-35); the host infers the signing scheme from the type | The manifest declares `stream_framing` per package and `signing.scheme` for `host_signed` (both closed sets); south provides one eventstream deframer with a canonical re-encoding, in `south-contracts` | §5 |
| d | Usage | Already ruled to belong to the component (canonical-ir-inventory.md:187-203), but three reference implementations fill missing fields with 0 and gate ② does not require usage samples | Make the reference implementations strict; gate ② requires usage rows and a "delete the usage" mutation check; the manifest declares `usage_evidence`; the host checks only bounds it computes itself and internal consistency, with a defined funds outcome | §6 |
| e | Request facts / endpoint / capabilities | The manifest has no location declarations; the host builds region- and project-bearing URLs; `model-capabilities` only echoes what the operator declared | The manifest declares cap, model and stream locations and an endpoint template per family; per-model differences go through dialect words; the model catalog becomes a south data artifact | §7 |
| f | Compatibility | The four tuple items must be exactly equal (manifest.rs:657-694) | `runtime_abi` epoch + south runtime range + exact kernel contract numbers + explicit south contract numbers; per-package admission report | §8 |
| g | Package discovery | A release carries only archives and `SHASUMS256.txt` (release.yml:98-106) | Publish a machine-readable index, `south-release-index.json` | §9 |
| — | Provider instances in closed vocabularies | Secret header names, query names, quota header names and the user-agent value are compiled into the host | "Closed mechanisms, declared instances" | §10 |
| h | Follow-on components and non-chat operations | None | Scope and dependencies listed | §11 |

§12 lists what the synthetic unseen-provider guests (T21) must prove, §13 gives the phasing, and §16 lists the
questions that need a ruling from the south maintainers, the kernel maintainers or lv.

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
| J2b① auth | The manifest declares only `header_secret`, yet the upstream still receives Bearer | `auth_arms` is only a vocabulary (manifest.rs:234-238, 355-362); south has no public "descriptor auth → contract auth" function, and the raw prelude states that the host picks the auth arm (raw.rs:107), so the host builds a table by type (server:gateway/src/modules/inference/engine/south_adapter.rs:157). Half of the root is host-side: the text seam hands the component `auth: None` (server:…/south_component.rs:735, 746), so the component names no slot and the host's table decides (§4.1) |
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
proposal in this record states whether a new provider still touches the link layer afterwards; §10 deals
specifically with the category most likely to trip on this.

## 2. General rules

**R1 Closed mechanisms, declared instances.** Closed sets are used only to name **mechanisms**: auth arms, framing
forms, signing schemes, token-exchange step kinds, JWT algorithms, value syntaxes — they correspond to public
standards or to south's own protocol (P21 §1.1, DP4, DP9). **Instances** specific to one provider — header names,
query names, endpoints, field pointers, dialect words, user-agent values — are declared by the component in the
manifest, and south validates them at gate ① against syntax and safety rules. A new mechanism is a south contract
upgrade plus one host link-layer upgrade (what P21 §1.4 calls "a new public-standard executor"); a new instance is
just a new package.

**R2 The component declares, the host executes, and the host never chooses by provider identity.** The host writes
one generic executor per mechanism; which executor to use, and with what parameters, is decided only by the
manifest and the descriptor, never by `provider_type`, row name or domain.

**R3 Boundaries that do not move.** Components have no network and no filesystem (manifest.rs:331-344;
loader.rs:24-32); credential values never enter a component (provider-adapter.wit:56-60; the kernel's `Auth`
carries only a `SecretRef`, kernel:http.rs:150-166); the funds discipline — reservation, terminal-frame
withholding, settlement, `delivery_unknown` — stays in the host (canonical-ir-inventory.md:187-203); material whose
leak impact exceeds one key (service-account private keys, KEKs) and logic whose "wrong decision is money or an
unrecoverable credential" (rotation concurrency guards, CAS) stay in the host (ARCHITECTURE.md:117-123). What this
record moves out of the host is only **per-provider description** — not material, and not guards. A manifest is
untrusted third-party input (manifest.rs:212-216), so every declaration that steers material or money is either
bounded by something the operator or the host controls, or listed in an undetectable zone (§3.4, §6.3, §7.2).

**R4 Every new host execution mechanism gets a gate ③ host suite.** The same approach as header-auth and
controlled-query: the mechanism's fixtures are frozen in south, and only once both hosts pass them is the
mechanism marked `verified` under `host_capabilities` in `compatibility.json`. Otherwise "open-source and
closed-source hosts share southbound work" holds for only one host (P21 §7).

**R5 An absent new field must not silently change behavior.** The manifest is `deny_unknown_fields`
(manifest.rs:212-219), so a new field is itself a south-minor wire-format change
(2026-08-27-manifest-schema-beyond-one-world.md:147-160). When a new field is absent, the semantics must equal
today's behavior, or the package must be refused at load; there is no third outcome.

**R6 Request-side declarations are per family; response-side declarations are per package.** `build-http-request`
receives `ProviderConfig`, whose `provider` names the family, so a package can serve several request shapes. But
`parse-response`, `parse-stream-chunk` and `map-provider-error` receive no configuration
(provider-adapter.wit:110-132; the native trait likewise, component.rs:90-103): one package parses one response
wire. So `request_facts`, the endpoint template and the config schema (§7) are keyed by family, while
`stream_framing` (§5.2) and `usage_evidence` (§6.2) are package-level, and a family whose response wire differs
belongs to its own package (§7.4, §11). `auth_arms` and `emits` are already package-level (manifest.rs:234-245) and
`host_signed` must stand alone (manifest.rs:385-387), so a family that needs a different auth arm from its siblings
also belongs to its own package.

**R7 Gate ② is evidence only when someone other than the author runs it.** For south's own packages gate ② proves
"wasm ≡ native reference" (tests/usage_ir_contract_v1.rs:5-8) and runs in south's CI. For a package south did not
build there is no native reference and the expected outputs are the author's own; the host does not run the suite
at admission. Every claim in this record of the form "gate ② guarantees X" therefore holds for first-party packages
only, unless gate ② is run by the installer (§9.3, §16 Q17). A tightened gate ② binds a package only if the host
refuses packages built before the tightening (§8.6).

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
  (server:gateway/src/modules/inference/engine/token_refresh.rs:387-493; the five implementations start at :120,
  :719, :914, :1302 and :1584; the seven skeleton steps are at :477-493). The trait methods are exactly "everything
  that differs for one family": endpoint and request, response parsing, clock convention, whether it rotates, which
  material to preserve on write-back (Copilot overrides it, :439-449), what to do without refresh material, error
  classification, and Copilot's "404 is another kind of success". Kling's JWT sits outside the skeleton
  (server:gateway/src/modules/inference/engine/upstream.rs:435-460); on the task side there are also binding auth
  recipes named after vendors, `KlingJwt` / `VertexSa` (server:gateway/src/core/task_execution.rs:25-38).
- The host's token endpoints are vendor constants in reviewed host code; the trait says so (token_refresh.rs:413-415).
  Test seams let credential extras override them (`extras.tokenUrl`, :179-180, :696-698; Kiro `refreshUrl`,
  :1171, :1231-1234). Kiro's region, which is substituted into its refresh host, has been validated as a region
  label since `a82c852b` (:1221-1230).

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

This subsection is the one recipe v1 vocabulary; the component records (Kiro, OpenAI Responses) use exactly these
names. Four parts:

- **`fields`**: which fields this kind of credential consists of — name, whether it is secret, value syntax,
  whether it is required, an optional `default`, media type (e.g. service-account JSON). `require_one_of` (a list
  of field groups) states that at least one field of each group must be present. The host renders the credential
  form, stores and redacts from this, replacing the per-family credential pages (the 260 lines of D2 in P21
  Appendix B.3).
- **`import`**: an optional import mapping — which JSON Pointer in which kind of file lands in which field (Codex's
  `auth.json`, Claude Code's credential file, Kiro's sign-in file, service-account JSON). A field may list
  **ordered candidate pointers**; the first present one wins. An optional `seed` names a usable minted value in the
  file (marked secret) and its expiry, so the first request needs no exchange. The host no longer guesses the
  family from what a file looks like.
- **`slots`**: each slot in `permissions.secrets` is either `static` (the operator-entered value is used as is;
  this is what absence means, R5) or `minted` (minted by a recipe). The descriptor still names only the slot;
  **neither the IR nor the WIT changes**.
- **`recipes`**: minting recipes. Each recipe is a small state machine of at most 4 steps, with step kinds drawn
  from a closed set:

| Step kind | Standard | Covers |
|---|---|---|
| `oauth2_token` | RFC 6749 §6 (refresh_token), §4.4; parameter names as the RFC defines them, form or JSON encoding | Codex, Claude Code |
| `oauth2_token` + `grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer` | RFC 7523 §2.1 | Vertex service account exchanged for an access token |
| `jwt_sign`, algorithms from the closed set HS256 / RS256 / ES256 | RFC 7519 / 7515 / 7518 | Kling (output used directly as the bearer); the Vertex assertion |
| `http_exchange` | Plain GET / POST with **declared** parameter names, form or JSON encoding, presenting a field or a previous step's output under the declared auth-scheme (RFC 7235 syntax) | Copilot's second hop (GitHub token → Copilot token); Kiro's social and IdC refreshes, which send camelCase JSON and, in the social form, no grant type at all (server:…/token_refresh.rs:1277-1291) |
| `http_probe` | As above, but looks only at the status | Seat re-verification on Copilot's direct-use flow |

`oauth2_token` keeps the RFC parameter names; a token endpoint that does not speak RFC 6749 is an `http_exchange`
step. That keeps the RFC-named step honest instead of turning it into a generic POST with an OAuth label.

The complete vocabulary:

| Level | Form | Meaning |
|---|---|---|
| field | `secret`, `required`, `syntax`, `media`, `default`, `description` | As above; `default` is a non-secret value used when the field is empty; `description` is one line for the operator form (§13.5 D4) |
| credential | `require_one_of: [[field, …], …]` | Each group needs at least one present field; checked when the credential is saved |
| credential | `families: [family, …]` | The families the section applies to; absent means every family of the package (§13.5 D2) |
| slot | `{"minted": recipe}` / `{"field": name}` | Minted by a recipe, or the stored value of a declared secret field; where a section applies every slot names one of the two (§13.5 D3) |
| import | `pointers: [pointer, …]` | Ordered candidates; first present wins |
| import | `seed: { present: {pointer, secret: true}, expires_at: {<clock>: pointer} }` | A minted value already in the file; clock forms as in `extract`, plus `rfc3339_or_epoch_seconds` (import only) |
| step | `endpoint` | A constant or a template (§3.4 applies to both) |
| step | `endpoint_params: { name: {field: name} }` | Fills the template; each field's value syntax is checked before substitution |
| step | `requires: [field, …]` | Fields that must be present before the step runs; absence is a configuration error with no network call |
| step | `params` | `{"const": …}`, `{"field": name, "pointer": …}`, `{"output": "step.name"}`, `{"now_plus": seconds}`; encoding `form` / `json` |
| step | `headers` | Ordinary headers under the descriptor safe-header rules; editor and client-identification headers on exchange requests go here (Q10 ruling) |
| step | `on_status` | Exact codes and the class keys `4xx` / `5xx` map to `reauth_required` / `transient` / `{"goto": step}`; an exact code wins over its class; with no entry, 4xx → `reauth_required`, 5xx → `transient` |
| step | `extract: { name: { pointer, secret, optional } }` | Extraction by JSON Pointer; an `optional` extraction may be absent without failing the step |
| step | clock forms in `extract` | `relative_seconds` / `epoch_seconds` / `epoch_millis` / `jwt_exp` / `fixed_window` |
| step | `jwt_claim: { token, pointer }` | Decodes a JWT payload without verifying the signature |
| step | `must_equal_field: field` | On an extracted value: when both it and the named stored field are present and differ, the refresh fails as `reauth_required` and nothing is written back |
| recipe | `present` | Which output becomes the slot value: one step output, or ordered candidates, each a step output or `{"field": name, "validity_seconds": n}`; the first present one wins (§13.5 D1) |
| recipe | `rotates_refresh_material` | **Required, no default** |
| recipe | `write_back: { field: "step.output" }` | Where rotated refresh material goes; **required whenever the recipe rotates**; an absent output keeps the stored value (§3.5) |
| recipe | `refresh_margin_seconds`, `without_refresh_material` (`use_stored` / `fail`) | As in the host skeleton |
| recipe | `default_seconds`, `fixed_validity_seconds` | Expiry fallback, and the window of `fixed_window` |
| recipe | `min_ttl_seconds`, `max_ttl_seconds` | Optional; may only narrow the host's clamp (§3.5) |
| recipe | `select: [ { when: <predicate>, recipe }, …, { recipe } ]` | Ordered rules, first match wins, the last rule has no test; predicates `field_present`, `all_present`, `field_in` (ASCII case-insensitive). Presence predicates may test secret fields — presence only, never the value; `field_in` only non-secret fields |
| recipe | a recipe holding only `select` | A selector: no steps, no `present`, no `rotates_refresh_material`; the slot may name it, and every recipe it names must itself be complete |
| recipe | `attributes: { name: { field, export: true, persist } }` | Exported non-secret values (below) |
| value syntax | `aws_region`, `aws_arn`, `gcp_project_id`, `api_version_date`, `digits`, `token`, `printable_ascii` (bounded length), `enum[…]` | The closed set shared by credential fields, config keys (§7.3) and template parameters |

Notes on the forms:

- **Endpoint**: a template's parameters take only non-secret fields validated against their value syntax (e.g.
  `aws_region`, the rule the host already applies to Kiro at server:…/token_refresh.rs:1221-1230). The host is
  never taken from credential contents or operator extra configuration. Every endpoint — constant or template — is
  subject to the trust rules of §3.4.
- **`on_status`**: Copilot's "404 is another kind of success" (server:…/token_refresh.rs:1006-1041) is
  `"404": {"goto": "direct"}`; Kiro, where only 400 and 401 are terminal today, is
  `{"400": "reauth_required", "401": "reauth_required", "4xx": "transient"}`.
- **Clocks**: `fixed_window` ignores any expiry in the response and uses `fixed_validity_seconds` (the host does this
  for Codex today with a fixed 50-minute window after refresh). `jwt_exp` on a value that is not a JWT, or any
  convention whose field is missing, falls back to `default_seconds` when declared and is otherwise a `transient`
  failure. Every expiry then goes through the host's clamp (§3.5).
- **`jwt_claim` is a check, not a source of exports.** Codex's `id_token` claim is compared with the stored
  `account_id` through `must_equal_field`; the exported account id comes from the stored field.

Making the clock convention and rotation "no default" copies the host's lessons: getting either backwards raises no
error — the former makes tokens never expire or always expire, the latter wipes out the minting key
(server:…/token_refresh.rs:399-437). Absence means gate ① refuses.

**Exported attributes.** Some values are not secret but the component needs them: Codex's account id (goes into a
request header), Vertex's project id (goes into the URL), Kiro's profile ARN (goes into the body; a non-secret field
with syntax `aws_arn`). A recipe lists them under `attributes`; each takes its value **only from a field declared
non-secret** — never from a secret field and never from an exchange response, because secrecy of a response value
is the author's claim and gate ① cannot check it. An attribute may declare `persist: true` (the host stores the last
value and keeps using it when it is later absent). How the attribute reaches the component is open: the current
`ProviderConfig` fence admits no such channel (canonical-ir-inventory.md:155-164 and D5 at :249-255;
provider-adapter.wit:89-90). §16 Q14 asks for the route; the recommendation is a typed field through the kernel
chain (D5's own promotion path), the alternative an explicit, argued amendment of the fence. Either way the host
strips any client-supplied key that collides with a reserved name.

Sketch (field names are a draft):

```json
"credentials": {
  "schema": "south.credential-recipe.v1",
  "fields": {
    "service_account": { "secret": true, "required": true, "media": "application/json" },
    "project_id":      { "secret": false, "required": true, "syntax": "gcp_project_id" }
  },
  "import": { "service_account": { "file": "service-account-json", "pointers": [""] },
              "project_id":      { "file": "service-account-json", "pointers": ["/project_id"] } },
  "slots": { "provider_api_key": { "minted": "vertex_sa" } },
  "recipes": {
    "vertex_sa": {
      "steps": [
        { "id": "assertion", "kind": "jwt_sign", "alg": "RS256",
          "key":    { "field": "service_account", "pointer": "/private_key" },
          "claims": { "iss":   { "field": "service_account", "pointer": "/client_email" },
                      "scope": { "const": "https://www.googleapis.com/auth/cloud-platform" },
                      "aud":   { "endpoint_of": "token" },
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
      "attributes": { "project_id": { "field": "project_id", "export": true } }
    }
  }
}
```

Kling, by contrast, is a single `jwt_sign` step (HS256; key from `secret_key`; claims `iss`←`access_key`,
`exp`←now+1800, `nbf`←now−5), with `present` pointing at that step's JWT. Its JWT is presented to the inference
upstream, not sent to a token endpoint, so the `aud` rule of §3.4 does not apply; the presentation is confined by
`ProviderConfig::authorize` like any bearer (kernel:provider.rs:363-378).

### 3.4 Trust model for recipes

A recipe decides where derived credential material goes: a refresh token is sent to the step's endpoint, a signed
assertion is sent to the next step's endpoint. For the inference request the only anchor is the operator's
`base_url` (`ProviderConfig::authorize` requires the same origin, kernel:provider.rs:65-84, 363-378); a recipe has
no such anchor unless this record gives it one. Without the rules below, a package could, for example, sign a
Google assertion with `aud` = the Google token endpoint and full scope, send it to its own endpoint, and redeem it
there — a leak whose impact exceeds one key, the first 2026-09-08 test. So:

1. **Endpoints are confirmed, not declared.** Every endpoint a recipe can reach (constants and the host part of
   templates) is shown to the operator when a credential of that kind is created, and the operator confirms the
   list for that package digest. A package update that changes the list needs a new confirmation. On the host side
   this is the only mechanism: a host-side allowlist is rejected, because it would make a new provider endpoint a
   host change, against DP0 (§15; ruled under §16 Q18).
2. **Assertions are bound to their destination.** A signed assertion that is sent to a step must carry
   `aud` = that step's endpoint (`{"endpoint_of": step}` is the only admitted `aud` source when the output is sent
   anywhere); gate ① refuses anything else.
3. **No constant `sub`.** A recipe may not set `sub` (or any claim that selects a principal other than the key's
   own) to a constant; if a future flow needs delegation, `sub` comes from an operator-entered field.
4. **Export only from non-secret fields** (§3.3). A recipe cannot move a secret into the component by labelling
   it.
5. **Third-party recipes wait for signing.** Until package signing exists (§9.3, §16 Q11), the host enables
   recipes only for packages it verifies as south first-party releases by digest against the release index;
   a third-party package that declares `credentials.recipes` is admitted with every slot treated as `static`, or
   refused, at the operator's choice.
   The same holds for **signing** (host feedback SF19, §13.6, §16 Q34): a host's finalizer signs whatever request
   the component builds for the template's host, with the operator's credential, so a third-party package declaring
   `signing` could obtain signed requests to any path on that host. Until package signing exists, a host enables
   `signing` only for a package it verifies as a south first-party release by digest; any other package declaring
   `signing` is refused.
6. **Test endpoints are a build feature, not configuration.** The host's generic executor needs a fake token
   endpoint for gate ③ (§3.7). That override is compiled only into test builds; the production executor has no
   configuration or credential field that replaces an endpoint. This retires the `extras.tokenUrl` / `refreshUrl`
   seams of §3.1 rather than inheriting them.

### 3.5 Host invariants, and why this does not violate the two 2026-09-08 tests

The host executor keeps two invariants that hold whatever a recipe declares:

- **No wipe.** It never overwrites non-empty refresh material with an empty value, and it keeps the previous
  generation so that a wrong rotation can be rolled back. An absent `write_back` output therefore keeps the stored
  value.
- **TTL clamp.** Every expiry, whatever its source, is clamped to a provider-agnostic host range of 60 seconds to
  24 hours. A recipe may declare `min_ttl_seconds` / `max_ttl_seconds` only to narrow that range; gate ① does not
  require a recipe clamp.

Against the two tests:


- **Leak impact**: private keys, service-account JSON and refresh tokens remain only in host storage and the host
  process; the component receives only slot names and attributes taken from non-secret fields. South's crates do
  not touch these values either — the recipe is data, and the executor is in the host. Where the material is
  **sent** is bounded by §3.4 rules 1–3, not by the recipe alone.
- **Money and unrecoverable credentials**: named locks, re-reading after taking the lock, CAS write-back, and CAS
  losers re-reading the authoritative new generation (the seven steps at server:…/token_refresh.rs:477-493) all
  stay in the host's generic executor. The rotation flag is different: it is exactly a "wrong decision is an
  unrecoverable credential" item (token_refresh.rs:431-437), and a recipe is the author's decision. The no-wipe
  invariant above is what makes that acceptable: a mis-declared recipe costs a failed refresh, not the
  credential. The same invariant covers what Copilot's
  `preserved_refresh_material` override protects today (token_refresh.rs:439-449).

So what needs revising is the **concluding sentence** of ARCHITECTURE.md:123-126, not the two tests: the
"per-provider authentication layer" changes from host code to data declared by the component, and "remain host
code" becomes "their **execution**, their **material**, the **destinations** they may reach and the **host
invariants** belong to the host".

### 3.6 Host counterpart

One generic recipe executor, replacing the five `MintStrategy` implementations, Kling's JWT minting and the
vendor-named recipes in task bindings; the skeleton is unchanged. The timing follows raw.rs:737-745: minting
completes before the funds marker, a failure is a pre-admission error that moves no money, and taking values after
resolution cannot fail. Token-exchange egress uses the same guard as webhooks (https only, pinned address, no
redirects, no system proxy — P21 §9 E-1), with a bounded response body, and only to confirmed endpoints (§3.4).
A health probe never forces a refresh on a recipe that declares `rotates_refresh_material: true` (a probe that
rotates can burn the chain; Kiro record P-8).

### 3.7 Conformance

- **gate ①**: recipe structure validation — step kinds and algorithms are in the closed sets; endpoints are constants
  or restricted templates; both no-default fields are present (a selector recipe, which holds only `select`, is exempt
  and each recipe it names is checked instead), and `write_back` whenever the recipe rotates; `present` is reachable;
  `goto` is acyclic with at most 4 steps; the §3.4 rules (`aud` binding, no constant `sub`, export only from
  non-secret fields); a declared clamp lies within the host range; `select` predicates test only presence of secret
  fields; `requires` and `must_equal_field` name declared fields.
- **gate ②**: a new fixture family `credential.*`: given fake field values and fake responses, assert the rendered
  exchange request (method, URL, encoded body, headers) and the extraction results; it must include one rotation
  sample, one `on_status` transition sample and one clock-convention sample. Executing these fixtures needs a
  south-provided **reference recipe interpreter**, which runs only in tests and sees only fixture fake values; the
  production executor is the host's. Per R7 this is evidence for first-party packages.
- **gate ③**: host suite `south.credential-recipe.v1`: a fake token endpoint (test builds only, §3.4 rule 6),
  covering exchange failure, concurrent refresh (two requests hit the upstream only once), expiry and clamping,
  rotation write-back, a rotation that returns empty material (must not wipe; §3.5), CAS loser re-read, no retry on
  `reauth_required`, and a probe that must not rotate.

### 3.8 Versioning

A new optional manifest section: south minor (R5: absent means every slot is `static`, which equals today). Once
recipes land, the `oauth` arm and the kernel's `Auth::OAuth` become redundant: keep parsing them, mark them
deprecated, stop recommending them; removing them would have to go through the kernel chain and is not worth it.
Host link layer: the recipe executor is a one-time new generic mechanism (P21 §1.4); after that, an OAuth family
that fits within the step vocabulary is a package-layer change. **A new flow outside the vocabulary** (a sixth step
kind) is a south contract upgrade plus a host executor upgrade — this is the edge of DP0's coverage under this
proposal; see §16 Q3. A third-party provider that needs a recipe is also outside DP0 until signing exists (§3.4
rule 5).

### 3.9 Coverage check

| Family | In the host today | Recipe expression |
|---|---|---|
| Codex | JSON refresh_token grant; a fixed 50-minute window after refresh; stored account id, refusing a refresh whose `id_token` names another account; rotates | `oauth2_token` (json) + `fixed_window` (`fixed_validity_seconds` 3000); `account_id` exported only from the stored non-secret field; the `id_token` claim only as a `must_equal_field` check; rotates = true, `write_back` = `refresh_token`; `without_refresh_material: use_stored` (Responses record §10.2 is the full recipe) |
| Claude Code | JSON refresh_token grant; millisecond clock; rotates | `oauth2_token` + `relative_seconds` (the stored convention is normalized by the host); rotates = true, `write_back` to `refresh_token` |
| Copilot | GET + `Authorization: token …` + editor headers; a 404 switches to the direct-use flow and re-verifies the seat; does not rotate | `http_exchange` + `on_status` 404→`goto` + `http_probe`; `present` candidates end with the GitHub token field and its 3600 s validity (§13.5 D1); rotates = false; the editor headers are declared on the step (Q10) |
| Kiro | Social / IdC forms, camelCase JSON bodies, endpoint built from a region template; only 400 / 401 terminal; rotates | `select` (`field_in` / `field_present` / `all_present`) + two `http_exchange` recipes with `endpoint_params` (`aws_region`), `requires`, `on_status` class keys, import `seed`; profile ARN a non-secret `aws_arn` field exported as an attribute; rotates = true, `write_back` = `refresh_token` |
| Vertex service account | RS256 assertion exchanged for a token; project id goes into the URL; does not rotate | The §3.3 sketch; the project id is a non-secret field imported from the same file |
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
- The host's text seam builds `ProviderConfig` with `auth: None` (server:…/south_component.rs:735; the comment at
  :746 says so). The references derive the descriptor's auth from `config.auth`, so they name no slot, `authorize`
  admits `(None, None)` (kernel:provider.rs:380), and the host's table decides. A south function alone cannot
  flip J2b①.

### 4.2 Proposal

1. South provides
   `admit_descriptor_auth(manifest, config, descriptor) -> Result<AdmittedAuthV1, DescriptorAuthErrorV1>` in
   `south-component-conformance` (the in-repo sanctioned consumer of IR types,
   south-component-conformance/src/lib.rs:23-28; the host already calls components through its `sandbox` feature).
   Rules:
   - `Auth::Bearer` → the manifest must contain `bearer` → `RawAuthV1::Bearer`;
   - `Auth::Header { name }` → the manifest must contain `header_secret`, and `name` must be an admitted secret
     header (today the five of `SecretHeaderV1`; §10 makes them manifest-declared) → `RawAuthV1::HeaderSecret`;
   - `Auth::OAuth` → the slot must be `minted` in a §3 recipe, and is presented as bearer (this rule lands with
     recipes in B4; until then `Auth::OAuth` is refused, as it is today on the task path);
   - `None` → only when `ProviderConfig.auth` is also empty (the kernel's `ProviderConfig::authorize` already judges
     this pair, kernel:provider.rs:361-378);
   - the manifest is `host_signed` → the descriptor must carry no `auth`, and the signing scheme follows §5.3.
   Any mismatch is refused before admission, with zero upstream calls.
2. The host passes the provider row's slot in `ProviderConfig.auth` for every package whose arms are not
   `host_signed`, and passes `None` for `host_signed` packages (otherwise `authorize` returns `MissingCredential`,
   kernel:provider.rs:373). Only then does the component's presentation reach the wire.
3. Both the host's text path and its task path switch to calling the admission function; the host's per-type auth
   tables (P21 §2.5, "two to three tables from the same source") are deleted along with it.

### 4.3 Gap: the combined arm cannot be expressed by a component

The contract has `BearerAndHeaderSecret` (south-contracts/src/lib.rs:1331-1346, auth contract 4), which the host
uses for Gemini's OpenAI-compatible surface, which requires the key twice
(2026-09-08-bearer-with-header-secret-auth.md:12-15). That surface takes an OpenAI-shaped request: the host
selects the combined arm when a Gemini row uses a transport other than the native one
(server:…/text_admission/sender.rs:944-955). The package that serves it is therefore
`provider-openai-compatible` (arms `bearer`, `header_secret`), not `provider-gemini`, which only builds native
Gemini URLs (reference_gemini.rs:483-494). The manifest vocabulary has no combined arm (manifest.rs:89) and the
kernel's `Auth` has no corresponding variant (kernel:http.rs:150-166). Once the host presents strictly according
to the descriptor, this path loses any way to be expressed. Options:

- **A (recommended)**: go through the kernel chain to add `BearerAndHeader { name, secret }` to `Auth`, and add
  `bearer_and_header_secret` to the manifest vocabulary; the OpenAI-compatible package gains a family (for example
  `gemini-openai-compatible`) that presents it. P21 §7 has to go through the kernel chain anyway (`Usage` cache
  buckets); batch the two together (B7b).
- **B (interim)**: the OpenAI-compatible package declares for that family that "its header_secret is also mirrored
  as Bearer". The kernel is left alone, but the same descriptor then means different things under different
  families; interim only.

### 4.4 Conformance and versioning

A new gate ② check, `DescriptorAuthWithinManifest`: run the §4.2 admission on the descriptor produced by every
request fixture. T21 adds a `rogue-arm` mode (§12). The host-side admission (§4.2 item 3) is what binds third-party
packages; the gate ② check is early warning (R7). A new public function and one check: south minor. The four
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
  feeding the component (reference_bedrock_converse.rs:21-35, 549-557). The host re-encodes the **parsed** payload,
  `event: {event_type}\ndata: {json}\n\n` with `json` a `serde_json::Value` (server:…/south_component.rs:849-857),
  so the component sees compact re-serialized JSON, not the upstream payload bytes. This is an agreement between one
  reference implementation and the host, not a south ruling: the WIT chose byte chunks precisely so that eventstream
  could go into the component (provider-adapter.wit:20-25), the plan line that S0 D2 cites is exactly "eventstream
  decode goes in the component" (canonical-ir-inventory.md:223-233), and framing syntax was left to gate ② fixtures
  (same record, :277-278).
- So P21 §3.1, citing reference_bedrock_converse.rs:22 as south's position that "the framing layer belongs to the
  host", overstates its scope: that holds only for eventstream.
- Signing: the `host_signed` arm declares only `emits` (manifest.rs:239-245); the contract deliberately does not
  know the scheme (south-contracts/src/lib.rs:930-938). "This package needs SigV4" can only be inferred by the host
  from the provider type.

### 5.2 Proposal: `stream_framing`

Declared **per package** (R6: the parser receives no configuration, so it cannot be told which framing a family
used); absent = `bytes` (today's behavior for the three SSE dialects, R5):

| Value | What the host feeds to `parse-stream-chunk` | What the host must implement |
|---|---|---|
| `bytes` | Upstream bytes unchanged | Nothing |
| `aws-eventstream` | The canonical re-encoding of each message (below) | Call south's deframer |

**Canonical re-encoding** (`reencode_eventstream_v1`), one SSE frame per eventstream message. The compact payload
is the payload's own JSON text, validated as exactly one UTF-8 JSON value, with every insignificant whitespace byte
removed and nothing else changed — member order, number spelling and string escapes are kept. (Amended 2026-10-02:
the earlier wording said "parsed and re-serialized"; a re-serialization depends on the consumer's `serde_json`
features, such as `preserve_order`, so two hosts would not produce the same bytes.) No CR or LF can reach the SSE
line. A payload that is not UTF-8 JSON is a re-encoding error, never passed through; the deframer itself does not
read payloads, so a host calls both functions:

- `:message-type` `event`: `event: <:event-type>` / `data: <compact payload>`;
- `:message-type` `exception`: `event: exception:<:exception-type>` / `data: <compact payload>`;
- `:message-type` `error`: `event: error:<:error-code>` / `data: {"message":<:error-message as a JSON string>}`
  (these frames carry their detail in headers, not in the payload).

The `event` form equals what the host's seam produces today for Converse's JSON payloads (§5.1), so the existing
Converse fixtures keep their meaning, and south's golden vectors pin the exact bytes so both hosts produce the same
ones. The `exception` and
`error` forms are new: today the Converse stream parser ignores every unknown event, on the grounds that "the host's
own strict validator lives upstream of here" (reference_bedrock_converse.rs:736-739) — and that upstream validator
is precisely the dialect knowledge being moved out of the host. Instead, the host passes these frames through in
canonical form, the component maps them to `StreamEvent::Error` (kernel:stream.rs:99-101), and fixtures pin this.
The Converse package gains that mapping and an identity bump.

**Non-streaming responses in eventstream.** Some upstreams answer every request with an eventstream body, including
requests the client made without streaming (Kiro has no stream switch; its record, P-5). The **buffered path** is
triggered by declarations only, and only by both together: the package declares `stream_framing: aws-eventstream`
**and** the family declares `request_facts.stream: "none"` (§7.2). For a non-streaming client request to such a
family the host buffers the whole 2xx body, deframes it, and hands `parse-response` the concatenated canonical
re-encoding; the host never chooses this path by sniffing a content type. `HttpResponseParts.body` stays text, since
the re-encoding is UTF-8; the kernel's comment that a binary body "would need a `-v2` field" (kernel:http.rs:379-382)
still holds but should say that eventstream reaches the component re-encoded (§14). Codex is different on purpose:
its upstream has a stream switch and its stream is SSE under `bytes` framing, and lv ruled that non-streaming Codex
callers are refused at build time (Responses R-Q5); a package whose upstream has a switch does not declare
`stream: "none"`.

The `sse` / `ndjson` / `json` values in DP4's recommendation are not added: their splitting already lives in the
components and the host has nothing to do; putting them in the set would only give the host a branch that "picks,
by declaration, a decoder it does not actually use". In the provider world the host never picks an SSE decoder to
parse a provider's stream for the component. If the host needs to know the upstream format for diagnostics, a
separate read-only informational field is enough.

**South provides the deframer** (`deframe_aws_eventstream_v1`: prelude, two CRC32s, frame-length bound) and the
re-encoding, as pure, bounded functions with golden vectors in **`south-contracts`** — the crate where south's
parsing grammars live under a fuzz obligation (fuzz/fuzz_targets/contract_parsers.rs); raw.rs:11-12 states that the
host prelude in `south-core` introduces no grammar. That way "each form is implemented once" holds for both hosts at
the same time, and the host's roughly 235 lines of deframing code (P21 §2.5) retire. A component may still declare
`bytes` and split eventstream itself — the WIT allows it and DP4 does not forbid it; what is forbidden is the host
choosing a decoder by provider identity. Kiro is also eventstream (P21 §2.5) and is covered by the same declaration.

**Its SSE sibling, `decode_sse_v1`**, also lives in `south-contracts`, with golden vectors and a fuzz target, and is
released with the image world's minor. It has two uses outside the provider-world rule above: the media worlds,
whose `response_body_form` declares an SSE body and where the host builds the component's view (speech record), and
`north_passthrough`, where the host splits the northbound frames only to find the terminal frame (Responses
record). Both hosts call the same function, so the split is identical.

### 5.3 Proposal: `signing` for `host_signed`

```json
"auth_arms": ["host_signed"],
"emits": ["authorization", "x-amz-date", "x-amz-content-sha256", "x-amz-security-token"],
"signing": { "scheme": "aws-sigv4", "service": "bedrock",
             "region": { "template_param": "region" },
             "credentials": { "access_key_id": "access_key_id", "secret_access_key": "secret_access_key",
                              "session_token": "session_token" } }
```

`signing` is package-level, like `auth_arms` and `emits` (R6). `scheme` is a closed set (today only `aws-sigv4`),
`service` is component data, and `region` names the endpoint-template parameter of §7.3, so the region the host signs
for is the region in the origin it sends to — they cannot disagree. `credentials` maps each input the scheme needs
to a field declared through §3.3's `fields`; `aws-sigv4` requires `access_key_id` and `secret_access_key` and
admits `session_token`. The host picks the finalizer by `scheme` — DP9: SigV4 is handled as a public-standard
executor, kept in the host and selected by declaration — and `aws_sigv4` among the host's credential kinds (P21
§2.6) becomes a generic field set. The contract-level `SignedHeaderSetV1` stays scheme-agnostic and does not change.

### 5.4 Conformance and versioning

- Deframer and re-encoding: property tests, a fuzz target (CONTRIBUTING's requirement for untrusted parsers) and
  golden vectors; fixtures include frames split across chunks, CRC errors, oversized frames, non-JSON payloads,
  exception frames and error frames.
- gate ②: stream fixtures for `aws-eventstream` packages are written in the canonical re-encoding (consistent with
  today's Converse fixtures); gate ① validates that `signing` and `emits` are compatible (`aws-sigv4` requires at
  least `authorization`, `x-amz-date` and `x-amz-content-sha256`) and that every `signing.credentials` entry names a
  declared field. (The field check landed with host feedback SF13, §13.6: each entry names a declared **secret**
  field of the section that applies to every signed family, and an input the scheme requires names a required
  field.)
- New manifest fields: south minor. `provider-bedrock-converse` needs `stream_framing`, `signing` and the
  exception / error mapping added, with an identity bump. The deframer is a new link-layer API, recorded under
  `host_capabilities` once the host adopts it.

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
     contradicting "subset". A measurement on 2026-10-01 settles it (§16 Q13): thoughts are not included in
     candidates, and `totalTokenCount = promptTokenCount + candidatesTokenCount + thoughtsTokenCount`.
- P21 §3.2 S5 says the conformance tests contain "not a single usage sample", which is inaccurate: there are some,
  and there is a documentation-derived judge; what is missing is **enforcement** and **strictness**.

### 6.2 Proposal

1. **Make the reference implementations strict**, with the same convention as Converse: a non-streaming 2xx that
   lacks the usage object or a count this dialect requires → `provider_protocol_error`; a verifiable relationship
   given by the upstream (e.g. OpenAI's `total_tokens == prompt_tokens + completion_tokens`) that does not hold →
   protocol error. For Gemini the relation is `total = prompt (+ tool-use prompt) + candidates + thoughts` (Q13,
   measured); checking `prompt + candidates` alone would refuse every real thinking response.
2. **gate ② requires usage rows by name** (provider world): `response.usage`, `response.missing-usage` (expects a
   protocol error), `response.cached-usage` (partition convention), `stream.usage-terminal`, `stream.no-usage`.
   Packages declaring `usage_evidence: absent` (item 4) instead require the rows that show they never produce usage
   (`AbsentFamilyEmitsNoUsage`, item 4).
3. **An automatic mutation check, `UsageNeverDefaulted`**: response fixtures carry `usage_pointer` metadata (where
   the usage object sits in the upstream body); the suite deletes that object and calls again, requiring the
   component to report an error rather than produce zeros — the same technique as `unknown_field_tolerance`
   (suite.rs:387-420). The three lenient reference implementations would turn red on this today, which is exactly
   what it is meant to prove. The pointer is written by the fixture author, so for a third-party package the check
   proves only what the author chose to point at (R7).
4. **A manifest `usage_evidence`**, package-level (R6): `reported` (default) | `absent`. `absent` means the
   upstream never reports tokens — e.g. Kiro, which lv ruled on 2026-09-22 is billed by estimate and labeled
   truthfully (server:crates/gateway-provider-protocol/src/usage_types.rs:131-147). For an `absent` package:
   `parse-response` returns `usage` all zero (the IR field is not optional), the component never emits
   `StreamEvent::Usage`, and the host never reads either. The host uses a provider-agnostic estimator and writes
   `tokens_estimated = 1`, `quantity_estimated = 1`, no longer judging by family name (Q12 ruling). Gate ② checks
   this as `AbsentFamilyEmitsNoUsage`. A package whose families differ on this is two packages. The all-zero usage
   contradicts the WIT's "a 2xx whose body cannot yield exact usage is an error, never a zero"
   (provider-adapter.wit:110-116), which must be amended for `absent` packages (§14).
5. **Usage judges become part of south's release discipline**: every provider package south publishes must have a
   `usage_ir_contract`-style judge with expectations **not derived from the reference implementation** — from
   provider documentation where it exists, otherwise from captured upstream traffic archived with the fixtures
   (Kiro has no public documentation; its record §11.2), and for `absent` packages the property of item 4. It does
   not go into gate ② — a third-party package author cannot vouch for documentation semantics on south's behalf —
   it is south's commitment for its own released packages.
6. **Freeze the reasoning convention**: following how v0.40.0 handled the cache buckets, write "`reasoning_tokens`
   ⊂ `output_tokens`" into the IR usage contract and add reasoning rows to the judges, and change the Gemini
   reference implementation to `output_tokens = candidatesTokenCount + thoughtsTokenCount` (with `reasoning_tokens =
   thoughtsTokenCount`), since the measurement shows thoughts are not within candidates (§16 Q13). The host's
   settlement already counts output this way (server:crates/gateway-provider-protocol/src/usage_evidence.rs,
   `parse_gemini_usage`: output = candidates + thoughts, checked against the total), so the amounts the host charges
   do not change. What changes is the usage a component reports, which is what clients see today (the host's gap
   ledger #65) and what the host settles on once usage moves to components; left unchanged, thinking tokens would go
   unbilled after that move. A dual run confirms the component's IR usage equals the host's evidence.

### 6.3 Bounds the host checks, the funds outcome, and the undetectable zone

**Bounds come from the host.** Every bound the host checks is computed by the host from the **northbound
request**, with no provider knowledge. The shared rule for all worlds (image, speech and embeddings reference this
section instead of defining their own):

- Input bound (chat instance): the northbound request's bytes plus a fixed, host-configured allowance per media part
  (an image or document referenced by URL is a few bytes but many tokens, so bytes alone would misfire). The
  allowance is host configuration, the same for every provider.
- Output bound: the authorized output cap × number of choices.
- **Reservation** uses `min(host bound, component bound)`: a component may supply a bound only to tighten what is
  reserved.
- **Every check compares against the host bound only.** A component-supplied number never checks the component that
  supplied it.
- Each world may state its own **instance** of the host rule, provided it stays provider-agnostic and says whether
  media bytes count (for example, embeddings counts no media bytes and adds a per-input allowance). Image, speech and
  embeddings state their instances in their own records.

**Checks.** Independent of dialect:

- **Out-of-bound checks**: `output_tokens ≤ output bound`; `reasoning_tokens ≤ output_tokens` (must wait for §6.2
  item 6 to land, otherwise today's Gemini mapping raises false positives); `input_tokens ≤ input bound`; settled
  amount ≤ reservation (exists today).
- **Internal-consistency checks**: `cache_read_tokens + cache_write_tokens ≤ input_tokens` (the kernel partition
  contract, kernel:usage.rs:52-61); `cache_write_5m_tokens + cache_write_1h_tokens ≤ cache_write_tokens`; exactly
  one streaming terminal state and no `Usage` after `Done`; a `reported` package with no `Usage` at all cannot be
  settled.

**Funds outcome of a hit.** A hit never settles as zero and never releases the reservation. The call is recorded
with the reported usage and a review flag; the reservation stays held while the flag is open; review resolves it to
the reported amount or to the bound. This is the existing generic manual-review path that lv's ruling on Responses
R-Q5 / Kiro K-Q1 already sends "settlement above the reservation" to; this record extends it to every bound hit and
does not change that ruling. The input allowance is expected to misfire on inputs the northbound request does not
show — server-side tool results, grounding, prompts the upstream injects — so the host should size the allowance
and watch the flag rate before cutover rather than treat each hit as fraud.

**Families that cannot send the cap.** A family that declares `output_cap: []` (§7.2) gives the upstream no cap.
Per the ruling above: with `usage_evidence: absent` the host enforces the authorized cap on its own output meter and
ends the answer with `length` (after cutover; today's behavior during the dual run); with reported usage the host
does not cut, and a settlement above the reservation goes to manual review.

**The undetectable zone, written into acceptance.** The host cannot detect:

- under-reporting, and over-reporting or deviation that falls within bounds — the only thing the host can compare
  against is an upper bound it computes itself, and any number below it is equally credible;
- a cap written where the upstream does not read it, while a larger value sits where it does (§7.2): the seal
  checks consistency with the declaration, not what the upstream honors;
- for URL-form model placement, anything beyond the path template check of §7.2.

Trust comes from only these places: pinned package digests (§9), gate ② usage samples and usage judges (§6.2 items
2, 3 and 5) **for first-party packages** (R7), and dual-run reconciliation against the native arm before cutover
(host P21 S5). If the "lower-bound signal" that P21 S5 envisions is to be established, it is host policy; south
neither provides nor blocks it.

Unchanged: `Usage::absorb`'s folding semantics (kernel:usage.rs:63-85); S0 D3, "do not add provenance to
`StreamEvent`" (canonical-ir-inventory.md:235-243); the derivation rules for thinking markers (same record,
:215-221).

### 6.4 Settlement outcomes shared by every world

In every world that has a `rejected` outcome — an upstream answer that proves nothing was produced (a 4xx, or a 2xx
the component shows produced nothing, such as the image world's all-filtered result, image §9.2) — **`rejected`
releases the reservation**. Which answers qualify is classified by the component per dialect, not by the host. This
is lv's ruling on image Q7, stated once here; it applies after the dual run, and until then each path keeps its
current behavior. Embeddings adopts a `rejected` outcome for an upstream 4xx that proves nothing was produced.
The image, speech and embeddings records point here rather than restating it.

### 6.5 Versioning

Making the reference implementations strict is a behavior change: for a host that links the reference
implementations directly (the community host), responses previously treated as zero-usage successes become
protocol errors, which needs the community host's confirmation (§16 Q9). Three package identities bump. The new
gate ② checks and by-name enforcement: south minor; third-party packages need to add fixtures, and they are bound by
it only through the runtime floor of §8.6. Normalization of quota headers (`ProviderQuotaMetadataFieldV1`
enumerates header names per provider, south-contracts/src/lib.rs:1686-1706) is covered in §10.

### 6.6 Implementation of B1 (2026-10-02)

Phase B1 is implemented on branch `feature/b1-usage-strictness`; nothing is released. Where this section left a
choice open, the implementation chose as follows.

- **Strictness follows the production host's evidence**, so the dual run of item 6 compares like with like
  (server `crates/gateway-provider-protocol/src/usage_evidence.rs`).
  - OpenAI-compatible: `prompt_tokens`, `completion_tokens` and `total_tokens` are required, and the total must
    equal prompt + completion. Sakana's `*_tokens_details.orchestration_*` counts fold into the buckets; the
    total may then also include the orchestration input. Bailian's
    `prompt_tokens_details.cache_creation_input_tokens` is read as the cache-write subset. Cached + cache-write
    must fit in the prompt, and reasoning in the completion.
  - Anthropic: a message requires `input_tokens` and `output_tokens`. A stream's `message_start` requires input,
    and its terminal `message_delta` requires output. The `cache_creation` 5-minute / 1-hour tiers now map to the
    IR and must add up to `cache_creation_input_tokens`.
  - Gemini: `promptTokenCount` and `totalTokenCount` are required. IR input is prompt + `toolUsePromptTokenCount`;
    IR output is candidates + thoughts, with `reasoning_tokens` = thoughts (item 6). The wire omits zero counts,
    so a missing `candidatesTokenCount` is zero only when the total closes without it.
  - Converse was already strict and is unchanged.
- **Fixture form.** A response case may expect a refusal: its expected file is then exactly
  `{"error": <ErrorEnvelope>}`. `usage_pointer` lives in an optional sidecar,
  `provider.response.<case>.meta.json`, holding exactly `{"usage_pointer": "/…"}`. Any other key, an empty
  pointer, or a sidecar on a non-response case makes the pack fail to load, so a misspelt key cannot silently
  disable the check.
- **Gate ② checks.**
  - `Coverage` names the five rows of item 2 for a `reported` package.
  - `UsageRows` checks that each row shows what its name says. `response.usage` must carry the pointer.
  - `UsageNeverDefaulted` runs on every response case that has a pointer.
  - An `absent` package owes no named rows. Instead, `AbsentFamilyEmitsNoUsage` runs on every response and
    stream case.
  - One check is added beyond item 3: `UsagePartition`, the gate ② half of item 6. On every report it requires
    cache read + write ≤ input, cache-write tiers ≤ cache write, and reasoning ≤ output.
  - A host passes the manifest's value through `run_provider_component_suite_v1_with_usage_evidence`. The
    existing entry point means `reported`.
- **Manifest.** `usage_evidence` is the package-level scalar of item 4. It is omitted from the wire when it has
  the default `reported`, so every existing manifest is unchanged. Declaring `absent` outside the provider world
  is refused (`UsageEvidenceIsAProviderWorldDeclaration`). The WIT `parse-response` comment now states the
  `absent` exception (§14). Doc comments are not compiled into the components, so a component's bytes do not
  change.
- **Judges.** The usage judge gains cases for output, reasoning and refusal. Their expectations come from
  provider documentation and the Q13 measurement, and each was shown to fail on the previous references.
  `CONTRIBUTING.md` states the release discipline of item 5.
- **Identities.** The following package identities bump: `provider-openai-compatible` 2.1.4 → 2.1.5,
  `provider-anthropic` 1.0.8 → 1.0.9, and `provider-gemini` 1.1.4 → 1.1.5. The runtime version and
  `compatibility.json` are left to the release.
- **Known differences from the host's evidence**, for the dual run to account for:
  1. Bailian's explicit-cache reads are priced apart from implicit hits by the host. The IR has a single read
     bucket and cannot express the difference.
  2. A thinking response from DashScope that reports no `reasoning_tokens` is recognised by the host through
     `reasoning_content`. The IR carries only the count.
  3. The host refuses non-zero Chat audio tokens as a cost-admission rule. This is host policy and stays there.
  4. Stream terminal requirements stay host-side internal-consistency checks (§6.3): a finish reason, `[DONE]`,
     `message_stop`, and exactly one terminal.
- **Q9** is ruled (lv, 2026-10-02): the community host confirms the behavior change of §6.5, so nothing in §6
  remains open before release.

## 7. Request facts, endpoints, non-secret configuration and capability metadata (problem e)

### 7.1 Today

- In its sealing phase the host recognizes only a few closed locations: the cap in top-level `max_tokens` /
  `max_completion_tokens`, `model` at top level, `stream` at top level; T03's documentation calls these "Three
  fields that are not free" (t03-canary-provider/src/lib.rs:16-37). Real dialects do not look like this: Gemini's
  cap is at `generationConfig.maxOutputTokens` (reference_gemini.rs:224), Converse's at `inferenceConfig.maxTokens`
  (reference_bedrock_converse.rs:343), and both put model and stream in the URL. So the host wrote a location table
  keyed by `provider_type`, which also refuses cap fields at unexpected locations
  (server:gateway/src/modules/inference/engine/text_admission.rs:1030 onward). The manifest has no place to declare
  any of this.
- URLs: the host takes only the **body** from the component; URL and auth stay host-owned
  (server:…/south_component.rs:777). For AWS and Vertex rows the component receives a placeholder endpoint
  (`HOST_OWNS_THE_URL`, :298, :731) and the host builds the region- or project-bearing URL itself. A component
  cannot put a region into the origin: `ProviderEndpoint::permits` requires the descriptor's origin to equal the
  configured one (kernel:provider.rs:65-84). The Gemini and Converse references append only a path to `base_url`
  (reference_gemini.rs:483-494, reference_bedrock_converse.rs:825-834).
- Non-secret configuration: the kernel's `ProviderConfig` has only `provider` / `base_url` / `auth` / `models` and
  the flattened `extensions` (kernel:provider.rs:318-336), and S0 fences the component to the first four
  (canonical-ir-inventory.md:155-164; provider-adapter.wit:89-90). Vertex's project, Bedrock's region and Azure's
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
  "gemini": { "output_cap": ["/generationConfig/maxOutputTokens"],
              "model": { "url": "/models/{model}:" }, "stream": "url" },
  "kiro":   { "output_cap": [], "model": { "body": "/modelId" }, "stream": "none" }
}
```

- `output_cap`: JSON Pointers into the descriptor body where the component may write the cap (at most 4), or `[]`
  for a wire that has no cap field (Kiro; the Codex family unless measurement shows otherwise — Responses R-Q5).
  The host always writes the authorized cap into the IR's `sampling.max_output_tokens`. When the list is non-empty,
  the generic seal check requires **exactly one** declared location to hold a value equal to that cap and the
  others to be absent. When it is empty, no location may be checked and §6.3's rule for families that cannot send
  the cap applies. This replaces the per-type location table and removes T03's constraint 2.
- `model`: `{"body": pointer}` — the host checks the value; or `{"url": template}` — a path template containing
  exactly one `{model}` placeholder, which the host checks against the descriptor URL's path: the authorized model,
  percent-encoded as one segment, must sit exactly where the placeholder is. Endpoint confinement alone checks only
  the origin and a path prefix, which would let a component send the request to a different model than the one
  reserved for.
- `stream`: `{"body": pointer}` (the host checks the value), `"url"` (the host checks only that the response's
  content type matches the IR's stream flag), or `"none"` — the upstream always streams and has no switch; the
  host takes the buffered path of §5.2 for non-streaming callers.
- Absent = today's three top-level fields (R5).

**Limits of the seal.** These are the component's own declarations. The seal proves the descriptor is consistent
with them; it cannot prove the upstream reads the cap where the component wrote it. That gap is in §6.3's
undetectable zone; T21's `rogue-cap-twice` mode (§12) documents it rather than claiming the host catches it.

### 7.3 Proposal: `endpoint` and `config_schema` (per family)

- **`endpoint`**: an origin (and optional path) template, e.g.
  `"https://bedrock-runtime.{region}.amazonaws.com"` or
  `"https://{region}-aiplatform.googleapis.com/v1/projects/{project}/locations/{region}"`, whose parameters are
  `config_schema` keys. The host fills it from validated values and passes the result as `ProviderConfig.base_url`;
  confinement then applies to the filled origin, and the component only appends paths as it does today. The operator
  may still enter a full `base_url` instead; the host then checks it against the template. The template is the
  per-family default the host seeds today by type (`form_preset`). Host part and parameters follow the same rules
  as recipe endpoint templates (§3.3).
- **`config_schema`**: the non-secret keys of this family: name, value syntax (from the closed set of §3.3), whether
  required, and a one-line description. Kiro's profile ARN is not a config key: it belongs to the credential, so it
  is a non-secret credential field with syntax `aws_arn` (which admits the `:` and `/` an ARN needs), exported as an
  attribute (§3.3). The host renders the
  operator form from this and validates the values, replacing `form_preset` and the per-type "required extra
  fields".

Keys consumed by the endpoint template never reach the component. Keys the component itself needs (for example Azure's
api-version as a query value) need a channel into the component, and the current fence admits none (§7.1); Kiro's
profile ARN is a credential attribute (§3.3), not a config key, and needs the same channel. That channel is §16 Q14,
the same question as §3.3's attributes; the host strips any client-supplied key that collides with a reserved name.

### 7.4 Per-model differences: dialect words; different response wires: separate packages

Model-level differences **in the request** — "is the cap field called `max_tokens` or `max_completion_tokens`",
"supports countTokens" — are expressed as `supported_parameters` dialect words and interpreted by the component —
the same mechanism as the Claude dialect words. A new word is a package-layer change; the host only passes the
words declared on the model row to the component unchanged. A difference in the **response** wire cannot be a
dialect word, because the response-side functions receive no configuration (R6). "Responses only" is therefore
expressed by the provider row belonging to a family of a separate package (the Responses upstream package, §11),
not by a word.

### 7.5 Model catalog

The roughly 691 lines of built-in capability profiles in `capabilities.rs` (P21 Appendix B.2) are **data**, not
translation logic. Three possible homes:

- **A Built into the component**: `model-capabilities` returns the merge of a built-in catalog and the operator's
  declarations. Every new model means a package release, and the package digest changes with the data.
- **B Catalog data published by south (recommended; ruled by lv for the host side, §16 Q7)**: JSON in
  `south.model-catalog.v1` format, published per family and listed in the §9 index; the host loads it as data, with
  operator rows overriding it; `model-capabilities` stays as a hook for "supplementing from the upstream". For
  families whose upstream cannot send a cap (§7.2), the catalog's maximum output is what the host reserves against.
- **C Pure operator data**: south defines only the vocabulary (capability fields, dialect words), and each host's
  operators maintain the catalog.

B lets the two hosts share the catalog but turns "keeping up with providers' new models" into a south maintenance
burden; C costs south the least, but each host maintains its own copy, which contradicts DP0's rationale (sharing
southbound work). Prices do not enter south (ARCHITECTURE.md:108-112).

### 7.6 Conformance and versioning

A new gate ② check, `RequestFactsHonoured`: every request fixture asserts that the IR cap appears in exactly one
declared location, that body-form model and stream agree with the IR, and that URL-form model matches the template.
For a family that declares `output_cap: []` it is a **mutation check**: the suite changes the IR's
`sampling.max_output_tokens` and builds again, and the built body must be byte-identical — a body that changes with
the cap has put it somewhere undeclared. The Kiro and Responses records point here for this definition. Fixtures
missing a required `config_schema` key expect a capability error. The host's seal (§7.2) is what binds third-party
packages; the gate ② check is early warning (R7). New manifest fields: south minor; the catalog format is a new
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
- The kernel makes no compatibility promise below 1.0: "The API surface … may change between minor versions … nothing
  here is a stability promise until 1.0" (kernel README.md:10-13). It does publish contract numbers in its
  `compatibility.json` (`canonical_ir` 2, `stream` 2, `error_catalog` 1 at `f585bc83`).

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
| IR (`ir_schema_id`) | Exact | The component declares the kernel's contract numbers it was built against (`canonical_ir`, `stream`, `error_catalog`); each must **equal** the host's. `ir_schema_id` is kept for provenance |
| `kernel_version` / `kernel_revision` | Exact | Recorded for provenance only; not part of the decision |
| `south_runtime` | Exact | `host.minimum ≤ component declaration ≤ host.runtime` |
| New `runtime_abi` (integer epoch) | — | Equal; incremented only on incompatible changes to loader, sandbox or WIT semantics; recorded in `compatibility.json` |
| New `contracts` (e.g. `{"task": 7}`) | Implicit | Declared by the component; the host accepts a set; south's codecs can decode every version in the set |

Why equality on the kernel numbers rather than a range: the kernel promises nothing within a minor line, and a
newer host sending a newer IR to an older component is not refused — gate ② requires the component to ignore
fields it does not model (`unknown_field_tolerance`, suite.rs:381-414) — so a range would turn "a field the caller
relies on" into a silently dropped field, which the tuple exists to prevent (manifest.rs:197-203). The kernel's
contract numbers change far less often than crate versions, so equality on them still removes the per-release
re-stamp. A range on the IR becomes possible only if the kernel publishes, per contract increment, what was added,
and the host refuses a request that uses an addition newer than the component's number; that is §16 Q5.

`runtime_abi` is the operational stand-in for DP5's "runtime major version" under 0.x; when south reaches 1.0 it can
merge with the major version.

### 8.4 API

Add `compatibility_admits(manifest, &HostRangeV1) -> Result<(), CompatibilityMismatchV2>`, with
`HostRangeV1 { runtime_abi, south_runtime_min, south_runtime, kernel_contracts, contracts }`; the loader switches to
taking `HostRangeV1`. `compatibility_matches` is kept for one version and marked deprecated.

### 8.5 Per-package isolation

Add `load_package_set(runtime, root, &HostRangeV1) -> PackageSetReportV1`: walk the directory (or follow the §9
index); each package independently goes through gate ①, the range handshake, the import scan and the identity
probe; return `admitted` and `refused { package, reason }` sorted by package name, never failing as a whole because
one package failed. Accompanying rules, written into ARCHITECTURE:

- A refused package makes only the families it declares unavailable; startup continues, and the readiness probe
  reports truthfully.
- **No fallback to native reference implementations** (J3①): the reference implementations are gate ②'s judges
  and an optional native engine for the community host, not stand-ins for missing or refused packages. This is the
  recommendation of §16 Q8, not yet its ruling.
- If two admitted packages in the same world declare the same family, both are unavailable and the operator is
  required to pin one by digest (the task side already selects packages by pin today); **ties are never broken by
  load order**.

### 8.6 Changes to release discipline

- No more re-stamping every package on every release; if a package's content is unchanged, its identity and digest
  are unchanged. This needs a mechanism, because release.yml rebuilds every package from source on every tag
  (release.yml:57-103) and a component's wasm also contains shared crates (the conformance references, the kernel
  types): release CI builds each package, compares the `component.wasm` digest with the previous release's for any
  package whose version did not change, and fails on a difference (the version must bump) — or, equivalently,
  carries the previous release's bytes forward. A reproducible build is therefore a release requirement, checked,
  not assumed.
- Contract changes must be additive (new keys have defaults, old shapes still decode), or the codecs must also
  accept the older versions in the declared set. "Reject when the new key is missing; it will be re-stamped anyway"
  (2026-09-27-task-contract-v6-facts.md:22-23) no longer holds.
- **A package declares the oldest runtime it needs** (amended 2026-10-05, §13.6, §16 Q23): its `south_runtime` moves
  only when the package starts relying on something a release introduced, and release CI loads each package under
  exactly the runtime it declares. A package whose `component.wasm` and `manifest.json` are unchanged keeps its
  version, so `south_runtime` is never re-stamped without a version bump.
- **A tightened gate ② binds only through the floor.** Each release that tightens gate ① or ② (B1's usage rows,
  `DescriptorAuthWithinManifest`, `RequestFactsHonoured`) states so, and a host that wants the tightening enforced
  raises `south_runtime_min` to that release; packages declaring an older runtime are then refused (R7).
- Incrementing `runtime_abi` is a breaking event and requires a design record.

### 8.7 Conformance and versioning

`crates/south-contracts/tests/compatibility_manifest.rs` and `crates/south-provider-api/tests/provider_api_v2.rs`
gain range cases (upper and lower bounds, a different epoch, a different kernel contract number, a contract not in
the set); T21's skew package (§12) gets only itself refused. South minor; the manifest's `compatibility` gains
`runtime_abi`, `kernel_contracts` and `contracts`.

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
    "auth_arms": ["header_secret"], "stream_framing": "bytes", "usage_evidence": "reported",
    "credential_recipes": false,
    "compatibility": { "south_runtime": "0.43.0", "runtime_abi": 1,
                       "kernel_contracts": { "canonical_ir": 2, "stream": 2, "error_catalog": 1 },
                       "contracts": {} },
    "archive": "provider-gemini-v0.43.0.tar.gz",
    "archive_sha256": "…", "manifest_sha256": "…", "component_sha256": "…",
    "gate2_report_sha256": "…"
  }],
  "catalogs": [{ "family": "gemini", "file": "catalog-gemini-v0.43.0.json", "sha256": "…" }]
}
```

The host's fetch script and startup gate discover packages through the index and verify them by digest; operators
still pin packages by digest. `gate2_report_sha256` names the gate ② report south's CI produced for that exact
`component_sha256`, published beside the archives, so a host can show that a first-party package passed gate ② at
the release it came from (R7). The index's `providers` can also serve as a lower-bound source for the J1 vocabulary
(P21 S0 has already pointed out that family names are not vendor names and cannot replace the vendor list).

### 9.3 Trust and versioning

Like the archives, the index has only checksums and no signature (released-artifacts §6 lists signing as the next
slice). While only south's first-party packages are installed, the root of trust is the tag and the CI build. Two
things wait for signing (§16 Q11): loading **third-party packages** through the index, and enabling **credential
recipes** in any package that is not a verified first-party release (§3.4 rule 5). For third-party packages, who
runs gate ② and whether its result means anything beyond self-consistency is §16 Q17. This is a release-behavior
change (CONTRIBUTING requires a design record — this one) and does not touch contract numbers.

## 10. Provider instances in closed vocabularies

The link-layer problem of §1.3 is sharpest here. The following sets are compiled into the host, yet their values
belong to specific providers:

| Set | Location | Values |
|---|---|---|
| `SecretHeaderV1` | south-contracts/src/lib.rs:877-923 | `api-key` (Azure, Ideogram), `x-api-key` (Anthropic), `x-goog-api-key`, `xi-api-key` (ElevenLabs), `ocp-apim-subscription-key` (Azure Speech) |
| `QueryParameterV1` | south-contracts/src/lib.rs:1051-1095 | `api-version`, `alt`, `GroupId` / `task_id` / `file_id` (MiniMax) |
| `ProviderQuotaMetadataFieldV1` | south-contracts/src/lib.rs:1686-1706 | OpenAI-style and Anthropic-style rate-limit headers |
| `ControlledUserAgentV1` | south-contracts/src/lib.rs:1229-1246 | Any value, but only a `&'static str` that "must exist in host program text" — so every provider's value is a host literal |
| `CREDENTIAL_HEADERS` | kernel:lib.rs:86-96 | Corresponds to `SecretHeaderV1`, plus `authorization`, `cookie` and others; both construction and deserialization of `Auth::header` consult it (kernel:http.rs:182-188, 209-218) |

As soon as a new provider uses a secret header name, query name or user-agent value that is not in these sets,
south (and even the kernel) must release and the host must re-pin and rebuild — J2 is red. These sets were closed
for real security reasons: a secret header must also be on the reserved-header denylist so that it cannot be
smuggled through the ordinary header channel (lib.rs:877-883); the query is the part of a request most often logged
(lib.rs:1051-1062); the user-agent value was closed so that no path leads from configuration or request data to it
(lib.rs:1233-1243; 2026-08-20-controlled-user-agent.md). Per R1, separate the mechanism from the instances:

- **Secret headers**: the manifest declares `secret_headers` (name syntax restricted; no hop-by-hop headers, no
  `host`, no framing headers, no name already reserved for another purpose); for that package's requests the
  runtime merges these names into the reserved set (the ordinary header channel refuses them), and transcripts and
  logs always redact them. What is closed is the "secret header" mechanism and its safety rules, not the list.
- **Query parameters**: the manifest declares `query_parameters`, each choosing one entry from a closed set of
  **value syntaxes** (`digits`, `token`, `enum[…]`, `date`), with restricted parameter-name syntax. `ProviderAuthV1`
  remains the only channel through which secrets go on the wire; query values never come from credential
  resolution — that rule does not change.
- **Quota headers**: the component normalizes its dialect's rate-limit headers in `parse-response` /
  `map-provider-error` and hands them over (a south-local response extension); which headers the transport captures
  is declared by the package (P21 S5).
- **User-agent**: the manifest declares a `user_agent` value per family, and gate ① validates it against the
  existing value grammar of `ControlledUserAgentV1`. South adds a new owned type, `DeclaredUserAgentV1`, constructed
  only from a manifest value that passed gate ①, with a fuzz obligation on its parser; `ControlledUserAgentV1` keeps
  its `'static` constructor for host literals. The header name stays fixed and reserved, so "exactly one
  `user-agent` on the wire" still holds. This explicitly reopens the 2026-08-20 ruling that the value must come from
  host program text (§16 Q15), and it means south would publish impersonation values in its packages (§16 Q16). The
  Kiro record (P-3) and the Responses record (R-Q6) point here instead of carrying their own versions.
- **Kernel catalog**: `Auth::header` checks the name inside serde (`try_from` → `Auth::header` →
  `is_credential_header`, kernel:http.rs:182-188, 209-218) against a static list, so a caller-supplied set cannot be
  threaded through deserialization. Either `Auth`'s wire type changes so that the name is checked after
  deserialization against a set the caller supplies, or the kernel drops the check and the §4.2 admission (which has
  the manifest) performs it. Both go through the kernel chain, merged with §4.3 (B7b).

**Contract changes this implies** (south side, B7a):

| Contract / artefact | Change |
|---|---|
| auth (4 → 5) | `ProviderAuthV1::HeaderSecret(SecretHeaderV1)` is a closed enum; add a variant carrying a declared, validated header name (the closed variant stays) |
| reserved header policy (1 → 2) | The request-side reserved check (`RESERVED_HEADERS`, lib.rs:232; applied when the raw prelude parses ordinary headers, raw.rs:103-104) and its response-side transcript mirror (lib.rs:182) accept a per-package addition |
| controlled query | `QueryParameterV1` gains a declared-name form with a value syntax |
| quota metadata | Captured header names become a per-package declaration; the normalized field set stays closed |
| user-agent | New `DeclaredUserAgentV1` beside `ControlledUserAgentV1` |
| conformance | `south.header-auth.v1`, `south.controlled-query.v1` and `south.controlled-user-agent.v1` gain declared-instance cases, run by both hosts (R4) |
| manifest | `secret_headers`, `query_parameters`, `quota_headers`, per-family `user_agent` |

Without this step, the accurate statement of DP0 is: "If a new provider uses only existing instances, the host
needs zero changes; otherwise the host must bump its south pin." lv ruled on 2026-09-30 (§16 Q1) that this
statement is not acceptable: bumping the pin counts as modifying the host. This step is therefore a necessary
condition of DP0, not an improvement, and its south part comes before the follow-on components that need it (§13
B7a).

## 11. Follow-on components and non-chat operations (problem h)

| Work item | Scope | Depends on |
|---|---|---|
| OpenAI Responses upstream dialect component (Codex and others) | New provider package, family `openai-responses` (a separate package, because its response wire differs from Chat Completions, R6): IR → Responses request (instructions, input items, tools, reasoning effort); Responses response and `response.*` stream events → IR; strict usage (`input_tokens`, `input_tokens_details.cached_tokens`, `output_tokens`, `output_tokens_details.reasoning_tokens`). Terminal frames: `response.completed`, and `response.incomplete` with usage, which settles as a success only for a closed set of reasons (owner ruling, Responses R-Q2); any other reason is a protocol error. The request is always stateless: the IR and `south-north-codec` carry neither `store` nor `previous_response_id` nor `include`; a client's `previous_response_id` is refused on the host's northbound side, before translation (Responses record §4.4). Nothing on the wire is shared with `south-north-codec`, whose Responses module works on `serde_json::Value`; what can be shared is the event vocabulary, extension key names, fixtures and the round-trip judge. "Responses only" is the row's family, not a dialect word (§7.4); client-identification headers are declared per the Q10 ruling and §10 | §3 (Codex recipe), §6, §7, §10 (B7a) |
| Kiro component (DP6, migration recommended) | New provider package: conversationState request shape, `aws-eventstream` (§5.2) including non-streaming bodies, `request_facts` with `output_cap: []` and `stream: "none"` (§7.2), `usage_evidence: absent` (§6.2), social / IdC `http_exchange` recipes (§3.9), the profile ARN as a non-secret credential field with syntax `aws_arn` exported as an attribute (§3.3), a declared user-agent (§10). The host's three-hop translation and the Kiro part of the leaf crate retire with it (P21 §2.5) | §3, §5, §6, §7, §10 (B7a) |
| The Anthropic variant of Bedrock InvokeModel | A **separate package** (e.g. `provider-anthropic-bedrock-invoke`) sharing source with `provider-anthropic`: `host_signed` must stand alone (manifest.rs:385-387) while `provider-anthropic` uses `header_secret`, so it cannot be a family of that package (R6). The body carries `anthropic_version` and the model is in the URL; the stream is eventstream, and the `bytes` in the payload is base64-wrapped Anthropic event JSON; signing is `aws-sigv4` | §5 |
| Non-chat operations such as model listing | The kernel has `ProviderApi::Models` (kernel:provider.rs:137-143); the provider world has no corresponding function. A WIT world cannot have optional exports, so adding a function to v2 makes a new world; recommended is a separate small world (e.g. `provider-catalog-v1`: build a list request, parse a list response), used by the host for health probing and model discovery, replacing the per-type probe fallback table (P21 Appendix B.3) | §8 (multiple worlds coexisting) |
| Quota header normalization | §10 | B7a |
| IR explicit / implicit cache buckets | Kernel chain (P21 §7) | kernel (B7b) |
| Task estimate in "whole-order milliunits" | An additive change to the task contract (P21 S5) | §8.6 |

## 12. The synthetic unseen-provider guests (T21)

Location and form follow T03: `crates/south-provider-runtime/tests/guests/t21-unseen-provider/` (and siblings named
below), not published with releases; a host script outside the commit gate builds them from a local south checkout.
T03 is not extended directly: the host's T03 canary has already pinned its wire format and its "three fields that
are not free" as acceptance facts (t03-canary-provider/src/lib.rs:16-37), and changing it would change the meaning
of existing acceptance. Because response-side declarations are per package (R6), T21 is three packages. What they
must prove, each item mapping to a host red item:

1. `t21-unseen-provider`, family `t21-unseen-wire`, with a wire format unlike every known dialect (as with T03: a
   known parser handed it can only fail), `stream_framing: bytes` (reusing T03's non-SSE line format).
2. **Auth**: the manifest declares only `header_secret` with a secret header name outside the compiled-in set
   (§10), and the descriptor presents it; another model mode uses a `minted` slot — a single-step `jwt_sign`
   (HS256) recipe, with the fake upstream verifying the signature against a known key. → J2b①, §3, §4, §10.
3. The **cap** is written at the nested location `/t21/limits/max_out`, declared by `request_facts`; the model is
   in the URL under a declared `{model}` template. → J2b⑤, §7.2.
4. It needs one non-secret config key (declared by `config_schema`, syntax `token`) consumed by its `endpoint`
   template, and declares a user-agent value. → §7.3, §10.
5. `t21-unseen-eventstream` (implemented 2026-10-05 with the `host_signed` mode as well, §13.6 SF18):
   `stream_framing: aws-eventstream`, `stream: "none"`; the fake upstream sends real
   eventstream frames, including an `exception` and an `error` frame, and answers non-streaming requests with an
   eventstream body. → §5.2.
6. **Usage** has its own shape, including cache buckets; the fixtures carry every required usage row and
   `usage_pointer`. `t21-unseen-absent`: `usage_evidence: absent`, `output_cap: []`; it never emits usage and the
   host estimates and enforces the cap on its own meter. → §6.
7. A two-model **catalog** ships with the first package, one model carrying a new dialect word; the host's admission
   results change when the catalog changes. → §7.5, P21 S6 acceptance.
8. **Compatibility**: `south_runtime` takes an older value within the range and loads normally; a separate skew
   package with a different `runtime_abi` is also prepared, and only it is refused, with other packages and the
   known dialects unaffected. → J2b③, §8.
9. They appear in a test **release index**, and the fetch script needs no list. → J2b④, §9.
10. **Rogue modes** (triggered by model name, as T03 does): T03's four, plus `rogue-arm` (the descriptor's auth arm
    is not in the manifest), `rogue-cap` (the cap only at an undeclared location), `rogue-model-url` (the URL names
    a different model than the template allows) and `rogue-zero-usage` (a 2xx without usage that still produces
    zeros). The host must refuse T03's four, `rogue-arm`, `rogue-cap` and `rogue-model-url` with zero upstream
    calls; `rogue-zero-usage` must turn red at gate ②. Plus `rogue-cap-twice` (the authorized cap at the declared
    location and a larger one elsewhere), which the host is **expected to pass** — it documents the undetectable
    zone (§6.3). Gate ① counterexamples: a recipe whose endpoint is not on the confirmed list, an assertion whose
    `aud` is not its destination, a constant `sub`, and an attribute exported from a secret field (§3.4).

The J3 counterpart: delete the T21 packages but keep their catalog rows → startup reports the families
unavailable, requests fail fast as "not served", and no built-in implementation takes over (§8.5).

**What T21 cannot prove**: that a component reports usage faithfully according to the upstream documentation, or
that the upstream reads the cap where the component wrote it (the undetectable zone of §6.3), or anything outside
the link layer.

## 13. Phasing

| Phase | South delivers | Unlocks in the host | Red items flipped | Release |
|---|---|---|---|---|
| B0 | This record ruled on by the south maintainers; existing text revised per §14 | — | — | Docs |
| B1 | Usage strictness: the three reference implementations, gate ② by-name enforcement, `UsageNeverDefaulted`, `AbsentFamilyEmitsNoUsage`, `usage_evidence`, usage judges in the release discipline, the reasoning convention and Gemini total relation (Q13 measured 2026-10-01) | P21 S5 (and T-7) | Prerequisite for S5 acceptance | minor; three package identities bump |
| B2 | Descriptor auth admission (static slots) and `DescriptorAuthWithinManifest`; `request_facts`, `endpoint`, `config_schema`, `stream_framing`, `signing`; the eventstream deframer and re-encoding in `south-contracts` | P21 S1, S4 (host passes the slot in `ProviderConfig.auth`, §4.2) | J2b① (static slots), ⑤ (J2b② is purely host-side: descriptors carry the URL today, and gate ②'s `EndpointConfinement` already guards it) | minor |
| B3 | `runtime_abi`, ranges and kernel contract numbers, `load_package_set`, release index with gate ② report digests, digest-stability check in release CI | P21 S2 | J2b③④, J3③ | minor + release behavior |
| B4 | Credential recipe v1: manifest section, trust rules, reference interpreter, gate ② fixtures, gate ③ host suite; the `Auth::OAuth` admission rule | P21 S3; P22 Vertex, P23 Vertex TTS | J2b① (minting part, first-party packages) | minor |
| B7a | §10 instance declarations on the south side: declared secret headers, query parameters, quota headers, `DeclaredUserAgentV1`; the contract changes of §10 | P21 S7 | New instances no longer touch the south link layer | minor |
| B5 | T21 guests, gaining modes phase by phase alongside B1–B4 and B7a | J2 standing pilot | All of J2b | Not published |
| B6 | Responses upstream, Kiro, InvokeModel-Anthropic, catalog data, catalog world | P21 S6, S7 | J1 keeps falling | minor each |
| B7b | Kernel chain: `Auth` combined arm, credential header catalog (§10), cache buckets. **Landed 2026-10-08 (§13.7).** | P21 S7 | New secret header names no longer touch the kernel | minor + kernel |

B1, B2 and B3 are independent of one another and can proceed in parallel; B4 depends on B2's descriptor auth
admission; B7a depends only on B0 and, after the Q1 and Q10 rulings, is required for DP0; B6 depends on B1–B4 and
on B7a because the Kiro component needs a declared user-agent (the Responses component needs no new instance: its
headers are ordinary descriptor headers). B7b comes last
only because it goes through the kernel chain; until it lands, a new secret header name still needs a kernel
release, and a provider needing one is outside DP0.

### 13.1 Implementation of B2 (2026-10-02)

Phase B2 is implemented on branch `feature/b2-descriptor-facts`, which is stacked on B1 (§6.6); nothing is
released. Q6 was ruled as recommended under the owner's standing rule that a recommendation consistent with DP0
is adopted: the deframer lives in `south-contracts`. Where this record left a choice open, the implementation
chose as follows.

- **Descriptor auth (§4).** `admit_descriptor_auth` returns `AdmittedAuthV1` (`None`, `Bearer`,
  `HeaderSecret(SecretHeaderV1)` or `HostSigned`) for the host to map onto its raw-call arm. It runs the kernel's
  `ProviderConfig::authorize` first. `DescriptorAuthWithinManifest` runs through a new entry point,
  `run_provider_component_suite_v1_for_manifest`, which also carries `usage_evidence`; it replaces B1's unreleased
  `run_provider_component_suite_v1_with_usage_evidence`. All four shipped provider packages pass unchanged, as
  §4.4 predicted.
- **Request facts (§7.2, §7.6).**
  - The cap mutation check runs for every family, not only those declaring no cap. A body rebuilt with another cap
    may differ only at the declared locations, so a cap also written somewhere undeclared is caught when it moves.
    A wrapper object the removal leaves empty (Gemini's `generationConfig`) counts as part of the location.
  - The references now encode the model as one URL segment, sharing the encoder with the check, so a Bedrock
    inference-profile ARN cannot split the path.
  - Gemini and Converse declare their facts; OpenAI-compatible and Anthropic use the top-level default.
- **Endpoint and config keys (§7.3).**
  - Gate ① refuses a template that a parameter could steer. The host part must end in a fixed domain of at least
    two labels, and a host parameter's syntax must fit in a DNS label. Ports, queries, fragments, userinfo and
    escapes are refused.
  - South provides `validate_config_values`, `fill_endpoint` (path parameters encoded as one segment) and
    `endpoint_admits` (an operator-entered `base_url` checked against the template).
  - Until Q14 gives the component a channel, a config key may only feed the endpoint. Gate ① refuses a key the
    template does not use, and `config_schema` without an endpoint.
- **Deframer and re-encoding (§5.2).** `AwsEventStreamDeframerV1` is pull-based (`push`, then `next_message`), and
  its first error is sticky. It adds a 128 KiB header-block bound, as the AWS SDKs have. It is stricter than
  today's host:
  1. A missing `:message-type` is an error rather than "event", and a missing `:exception-type` or `:error-code`
     is an error rather than a synthetic name.
  2. Duplicate headers are refused.
  3. Non-JSON payloads are reported by the re-encoding, not the deframer.
  Converse declares `stream_framing: aws-eventstream`. It maps `exception:` and `error:` frames to
  `StreamEvent::Error`, comparing an exception's first letter without case, and emits nothing after the stream
  closes.
- **Signing (§5.3).** `signing` is optional and allowed only with `host_signed`, so task-world `host_signed`
  packages are unaffected. Gate ① checks the scheme's emitted headers and its inputs, and that the region
  parameter appears in every family's endpoint. Checking credential field names waits for recipes (B4).
- **Identities.** `provider-bedrock-converse` 1.0.5 → 1.0.6. The manifest changes to `provider-anthropic` and
  `provider-gemini` ride their B1 bumps (1.0.9, 1.1.5). If B1 is released without B2, those two packages need
  another bump when B2 ships.
- **Not in B2:**
  - host adoption (P21 S1, S4);
  - `decode_sse_v1`, which ships with the image world's minor;
  - the component's channel for config keys (Q14);
  - the combined auth arm (Q4, B7b).
- **Found while fuzzing.** The fuzz work turned up an existing defect, unrelated to B2:
  `ProviderEndpointV1::parse` is not idempotent for some inputs. It is fixed separately.

### 13.2 Implementation of B3 (2026-10-02)

Phase B3 is implemented on branch `feature/b3-compat-range`, which is stacked on B2 (§13.1); nothing is released.
Q5 and Q8 were ruled as recommended under the owner's standing rule that a recommendation consistent with DP0 is
adopted: a `runtime_abi` epoch (south stays 0.x), exact equality on the kernel's contract numbers, and no fallback
to native reference implementations. Where §8 left a choice open:

- **Range handshake (§8.3, §8.4).**
  - `compatibility` gains `runtime_abi`, `kernel_contracts` and `contracts`, all optional on the wire, so manifests
    written before B3 still parse. Only the exact handshake can admit them: the range handshake refuses a package
    with no `runtime_abi`.
  - `RUNTIME_ABI` is 1, and `compatibility.json` (schema 5) records it together with the kernel contract numbers
    this release distributes.
  - Kernel contracts must match in both directions: a missing, different or extra name is refused.
  - `compatibility_matches` stays; it is marked superseded in its documentation but not `#[deprecated]`, so a
    host building with `-D warnings` is not broken by the upgrade.
- **One loader for both handshakes.** Every load path takes `&impl HostCompatibilityV1`, which both
  `HostExpectationsV1` and `HostRangeV1` implement, so a host's call sites do not change when it moves.
  `LoadErrorV1` gains `OutsideRange`.
- **Per-package isolation (§8.5).** `load_package_set(runtime, root, host, pins, signer)` returns admitted
  packages (with their `component.wasm` SHA-256 and the families they serve), refused packages with reasons, and
  contested families. A pin selects a claimant only when exactly one claimant has that digest; an ambiguous pin
  selects none. A package that loses every family it declares is refused with `EveryFamilyContested`.
- **Identities.** Declaring the range handshake changes every manifest. The nine task packages retire their 0.42.0
  identities: `task-kling` 1.0.5, `task-kling-v2` 0.32.2, `task-minimax-v2` / `task-bailian-v2` 0.31.2,
  `task-byteplus-v2` 0.36.2, and the other four 0.35.2. The four provider packages already moved in B1 and B2 and
  are unreleased. If B1, B2 and B3 are not released together, they need another bump.
- **South's own tests use the range.** Every test that admits a shipped package goes through a shared
  `HostRangeV1` built from `compatibility.json`; the tests that exercise the exact handshake take their true tuple
  from the manifest. The shipped-package rule is `south_runtime` ≤ the workspace version, not equality, so a
  package whose content is unchanged keeps its identity across releases instead of being re-stamped.
- **Release index (§9).** Release CI generates `south-release-index.json` (schema `south.release-index.v1`) with
  `scripts/release_index.py` from the archived manifests and wasm, never by hand, and lists it in `SHASUMS256.txt`.
  Beyond §9.2's example, each entry names its `gate2_report` file. Where a field does not apply, it is `null`:
  `stream_framing` and `usage_evidence` outside the provider world, and absent `compatibility` keys.
- **Gate ② reports (§9.2).** Release CI runs each package's sandbox parity test against the bytes it just built
  and writes a `south.gate2-report.v1` report: the suite, the identity, the manifest and `component.wasm` digests,
  and every outcome, sorted. The component is digested before and after the run. The index generator refuses a
  report that is missing, failing, or whose identity or digests differ from the archived package.
- **Digest stability (§8.6).** "Previous release" means the latest earlier non-draft `vX.Y.Z` release; a previous
  release without an index skips the check, with a log line. Otherwise a package that keeps its version but changes
  its `component.wasm` digest fails the release. Only the wasm is compared: manifests may change without a version
  bump, since `south_runtime` moves when a package is re-verified (withdrawn 2026-10-05, §13.6 and §16 Q25: the
  manifest is compared too). Reproducibility is proven only locally so far: a
  clean rebuild of `provider-gemini` was byte-identical. Builds on a different runner image would surface as a false
  "bump the version".

### 13.3 Implementation of B4 (2026-10-02)

Phase B4 is on branch `feature/b4-credential-recipes`, stacked on B3 (§13.2). Under the owner's standing rule,
three questions were taken as recommended:

- Q11: package signing comes before third-party recipes.
- Q18 (south half): the operator confirms every recipe endpoint per package digest.
- Q2: the ARCHITECTURE sentence is revised as §3.5 proposes.

**Gate ① (§3.7)** is implemented as the `credentials` section of the manifest (`CredentialsV1`).
- It checks the closed step kinds and algorithms, and caps a recipe at 4 steps.
- Rotation has no default. A recipe that rotates requires `write_back`, which must target a secret field.
- `present` must be reachable.
- `goto` may only name a later step, so the graph is acyclic by construction.
- Endpoints follow the family-endpoint template rules (§7.3). Their parameters come only from whole non-secret
  fields that have a syntax.
- It enforces the §3.4 rules: `aud` is bound to the receiving step, `sub` is never a constant, and attributes
  come only from non-secret fields.
- A clamp declared by a recipe must lie within 60 seconds to 24 hours.
- A selector may test whether a secret field is present, never its value.
- Its test proves the vocabulary expresses the Vertex, Codex, Copilot, Kiro and Kling recipes of §3.9.

**Three rules the record did not pin, decided here:**
- A recipe may not set the headers `authorization`, `cookie`, `host` or the framing headers. A credential is
  presented through the step's `auth` (scheme plus value).
- The signing key of `jwt_sign` must come from a secret field.
- A seed fills only a minted slot.

**Endpoints for confirmation.** `CredentialsV1::endpoints()` lists every endpoint a package's recipes can reach,
which is what an operator confirms.

**OAuth admission.** `admit_descriptor_auth` now admits `Auth::OAuth`, as Bearer, exactly when a recipe mints the
slot (§4.2).

**Gate ② (§3.7)** is `south_component_conformance::credential_recipe`, the reference interpreter, and the check
`CredentialRecipeMatch`.
- The interpreter runs only in tests. Time, the JWS signer and the responses to exchanges are injected; it has no
  network, clock or crypto of its own. It builds the JWS compact form itself and asks the signer only for signature
  bytes. Fixtures use `FixtureSignerV1`, an FNV-1a stand-in that is not a signature.
- Fixtures are `credential.<family>.<case>.{input,expected}.json` beside a package's other fixtures. The input names
  the slot, `now`, the fake field values and a fake response per step. The expected file is the whole run: the
  recipe after any selector, every rendered request, and the outcome.
- Both suites run the cases for a manifest that declares `credentials` (`run_task_component_suite_v2_for_manifest` is
  new). A package with recipes owes a `clock` sample that mints. It also owes a `rotation` sample when a recipe
  rotates, and an `on-status` sample that meets a non-2xx status when a recipe makes an exchange. A family is
  judged by what its cases do, not by their names.
- `task-kling-v2` 0.32.3 declares the Kling recipe. With the same signature bytes, its JWT equals the host's byte for
  byte: `jsonwebtoken` writes the header as `{"typ":"JWT","alg":"HS256"}`, and the claims `iss`, `exp`, `nbf` come out
  in that order because the interpreter writes RFC 7519's registered claims first, in RFC order.

**Rules the record did not pin, decided here:**
- A field with a `default` is always present.
- A step whose `requires` is unmet ends the run as `use_stored` when the recipe declares
  `without_refresh_material: use_stored`. Otherwise it is a configuration error.
- A step that some `goto` targets is entered only through that `goto`. Falling through into it ends the recipe, so a
  successful exchange does not run its alternative branch.
- The expiry of the presented value is the clock of the step that produced it, or else `now + default_seconds`. With
  neither, the run is `transient`. So a `jwt_sign` recipe declares `default_seconds`.
- A JSON `null` is absent. An absent or empty write-back output keeps the stored value.

**Two further gate ① rules, found while building gates ② and ③:**
- `refresh_margin_seconds` must be shorter than the shortest validity a minted value can have, which is the recipe's
  `min_ttl_seconds`, else the host's 60 s floor. Otherwise every freshly minted value is already due, and the host
  exchanges on every request.
- A presented JWT that the recipe signs itself declares `exp` as `now_plus`, and `default_seconds` no longer than
  that lifetime. Otherwise a host could cache a token past its own expiry.

**Gate ③ (§3.7)** is the host suite `south.credential-recipe.v1` in `south-provider-conformance`.
- The host implements `CredentialRecipeHarnessV1`: open a session over a fresh store and its own generic executor,
  with token egress injected to the suite's `FakeTokenEndpointV1`. The injection exists only in test builds, which is
  how §3.4 rule 6 is met.
- Nine cases: an exchange failure writes nothing; `reauth_required` is not retried; concurrent refreshes make one
  exchange; the host clamp; the recipe clamp; rotation writes back and keeps the previous generation; an empty
  rotation does not wipe; a CAS loser re-reads the winner; a probe does not rotate.
- An in-memory reference host passes all nine. Nine deliberately broken hosts each fail exactly the case guarding
  their invariant.
- `compatibility.json` registers the suite as `not_verified` on both hosts.

**What gate ③ pins where §3.5 was silent:**
- "Until the operator acts" means a new credential generation. A `reauth_required` latch is tied to the generation,
  and any operator write clears it.
- A transient failure writes nothing and does not latch. A cooldown keyed by credential id, which outlives a
  generation change, would fail the suite.
- The previous generation that §3.5 keeps holds the replaced write-back values.
- For no-wipe, an omitted, `""` or `null` refresh token all mean "keep the stored one".
- A CAS loser uses the winner's value.

**Left open:**
- Whether a probe may refresh a non-rotating recipe.
- What a loser does when the winner's value is already stale. Today's server errors.

**Gaps found.**
- **The production host fails case 2 today.** Its terminal-failure path writes an audit entry and a metric, but holds
  no latch, so a `reauth_required` credential is retried on the next request. The generic executor of P21 S3 has to
  add the latch.
- Copilot's direct-use flow (§3.9) presents the GitHub token itself after the `404 → goto` branch. `present` names
  one step output, so that recipe cannot say what it presents. This is an **open amendment for B6**, recommended
  as: `present` takes ordered candidates, each a step output or `{"field": name}` of a secret field, and the first
  one present wins. No shipped package needs it before a Copilot component exists. Resolved in §13.5 D1, with a
  field candidate carrying its own validity.
- Claim order is canonical, not declared, because `claims` is a map. That matches the host for Kling. The host's
  Vertex struct writes `iss, scope, aud, iat, exp`: the JSON is equal, but the bytes differ.

### 13.4 Implementation of B7a (2026-10-02)

Phase B7a is implemented on branch `feature/b7a-instance-declarations`, stacked on B4 (§13.3); nothing is released.
Q15 and Q16 were ruled on 2026-10-02 (§16). B7a adds only the declaration mechanism; no shipped package declares a
client user-agent or identification header yet — those arrive with the components that need them (B6).

**Contract numbers**
- auth 4 → 5
- reserved header policy 1 → 2
- HTTP 10: declared query parameters and the declared user-agent
- provider quota metadata 1 → 2

These suites gain declared-instance cases, and their versions stay at 1, following the precedent of earlier
additive cases:
- `south.header-auth.v1` goes from 4 to 7 cases;
- `south.controlled-query.v1` from 6 to 9;
- `south.controlled-user-agent.v1` from 5 to 8.

`token-station-server` was verified against the smaller `header_auth` and `controlled_query` tables. Both entries
are now `not_verified` until the host re-runs them.

**Secret headers.** The manifest declares `secret_headers`: at most 8 lowercase RFC 9110 token names.
- **Refused names:** the reserved-header set, the transcript denials, `accept`, `content-type`,
  `content-encoding`, `retry-after`, the closed diagnostic and quota names, and the five sanctioned names (which
  need no declaration). Upper case is refused, not folded.
- **Auth arm:** `ProviderAuthV1::DeclaredHeaderSecret` carries a declared name; the closed `HeaderSecret` arm stays.
- **No smuggling:** `SafeHeaders` built with the package's declaration refuses those names on the ordinary channel.
- **Transcripts** drop declared names. Under policy 2 they also drop the five sanctioned names; before, an upstream
  echoing `x-api-key` was transcribed.
- **Admission:** `admit_descriptor_auth` admits a declared name, and refuses a declared name found among the
  descriptor's ordinary headers.
- **Blocked on B7b:** the kernel's `Auth::header` still checks names against its static list while deserializing,
  and the only names it accepts beyond `SecretHeaderV1` are undeclarable. So no component descriptor can name a
  declared header until B7b. A test pins this and is meant to fail when B7b lands. *(Landed 2026-10-08: §13.7 item 5
  replaces that test.)*

**Query parameters.** The manifest declares `query_parameters`: at most 16 entries, provider world only.
- **Value syntax:** each declaration names one closed value syntax, `digits`, `token`, `date` or `enum`. `token` is
  RFC 3986 unreserved bytes; RFC 9110's token admits `&`, `#`, `%` and `+`, which would split or escape a query.
- **Refused names:** credential-like names and fragments (`key`, `token`, `apikey`, `secret`, `signature`, …), and
  the fixed names in any spelling. Names are normalized before the check.
- **Order on the wire:** declared parameters follow the fixed ones, in name byte order.
- **No credentials:** query values still never come from credential resolution.
- **Task world:** not admitted. A new task-world query name still needs a south release; widening is additive later.

**Quota headers.** The manifest's `quota_headers` maps a response header to one of the nine closed normalized
fields, one header per field.
- A declaration replaces the canonical map; without one, capture is as in contract 1. An empty list therefore
  cannot opt out of capture.
- The transport is configured per package through `with_quota_headers`.
- South cannot check that a declared header carries the field's meaning or format, for example a reset given as a
  duration rather than a timestamp. That stays in the undetectable zone.
- Gate ① refuses a name declared both as a secret header and as a quota header.
- The transport also skips any quota entry naming one of the request's declared secret headers, so the rule holds
  even for a host that builds the map without running gate ①.

**User-agent.** The manifest declares `user_agent` per family, validated by the unchanged `ControlledUserAgentV1`
grammar.
- `DeclaredUserAgentV1` is built from that value, and the request slot is `UserAgentV1` (`Controlled` or
  `Declared`), so exactly one `user-agent` still reaches the wire.
- "Built only from a value that passed gate ①" is a discipline across the crate boundary, not a type guarantee,
  just as the `'static` rule was. `DeclaredInstancesV1::from_manifest` (conformance) validates first and is the
  sanctioned path.
- The 2026-08-20 record carries an amendment note.

**Breaking API on re-pin**
- Raw call structs gain `secret_headers`, and their `user_agent` becomes `Option<UserAgentV1>`.
- `QueryParameterV1` is no longer `Copy`, and `wire_name` is no longer `'static`.
- `auth_headers()` names are no longer `'static`.
- Several enums gain variants.
- The server has about 18 raw-call struct literals to update.

**Error contract.** The error contract version governs the provider-call codes (`PreparationErrorV1`); it moved to 2
when the finalization codes were added. B7a adds no provider-call code: a refused declared instance surfaces through
the existing `UNSUPPORTED_AUTH_SHAPE` and `INVALID_RELATIVE_PATH`. The new codes are `ContractErrorV1` parse errors,
which the multipart body added the same way without a bump.

### 13.5 S3b prerequisites: the Copilot recipe and the host's feedback (2026-10-05)

The host completed P21 S3a on v0.43.0: its generic recipe executor passes gate ③ 9/9 at token-station-server
`8777b84f`, and Kling mints through the recipe. Its first OAuth family is Copilot (host ruling Q-F), which needs a
south release. This section lists what that release adds, and the host's feedback items SF1–SF6. Under the owner's
standing rule (a question whose recommendation keeps the host vendor-neutral is taken as recommended), Q19–Q22 below
were taken as recommended; the one item that could reasonably go the other way is called out under Q20.

**Copilot today, in the host** (server `8777b84f`, `token_refresh.rs` Copilot section, `upstream.rs`,
`south_adapter.rs`):

- The stored GitHub OAuth token is exchanged by `GET https://api.github.com/copilot_internal/v2/token` with
  `authorization: token <github token>`, `editor-version: vscode/1.123.0`, `editor-plugin-version: copilot-chat/0.43.0`,
  `user-agent: GitHubCopilotChat/0.43.0`, `x-github-api-version: 2025-04-01` and `accept: application/json`. A 2xx
  carries `token` and an absolute `expires_at` in epoch seconds. Nothing rotates.
- A 404 means the token itself is the chat bearer (the Copilot CLI and Enterprise flow). The host first confirms the
  seat with `GET https://api.github.com/copilot_internal/user` under the same headers, then presents the GitHub token
  with a synthetic validity of 3600 s: a re-validation cadence, not a real expiry.
- The chat request goes to `{base}/chat/completions` (no `/v1`) with the Copilot bearer, a declared user-agent and
  six constant headers: `copilot-integration-id: copilot-developer-cli`, `editor-version`, `editor-plugin-version`,
  `openai-intent: conversation-panel`, `x-github-api-version: 2025-04-01` and
  `x-vscode-user-agent-library-version: electron-fetch`. The body is the OpenAI-compatible body.

**D1 `present` takes ordered candidates (§16 Q19).** §13.3 left the direct flow unexpressible: `present` named one
step output and `http_probe` produces none, so the reference interpreter ended the 404 branch as "present missing",
`transient`. `present` now also accepts an array of candidates, the first present and non-empty one wins:

- a candidate is a step output (`"exchange.token"`, as before) or `{"field": name, "validity_seconds": n}`, which
  presents the stored value of a declared **secret** field;
- a single string keeps its meaning, so every existing manifest reads the same;
- the expiry of an output candidate is its step's clock, else `now + default_seconds`, as before; a field candidate has
  no step, so it carries its own `validity_seconds` (60 s to 24 h). It does not borrow `default_seconds`, which would
  also become the fallback of every clock in the recipe: Copilot's exchange must fail when its `expires_at` is
  missing, not quietly trust the token for an hour;
- gate ①: one to four distinct candidates, every output candidate produced by some step, every field candidate a
  declared secret field with its validity in range; the presented-JWT rule applies to each JWT candidate.

Trust: a field candidate sends the field to the inference upstream only, under `ProviderConfig::authorize`, exactly as
a static slot sends an operator key; it never reaches a recipe endpoint through `present`. §3.4 is unchanged.

The Copilot recipe:

```json
"steps": [
  { "id": "exchange", "kind": "http_exchange", "method": "GET",
    "endpoint": "https://api.github.com/copilot_internal/v2/token",
    "auth": { "scheme": "token", "value": { "field": "github_token" } },
    "headers": { "<the five exchange headers above>": "…" },
    "on_status": { "404": { "goto": "direct" }, "429": "transient" },
    "extract": { "token": { "pointer": "/token", "secret": true },
                 "expires_at": { "epoch_seconds": "/expires_at" } } },
  { "id": "direct", "kind": "http_probe", "method": "GET",
    "endpoint": "https://api.github.com/copilot_internal/user",
    "auth": { "scheme": "token", "value": { "field": "github_token" } },
    "headers": { "<the same five headers>": "…" },
    "on_status": { "429": "transient" } }
],
"present": ["exchange.token", { "field": "github_token", "validity_seconds": 3600 }],
"rotates_refresh_material": false,
"refresh_margin_seconds": 30
```

Three deliberate differences from the host's native arm, pinned by the family's `credential.*` fixtures; a host's
replay comparison classifies them as expected rather than as drift:

- **Status classes.** The native arm turns every non-404 failure, 5xx included, into an operator error. The recipe
  uses the vocabulary's defaults (4xx `reauth_required`, 5xx `transient`) and declares 429 `transient`, so a rate
  limit or an outage at GitHub does not latch a working credential.
- **Refresh margin.** The native arm refreshes 300 s early. Gate ① requires the margin to be shorter than the shortest
  validity after the clamp (§13.3); a recipe floor of 600 s would make the host trust a short-lived token for longer
  than GitHub granted it. The recipe declares 30 s, the host's default (P21 Q-A).
- **A 2xx without `token` or `expires_at`** (host feedback SF8). The native arm fails to parse it and answers 400,
  the operator-error class; the recipe ends `transient`, as the vocabulary does for any extraction a successful
  response lacks (§3.3). Under DP0 the recipe's answer is the right one: a malformed answer from GitHub says nothing
  about the stored GitHub token, so it must not latch the credential, and only `on_status` may call a credential
  unusable. The native 400 did not latch either (its latch matched only `(HTTP 400` / `(HTTP 401` in the message),
  so the practical difference is the class the client sees: 503 instead of 400.

**D2 a `credentials` section may be scoped to families (§16 Q20).** A manifest has one `credentials` section and its
`slots` are package-wide. Adding the Copilot recipe to `provider-openai-compatible` unscoped would mint
`provider_api_key` for the `openai-compatible` and `azure-openai-v1` rows too, and replace their operator form with
Copilot's. R6 already says request-side declarations are per family; credentials are request-side. So the section
gains an optional `families` list:

- absent: the section applies to every family of the package (today's meaning, R5);
- present: one or more of the manifest's `providers`, no repeats; every other family behaves as if the package
  declared no `credentials` (static slots, the operator's key, no recipe, no form);
- `ComponentManifestV1::credentials_for(family)` is the one accessor; `admit_descriptor_auth` admits `Auth::OAuth`
  only for a family the section covers.

A package needing two different credential kinds for two families (Copilot and a future Vertex family in the same
package) is not expressible yet; the natural extension is a list of scoped sections, left until a package needs it.

**D3 a static slot names its field (SF5, §16 Q21).** v0.43.0 said a static slot uses "the operator-entered value",
which was well defined only without a `credentials` section. With one, the host collects only the declared fields, so
it had to refuse the package (host R28) rather than guess a field. A slot may now say `{"field": name}`: the slot
holds the stored value of that declared secret field, presented as is. And wherever a `credentials` section applies,
gate ① requires every secret slot to name its source, minted by a recipe or a field; a slot with no entry or a bare
`static` is refused there. Outside a section's families nothing changes. The reference interpreter adds
`stored_slot_value_v1`, which applies the same field rules as a run (defaults, syntax, `required`,
`require_one_of`) and returns the field's value or a configuration error, so a host has an oracle for it.

**D4 `CredentialFieldV1.description` (SF4).** Optional, one line for the operator form, mirroring
`ConfigKeyV1.description`. Like that field it is shown, never interpreted.

**D5 gate ③ (SF2, SF3).**

- A tenth case, `TransientFailureIsRetried`: two transient failures on the same generation with no write in between,
  and the third resolve exchanges and mints. Case 1 writes a new generation between its two failures, so a host that
  latched transient failures to the generation passed it. The reference host gains the matching fault, which fails
  exactly this case. The suite stays version 1, as additive cases have before (§13.4).
- `CredentialRecipeSessionV1::write_generation` documents that it must produce a new generation even when the values
  equal the stored ones: case 1 rewrites the seed unchanged. A store that bumps its generation only on a changed value
  (the server's trigger) commits the harness's write through a path that forces the bump. The runner cannot observe a
  generation, so this stays a documented obligation rather than a check.

**D6 the Kiro draft (SF1).** The B4 test draft declared `auth_method` as `enum ["social", "idc"]` with the default
`social`. `enum` matches exactly, so a stored `IdC`, `enterprise` or `iam_identity_center`, all of which the host's
native arm accepts case-insensitively as IdC, would have been a configuration error; and the default made an explicit
`social` with client fields select IdC. The draft now follows the Kiro record's four-rule selector: `auth_method` has
syntax `token` and no default; `field_in` (already ASCII case-insensitive) lists the three IdC spellings; any other
explicit value selects social; with no value, client fields decide. The host needs no alias table and the hand-over
needs no normalization. The Kiro record's `field_in` was also rewritten in the implemented `{field, values}` shape.

**D7 the `github-copilot` family in `provider-openai-compatible`.**

- `providers` gains `github-copilot`. Its `endpoint` is `https://api.{plan}.githubcopilot.com`, and its one
  `config_schema` key `plan` admits `individual`, `business` or `enterprise` and defaults to `individual` (host
  feedback SF7). GitHub routes each plan to its own chat API host (Pro and Pro+ to `api.individual`, Business to
  `api.business`, Enterprise to `api.enterprise`, per GitHub's Copilot network allowlist reference); the host's
  native rows reach the latter two by an operator-edited endpoint, so a fixed host would have kept those seats off
  the component. An enum of DNS labels keeps the domain fixed (§7.3). Not offered: the plan-less
  `api.githubcopilot.com`, which the native default uses and which GitHub's allowlist no longer lists, and GitHub
  Enterprise Cloud with data residency (`*.ghe.com`), whose chat and token hosts both carry a per-tenant name that no
  enum can list. The token exchange and the seat check stay at `api.github.com` for every `github.com` plan; only the
  chat host varies.
- `config_schema` keys gain an optional `default`, the value used when the operator enters none, mirroring a
  credential field's `default`: only an optional key has one, it has the key's syntax, and an endpoint parameter is
  now a required key or one with a default. `fill_endpoint` and `validate_config_values` apply it.
- No `request_facts` entry, so the family uses the top-level locations (`max_tokens` or `max_completion_tokens`,
  `model`, `stream`), which is what its OpenAI-compatible body writes.
- `build-http-request` for the family posts to `{base_url}/chat/completions` (the kernel's `resolve` would add `/v1`
  to an origin-only URL), with the OpenAI-compatible body, `Auth::bearer` on the slot, and the six identification
  headers as ordinary descriptor headers.
- `user_agent` declares `GitHubCopilotChat/0.43.0` for the family (§10, Q15).
- `credentials`, scoped to `github-copilot`: the field `github_token` (secret, required, with a description), imported
  from the editor's `apps.json` / `hosts.json` (`/github.com/oauth_token`, then the VS Code app key), the minted slot
  and the recipe above.
- Body-dependent headers (`copilot-vision-request` for image parts) stay out, as they are in the host today.

*Descriptor headers, not a manifest declaration.* Q10 ruled that headers on the inference request are written by the
component. §10's declared sets exist where a name must be reserved (secret headers) or a value must not come from
request data (the user-agent). Neither applies to six non-secret constants: the ordinary channel already admits
them, gate ② pins them byte for byte in the family's fixtures, and a manifest list would be a new mechanism with no
safety property to enforce (R1). The user-agent is the exception because it is reserved, and it goes through the
existing `user_agent` declaration.

Package identities: `provider-openai-compatible` 2.2.0 (a new family; the other two families' requests, responses
and streams are unchanged, as their frozen fixtures show). The shared conformance and provider-api crates changed,
and a same-path rebuild of every package before and after showed a different `component.wasm` for all thirteen, so
the other twelve take a patch bump with unchanged behavior: `provider-anthropic` 1.0.10, `provider-bedrock-converse`
1.0.7, `provider-gemini` 1.1.6, `task-kling` 1.0.6, `task-kling-v2` 0.32.4, `task-minimax-v2` and `task-bailian-v2`
0.31.3, `task-byteplus-v2` 0.36.3, and `task-xai-v2`, `task-veo-v2`, `task-wan-image-v2` and `task-gmi-image-v2`
0.35.3.

**SF6.** `compatibility.json` first records `token-station-server`'s `credential_recipe` as verified against nine
cases (server `8777b84f`). D5's tenth case makes that evidence stale, so the same branch sets it back to
`not_verified` until the host re-runs the suite (the freshness test enforces it).

**Contract and versioning.** Every change is additive on the wire: a single-string `present`, a manifest without
`families`, a field without `description` and a gate ③ table of nine cases all keep their meaning. The schema tag stays
`south.credential-recipe.v1`; a config key without `default` keeps its meaning too. A runtime that predates them refuses a manifest using them (`deny_unknown_fields`), and
the package's `south_runtime` moves with the release, so an older host refuses the new package cleanly. The Rust API is
not additive: `RecipeV1::present` becomes `Option<PresentV1>`, `SlotV1` gains `Field`, `CredentialFieldV1`,
`CredentialsV1` and `ConfigKeyV1` gain a field each, and `CredentialRecipeCaseIdV1` gains a variant.

**What the host does after the release** (P21 S3b): re-pin; read credentials through `credentials_for`; implement
present candidates and field slots in its executor and lift R28; offer the `github-copilot` family's `plan` key with
its default when a row is created; apply the declared user-agent and run
`south.controlled-user-agent.v1` (lifting R12); re-run gate ③ (ten cases) and report it; then C11–C13 for Copilot.

### 13.6 Host S2 and S4 prerequisites (2026-10-05)

The host planned P21 S2 (version ranges and package discovery through the release index) and S4 (framing and signing
selected by declaration, each form implemented once) against v0.44.0 and sent ten items back, SF10–SF19. On
2026-10-05 the owner (lv) ruled every one of them as recommended. Where an item left a choice open, the choice below
keeps the host vendor-neutral and was taken as recommended under the owner's standing rule; §16 Q23–Q34 record each
one, and the items that still need the owner are called out there.

**SF10 a package declares the oldest runtime it needs (§16 Q23, Q24, Q25).** Until v0.44.0 every release re-stamped
every manifest's `south_runtime` with the release's own version. A host links one runtime and refuses a package that
declares a newer one (§8.2 item 2), so a package added in a later release was refused by every host that had not yet
re-pinned, even when it needed nothing new, and the S2 acceptance ("a new package installs without a host change")
could only hold on a synthetic release. The rule is now:

- `south_runtime` is the oldest south runtime that admits and correctly runs the package. It moves only when the
  package starts relying on something a release introduced (a manifest field or value an older gate ① refuses or reads
  differently, or a runtime-side meaning its descriptors depend on), and it moves to that release, with a version bump.
- A release re-stamps nothing. A package whose `component.wasm` **and** `manifest.json` are unchanged keeps its
  version; the digest-stability check (§8.6, §13.2) now also fails a package whose manifest changed under an unchanged
  version (`scripts/release_index.py compare`). The ruling of §13.2 that "manifests may change without a version bump,
  since `south_runtime` moves when a package is re-verified" is withdrawn with it.
- Release CI proves each declaration: `scripts/check-declared-runtime.sh` stages the packages, groups them by declared
  runtime (`release_index.py declared-runtimes`, which refuses a runtime newer than the release, older than the range
  handshake of 0.43.0, malformed, or without `runtime_abi`), and for every older runtime checks out that runtime's
  release tag and runs `crates/south-provider-runtime/tests/declared_runtime_v1.rs` there. That test loads each package
  with the tag's own `LoadedComponentV1::load` under a `HostRangeV1` whose floor and ceiling are both the declared
  runtime: gate ① (`gate_manifest`), the range handshake (`compatibility_admits`), the import scan and the identity
  probe. The packages declaring the version being built are judged by this tree. `release.yml` runs it on the archives
  before the index is generated; `ci.yml` runs it on `release/*` pull requests only, since it builds every component
  and one older runtime per distinct declaration.
- **What the check does not prove.** It proves the declared runtime *loads* the package. It does not prove that the
  declared runtime's link layer (descriptor auth admission, request-facts sealing, the codecs) accepts every request
  the package builds; a package whose manifest parses under an older runtime but whose descriptors rely on a newer
  link-layer meaning would pass. Running the declared runtime's gate ② over the package's own fixtures would close that,
  at the cost of the fixture format becoming a cross-release contract; §16 Q24 recommends it as a follow-up and flags
  it. Until then the rule's first bullet is held by review.
- Building an older runtime costs one more compile of the runtime crate and wasmtime per distinct older declaration;
  the script shares the tree's target directory so registry dependencies compile once. It was exercised locally
  against `v0.43.0`: a package declaring 0.43.0 whose manifest uses a 0.44.0 field (`CredentialFieldV1.description`)
  is refused by the 0.43.0 loader, and one using nothing newer is admitted.

Under this rule the packages of this change declare: `provider-bedrock-converse` 1.0.8 and the new
`provider-bedrock-converse-bearer` 1.0.0 declare 0.44.0 (their manifests use 0.44.0's credential field descriptions and
nothing newer); the other twelve keep 0.44.0. The release that carries this change bumps the workspace version and
re-stamps no manifest.

**SF11 provider packages declaring link-layer contracts (§16 Q26): written up, not implemented.** The request was cheap
only if additive, and it is not: `compatibility_admits` refuses a declared contract the host does not list, and hosts
list only `task` today, so a provider package declaring `{"http": 10}` would be refused by every current host. §16 Q26
records the recommendation.

**SF12 one instance per package (§16 Q27).** `SandboxedComponentV1` holds an `Arc<LoadedComponentV1>`;
`SandboxedComponentV1::shared` takes one, so a host serving the three families of `provider-openai-compatible` loads
and instantiates the package once. `new` still takes a `LoadedComponentV1`. Calls on the shared instance are serialized
by the runtime as before, and every stream still gets its own instance. `new` and `inner` are no longer `const fn`.

**SF13 Converse declares its signing credential fields (§16 Q28).** `provider-bedrock-converse` declares a
`credentials` section with three secret fields: `access_key_id` and `secret_access_key` (required) and
`session_token` (optional), each with a description. Gate ① now checks what §5.4 promised and §13.1 deferred: every
`signing.credentials` entry names a declared secret field of the section that applies to each signed family
(`credentials_for`), and an input the scheme requires (`SigningSchemeV1::requires`) names a required field. A host
collects exactly those fields and needs no field set of its own; its R32 becomes unreachable. The check refuses a
signing package with no section at all, which includes the published `provider-bedrock-converse` 1.0.7: a host linking
the next runtime must take 1.0.8 with it.

**SF14 Converse sends the native arm's headers (§16 Q29).** The descriptor adds `accept` (`application/json`, or
`application/vnd.amazon.eventstream` when streaming) and `x-amzn-bedrock-accept: application/json`, the values the
host's native Converse arm sends (server `p21-s3b` `text_admission/sender.rs`, `bedrock.rs`). The transport's own
`accept: */*` is replaced by the descriptor's, as for any ordinary header. A request moved from the native arm onto the
component therefore reaches the upstream with the same headers; the host's `component`-row code that writes `accept`
itself must stop doing so for this package. Both are ordinary descriptor headers pinned by the fixtures, for the reason
§13.5 D7 gives for Copilot's. A streaming request fixture is added. Identity 1.0.7 → 1.0.8.

**SF15 gate ③ suites for framing and signing (§16 Q31).** Two host-implemented suites in `south-provider-conformance` let a host prove its own
wiring of the B2 declarations; `compatibility.json` records both as `not_verified` on both hosts until a host runs them.

- `south.eventstream-framing.v1` (nine cases) drives the host's framing executor through
  `EventStreamFramingHarnessV1`: a streaming method that takes the package's `stream_framing` and the upstream body as
  chunks and reports what the host fed `parse-stream-chunk` and whether the stream completed or failed, and a buffered
  method (for `aws-eventstream` with a family declaring `stream: "none"`) that reports the text handed to
  `parse-response`. The cases check that `bytes` framing passes the body unchanged under every split; that `event`
  frames re-encode byte for byte and `exception` / `error` frames pass through in canonical form; that every
  byte-boundary split delivers the same bytes; that a checksum mismatch, a truncated tail at end of input (the host must
  call `finish`) and a non-JSON payload each fail the stream after delivering exactly the messages before the fault; and
  that the buffered path concatenates the re-encoding or fails without handing anything over. Bodies come from a
  test-only encoder in the suite crate, pinned to a frame computed with Python's `zlib`.
- `south.request-signing.v1` (seven cases) drives the host's declaration-selected finalizer through
  `RequestSigningHarnessV1` with the declaration (`SigningV1` and `emits`), the family's configuration values, credential
  values keyed by field name, an injected time and the finished request. It verifies rather than compares (§16 Q31): it
  recomputes SigV4 from the request and the emitted `SignedHeaders` and checks the signature, the credential scope
  `<date>/<region>/<service>/aws4_request`, `x-amz-date`, the payload hash, the session token, that the emitted names
  are the expected subset of `emits` (`emits` is the upper bound; the per-request set follows the credential), and that
  `SignedHeaders` is sorted, covers `host`, `x-amz-date`, `x-amz-content-sha256` and any session token, and names only
  headers the request carries. The cases cover the exact body bytes, a sent and signed session token, three headers
  without one, service and region taken from the declaration and configuration (`sagemaker`, `eu-central-1`, parameter
  `aws_region`), credential fields read through the mapping, path segments encoded twice (an inference-profile ARN),
  and refusal without a secret access key. The verifier stays private to the crate, so the suite offers no signer for
  a host to adopt; its HMAC is written over `sha2` and pinned to RFC 4231, to AWS's `get-vanilla` vector and to a POST
  vector computed with Python's standard library. It covers the fixture shapes only (`https`, no query, no dot
  segments), so a host's own query or path normalization is not exercised.

As with `south.credential-recipe.v1`, each self-test runs a reference host that passes every case and deliberately
broken hosts, each failing exactly the cases that guard its mistake; the signing reference host is a second SigV4
implementation written apart from the verifier.

**SF16 a Bearer sibling for Bedrock API keys (§16 Q30).** Bedrock also accepts an API key as `Authorization: Bearer`.
`host_signed` admits no second arm, so the Bearer form is a fourteenth package, `provider-bedrock-converse-bearer`
1.0.0, with the family `bedrock-bearer` on the `bearer` arm and the slot `provider_api_key`. Every other declaration
equals Converse's (endpoint template, `region` key, `request_facts`, `stream_framing`, capabilities, compatibility), and
a test pins that. Its reference, `BedrockConverseBearerReferenceV1`, delegates parsing, streaming, errors and
capabilities to `BedrockConverseReferenceV1` and builds the same request through one shared function, differing only in
its identity, the family it serves and the descriptor's auth. The source is shared; the component crate is a separate
shell, because a package's reported identity is compiled in and gate ① compares it with the manifest. Its fixture pack
is the Converse pack with the auth delta applied (request inputs name the family and the slot, request expectations
carry the bearer auth), and a test fails if the two packs drift. It has its own build script, release workflow entries,
sandbox parity test, gate ② report and usage judge rows. A host chooses between the two packages by the row's family,
never by the shape of the stored credential.

**SF17 recovering endpoint parameters from a base URL (§16 Q32).** `ComponentManifestV1::endpoint_values(family,
base_url)` is the reverse of `fill_endpoint`: it returns the template parameters an operator-entered URL was filled from.
It returns values only when exactly one assignment matches and filling the template with it gives back the same URL;
otherwise `EndpointValuesErrorV1::{NoEndpoint, NotThisEndpoint, Ambiguous}`. A host holding only a `base_url` can
derive a signing package's region from it instead of refusing the row.

**SF18 the T21 eventstream guest (§16 Q33).** `crates/south-provider-runtime/tests/guests/t21-unseen-eventstream/`
(1.0.0, family of the same name, not released) is §12 item 5's guest, with the `host_signed` mode as well. Like T03
it is a standalone JSON-only crate that the host builds from a south checkout (`cargo build --target wasm32-wasip2` in
that directory), and the host synthesizes its manifest. That manifest declares `stream_framing: aws-eventstream`,
`host_signed` with `aws-sigv4` `signing` for a non-Bedrock service, a credentials section naming the three SigV4 fields,
`request_facts {output_cap: ["/t21_limits/max_out"], model: {url: "/t21/models/{model}/invoke"}, stream: "none"}`, an
`https://api.{region}.p21-unseen.test` endpoint and a required `aws_region` key. Its request has no stream switch: one
URL and one body (`t21_turns`, `t21_limits.max_out`) serve both callers, and the descriptor carries no auth unless the
host grants a slot (then bearer, for a bearer variant). Its upstream events `t21Say`, `t21Meter` (exact `in`/`out`,
exactly once, before the end) and `t21End` (`reason`, `ticket`, `served_model`) are read only in south's canonical
re-encoding, split anywhere. `exception:` and `error:` frames end the stream with the IR error event; an unknown event
or a non-canonical frame is refused, never skipped. `parse-response` accepts only the concatenated re-encoding and
refuses plain JSON, so a host that skipped the buffered path is observable. Four rogue sentinels each change one
descriptor field and must be refused with zero upstream calls: `rogue-signed-auth` (a bearer slot on a `host_signed`
package; descriptor auth admission, §4), `rogue-origin` (another origin under the same parent domain), `rogue-cap` (the
cap only at top-level `max_tokens`, §7.2) and `rogue-model-url` (`t21-decoy` in the URL; the `{model}` template).
`crates/south-provider-runtime/tests/t21_unseen_eventstream_v1.rs` proves the south half: gate ①, loading under a host
range, every mode on the JSON face, and stream splits at every byte boundary. Its frames are written by hand, since the
runtime crate does not depend on `south-contracts`; the golden vectors pin that shape. The other §12 rogue modes
(`rogue-arm`, `rogue-zero-usage`, `rogue-cap-twice`) belong to the other T21 guests and are not part of this one.

**SF19 third-party signing packages wait for package signing (§16 Q34).** Recorded as §3.4 rule 5's second paragraph.

**Package identities.** `provider-bedrock-converse` 1.0.7 → 1.0.8 (SF13, SF14) and the new
`provider-bedrock-converse-bearer` 1.0.0. The shared conformance and provider-api crates changed, and a same-path
rebuild of every package before and after showed a different `component.wasm` for all thirteen existing packages, so the
other twelve take a patch bump with unchanged behavior: `provider-openai-compatible` 2.2.1, `provider-anthropic` 1.0.11,
`provider-gemini` 1.1.7, `task-kling` 1.0.7, `task-kling-v2` 0.32.5, `task-minimax-v2` and `task-bailian-v2` 0.31.4,
`task-byteplus-v2` 0.36.4, and `task-xai-v2`, `task-veo-v2`, `task-wan-image-v2` and `task-gmi-image-v2` 0.35.4. Every
package declares `south_runtime` 0.44.0.

**Contract and versioning.** No contract number changes. Two gate ① tightenings: a signing package must declare its
credential fields (SF13), and the manifest-digest half of the digest-stability check (SF10) is a release-behavior
change. New API: `EndpointValuesErrorV1`, `ComponentManifestV1::endpoint_values`, `SigningSchemeV1::requires`,
`SandboxedComponentV1::shared`, `BedrockConverseBearerReferenceV1` with `BEARER_FAMILY`, and the two gate ③ suites (`EventStreamFramingHarnessV1`, `RequestSigningHarnessV1` and their case tables and runners).
Breaking only in const-ness: `SandboxedComponentV1::new` and `inner` are no longer `const fn`.

**What the host does after the release** (P21 S2 and S4):

- Re-pin, and take `provider-bedrock-converse` 1.0.8 with the runtime: the new gate ① refuses 1.0.7.
- S2: keep the host range's ceiling at the linked runtime and its floor where the host chooses; packages from later
  releases that declare an older runtime now load without a re-pin, which is the S2 acceptance on a real release.
- S4: read the signing fields through `credentials_for` and lift R32 (now unreachable) and the implied field set; use
  `endpoint_values` where a row has only an endpoint (R33 shrinks to "no value and no derivable value"); stop writing
  `accept` for `aws-eventstream` component rows, since the component now writes it; run
  `south.eventstream-framing.v1` and `south.request-signing.v1` and report them, which moves their
  `host_capabilities` entries from `not_verified`; serve Bedrock API-key rows with `provider-bedrock-converse-bearer`
  (family `bedrock-bearer`), lifting CP3, and treat that family by its declarations, not by the name `bedrock`
  (the host's Claude-dialect handling keyed on the dialect name must follow); use `SandboxedComponentV1::shared` for
  multi-family packages; build the T21 eventstream guest for the J2 synthetic tests.
- Keep R35 (signing only for verified first-party packages, §3.4).

### 13.7 Kernel re-pin to protocol 0.5.0, the B7b closeout and issue #138 (2026-10-08)

The kernel chain finished: `token-station-protocol` 0.5.0 (upstream tag `kernel-v0.5.0`, commit `8e34f5a0`, token-station
#40 and #41) is mirrored as `ballast-ai/token-station-kernel` `v0.4.0` (commit `c2581f37`), and the mirror records
`canonical_ir` 3. lv approved on 2026-10-08 that South re-pins, absorbs the API changes, finishes B7b, closes #138 and
wires Q14. The re-pin itself, with its compatibility values, package identities and the `south_runtime` decision, landed
first as #151 and is recorded in `2026-10-08-kernel-repin-protocol-0.5.0.md`; this section is what comes on top of it: the
combined auth arm and the `gemini-openai-compatible` family (B7b), issue #138, and one gap the first test found (Q40). The
Q14 value channel is wired in §13.8 and nothing here depends on it.

**What the kernel changed, and what South does with each** (`f585bc83..c2581f37`, `crates/protocol`):

| Kernel change | Effect on South |
|---|---|
| `Auth::BearerAndHeader { name, secret }`: the secret as `Authorization: Bearer` and verbatim in `name` | A descriptor can now ask for the combined arm. Admission is new (B7b, below). |
| `Auth::header` and its deserialization admit any lowercase RFC 9110 token of at most 64 bytes outside the kernel's `NEVER_CREDENTIAL_HEADERS`; the 0.4.0 catalog names keep any case as written; `CREDENTIAL_HEADERS` and `is_credential_header` are public | The kernel no longer decides which names a package may use. A component's descriptor can name a header the manifest declares in `secret_headers`; `admit_descriptor_auth` decides (B7b). |
| `Usage.explicit_cache_read_tokens`, a subset of `cache_read_tokens`, omitted from the wire at zero; `Usage::is_partitioned` | No reference reports it, so every package's wire is unchanged. The gate ② partition judge learns the subset rule so a package that starts reporting it is judged. |
| `ProviderConfig.declared` and `ChatRequest.host_values` become `ComponentValues` (keys of 1 to 64 bytes of `[a-z0-9_]`, values of 1 to 4096 bytes of printable ASCII, validated on construction and on deserialization) | South builds neither outside fixtures and no fixture sets them. Wiring is §13.8. |
| `ProviderEndpoint::permits` admits an encoded slash inside one segment below the endpoint path when every decoded piece is non-empty and neither `.` nor `..`; the endpoint path itself still refuses it | Issue #138. The same rule would admit a task id containing `/` in an observe URL; #151 made the seven task-v2 components refuse it themselves (its record §4.1), so no task test moves here. |

**1. What #151 already did (§16 Q37).** The pin (`token-station-protocol` 0.5.0 at mirror `c2581f37`), the compatibility values
(`ir_schema_id` `token-station-protocol@0.5.0/v0.4.0`, `kernel_version` `0.4.0`, `kernel_revision`
`8e34f5a089d0b9c7273b49ddb6952dd87e960019` and `kernel_contracts.canonical_ir` 3, each derived there from the definitions in
`manifest.rs` and the mirror's own `compatibility.json`), the patch bump of thirteen packages, and the decision to keep every
package's `south_runtime` at 0.44.0 until the release step are recorded in that document (§1 to §5) and not repeated here. An
independent derivation of the tuple from the previous pin's mapping gave the same values. The range handshake compares
`kernel_contracts` by exact equality in both directions, so a host that records `canonical_ir` 2 refuses every package of
this tree and a host that records 3 refuses every earlier one: a one-way flag day, which is why all fourteen packages moved
together.

**2. The consequence of that decision, and the release step.** A package declares `canonical_ir` 3 under `south_runtime`
0.44.0, and `scripts/check-declared-runtime.sh --build` loads it under the `v0.44.0` tree, whose `compatibility.json` records
2, so that script **fails until the release step** moves every package's `south_runtime` to 0.46.0 and the workspace version
to 0.46.0 together (the tree must contain the runtime a package declares, which `shipped_packages_v1` also requires). This
change does not hide that failure and does not move the field early. At the release step the oldest runtime that admits the
packages is 0.46.0: a 0.45.0 host refuses them through the contract number and through a declared runtime newer than itself,
and a test pins each fence on its own (`a_host_recording_the_old_kernel_contract_refuses_every_shipped_package` pins the
first today; the second needs the declaration the release adds). Both stacked pull requests belong in one release: a tag
between them would put the second one's changes into `component.wasm` under unchanged versions, and the digest-stability
check would demand another round of bumps.

**3. Package identities.** This change moves exactly one: `provider-openai-compatible` 2.3.0 (merged by #150 after 0.45.0,
which published 2.2.1; #151 left it there) → **2.4.0**, because the new family and arm change its manifest and its
`component.wasm`. The other thirteen keep the versions #151 gave them: a fixture row (#138) is not package content, and the
versions are unreleased, so a second bump would only skip numbers.

**5. B7b: the combined auth arm (§4.3, §16 Q4 option A).**

- The provider world's auth vocabulary gains `bearer_and_header_secret`. The task worlds do not admit it: `TASK_AUTH_ARMS`
  is the old four, because no task consumer needs it and a vocabulary word is a promise the task host path would have to
  honor (§16 Q36). Gate ① refuses it in a task manifest as an unknown word, and `host_signed` still admits no other arm.
- `admit_descriptor_auth` admits `Auth::BearerAndHeader` as `AdmittedAuthV1::BearerAndHeaderSecret(SecretHeaderV1)` when
  the manifest declares `bearer_and_header_secret` and the descriptor's name is one of the five sanctioned secret headers
  (any case). A host maps it onto `RawAuthV1::BearerAndHeaderSecret`; auth contract 5 already has the arm, so no
  contract number changes. Without the arm in the manifest it is refused with #151's `BearerAndHeaderNotDeclared`, which this change keeps for exactly
  that case and builds on, with its two tests. The arm is
  independent of `bearer` and `header_secret`: declaring either does not admit the combined descriptor.
- A name the manifest declares in `secret_headers` is **refused** on the combined arm
  (`CombinedHeaderNotSanctioned`). The contract's combined arm is closed over the sanctioned set, and widening it needs an
  auth-contract bump (`BearerAndDeclaredHeaderSecret`, auth 6) for a consumer that does not exist (§16 Q35).
- Declared secret header names now reach descriptors. A deserialized `Auth::Header` naming a header in the manifest's
  `secret_headers` is admitted as `DeclaredHeaderSecret`; the same name undeclared is `HeaderNotSanctioned`. The placeholder
  test `until_b7b_no_component_descriptor_can_name_a_declared_header` is replaced by tests of exactly these cases. Two
  pins keep the layers honest: South's `UNDECLARABLE_SECRET_HEADER_NAMES` contains every kernel `NEVER_CREDENTIAL_HEADERS`
  name, and every kernel `CREDENTIAL_HEADERS` name is either sanctioned or undeclarable, so the kernel's looser rule cannot
  open a name South forbids. A host that presents a credential in a declared name must redact it as well as the kernel's
  default set (`CREDENTIAL_HEADERS`).
- **A gap the first pin found (Q40).** The kernel's documentation of `NEVER_CREDENTIAL_HEADERS` says admitting layers
  may refuse more names and that South does, but South's undeclarable list lacked five of the kernel's names: `accept-encoding`, `forwarded`,
  `http2-settings`, `via` and `www-authenticate`. A package could declare one at gate ①, and the kernel would then refuse
  it in every descriptor, so the declaration could never work. Both copies of the list (`south-contracts` and
  `south-provider-api`) gain the five names. No package declares any `secret_headers`, and before 0.5.0 no descriptor
  could name a declared header, so the narrowing breaks no consumer; the auth contract and the reserved header policy keep
  their numbers.

**6. B7b: the `gemini-openai-compatible` family (§16 Q38).** The family lives in `provider-openai-compatible`, not in
`provider-gemini` and not in a new package. Gemini's OpenAI-compatible surface (`/v1beta/openai/chat/completions`) takes
an OpenAI-shaped body and demands the key twice, as `Authorization: Bearer` and in `x-goog-api-key` (2026-09-08
record). `provider-gemini` builds native `generateContent` URLs and bodies and presents `x-goog-api-key` alone, so it
cannot serve that surface; a new package would copy the OpenAI-compatible translation for one different descriptor field.
The family reuses the translation unchanged and differs in two declarations: its descriptor presents
`Auth::BearerAndHeader` over `x-goog-api-key`, and its endpoint template is
`https://generativelanguage.googleapis.com/v1beta/openai`. The package's `auth_arms` become `bearer`,
`bearer_and_header_secret` and `header_secret`. A host that wants the family must take `provider-openai-compatible` 2.4.0,
which needs this runtime (§16 Q20's caveat applies again). Gate 2 gains two rows in the OpenAI-compatible pack,
`provider.request.gemini-openai-compatible` and `provider.request.gemini-openai-compatible-stream`; no response or stream row
is added, because the family parses with the plain family's code, and no capture of the Gemini surface's responses backs that
claim beyond the documented OpenAI wire. The gate 2 test that pinned the package's arm set follows the manifest.

**7. Issue #138.** The Converse and Gemini references already encode the model as one path segment, so an inference-profile
ARN such as `arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.claude-x` becomes `…%2F…`. Kernel 0.5.0
admits it (option 1 of the issue, in its generic form: the kernel does not know `request_facts`, and the tail rule gives
the same guarantee that the target stays at or below the endpoint). Gate ② already runs `authorize` on every request case
(`EndpointConfinement`), so the closeout is data: `provider.request.model-id-with-a-slash-stays-one-segment` in the
Converse, Converse-bearer and Gemini packs. A test pins the other half: a model whose pieces would traverse (`a/../b`,
`..`) or collapse (`a//b`) is built into a descriptor `authorize` refuses. Run against the previous pin, the new rows fail
`EndpointConfinement` for all three packages (the check under "Evidence").

**Known remaining risk (kernel behavior, not changed here).** Reading the 0.5.0 source, `permits` decodes each escape once
and does not refuse a decoded percent sign. A model id that itself contains `%2e%2e` is encoded by South as `%252e%252e`
and admitted; an upstream that decodes the path twice would read `..`. Model ids come from the operator's catalog, not from
clients. South does not add its own rule on top; a later kernel change could refuse an encoded percent sign.

**Rejections and limits this change introduces** (every one is also in §16):

1. The combined arm over a declared header name (Q35).
2. The combined arm in a task manifest (Q36).
3. Packages built against the old kernel are refused by a host that records `canonical_ir` 3, and the new ones by a host
   that records 2: a one-way flag day, unchanged in kind from the 0.39.0 re-pin (#151).
4. Declaring `accept-encoding`, `forwarded`, `http2-settings`, `via` or `www-authenticate` as a secret header (Q40).
5. Nothing else is newly refused. The `declared` / `host_values` grammar is the kernel's, applied when a fixture or a host
   deserializes a `ProviderConfig` or `ChatRequest`.

**What the host does after the release** (re-pin and the two follow-ons that were blocked on this):

- Re-pin South and the kernel mirror (`c2581f37`), set `canonical_ir` 3 in `HostRangeV1.kernel_contracts`, and take all
  fourteen packages at their released versions: a host cannot mix the two kernel lines.
- Follow the Rust type changes: `AdmittedAuthV1::BearerAndHeaderSecret`, `DescriptorAuthErrorV1::BearerAndHeaderNotDeclared`
  and `CombinedHeaderNotSanctioned`, `PROVIDER_AUTH_ARMS` and `TASK_AUTH_ARMS`, and the kernel's `Auth::BearerAndHeader`,
  `Usage.explicit_cache_read_tokens`, `ProviderConfig.declared` and `ChatRequest.host_values`.
- R17 (an upstream model id with `/`, for example a Bedrock inference-profile ARN) lifts with the re-pin; add a host test
  with an ARN model through Converse. The Bearer sibling serves the same ids.
- Redact a declared secret header name as well as the kernel's default set before presenting a credential in it. R10
  (declared secret headers) still waits for the host's S7.
- Bedrock's dialect name (§16 Q30): the host's Claude-dialect handling keyed on the name `bedrock` must follow the package
  declarations, or `bedrock-bearer` rows lose it.
- R24 (a recipe that exports `attributes`) and R9b (more than one slot) are unchanged by this section; R24 moves with §13.8.

**Evidence (2026-10-08, branch `feature/kernel-repin`).** Every command below was judged by its own exit code, all 0.

- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
  `cargo nextest run --workspace --all-features` with `PROPTEST_CASES=32` (1185 passed, 1 skipped); the doctests;
  `cargo test --workspace --no-default-features`; `cargo check --manifest-path fuzz/Cargo.toml --all-targets --locked`;
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features`; `rustup run 1.96.0 cargo check --workspace
  --all-targets`; `scripts/check-boundaries.sh` (self-test and run); `python3 -m unittest discover -s scripts -p 'test_*.py'`;
  `scripts/check-language.sh` (self-test, tracked files, and `--commits origin/main..HEAD`); `cargo deny`, `cargo audit` and
  `cargo machete` for the workspace and `fuzz/`.
- A local run of the release workflow's steps with the tag `v0.46.0`: all fourteen build scripts; the gate 2 reports from
  the sandbox parity tests (`SOUTH_GATE2_REPORT_DIR`); the archives; `scripts/check-declared-runtime.sh --dist` and
  `--build` (all fourteen load under the runtime they declare, 0.46.0, which is this tree); `release_index.py generate
  --require-gate2-reports`; and `release_index.py compare` against the `v0.45.0` index (every package changed version, as
  the check requires; `provider-openai-compatible` goes 2.2.1 to 2.4.0 against the published index).
- Mutations, each restored with `cp` and `touch` and checked with `git diff --quiet`: removing the combined word from the
  provider vocabulary (`combined_auth_arm_v1` fails); admission no longer requiring the manifest arm
  (`the_combined_arm_is_admitted_for_a_sanctioned_header_and_a_declaring_manifest` fails); dropping `via` from South's
  undeclarable list (`south_forbids_everything_the_kernel_never_lets_carry_a_credential` fails); the partition forgetting the
  explicit-read subset (`explicit_cache_reads_beyond_all_cache_reads_break_the_partition` fails); the task world given the
  provider vocabulary (`a_task_manifest_may_not_declare_the_combined_arm` fails); the Gemini family presenting a single
  header (`gemini_openai_compatible_v1` fails in two tests).
- Issue #138 against the previous pin: a scratch worktree of `origin/main` (kernel 0.4.0) carrying only the six new fixtures
  and `encoded_model_segment_v1.rs` fails five tests, among them gate 2 of all three packs
  (`endpoint_confinement` and `descriptor_auth_within_manifest` fail on `provider.request.model-id-with-a-slash-stays-one-segment`
  with "outside the configured endpoint"). On this branch the same rows pass.

## 14. Existing text to revise in step

- ARCHITECTURE.md:117-126: change the concluding sentence to "execution, material, reachable destinations and the
  host invariants belong to the host; per-provider description belongs to the component" (§3.5).
- ARCHITECTURE.md:205-207: "the runtime and the thirteen packages must be upgraded in one batch" lapses with §8.
- The Kling comment at manifest.rs:128-132: point it to task-v2's bearer approach and §3.
- reference_bedrock_converse.rs:21-35, 549-557, 736-739: restate as "this package declares
  `stream_framing: aws-eventstream`", and replace "ignored because the host validates upstream" with the exception /
  error mapping.
- 2026-09-10-released-component-artifacts.md:152-154: native reference implementations are not stand-ins for
  missing packages (§8.5), subject to Q8.
- The rationale at 2026-09-27-task-contract-v6-facts.md:22-23 lapses with §8.6; later contracts follow the additive
  rule.
- Depending on Q14: `2026-08-21-canonical-ir-inventory.md` §6 and D5 and the `provider-config` doc comments in
  provider-adapter.wit:89-90 and task-adapter.wit:87-88 (if the fence is amended), or nothing (if a typed kernel
  field is added).
- Depending on Q15: `2026-08-20-controlled-user-agent.md` and the `ControlledUserAgentV1` documentation
  (lib.rs:1229-1243).
- The kernel's `HttpResponseParts` comment (kernel:http.rs:379-382, "binary responses would need a `-v2` field"):
  add that an eventstream body reaches the component as the UTF-8 canonical re-encoding on the buffered path (§5.2).
- The WIT's `parse-response` rule "a 2xx whose body cannot yield exact usage is an error … never a zero"
  (provider-adapter.wit:110-116): except for an `absent` package, whose usage is all zero and never read (§6.2
  item 4).

## 15. Rejected alternatives

- **Kling via `HostSigned`** (§3.2): the provider knowledge merely moves to another place in the host.
- **Token exchange as a WIT function, with component code generating the exchange request**: the component would
  either touch secret values such as refresh tokens, or need a "placeholder substitution + response redaction"
  protocol to keep secrets out of the component; a data recipe can be validated as a whole at gate ①, read line
  by line in review, and its destinations shown to the operator; its expressiveness suffices for the six known
  families. The cost is that a flow outside the vocabulary requires a contract upgrade (§3.8).
- **Trusting recipe endpoints because they are constants**: a constant in an untrusted manifest is the author's
  choice, not a vendor fact (§3.4).
- **The host choosing an `sse` / `ndjson` decoder by declaration to parse a provider's stream for the
  component**: the components already split these themselves, so the host would gain a useless branch (§5.2). This
  does not reject `decode_sse_v1` itself, which the media worlds and `north_passthrough` use for their own purposes
  (§5.2).
- **Putting the deframer in `south-core`** (this record's first draft): parsing grammars live in `south-contracts`
  with their fuzz obligation; `south-core`'s prelude introduces none (raw.rs:11-12).
- **Per-family `stream_framing` / `usage_evidence`**: the response-side functions cannot tell families apart (R6).
- **An IR compatibility range on the protocol crate version**: the kernel promises nothing within a minor line, and
  additions would be dropped silently by older components (§8.3).
- **Keeping the exact tuple and automating "re-stamp everything"**: the collateral damage of J2b③ would not go away
  — any package from a different batch still would not load.
- **Compiling the capability catalog into each package's wasm** (§7.5 A): models move far faster than dialects, so
  package digests would change frequently because of data, diluting the point of pinning digests.
- **The host inferring the credential kind from what files such as `auth.json` look like**: judging by what a file
  looks like is judging by provider; declare it through the manifest's `import` instead.
- **Settling a bound hit at `min(reported, bound)` automatically** (considered in review): it would change lv's
  ruling that such cases go to the manual-review path; §6.3 keeps that path and only fixes that the reservation
  stays held and nothing settles to zero.
- **A host-side allowlist of recipe endpoints** (§3.4 rule 1): a new provider endpoint would then require a host
  change, against DP0; the host relies on operator confirmation per package digest only (Q18).

## 16. Open questions

Tags: S = south maintainers, L = lv, K = kernel.

- **Q1 (L) Link layer**: does DP0 count "bumping the south / kernel pin" as modifying the host? If it does, §10's
  instance declaration is a necessary condition for DP0, not an improvement.
  **Ruled (lv, 2026-09-30): it counts.** A new provider that needs a secret header name, query name or quota header
  outside the compiled-in sets would otherwise force both hosts to re-pin, rebuild and release. §10 is a necessary
  condition of DP0 and phase B7 is required.
  Note (2026-10-01): B7 is now split into B7a (south) and B7b (kernel) (§13); the ruling covers both.
- **Q2 (S)** Accept revising the concluding sentence of ARCHITECTURE.md:117-126 (§3.5), including the destination
  rules and the host invariants.
- **Q3 (S, L)** Credential recipes as a closed data vocabulary (recommended) or as WIT functions; and accept the DP0
  boundary that "a new flow outside the vocabulary requires a contract upgrade".
  **Ruled for the host side (lv, 2026-09-30): a closed data vocabulary; the contract-upgrade boundary is accepted.**
  The south maintainers' half remains open.
  Note (2026-10-01): that half now includes the §3.4 trust rules and the §3.5 host invariants.
- **Q4 (S, K)** The combined arm: add a variant to the kernel's `Auth` (recommended), or an interim family-level
  mirror in the OpenAI-compatible package (§4.3).
  **Resolved (2026-10-08): option A.** Kernel protocol 0.5.0 has `Auth::BearerAndHeader`; South admits it and serves it
  from a `gemini-openai-compatible` family (§13.7, Q35, Q36, Q38).
- **Q5 (S, K)** Refining DP5: a `runtime_abi` epoch (recommended) or south going straight to 1.0; the IR line as
  exact equality on the kernel's published contract numbers (recommended), or a range once the kernel publishes what
  each contract increment added and the host refuses requests using newer additions; contracts made additive, or
  multi-version decoding (§8.2, §8.3, §8.6).
- **Q6 (S)** Does the eventstream deframer go into `south-contracts` (recommended; where grammars and fuzz targets
  live), or does each host implement its own (§5.2)?
- **Q7 (L, S)** Model catalog: a south data artifact (recommended), built into the components, or pure operator
  data; if it belongs to south, who keeps up with providers' new models (§7.5)?
  **Ruled for the host side (lv, 2026-09-30): a south data artifact.** Who maintains it remains open for the south
  maintainers.
- **Q8 (S)** Do the native reference implementations remain a supported production engine? If so, J3 needs the
  host to disable fallback explicitly (§8.5).
- **Q9 (S, community host)** Reference-implementation strictness is a behavior change for the community host
  (§6.5); in addition, ARCHITECTURE.md:114-115 requires a metering vocabulary to have "a second consumer in sight" —
  both `usage_evidence` and the recipes need the community host to confirm its intent to adopt them (P21 §7
  recommends implementing in step).
  **Ruled (lv, 2026-10-02): lv maintains the community host as well and confirms both halves** — the strictness
  change and adopting `usage_evidence` and recipes in step. The "second consumer in sight" test is met.
- **Q10 (L)** DP7: does south take in Copilot's editor headers, Claude Code's impersonation headers and Codex's
  client-identification headers? They would appear in both recipes and components.
  **Ruled (lv, 2026-09-30): south takes them in; the host keeps no special case.** Headers on the inference request
  are written by the component; headers on the exchange request are written by the credential recipe; names outside
  the compiled-in sets are declared per §10. The per-provider lists are specified in the component records
  (`2026-09-30-kiro-provider-component.md`, `2026-09-30-openai-responses-upstream-component.md`; Claude Code and
  Copilot to follow).
  Note (2026-10-01): the user-agent part of this ruling depends on Q15 and Q16, which the south maintainers may
  answer differently; if they decline, DP0 cannot be met for providers that need a client user-agent, and that
  conflict goes back to lv.
- **Q11 (S)** Must artifact signing be completed before third-party packages are loaded through the index (§9.3)?
  This record additionally makes signing a prerequisite for credential recipes in any package that is not a verified
  first-party release (§3.4 rule 5).
- **Q12 (L)** Is the estimate for `usage_evidence: absent` made by the host's generic estimator (this record's
  recommendation, by characters), or reported by the component as an estimate when it builds the request (the
  embeddings record takes the latter; see `2026-09-30-embeddings-contract.md` §7)? Chat goes through the kernel IR,
  which has no place for a component estimate; the embeddings contract is south-local and can hold one. Is it
  acceptable for the two worlds to follow different conventions?
  **Ruled (lv, 2026-09-30): the two conventions may coexist.** On the chat family the estimate for
  `usage_evidence: absent` is the host's generic estimator; in south-local contracts that can hold one, the component
  reports the estimate at build time. Both are labeled as estimates in the ledger.
  Note (2026-10-01): a component estimate may tighten only the reservation; every check compares against the host
  bound (§6.3).
- **Q13 (S, L)** The reasoning-token convention (§6.1 item 5, §6.2 items 1 and 6): confirm whether Gemini's
  `thoughtsTokenCount` lies outside `candidatesTokenCount`; if it does, the Gemini reference implementation's
  `output_tokens` must change, and the host's amounts charged by output change with it, which needs lv's
  confirmation and a dual run.
  Note (2026-10-01), measured at lv's request on Vertex AI (`global`, `:generateContent`, `thinkingBudget: 512`):
  `gemini-2.5-flash` prompt 32 / candidates 6 / thoughts 286 / total 324; `gemini-3.5-flash` 32 / 7 / 208 / 247;
  `gemini-2.5-pro` 32 / 7 / 323 / 362. In every case `total = prompt + candidates + thoughts`, and candidates count
  only the visible answer: **thoughts lie outside candidates**. The Gemini API (`generativelanguage`) was not enabled
  in the measuring project; the Vertex result is taken as the dialect's convention. The second half of the question
  turns out not to hold: the host already settles output as candidates + thoughts, so no amount changes and there is
  nothing left for lv to confirm. What remains is the south half — the reference implementation change in §6.2
  item 6.
- **Q14 (S, K)** The channel for values the component needs that the fence does not admit. S0 §6 and D5 forbid a
  component from behaving on an `extensions` key. Two cases:
  - **per provider**: credential attributes (§3.3) and `config_schema` keys the component itself reads (§7.3);
  - **per request**: a host-minted value such as the Kiro record's attempt id (P-4), and the Responses record's
    request extension keys (R-Q15).
  Recommended: typed fields through the kernel chain (D5's own promotion path) — for the per-provider case e.g.
  `ProviderConfig.declared: BTreeMap<String, String>` whose keys must be declared by the package and whose values
  the host validates; for the per-request case a typed `ChatRequest` field. Alternative: an explicit, argued
  amendment of the fence and D5 admitting one nested reserved key per type. Either way the host strips
  client-supplied keys that collide with a reserved name. Keys the compatible reference already reads are existing
  precedent, recorded as such by the Responses record, not a ruling on this question.
  **Kernel half done (2026-10-08): typed fields.** Protocol 0.5.0 carries `ProviderConfig.declared` (per provider and per
  attempt) and `ChatRequest.host_values` (per request), both as the validated `ComponentValues` map (keys of 1 to 64
  bytes of `[a-z0-9_]`, values of 1 to 4096 bytes of printable ASCII). The `extensions` fence is unchanged. South's half
  is the manifest vocabulary that declares and exports into them (§13.8).
- **Q15 (S)** Reopen the 2026-08-20 controlled-user-agent ruling: may a user-agent value come from a manifest value
  validated at gate ① (`DeclaredUserAgentV1`, §10) rather than only from host program text? Recommended: yes, with
  the value grammar unchanged and a fuzz obligation on the new parser.
  **Ruled (lv, 2026-10-02): as recommended.** A user-agent value may come from a manifest value validated at gate ①;
  the value grammar is unchanged and the new parser carries a fuzz obligation.
- **Q16 (S)** Does south accept publishing impersonation values (client user-agents and client-identification
  headers of third-party tools) inside its packages? lv's Q10 ruling requires it for DP0 on those providers; the
  south maintainers decide whether the repository carries them.
  **Ruled (lv, 2026-10-02): yes**, consistent with lv's Q10 ruling. South's packages may carry client
  user-agents and client-identification headers of third-party tools. The owner takes the terms-of-service risk
  of publishing them. B7a adds only the declaration mechanism; concrete values enter packages with the components
  that need them (B6).
- **Q17 (S)** Who runs gate ② for a package south did not build: nobody (it is the author's self-attestation, and
  the host relies only on its own seals and bounds), the installer (the host runs the suite at admission on the
  package's own fixtures, proving self-consistency only), or a registry run by south? Recommended: the installer, as
  a cheap self-consistency check, with the record stating plainly that it is not evidence of correct usage.
- **Q18 (S, L)** Recipe endpoint confirmation (§3.4 rule 1): operator confirmation per package digest (recommended
  as the default), a host-side allowlist, or both.
  **Ruled for the host side (lv, 2026-10-01): operator confirmation per package digest only; a host-side allowlist
  is rejected**, because a host allowlist means a new provider endpoint requires a host change, against DP0. The
  south maintainers' half remains open.
- **Q19 (S)** How does a recipe say what it presents after a branch that produces no output (Copilot's direct flow,
  §13.3)? Options: `present` takes ordered candidates, a step output or a secret field with its own validity; or a
  probe step that presents a field. Recommended: candidates (§13.5 D1), since they keep "what is presented" in one
  place and need no new step semantics.
  **Taken as recommended (2026-10-05) under the owner's standing rule**: the host stays vendor-neutral either way.
- **Q20 (S)** How does a multi-family package declare a credential kind for one family only? Options: an optional
  `families` scope on the section (recommended, §13.5 D2); a separate package per credential kind
  (`provider-github-copilot`); or a section per family. Recommended: the scope, because R6 already makes request-side
  declarations per family and it adds one field.
  **Taken as recommended (2026-10-05) under the owner's standing rule.** Flagged for the owner: a separate package would
  have needed no schema change, at the cost of a fourteenth package, its build and its release line; and with the
  family inside `provider-openai-compatible`, a host must re-pin before it can take any later version of that package.
- **Q21 (S)** Where does a static slot's value come from when a package declares `credentials` (host feedback SF5)?
  Recommended: the slot names a declared secret field, `{"field": name}`, and gate ① refuses a slot with no source
  where a section applies (§13.5 D3).
  **Taken as recommended (2026-10-05) under the owner's standing rule.**
- **Q22 (S)** Must gate ③ forbid a host from holding a credential after a transient failure (host feedback SF2)?
  Recommended: yes; a transient failure is retried on the next resolve of the same generation, which is what §13.3
  already states and the server already does (§13.5 D5). A host wanting to protect a token endpoint during an outage
  does it outside the credential's state, for example by bounding concurrent exchanges.
  **Taken as recommended (2026-10-05) under the owner's standing rule.**
- **Q23 (S, L)** What does a package's `south_runtime` declare: the release that carried it, or the oldest runtime it
  needs (host feedback SF10, host Q-S2-1)? Recommended: the oldest runtime it needs, proven by release CI loading the
  package under exactly that runtime; otherwise a release that only adds a package is refused by every host that has
  not re-pinned, and the S2 acceptance holds only on a synthetic release.
  **Ruled (lv, 2026-10-05): as recommended** (§13.6 SF10).
- **Q24 (S)** How does release CI prove a declared runtime? Options: (a) load the package under the declared runtime's
  release tag (gate ①, range handshake, import scan, identity probe); (b) also run that runtime's gate ② over the
  package's own fixtures, which would exercise its link layer (descriptor auth admission, request-facts sealing);
  (c) only compare manifest fields against a per-release field list. Recommended and implemented: (a), the check lv
  specified, run in a checkout of the tag so the judging loader is the old one. (b) is the closest sound extension and is
  recommended as a follow-up, but it makes the fixture file format a contract across releases (an older loader must read
  newer packs), so it needs its own record. **Taken as recommended under the owner's standing rule; flagged for the
  owner:** until (b) exists, "a package declares a runtime whose link layer accepts its descriptors" is held by review,
  not by CI.
- **Q25 (S)** May a package's `manifest.json` change under an unchanged version? §13.2 allowed it because
  `south_runtime` was re-stamped every release. Recommended: no; with Q23 an unchanged package keeps its manifest byte
  for byte, and the digest-stability check compares the manifest digest too. **Taken as recommended under the owner's
  standing rule.**
- **Q26 (S, L)** Should provider packages declare the link-layer contracts they use in `compatibility.contracts`, so a
  host can later widen its ceiling (host feedback SF11, host Q-S2-2)? Not done, because it is not additive: today
  `compatibility_admits` refuses any declared contract the host does not list, and every host lists only `task`, so the
  first provider package declaring one would be refused everywhere. It is also unclear what an author would declare: a
  descriptor is the kernel's shape, and the south contracts (`http`, `auth`, `reserved_header_policy`,
  `provider_quota_metadata`) govern host code paths that the package's manifest selects. Recommended, when a host needs
  to widen its ceiling: (1) gate ① derives the set mechanically from the manifest (for example `secret_headers` implies
  `auth` ≥ 5, `query_parameters` implies `http` ≥ 10) instead of authors hand-writing it, so it cannot drift; (2)
  `compatibility_admits` judges only the contracts a host lists, with hosts adding the provider contracts in the same
  release; (3) the south maintainers commit that a minor release never changes the meaning of an existing manifest field
  or contract version. Until then hosts keep the ceiling at the linked runtime (host S2 §2.3), which Q23 already makes
  workable. **Written up, not implemented; needs the south maintainers and the owner** for (3), which is a policy
  promise rather than code. Low urgency.
- **Q27 (S)** Should a multi-family package be instantiated once (host feedback SF12, host Q-S2-8)? Recommended: yes,
  `SandboxedComponentV1` shares an `Arc<LoadedComponentV1>`. **Taken as recommended; done** (§13.6).
- **Q28 (S)** Should gate ① refuse a signing package whose `signing.credentials` does not name declared secret fields,
  given that this refuses the published `provider-bedrock-converse` 1.0.7 under the next runtime (host feedback SF13)?
  Recommended: yes, unconditionally; it is what §5.4 promised, the only affected package is first-party and replaced by
  1.0.8 in the same release, and a host that would otherwise infer a field set from the scheme (its R32 path) holds
  provider knowledge. **Taken as recommended under the owner's standing rule; the host must take 1.0.8 when it
  re-pins.**
- **Q29 (S)** Converse's `accept` and `x-amzn-bedrock-accept`: descriptor headers, or a manifest declaration (host
  feedback SF14, host Q-S4-1)? Recommended: descriptor headers pinned by fixtures, for §13.5 D7's reason (non-secret
  constants, no safety property a declaration would add). **Ruled (lv, 2026-10-05): headers in the component, values
  equal to the host's native arm**; byte-identical upstream requests after migration.
- **Q30 (S, L)** How is Bedrock's Bearer API-key form served (host feedback SF16, host Q-S4-6)? Options: a second arm
  in the Converse package (refused: `host_signed` admits no other arm, 2026-08-27 manifest-schema record D2–D3); a
  sibling package declaring the same family `bedrock` (two admitted packages claiming one family are contested, §8.5,
  so a host could serve only one credential form at a time); a sibling package with its own family. Recommended: the
  sibling `provider-bedrock-converse-bearer` with the family `bedrock-bearer`, so a host serves both forms side by side
  and picks by the row's family, never by the stored credential's shape. **Ruled (lv, 2026-10-05): a sibling package;
  the family name is taken as recommended under the owner's standing rule.** Flagged for the host: its Claude-dialect
  handling keyed on the dialect name `bedrock` (S4 plan §4.2) must not special-case the name, or `bedrock-bearer` rows
  lose it.
- **Q31 (S)** How do the framing and signing gate ③ suites judge a host (host feedback SF15, host Q-S4-8)? Recommended:
  framing compares the host's fed bytes with south's canonical re-encoding over the same frames, split at every byte;
  signing recomputes SigV4 from the request the host signed and the headers it emitted, instead of comparing with one
  reference signer's bytes, so a host may choose any SignedHeaders set that covers the required headers. Both register
  as `not_verified` for both hosts. **Ruled (lv, 2026-10-05): add both suites; the judging method is taken as
  recommended.**
- **Q32 (S)** Should south offer the reverse of `fill_endpoint` (host feedback SF17, host Q-S4-3)? Recommended: yes,
  but it never guesses: it returns values only for a unique assignment that fills back to the same URL, and names why
  otherwise. **Taken as recommended; done.**
- **Q33 (S)** How does T21 cover the host's synthetic J2 tests of framing, the buffered path and signing (host feedback
  SF18, host Q-S4-9)? Recommended: §12 item 5's `t21-unseen-eventstream` guest, one guest whose `host_signed` and bearer
  behaviors follow what the host grants (no slot, no auth), with the manifest synthesized by the host as for T03.
  **Ruled (lv, 2026-10-05): add the modes; the one-guest shape is taken as recommended.**
- **Q34 (S)** May a host enable `signing` for a package it has not verified as first-party (host feedback SF19, host
  Q-S4-7)? Recommended: no, until package signing exists (§3.4, Q11), because the finalizer signs any request the
  component builds for the template's host. **Ruled (lv, 2026-10-05): as recommended**; recorded in §3.4.
- **Q35 (S, L)** May the combined Bearer-plus-header arm carry a header the package declares in `secret_headers`
  (§13.7)? Auth contract 5 has the combined arm only over the five sanctioned names, and the only consumer (Gemini's
  OpenAI-compatible surface) uses a sanctioned one. Recommended: no; refuse it at admission
  (`CombinedHeaderNotSanctioned`) until a package needs it, then add `ProviderAuthV1::BearerAndDeclaredHeaderSecret`
  (auth contract 6, additive) in the same change as that package. Adding a contract variant for a consumer that does not
  exist would be a guess about its shape. **Taken as recommended under the owner's standing rule; needs lv only if a
  consumer appears.**
- **Q36 (S)** Do the task worlds admit `bearer_and_header_secret` (§13.7)? Recommended: no. `TASK_AUTH_ARMS` is the old four
  words; the task-v2 world already admits only `bearer` and `header_secret` on the host path, and a task manifest
  declaring the new word is refused as an unknown word. **Taken as recommended.**
- **Q37 (S, L)** What does a package rebuilt against a new kernel contract declare as `south_runtime`? The first draft
  of this section recommended the release that first records the contract (0.46.0), with the workspace version moving in
  the same change. **Ruled (lv, 2026-10-08), in #151: the field stays 0.44.0 until the release step**, when every package's
  `south_runtime` and the workspace version move to 0.46.0 together; `scripts/check-declared-runtime.sh --build` is known to
  fail until then and is not worked around. The reason the recommendation is still the end state: a runtime that records the
  old contract refuses the package, so the oldest runtime that admits it is the first one that records the new contract.
  The consequence for the owner is unchanged: the stacked Q14 pull request and this one belong in one release.
- **Q38 (S)** Where does Gemini's OpenAI-compatible surface live (§13.7 item 6)? Options: a family in
  `provider-openai-compatible` (recommended: the translation is identical and only the auth and endpoint differ); a family
  in `provider-gemini` (refused: that package builds native URLs and bodies, and a second wire in one package makes
  `request_facts` and `stream_framing`, which are per package or per family, carry two shapes); a new package (refused:
  fifteen packages and a build, release line and fixture pack for one descriptor field). **Taken as recommended.**
- **Q39 (S, K)** Is the kernel's generic encoded-slash rule enough for #138, or should South keep refusing `%2F` for
  models it does not know to be ARNs (§13.7 item 7)? Recommended: enough. The rule keeps every descriptor at or below the
  endpoint whatever the upstream decodes, and the references already encode the model as one segment. South adds fixtures
  and no rule of its own. Remaining risk, recorded not fixed: a double-decoding upstream and a model id containing
  `%2e%2e`. **Taken as recommended; it relaxes a shared security check, so lv confirms it when the re-pin is reviewed.**
- **Q40 (S, L)** Should South's undeclarable secret-header list contain the kernel's whole never-credential list
  (§13.7 item 5)? The kernel's documentation says South's list refuses more, and it lacked five names. Recommended: yes, in
  both copies of the list, with a conformance test that pins the inclusion. It narrows what gate ① and
  `DeclaredSecretHeaderV1::parse` accept, but nothing could use the five names (the kernel refuses them in a descriptor),
  so the contract numbers stay. **Taken as recommended; flagged for the owner because it edits a contract-level list
  without a number bump.**

## 17. Amendments found while drafting the component records (2026-09-30) — change log

The amendments found while drafting `2026-09-30-kiro-provider-component.md`,
`2026-09-30-openai-responses-upstream-component.md` and `2026-09-30-north-codec-render-gaps.md` are now folded
into the body. Where each landed:

| # | Amendment | Now in |
|---|---|---|
| A1 | A response-side difference cannot be a dialect word; "Responses only" is a separate package | R6, §7.4, §11 |
| A2 | `south-north-codec` has no Responses wire types to share | §11 |
| A3 | `output_cap` and `stream` admit "none" (`[]`, `"none"`); the rule for families that cannot send the cap | §7.2, §6.3 |
| A4 | The user-agent value is a closed instance set too | §10, Q15, Q16 |
| A5 | Kiro's exchanges are `http_exchange` steps; `select` is an ordered rule list | §3.3, §3.9 |
| A6 | Default and clamp for expiry, fixed validity window, `jwt_exp` on a non-JWT, persisted attributes, ordered import pointers, rotation write-back target | §3.3, §3.5 |
| A7 | Non-streaming requests answered with an eventstream body | §5.2 |
| A8 | What `parse-response` returns for an `absent` package | §6.2 item 4 |
| A9 | Usage judges for providers without public documentation | §6.2 item 5 |
| A10 | An ARN value syntax; Kiro's profile ARN is a non-secret credential field, not a config key | §3.3, §7.3 |
| A11 | `response.incomplete` as a terminal: ruled (Responses R-Q2), settles only for a closed set of reasons | §11 |
| A12 | The Responses request is stateless; `previous_response_id` is refused on the host's northbound side | §11 |

## Revision note (2026-10-01)

- Header: host baseline moved to `a82c852b`; host line numbers re-derived (Kiro and Vertex mint implementations
  moved to :1302 / :1584; Kiro's region validation cited).
- §2: added R6 (request-side declarations per family, response-side per package) and R7 (gate ② is evidence only
  when someone other than the author runs it); R3 now names the manifest as untrusted input.
- §3: recipe trust model (§3.4: operator-confirmed endpoints, `aud` bound to destination, no constant `sub`, export
  only from non-secret fields, third-party recipes wait for signing, test endpoints only in test builds); host
  no-wipe invariant and previous-generation rollback (§3.5); A5, A6 and Kiro P-6 / P-8 folded in; the attribute
  channel became Q14.
- §4: the host must pass the slot in `ProviderConfig.auth`; the `Auth::OAuth` rule lands with B4; §4.3 names the
  OpenAI-compatible package as the one serving Gemini's OpenAI-compatible surface.
- §5: `stream_framing` is package-level; canonical re-encoding re-serializes compact JSON and covers `exception` and
  `error` frames; buffered path for always-streaming upstreams (A7); deframer placed in `south-contracts`; SigV4
  region tied to the endpoint template and credential fields mapped explicitly.
- §6: `usage_evidence` package-level with A8 / A9 folded in; the bound rule shared by every world (host-computed,
  component may only tighten); funds outcome of a hit; undetectable zone extended to cap and URL-model placement;
  trust statements limited to first-party packages; new §6.4 states once that `rejected` releases the reservation.
- §7: `output_cap: []`, `stream: "none"`, URL-form model checked against a `{model}` template; new per-family
  `endpoint` template so region and project reach the origin; `aws_arn` syntax; the config channel became Q14;
  response-side differences are separate packages.
- §8: removed the claimed kernel promise; IR line is exact equality on kernel contract numbers; digest stability
  checked in release CI; gate tightenings bind through `south_runtime_min`.
- §9: index carries package-level framing / usage fields, recipe presence and a gate ② report digest; signing also
  gates recipes.
- §10: user-agent added (`DeclaredUserAgentV1`); kernel catalog options spelled out; table of contract changes.
- §11: InvokeModel-Anthropic is a separate package; Responses and Kiro rows updated with A1, A2, A11, A12.
- §12: T21 split into three packages; `rogue-model-url` and the expected-to-pass `rogue-cap-twice` added; recipe
  counterexamples follow §3.4.
- §13: B7 split into B7a (south instance declarations, before B6) and B7b (kernel); B4 owns the OAuth admission
  rule.
- §14–§16: texts to revise extended (fence / D5, controlled-user-agent); rejected alternatives extended; Q5 tagged
  K; new Q14–Q18; conflicts with lv's Q10 ruling stated under Q10.
- §17: amendments folded into the body; kept as a change log.
- Round 2, §7.6: `RequestFactsHonoured` for `output_cap: []` defined once, as a mutation check (changing the IR cap
  leaves the built body byte-identical).
- Round 2, §3.3 / §3.9: the `id_token` export example removed (it broke §3.4); the Codex row now matches the
  Responses record §10.2 (stored `account_id` export, claim only as `must_equal_field`, `fixed_window` 3000 s,
  `write_back`).
- Round 2, §6.3: reservation uses `min(host bound, component bound)`; every check compares against the host bound
  only; each world may state a provider-agnostic instance of the host rule.
- Round 2, §3.3: one complete recipe vocabulary table listing every form the component records use, under one set
  of names (`write_back`, `requires`, `on_status` class keys, `select` predicates, import `seed` with
  `rfc3339_or_epoch_seconds`, field `default`, `endpoint_params`, `optional`, `require_one_of`, `must_equal_field`,
  `printable_ascii`, `aws_arn`).
- Round 2, §3.5 / §3.7: the TTL clamp (60 s to 24 h) is a host invariant; a recipe may only narrow it and gate ① no
  longer requires one; `write_back` is required whenever a recipe rotates. §3.5 retitled "Host invariants"; no
  section renumbered.
- Round 2, §7.3 / §11 / §17 A10: Kiro's profile ARN is a non-secret credential field with syntax `aws_arn`,
  exported as an attribute, not a `config_schema` key.
- Round 2, §11 / §17 A11–A12: `response.incomplete` is ruled (Responses R-Q2); `previous_response_id` is refused on
  the host's northbound side.
- Round 2, §14 / §5.2 / §6.2: the kernel `HttpResponseParts` comment and the WIT "never a zero" rule are listed as
  normative text to amend.
- Round 2, Q14: covers both the per-provider and the per-request case and names Responses R-Q15.
- Round 2, §5.2 / §15: `decode_sse_v1` placed in `south-contracts` beside the deframer; the rejected alternative is
  narrowed to "the host decodes a provider's stream for the component".
- Round 2, §13: B6 depends on B7a because Kiro needs a declared user-agent; the Responses component needs no new
  instance.
- Round 2, Q1 / Q3 / Q10 / Q12: the ruling paragraphs restored verbatim; additions moved into "Note (2026-10-01)".
- Round 2, §5.2: the buffered-path trigger pinned to `aws-eventstream` framing and `stream: "none"` together, with no
  content-type sniffing.
- Round 2, §6.4: `rejected` widened to "an upstream answer that proves nothing was produced (a 4xx, or a 2xx the
  component shows produced nothing)", classified by the component per dialect, matching image §9.2.
- Rulings of 2026-10-01:
  - Q18 ruled for the host side: operator confirmation per package digest only; the host-side allowlist is rejected
    under DP0. §3.4 rule 1 states the ruled mechanism; §15 lists the allowlist as a rejected alternative.
- 2026-10-05, host S2 / S4 prerequisites: new §13.6 (SF10–SF19) and §16 Q23–Q34. §8.6 states that a package declares the
  oldest runtime it needs; §3.4 rule 5 extends "wait for package signing" to `signing`; §5.4 records that the signing
  field check landed; §12 item 5 points to the implemented guest. §13.2's allowance for a manifest change under an
  unchanged version is withdrawn (Q25).
