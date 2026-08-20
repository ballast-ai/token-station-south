# Provider API Promotion (S1)

Status: D1–D5 ruled 2026-08-21 (D1 ruled explicitly, as recommended; D2–D5
follow the S0 contract freeze); shipped as 0.8.0.

Date: 2026-08-21

Predecessors: `2026-08-21-canonical-ir-inventory.md` (S0 — the frozen boundary
contract this crate implements: ABI envelope §3, compatibility tuple §4,
promotion criteria §9), the community host's `crates/plugin-api` (donor:
`token-station:adapter@1.0.0`, both adapter roles, 1,036 lines).

## 1. Problem

S0 froze what crosses the component boundary; nothing yet owned it. The donor
ABI lives in the community host and carries both adapter roles; the southbound
role must be ownable by this repository so one component serves every adopting
host, and the S0 rulings (bytes chunks, seven-field tuple) are breaking against
the donor's v1 world. `south-provider-api` was a three-line placeholder.

## 2. What ships

- **`wit/provider-adapter.wit`** — package `token-station:adapter@2.0.0`,
  single world `provider-adapter-v2`: `common` (metadata/health), `host`
  (`sign` only — names a credential, never reads one), `provider-adapter`
  (metadata, healthcheck, model-capabilities, build-http-request,
  parse-response, parse-stream-chunk, map-provider-error). JSON payloads named
  by canonical type, exactly as in v1; the chunk entry point takes
  `list<u8>` (S0 D2). No `wasi:filesystem`, no `wasi:sockets`.
- **`ComponentManifestV1`** — the gate-① schema: identity (kebab name, semver
  triple, `api_version == provider-adapter-v2`), sandbox (network/filesystem
  refused with named reasons; secrets are reference names, never credentials),
  role (chat capability required; ≥1 kebab provider family; declared
  `auth_arms` ⊆ {bearer, header_secret, oauth}), conformance
  (`required_suite == south.provider-component.v1`, safe relative fixtures
  path), and the **compatibility declaration** completing the S0 seven-field
  tuple (`ir_schema_id`, kernel version + 40-hex revision, `wit_package`,
  `south_runtime`; `version`/`api_version`/`required_suite` are the other
  three). `deny_unknown_fields` throughout: a misspelt key fails loudly and a
  new field arrives through a schema bump. `compatibility_tuple()` assembles
  the borrowed seven-field view for the runtime handshake.
- **The signing act** — the reference component's manifest
  (`provider-openai-compatible`, as it will ship: bearer + header-secret arms,
  `provider_api_key` slot, `token-station-protocol@0.3.0/v0.2.0` at kernel
  `0.2.0` = `72458e3a…`) validates clean, round-trips through JSON, and its
  tuple reads back exactly (test
  `the_reference_component_manifest_signs_the_contract`).

## 3. Decisions — ruled 2026-08-21

- **D1 — package identity: stay in the `token-station:adapter` lineage,
  bump to `@2.0.0`, world `provider-adapter-v2`** (ruled explicitly). The S0
  tuple text already names `token-station:adapter@x.y.z`; the S4 transition
  ("old southbound ABI two minors, then removed") reads naturally as v1→v2
  inside one package family; a fresh package name would make the old↔new
  correspondence implicit. The community host keeps `@1.0.0` (both worlds)
  until its southbound half retires.
- **D2 — v2 surface deltas from the donor**: provider role only (the agent
  world stays community-owned); `adapter-metadata` drops the `kind` field
  (one-role package; the world name already is the discriminator);
  `parse-stream-chunk` takes `list<u8>` per S0 D2. Everything else keeps the
  v1 shapes and doc-comment obligations, updated to name the S0 rulings
  (usage-as-evidence on `parse-response`, per-report usage events on the
  chunk path, `sign` ≠ SigV4).
- **D3 — the gate-② suite name is frozen now**: `south.provider-component.v1`,
  following the existing `south.<object>.v1` naming. Freezing it lets the
  manifest validate exactly instead of carrying a free string until S2; S2
  must build the suite under this identifier.
- **D4 — "signs the contract" at S1 is manifest-level.** The reference
  component's shipping manifest validating against the schema and the WIT
  parsing with the frozen shapes is what S1 can honestly certify; behavioral
  signature (typed decode, fixtures, determinism) is gate ②'s job and lands
  with S2's native reference implementation. The plan's S1 verify is read
  accordingly.
- **D5 — version: 0.8.0** (per the renumbered allocation: 0.7.0 host-prelude,
  0.8.0 provider-api, 0.9.0 host-signed finalizer).

## 4. Obligations

- WIT structure is pinned by tests through `wit-parser` (dev-dependency,
  matching the donor's tooling): package name, sole world, sandbox posture,
  `host` import present, chunk parameter is `list<u8>`. The constants the
  manifest validates against and the WIT source cannot drift apart unnoticed.
- Manifest validation carries the donor's full adversarial table (credential
  pasted as a secret name, network/filesystem requests, path escape, identity
  before role) plus the tuple's field-by-field shape checks.
- No conformance suite changes; no new fuzz targets (the manifest parses
  through serde with `deny_unknown_fields`, no hand-rolled grammar beyond the
  name/path validators ported verbatim from the donor).
- This crate depends on no other south crate and no IR crate — enforced by
  review and the boundary gate's dependency expectations; the S3 runtime
  inherits the same rule with CI teeth (plan risk table).

## 5. Versioning

`south-provider-api` leaves placeholder status; workspace ships as **0.8.0**.
`compatibility.json` records `provider_api.wit_version =
"token-station:adapter@2.0.0"` and the crate descriptor
`provider_adapter_v2_wit_manifest_v1`. Hosts are unaffected until S4/S5 —
nothing consumes the WIT yet; the crate exists so S2 can build gates ①② against
it and S3 can hand it to `wit-bindgen`.
