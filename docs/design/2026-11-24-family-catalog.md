# The family catalog, `south.family-catalog.v1`

Status: **implemented** on branch `family-catalog` (issue #354); not yet released.

Date: 2026-11-24

Baseline: south `origin/main` = `4f900910` (release 0.85.1 plus #348 and #350).

## 1. Problem

A host's provider form offers two family pickers: the text family of a provider row (`south_dialect`, a
`provider-adapter-v2` family) and the row-level video family (`task_family`, a `task-adapter-v2` family). It
can only show family ids (`glm-coding`, `azure-openai-dated`, `vertex-veo-video`), which operators cannot read.
The host must not keep an "id → vendor name" table of its own: that is vendor knowledge, which the host-zero-vendor
boundary (`2026-09-30-host-zero-vendor-boundary.md`) assigns to South.

A second gap: a `task-adapter-v2` package does not say whether a family produces video or images. The row-level
task family serves video models only, but its candidates include `wan-image` and `gmi-image`, and a host cannot
tell them apart without guessing from the name.

## 2. Why data, not the manifest

`ComponentManifestV1` lives in `south-provider-api`, which every component links. A new manifest field would move
that crate's version and every `component.wasm`: all packages re-identified for display data. The model catalog
(boundary record §7.5, §13.11) set the precedent this record follows: South publishes a data file beside the
packages, lists it in the release index under `catalogs`, and a host loads it as data. Changing the data never
re-releases a package.

## 3. Format

Source `catalogs/family-catalog.json`, published verbatim as `family-catalog-<tag>.json`:

```json
{
  "schema": "south.family-catalog.v1",
  "groups": [{ "id": "cloud", "display_name": { "zh-CN": "…", "en": "Cloud platforms" } }],
  "entries": [
    {
      "world": "provider-adapter-v2",
      "family": "bedrock",
      "group": "cloud",
      "display_name": { "zh-CN": "…", "en": "AWS Bedrock · Converse" },
      "description": { "zh-CN": "…", "en": "…" },
      "config_keys": { "region": { "zh-CN": "…", "en": "…" } },
      "credential_fields": { "access_key_id": { "zh-CN": "…", "en": "…" } }
    },
    {
      "world": "task-adapter-v2",
      "family": "wan-image",
      "display_name": { "zh-CN": "…", "en": "Wan image" },
      "description": { "zh-CN": "…", "en": "…" },
      "modality": "image"
    }
  ]
}
```

- **Key: (world, family).** One family name appears in several worlds (`openai-compatible` is a provider, image,
  speech and embeddings family; `kling` is a task v1 and v2 family), so the name alone would collide.
- **Texts** (`display_name`, `description`, group `display_name`, every `config_keys` / `credential_fields`
  value) are objects with exactly `zh-CN` and `en`, both non-empty.
- **`group`** is optional and names one of `groups`. Groups and their names are data: adding one keeps the schema.
- **`modality`** (`video` or `image`) is required on `task-adapter-v2` entries and refused on every other world.
- **`config_keys`** and **`credential_fields`** are optional; they describe a package's `config_schema` keys and
  `credentials.fields` for the family in both languages, since the manifest's own descriptions are English only.
- Unknown fields are refused at every level; so is an unknown schema id.

What changes the schema id (`south.family-catalog.v2`): a field renamed, removed or added, a new required language,
a new modality value. Data changes (families added, renamed or re-described, groups added) keep it.

## 4. Coverage

A catalog must agree with the packages of the release it ships in:

1. every family a `provider-adapter-v2` or `task-adapter-v2` package declares has an entry (the image, speech,
   embeddings and task v1 worlds are optional in this version; an entry for one is checked like any other);
2. no entry names a (world, family) that no package of the release declares;
3. every `config_keys` key is in the declaring package's `config_schema` for that family, and every
   `credential_fields` field is in its `credentials.fields`, narrowed by `credentials.families` when present.

## 5. Where it lives

`south_core::FamilyCatalogV1` (`crates/south-core/src/family_catalog.rs`): `parse(bytes)` / `from_value(&Value)`
judge the document (§3); `entry(world, family)`, `group(id)`, `groups()` and `entries()` read it;
`check_against(&[DeclaredFamilyV1])` judges coverage (§4) and returns every disagreement, and
`declared_families_v1(&manifest)` derives its input from a manifest's JSON.

`south-core`, not `south-provider-runtime` (where `ModelCatalogV1` lives), because a host build without the
component runtime still renders provider forms (the server host's configuration-center build does not compile
`south-provider-runtime`). `south-core` is host-only: no component links it (`check-boundaries.sh`), so the new
module and its `serde_json` dependency re-identify no package.

## 6. Release plumbing

The model catalog's path, unchanged: `release.yml` copies every `catalogs/*.json` into `dist/` as
`<name>-<tag>.json`; `release_index.py generate` checks it is verbatim and lists it under `catalogs` with its
digest; `compare` refuses a later release that drops the schema unless it is retired.

`release_index.py` adds `south.family-catalog.v1` to `KNOWN_CATALOG_SCHEMAS` and, at `generate`, checks the
published family catalog against the release's manifests (`family_catalog_violations`: §4 plus the group and
modality references). A disagreement fails the release. The full shape check stays with the Rust reader, which the
test suite runs over the same source file (`crates/south-core/tests/family_catalog_v1.rs`), as for the model
catalog.

The catalog is the one data file allowed to carry CJK: `scripts/check-language.sh` lists
`catalogs/family-catalog.json` in its data allowlist, because the format requires a `zh-CN` text everywhere. Code,
tests and this record stay English.

## 7. Initial data

The 20 provider families and 11 task v2 families of release 0.85.1, filed under four groups: generic protocols,
cloud platforms, subscription accounts, coding plans (task families take no group). Names follow the draft in issue
#354, checked against each package's manifest and its upstream notes (`docs/upstreams/`); descriptions say what the
family calls and what credential a row needs. `config_keys` and `credential_fields` translate the manifests'
English descriptions for every key and field these families declare.

## 8. Tests

- `crates/south-core/tests/family_catalog_v1.rs`: the shipped catalog parses and agrees with every
  `components/*/manifest.json`; each §3 rule and each §4 rule has a reverse case.
- `scripts/test_release_index.py` (`FamilyCatalogTest`): `generate` lists a matching catalog and refuses a missing
  entry, an undeclared family, config key and credential field, a bad group reference and a misplaced modality; the
  repository's catalog matches the repository's manifests.

## 8a. Speech families (2026-11-25)

The 13 `speech-adapter-v1` families now have entries too (still optional in v1, so a speech family added later
without one does not fail the release). They add a `vendor-api` group for upstreams reached through the vendor's own
API. The server host shows them in the model page's speech binding picker. Data only: no package changes identity.

## 9. Not done

- The image and embeddings families have no entries yet (optional in v1).
- The scheduled fuzz target `contract_parsers` does not cover `FamilyCatalogV1::parse`.
- Host adoption (loading the file, the pickers) is the host's work.
