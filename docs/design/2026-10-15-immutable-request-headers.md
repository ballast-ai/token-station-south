# Immutable request headers: a provider-world manifest declaration

Status: proposed (requested by lv on 2026-10-15, host question Q-CC3); implemented on branch `protected-request-headers`
for the 0.67.0 cycle, not yet released. Open points in §8.

Date: 2026-10-15

Baseline: south `origin/main` = `e1244c0` (after release 0.66.0). Host references carry a `server:` prefix and refer to
token-station-server as read on 2026-10-15. Statements marked **unverified** are conclusions from reading code, not
from running anything.

## 1. Problem

A host's model rows carry operator request extras (server P5 "附加配置"): body fields and request headers the host adds
to the upstream request. For body fields a package already says what an extra must not touch: the provider-world
manifest field `immutable_body_paths` (per family; the `OpenAI` Responses upstream record §3.4, D11; used by
`provider-openai-responses` for `store`, host rule R49, and by `provider-anthropic` for `system` on `claude-code`).
Request headers have no such declaration.

The host's native `claude_code` arm refuses every header extra, because an extra could replace the CLI identity the
upstream checks (`user-agent`, `x-app`, `anthropic-beta`, `anthropic-version`) or add a second credential
(`server:gateway/src/modules/inference/engine/request_extras.rs` `HEADER_EXTRAS_BLOCKED_TYPES`, keyed on the native
provider type). A `component` row has no native provider type, so that refusal vanishes when a Claude Code row moves to
`provider-anthropic` (Claude Code record `2026-10-15-claude-code-family.md` §6 D5, host prerequisite H5). The host's
generic floor does not cover it: `server:…/request_extras.rs` `validate_header` refuses south's reserved headers
(`SafeHeaders`: `authorization`, `x-api-key`, `user-agent`, …) and its own `HOST_MANAGED_HEADERS` (`accept`,
`anthropic-version`, `content-type`, `user-agent`, …) for every row, but not `anthropic-beta`, `x-app` or
`anthropic-dangerous-direct-browser-access`.

lv ruled on 2026-10-15 (Q-CC3) that south adds a generic, vendor-neutral declaration of protected request headers, which
the host enforces.

## 2. The declaration

A provider-world manifest field, the header counterpart of `immutable_body_paths`, with the same shape and placement:

```json
"immutable_request_headers": {
  "claude-code": ["accept", "anthropic-beta", "anthropic-dangerous-direct-browser-access", "anthropic-version",
                  "authorization", "content-type", "user-agent", "x-api-key", "x-app"],
  "claude-code-setup-token": ["accept", "anthropic-beta", "anthropic-dangerous-direct-browser-access",
                              "anthropic-version", "authorization", "content-type", "user-agent", "x-api-key", "x-app"]
}
```

`ComponentManifestV1::immutable_request_headers: BTreeMap<String, Vec<String>>`, `#[serde(default)]`, omitted when
empty.

