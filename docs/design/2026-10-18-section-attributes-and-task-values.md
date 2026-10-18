# Section attributes and the task-v2 value channel

Status: accepted (SA-Q1 to SA-Q5 ruled by lv on 2026-10-18 as recommended; §8). Implemented (#272); released in 0.74.0. Requested by lv on 2026-10-18: the host's credential fixed-column record, CF-Q1 = A, relayed by
token-station-server-14 and confirmed by the user. Rulings in §8.

Date: 2026-10-18

Baseline: south `origin/main` = `ce9c148e` (after release 0.73.1 and #271). Host references carry a `server:` prefix
and refer to the host's credential fixed-column plan under `server:docs/product-review-v2/plans/` (dated 2026-10-18;
§1.8, CF5, §4 S1–S3), on server branch `trial-fix` `a452d663`. Statements marked **unverified** come from reading code,
not from running anything.

## 1. Problem

The host's credential table still has one vendor-shaped fixed column with a live reader: `account_id`. The GMI image
task sends `x-organization-id` from it. The host copies the credential's `account_id` into the task component's
`ProviderConfig.extensions["organization_id"]` at submit and again when it resumes a task (`server:` record §1.4).
`task-gmi-image-v2` reads that extension (`reference_gmi_image_task_v2.rs:167-180`).

lv ruled that the value becomes a field the package declares and that travels with the credential, like
`provider-vertex`'s `project_id` or `openai-codex`'s `account_id` (CF-Q1 = A). Two south rules stand in the way:

- **R1.** A credential attribute exists only under a recipe (`RecipeV1::attributes`). A recipe has one to four steps
  (`credentials.rs` `check_recipe`), so a package whose slot is a plain stored secret (`SlotV1::Field`) cannot
  export anything. `exported_attributes_v1` returns nothing for a slot that is not minted.
- **R2.** Gate ① refuses credential attributes in a world without `value_channel` (`values.rs:98-110`).
  `TASK_WORLD_SCHEMA_V2` has `value_channel: false`, because no task host built `ProviderConfig.declared`.

## 2. Change S1: section-level attributes

`CredentialsV1` gains `attributes: BTreeMap<String, AttributeV1>`, `#[serde(default)]`, omitted when empty. These are
the attributes the section exports whatever the slot's source, from a stored field and without any exchange:

```json
"credentials": {
  "schema": "south.credential-recipe.v1",
  "fields": {
    "api_key": {"secret": true, "required": true, "description": "The GMI Cloud API key."},
    "organization_id": {"secret": false, "required": false, "syntax": {"printable_ascii": 128},
                        "description": "The GMI Cloud organization id, sent as X-Organization-ID when set."}
  },
  "slots": {"provider_api_key": {"field": "api_key"}},
  "attributes": {"organization_id": {"field": "organization_id", "export": true}}
}
```

Rules (gate ①), the same as a complete recipe's attributes (`check_attributes`, §3.4 rule 4, §13.8):

1. Each name is lowercase snake_case, with `export: true`.
2. The source is exactly one `field`: a declared non-secret field with a value syntax. `selected_recipe` is refused,
   because there is no selector at section level.
3. `persist` is allowed, with the same meaning: the host keeps the last value and uses it when the field is later
   absent.
4. A section attribute's name may not also be an attribute of any recipe in the section. Otherwise two sources could
   answer one key.

Semantics:

- `CredentialsV1::attribute_names` and `admits_attribute` include section attributes, so `declared_keys`,
  `declared_values` and the config-key collision check (`values.rs:120-133`) cover them unchanged.
- `exported_attributes_v1(credentials, slot, fields, persisted)` returns the section attributes for every slot,
  `Field` and `Static` included, plus the minted slot's recipe attributes as today. Same value rules: a present field
  value, its `default`, or the persisted value.
- Nothing about slots changes: S1 adds no way to mint, and no zero-step recipe. The alternative the host record names
  (a zero-step recipe presenting a field) would dress a stored secret up as a minted one (host record §4 S1). It is
  not taken.

## 3. Change S2: task-v2 builds `declared`

`TASK_WORLD_SCHEMA_V2.value_channel` becomes `true`. The `task-adapter-v2` WIT already passes `provider-config` as
`json`, so the WIT does not change. Gate ① then admits, in task-v2:

- credential attributes, from a recipe or the section (S1); and
- a family's `config_schema`, since `value_channel` means both: a host that builds `declared` builds it from config
  values and attributes alike, through `declared_values`.

A task-v2 host then builds `ProviderConfig.declared` per call with `declared_values`, as the image, speech and
embeddings hosts do, for every call a task makes: submit, observe, render and fetch. A task resumed after a restart
gets the attributes recomputed from the selected credential with the same function. `TASK_WORLD_SCHEMA` (task v1)
stays `false`.

Whether `config_schema` should open with it is SA-Q2. The host record needs only attributes, but
splitting the flag would give `value_channel` two meanings.

## 4. Change S3: `task-gmi-image-v2` declares its credential

- The manifest gains the §2 `credentials` section, applying to `gmi-image`.
- The component reads `ProviderConfig.declared["organization_id"]`. When it is absent it falls back to
  `extensions["organization_id"]`, so a host that has re-pinned but not yet migrated keeps sending the header (SA-Q3).
  Either way an empty value sends no header, as today.
- The fixtures `task-v2.{prepare,observe}.organization-header.*` take the value through `declared`. One more pair keeps
  the extension fallback until SA-Q3 retires it.
- Version: 0.35.x → **0.36.0** (it now declares a section and reads a new input).
- `south_runtime` is raised in the release commit, as for the other declaring packages (§6). A runtime that still
  refuses task-v2 attributes therefore refuses the package, so a host on an older south fails closed.

## 5. What does not change

- No WIT, IR or `contracts.*` number moves. `ProviderConfig.declared` already exists in the kernel IR the task world
  uses, and the task contract stays 7 for this package. **Unverified:** whether a task-7 host that ignores `declared`
  should be refused by a contract bump instead of by the runtime floor. Recommended: the runtime floor is enough (SA-Q4).
- Recipes, slots, seeds, imports and the credential recipe suite are unchanged. The suite gains cases for section
  attributes (§7).

## 6. Compatibility and versioning

- **Q47.** `south-provider-api` changes (a public field on `CredentialsV1`, the task-v2 schema constant). It is
  already 0.62.0 on the unreleased branch #272, so this is the same minor if both ship in 0.74.0.
  `south-component-conformance` changes too (`exported_attributes_v1`, the GMI reference). If both land in one release,
  every package re-identifies once.
- **Runtime.** `CredentialsV1` is `deny_unknown_fields`, so an older runtime refuses a package with section
  attributes, and an older runtime refuses task-v2 attributes at gate ①. `task-gmi-image-v2` raises `south_runtime`
  in the release commit.
- **Release.** With #272, in 0.74.0 (the user's choice on 2026-10-18).

## 7. Tests (written first)

- `south-provider-api`:
  - Section attributes round-trip and are omitted when absent.
  - Each rule of §2 is refused by name: a secret field, a field without syntax, `selected_recipe`, `export: false`, a
    bad name, and a name shared with a recipe attribute.
  - `attribute_names` and `admits_attribute` include section attributes.
  - Task-v2 admits attributes and `config_schema`; task v1 still refuses both.
- `south-component-conformance`:
  - `exported_attributes_v1` returns section attributes for a `Field`, a `Static` and a `Minted` slot, with `default`
    and `persist` behaving as for recipe attributes.
  - The GMI reference sends `x-organization-id` from `declared`, falls back to the extension, and sends none when the
    value is empty.
  - The shipped GMI manifest equals the §2 section.
- `south-provider-conformance`: the credential recipe suite gains section-attribute cases (a stored field exported,
  absent then persisted).

## 8. Rulings

- **SA-Q1** **Ruled as recommended (lv, 2026-10-18).** S1 shape: a section-level `attributes` (recommended), or a zero-step recipe presenting a field. The host
  record also recommends the section level.
- **SA-Q2** **Ruled as recommended (lv, 2026-10-18).** Should task-v2 `value_channel` open `config_schema` too? Recommended: yes, one flag with one meaning; no
  task package declares a config key until it needs one.
- **SA-Q3** **Ruled as recommended (lv, 2026-10-18).** Should the GMI component fall back to `extensions.organization_id`? Recommended: yes, for 0.74.0, and
  drop the fallback in a later release once the host's CF5 has landed and stops writing the extension.
- **SA-Q4** **Ruled as recommended (lv, 2026-10-18).** Leave the task contract at 7 and rely on the runtime floor? Recommended: yes.
- **SA-Q5** **Ruled as recommended (lv, 2026-10-18).** The syntax of `organization_id`: `printable_ascii` of 128 bytes (recommended, since the value format is
  not documented), or `token`.

## Revision note

- 2026-10-18: first draft. Not implemented, built or tested.
- 2026-10-18: lv ruled SA-Q1 to SA-Q5 as recommended and agreed to ship with #272 in 0.74.0 (relayed by
  token-station-server-14, confirmed by the user). Status accepted.