| Property | Rule | Precedent |
|---|---|---|
| Keyed by | provider family; each key a family the manifest declares | `immutable_body_paths` |
| Worlds | provider world only (`WorldSchemaV1::request_declarations`); elsewhere gate ① refuses it as `DeliveryIsAProviderWorldDeclaration` | `immutable_body_paths`, `north_passthrough`, `subscription_quota_families` |
| Name syntax | 1 to 64 bytes of **lowercase** RFC 9110 `tchar`; uppercase is refused, not folded | `secret_headers` names (`validate_secret_header_name`) |
| Repeats | refused | `immutable_body_paths` |
| Count | at most 64 names per family (`MAX_IMMUTABLE_REQUEST_HEADERS`, the descriptor's ordinary header bound) | `MAX_IMMUTABLE_BODY_PATHS` = 64 |
| Reserved names | allowed (`authorization`, `x-api-key`, `user-agent`, …): protecting them is the point | — |
| Empty list | legal, says nothing | `immutable_body_paths` |
| Gate ① error | `ManifestErrorV1::InvalidImmutableRequestHeaders { family, detail }` | `InvalidImmutableBodyPaths` |
| Public helper | `are_immutable_request_headers(&[String]) -> bool`, constants `MAX_IMMUTABLE_REQUEST_HEADERS`, `MAX_IMMUTABLE_REQUEST_HEADER_NAME_BYTES` | `are_immutable_body_paths` |

Per family, not per package: the body-path precedent is per family, and families of one package differ (here
`anthropic` declares nothing, as native allowed header extras on `anthropic` rows).

## 3. Semantics

1. A host applies no operator request extra whose header name, compared **case-insensitively** (lowercase the extra's
   name, then compare bytes with the declaration), is declared for the row's family: it neither sets, replaces,
   appends to nor removes that header, whatever the extra's conflict mode.
2. A declared name the family's requests do not carry stays absent: an extra cannot add it. This is what keeps an
   extra from adding `authorization` beside a family that presents `x-api-key`, or `x-app` to the setup-token form.
3. **Nothing is protected implicitly by this declaration.** A family that wants its credential header, its
   `user-agent` or a header the host fills protected lists it. Reasons: (a) a host refuses an extra **when it is
   saved**, before any request descriptor exists, and which header the credential is presented in is decided by the
   component's code (`Auth::Header("x-api-key")` or Bearer), not by the manifest; only an explicit list lets the host
   decide at save time from the manifest alone; (b) `immutable_body_paths` has no implicit members either; (c) the
   credential presentation is in any case already guarded by a host-generic floor (south's reserved headers and the
   host's own managed list, §1), which this declaration does not replace and need not repeat, but may: listing a
   reserved name keeps the package's statement independent of any one host's or reserved-header policy version's
   floor.
4. The declaration changes nothing a component builds. It is a fact about the family, like
   `subscription_quota_families`. No gate ② check is added: unlike a body path, a protected header may be one the
   component never writes (the host writes `user-agent` and the credential header), so "present in every request or in
   none" is not a property the declaration claims. The package's own test pins that the list covers every header it
   writes (§6).

## 4. `provider-anthropic`

Both Claude Code families declare the same nine names (the reference constant
`reference_anthropic::claude_code::IMMUTABLE_REQUEST_HEADERS`): every header either form sends (`accept`,
`anthropic-beta`, `anthropic-version`, `content-type`, and for the login-token form `x-app` and
`anthropic-dangerous-direct-browser-access`), both credential presentations (`x-api-key`, `authorization`) and
`user-agent` (the manifest's CLI value for `claude-code`, the host's own for `claude-code-setup-token`). One list for
both forms, as one native refusal covered both (the `task-minimax-v2` precedent of one table for both generations,
record `2026-10-14-task-minimax-immutable-paths.md` §3). `anthropic` declares none.

**Difference from native, registered (PRH-D1):** native refuses **every** header extra on a `claude_code` row; the
component row refuses only the nine declared names, so an operator may still add an unrelated header (e.g.
`x-request-source`). **Unverified** whether Anthropic's subscription validator rejects unknown headers. See PRH-Q2.

## 5. Compatibility and versioning

- **No contract number changes.** The declaration is a manifest field; no WIT, IR or `contracts.*` number moves, as for
  `immutable_body_paths` (0.54.0) and `subscription_quota_families` (0.60.0).
- **Runtime declaration.** `ComponentManifestV1` is `#[serde(deny_unknown_fields)]`, so no runtime before the release
  that ships this field (0.67.0) parses `provider-anthropic`'s manifest. The package must declare `south_runtime`
  0.67.0. Following the 0.60.0 precedent (`2026-10-13-release-0.60.0.md` §3: the feature PR left the packages at their
  old runtime and the release commit raised them), this branch leaves `south_runtime` 0.60.0 and the **0.67.0 release
  commit raises it to 0.67.0** together with `shipped_packages_v1::declared_runtime_now`. That an older runtime refuses
  the manifest is read from code, not run. A host on an older south therefore fails closed: it refuses the package,
  it never admits it while ignoring the protection.
- **Q47.** `south-provider-api` changed: 0.58.0 → **0.59.0** (minor: a public field on `ComponentManifestV1`, a
  `ManifestErrorV1` variant, a function and two constants), with the requirements on it in `south-component-conformance`,
  `south-provider-conformance`, `south-provider-runtime` and `south-testkit`, every component lockfile, the
  synthetic-speech guest lockfile and `fuzz/Cargo.lock`. `south-component-conformance` changed (`reference_anthropic`:
  the constant and `metadata()` version) but is **not** bumped here: the cycle's conformance round (branch
  `speech-azure-openai`) moves it and gives every package its patch; this branch rebases on it.
  `scripts/check_crate_versions.py` fails until then. `south-contracts` is unchanged.
- **Packages.** `provider-anthropic` 1.2.0 (published with 0.66.0) → **1.2.1**, a patch: no request it builds changes.
  Every other package's `component.wasm` changes with the `south-provider-api` bump and takes its patch in the
  conformance round. On rebase `provider-anthropic` keeps one version for the cycle (1.2.1, or the round's value if
  higher).
- **Host re-pin.** A host matching `ManifestErrorV1` exhaustively adds the arm; `ComponentManifestV1` struct literals
  gain the field.

## 6. Tests

- `south-provider-api` `tests/delivery_declarations_v1.rs`: absent is omitted and changes nothing; a declared family
  round-trips; an undeclared family is refused by name; the grammar (lowercase `tchar`, uppercase / space / colon /
  CR LF / `(` / `/` refused, repeats refused, 64-byte and 64-name bounds at and past the limit); embeddings, task and
  image manifests refuse the field. `tests/provider_api_v2.rs`: the speech world refuses it.
- `south-component-conformance` `tests/claude_code_conformance_v1.rs`: the shipped lists equal the reference constant
  for both families and nothing for `anthropic`, the constant is sorted and valid; every header each family's requests
  carry (stream and not, with and without a system prompt), its credential header and `user-agent` are declared; the
  package's version is 1.2.1 and equals the reference's `metadata()`.
- `tests/shipped_packages_v1.rs`: the 0.66.0-cycle retirement test exempts `provider-anthropic` from its `now` pin.

## 7. Host checklist (token-station-server, after re-pinning to the release that ships 0.67.0)

1. **Read the declaration.** Add `component_immutable_request_headers(south_dialect)` beside
   `component_immutable_body_paths` (`server:…/inference/engine/request_extras.rs`), reading
   `manifest.immutable_request_headers.get(family)`; empty for a dialect not loaded, on the reference implementation, or
   undeclared.
2. **Save-time refusal.** Carry the list in the catalog scope (`server:gateway/src/modules/catalog/request_extras.rs`,
   beside `immutable_body_paths`); in the `ExtraPosition::Header` branch, after `validate_header` lowercases the name,
   refuse a name in the list with a message naming the package's declaration. Apply to every `component` row whose
   family declares the field, independent of provider type.
3. **Run-time skip.** Where header extras are merged into a component request (`merge_header_extras` /
   `apply_header_extras` for component rows), skip a declared name with a new `SkipReason` (e.g. `ImmutableHeader`),
   logged as the body path skip is, so stored extras that predate a re-pin or a dialect change cannot bypass it. Compare
   lowercased; skip regardless of `on_conflict`.
4. **Preflight.** The Claude Code cutover preflight (and any generic component-route preflight) FAILs when a row routed
   to a family with declared headers holds a stored header extra on a declared name, and WARNs on any header extra on a
   `claude-code` / `claude-code-setup-token` row if PRH-Q2 is ruled "refuse all"; each item with a reverse fixture.
5. **Keep the floor.** The generic refusal of reserved and host-managed headers stays for every row; the declaration
   adds to it. `HEADER_EXTRAS_BLOCKED_TYPES` stays for the native `ClaudeCode` type until that arm is deleted.
6. **Re-pin hygiene.** Accept `provider-anthropic` 1.2.1+ only with a runtime range reaching 0.67.0; add the
   `InvalidImmutableRequestHeaders` arm where `ManifestErrorV1` is matched; struct literals gain the field.
7. **Tests.** Save refusal (mixed case `X-App` refused), run-time skip of a stored extra, an undeclared header still
   injected, `anthropic` rows unaffected, and the dual run (Claude Code record H8) with a header extra on a declared name.

## 8. Open points

- **PRH-Q1** Explicit list, no implicit members (§3 item 3). **Recommended: yes**; the host floor covers credentials
  anyway.
- **PRH-Q2** Native refuses all header extras on `claude_code` rows; the component row refuses the nine names only
  (PRH-D1). Alternatives: (a) accept the difference (recommended: the declaration protects what the upstream checks;
  unrelated headers are the operator's choice); (b) a host-side rule refusing all header extras for families in
  `subscription_quota_families`; (c) a south wildcard form (e.g. `"*"`), which the body-path precedent does not have.
- **PRH-Q3** `anthropic` declares nothing (native allowed header extras there; `anthropic-version` and `x-api-key` are
  already in the host floor). **Recommended: leave it.**
- **PRH-Q4** Other packages with identity headers (Copilot's editor headers, Codex's `chatgpt-account-id`, Kiro) could
  declare the field in later changes; out of scope here.

## Revision note

- 2026-10-15: first draft with the implementation on `protected-request-headers` (not built or tested: builds were held
  on the machine; gates to be run before review).
