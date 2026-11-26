//! The family catalog data artifact, `south.family-catalog.v1`
//! (`docs/design/2026-11-24-family-catalog.md`).
//!
//! A South release publishes one family catalog beside its component packages (source
//! `catalogs/family-catalog.json`, published as `family-catalog-<tag>.json` and listed in the
//! release index with its SHA-256, the same plumbing as the model catalog). It gives each
//! provider family a package declares a display name, a description, an optional group and, for
//! task families, a modality, so a host can label its family pickers without keeping a
//! "family id → vendor name" table of its own.
//!
//! ```json
//! {
//!   "schema": "south.family-catalog.v1",
//!   "groups": [{ "id": "cloud", "display_name": { "zh-CN": "…", "en": "Cloud platforms" } }],
//!   "entries": [
//!     {
//!       "world": "task-adapter-v2",
//!       "family": "kling",
//!       "display_name": { "zh-CN": "…", "en": "Kling" },
//!       "description": { "zh-CN": "…", "en": "…" },
//!       "modality": "video",
//!       "credential_fields": { "access_key": { "zh-CN": "…", "en": "…" } }
//!     }
//!   ]
//! }
//! ```
//!
//! An entry is keyed by **(world, family)**: one family name appears in several worlds
//! (`openai-compatible` is a provider, image, speech and embeddings family), so the name alone
//! would collide.
//!
//! [`FamilyCatalogV1::parse`] judges the document on its own: exact fields everywhere, both
//! languages in every text, unique group ids and (world, family) keys, group references that
//! resolve, and a modality on exactly the `task-adapter-v2` entries. Whether the catalog matches
//! the packages of a release is a second judgement, [`FamilyCatalogV1::check_against`], fed with
//! the families each manifest declares ([`declared_families_v1`]): every family of a required
//! world has an entry, no entry names a family the release does not carry, and every
//! `config_keys` / `credential_fields` key is one the package declares for that family.
//!
//! The catalog is data, not a contract: it lives in this host-only crate, which no component links,
//! so changing it never changes a package's identity.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use thiserror::Error;

/// The schema id of the family catalog format this module reads.
///
/// A change a reader of this version could not read correctly (a renamed, removed or added field,
/// a new language requirement, a new modality) takes a new schema id. Data changes (families
/// added, renamed or re-described, groups added) keep the id and ship in an ordinary release.
pub const FAMILY_CATALOG_SCHEMA_V1: &str = "south.family-catalog.v1";

/// The languages every text in the catalog carries, in document order.
pub const FAMILY_CATALOG_LANGUAGES_V1: [&str; 2] = ["zh-CN", "en"];

/// The worlds whose every declared family must have a catalog entry. The other worlds (image,
/// speech, embeddings, task v1) may have entries; when they do, the same rules apply.
pub const FAMILY_CATALOG_REQUIRED_WORLDS_V1: [&str; 2] = ["provider-adapter-v2", "task-adapter-v2"];

/// The world whose entries must carry a modality, and the only one whose entries may.
pub const FAMILY_CATALOG_MODALITY_WORLD_V1: &str = "task-adapter-v2";

const TOP_LEVEL_FIELDS: [&str; 3] = ["schema", "groups", "entries"];
const GROUP_FIELDS: [&str; 2] = ["id", "display_name"];
const ENTRY_REQUIRED_FIELDS: [&str; 4] = ["world", "family", "display_name", "description"];
const ENTRY_OPTIONAL_FIELDS: [&str; 4] = ["group", "modality", "config_keys", "credential_fields"];

/// A refusal of a family catalog document ([`FamilyCatalogV1::parse`]).
///
/// Positions are zero-based indexes into `groups` and `entries`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum FamilyCatalogErrorV1 {
    /// The bytes are not JSON.
    #[error("the family catalog is not JSON: {reason}")]
    NotJson {
        /// The JSON parser's description of the problem.
        reason: String,
    },
    /// A field is missing, unknown, of the wrong type, or an empty string.
    #[error("the family catalog is not a south.family-catalog.v1 document: {reason}")]
    Malformed {
        /// Which field, and what is wrong with it.
        reason: String,
    },
    /// The `schema` field names another format.
    #[error("the family catalog schema is `{found}`, not `south.family-catalog.v1`")]
    UnknownSchema {
        /// The schema id the document declares.
        found: String,
    },
    /// Two groups share an id.
    #[error("family catalog group {group} repeats the id `{id}`")]
    DuplicateGroup {
        /// The later group's index.
        group: usize,
        /// The repeated id.
        id: String,
    },
    /// Two entries share a (world, family) key.
    #[error("family catalog entry {entry} repeats `{world}` / `{family}`")]
    DuplicateEntry {
        /// The later entry's index.
        entry: usize,
        /// The entry's world.
        world: String,
        /// The entry's family.
        family: String,
    },
    /// An entry's `group` names no group of the document.
    #[error("family catalog entry {entry} names the unknown group `{group}`")]
    UnknownGroup {
        /// The entry's index.
        entry: usize,
        /// The group id the entry names.
        group: String,
    },
    /// A `task-adapter-v2` entry has no `modality`.
    #[error("family catalog entry {entry} is a task-adapter-v2 family without a modality")]
    MissingModality {
        /// The entry's index.
        entry: usize,
    },
    /// An entry of another world has a `modality`.
    #[error(
        "family catalog entry {entry} has a modality, which only task-adapter-v2 entries carry"
    )]
    UnexpectedModality {
        /// The entry's index.
        entry: usize,
    },
}

/// A disagreement between a family catalog and the packages of a release
/// ([`FamilyCatalogV1::check_against`]).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum FamilyCatalogCoverageErrorV1 {
    /// A package of a required world declares a family the catalog has no entry for.
    #[error("the family catalog has no entry for `{world}` / `{family}`")]
    MissingEntry {
        /// The package's world.
        world: String,
        /// The declared family.
        family: String,
    },
    /// An entry names a (world, family) no package of the release declares.
    #[error("the family catalog describes `{world}` / `{family}`, which no package declares")]
    UndeclaredFamily {
        /// The entry's world.
        world: String,
        /// The entry's family.
        family: String,
    },
    /// An entry describes a `config_keys` key the package does not declare for the family.
    #[error(
        "the family catalog describes config key `{key}` of `{world}` / `{family}`, which the package does not declare"
    )]
    UndeclaredConfigKey {
        /// The entry's world.
        world: String,
        /// The entry's family.
        family: String,
        /// The described key.
        key: String,
    },
    /// An entry describes a `credential_fields` field the package does not declare for the family.
    #[error(
        "the family catalog describes credential field `{field}` of `{world}` / `{family}`, which the package does not declare"
    )]
    UndeclaredCredentialField {
        /// The entry's world.
        world: String,
        /// The entry's family.
        family: String,
        /// The described field.
        field: String,
    },
}

/// A text in both catalog languages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalizedTextV1 {
    zh_cn: String,
    en: String,
}

impl LocalizedTextV1 {
    /// The Simplified Chinese text (`zh-CN`).
    #[must_use]
    pub fn zh_cn(&self) -> &str {
        &self.zh_cn
    }

    /// The English text (`en`).
    #[must_use]
    pub fn en(&self) -> &str {
        &self.en
    }

    /// The text in `language` (`zh-CN` or `en`); `None` for any other language.
    #[must_use]
    pub fn get(&self, language: &str) -> Option<&str> {
        match language {
            "zh-CN" => Some(&self.zh_cn),
            "en" => Some(&self.en),
            _ => None,
        }
    }

    fn from_value(raw: &Value, at: &str) -> Result<Self, FamilyCatalogErrorV1> {
        let object = exact_fields(raw, &FAMILY_CATALOG_LANGUAGES_V1, &[], at)?;
        let text = |language: &str| non_empty_string(object, language, at);
        Ok(Self { zh_cn: text("zh-CN")?, en: text("en")? })
    }
}

/// A group families are filed under in a picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyGroupV1 {
    id: String,
    display_name: LocalizedTextV1,
}

impl FamilyGroupV1 {
    /// The group's id, which entries reference.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The group's display name.
    #[must_use]
    pub const fn display_name(&self) -> &LocalizedTextV1 {
        &self.display_name
    }
}

/// What a task family produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FamilyModalityV1 {
    /// The family produces video.
    Video,
    /// The family produces images.
    Image,
}

impl FamilyModalityV1 {
    /// The document form, `video` or `image`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Image => "image",
        }
    }
}

/// One family's display data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyEntryV1 {
    world: String,
    family: String,
    group: Option<String>,
    display_name: LocalizedTextV1,
    description: LocalizedTextV1,
    modality: Option<FamilyModalityV1>,
    config_keys: BTreeMap<String, LocalizedTextV1>,
    credential_fields: BTreeMap<String, LocalizedTextV1>,
}

impl FamilyEntryV1 {
    /// The world of the packages that declare the family, such as `provider-adapter-v2`.
    #[must_use]
    pub fn world(&self) -> &str {
        &self.world
    }

    /// The family id, as packages declare it in `providers`.
    #[must_use]
    pub fn family(&self) -> &str {
        &self.family
    }

    /// The id of the group the family is filed under, if any.
    #[must_use]
    pub fn group(&self) -> Option<&str> {
        self.group.as_deref()
    }

    /// The family's display name.
    #[must_use]
    pub const fn display_name(&self) -> &LocalizedTextV1 {
        &self.display_name
    }

    /// What the family is for and what a provider row of it needs.
    #[must_use]
    pub const fn description(&self) -> &LocalizedTextV1 {
        &self.description
    }

    /// What a task family produces; `None` outside `task-adapter-v2`.
    #[must_use]
    pub const fn modality(&self) -> Option<FamilyModalityV1> {
        self.modality
    }

    /// Descriptions of the package's `config_schema` keys for this family, by key.
    #[must_use]
    pub const fn config_keys(&self) -> &BTreeMap<String, LocalizedTextV1> {
        &self.config_keys
    }

    /// Descriptions of the package's `credentials.fields` for this family, by field.
    #[must_use]
    pub const fn credential_fields(&self) -> &BTreeMap<String, LocalizedTextV1> {
        &self.credential_fields
    }
}

/// One family as a package of a release declares it: the input of
/// [`FamilyCatalogV1::check_against`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredFamilyV1 {
    /// The package's world (`api_version`).
    pub world: String,
    /// The family, one of the package's `providers`.
    pub family: String,
    /// The `config_schema` keys the package declares for the family.
    pub config_keys: BTreeSet<String>,
    /// The `credentials.fields` that apply to the family: all of them when `credentials.families`
    /// is absent, none when it is present and does not list the family.
    pub credential_fields: BTreeSet<String>,
}

/// The families a component manifest declares, read from its JSON form.
///
/// Reads `api_version`, `providers`, `config_schema` (keyed by family) and `credentials`
/// (`fields`, optionally narrowed by `families`). It does not validate the manifest; a field of
/// the wrong shape is read as absent.
#[must_use]
pub fn declared_families_v1(manifest: &Value) -> Vec<DeclaredFamilyV1> {
    let Some(world) = manifest.get("api_version").and_then(Value::as_str) else {
        return Vec::new();
    };
    let credentials = manifest.get("credentials");
    let credential_families: Option<BTreeSet<&str>> = credentials
        .and_then(|credentials| credentials.get("families"))
        .and_then(Value::as_array)
        .map(|families| families.iter().filter_map(Value::as_str).collect());
    let credential_fields: BTreeSet<String> = credentials
        .and_then(|credentials| credentials.get("fields"))
        .and_then(Value::as_object)
        .map(|fields| fields.keys().cloned().collect())
        .unwrap_or_default();
    manifest
        .get("providers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|family| DeclaredFamilyV1 {
            world: world.to_owned(),
            family: family.to_owned(),
            config_keys: manifest
                .get("config_schema")
                .and_then(|schema| schema.get(family))
                .and_then(Value::as_object)
                .map(|keys| keys.keys().cloned().collect())
                .unwrap_or_default(),
            credential_fields: match &credential_families {
                Some(families) if !families.contains(family) => BTreeSet::new(),
                _ => credential_fields.clone(),
            },
        })
        .collect()
}

/// The config keys and credential fields declared for one (world, family).
type DeclaredKeys<'a> = (BTreeSet<&'a str>, BTreeSet<&'a str>);

/// A validated `south.family-catalog.v1` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyCatalogV1 {
    groups: Vec<FamilyGroupV1>,
    entries: Vec<FamilyEntryV1>,
}

impl FamilyCatalogV1 {
    /// Parses and validates a family catalog document.
    ///
    /// # Errors
    ///
    /// The first violation found, as listed on [`FamilyCatalogErrorV1`].
    pub fn parse(bytes: &[u8]) -> Result<Self, FamilyCatalogErrorV1> {
        let document: Value = serde_json::from_slice(bytes)
            .map_err(|error| FamilyCatalogErrorV1::NotJson { reason: error.to_string() })?;
        Self::from_value(&document)
    }

    /// Validates a family catalog document already parsed as JSON.
    ///
    /// # Errors
    ///
    /// The first violation found, as listed on [`FamilyCatalogErrorV1`].
    pub fn from_value(document: &Value) -> Result<Self, FamilyCatalogErrorV1> {
        let top = exact_fields(document, &TOP_LEVEL_FIELDS, &[], "the document")?;
        let schema = top
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("`schema` is not a string"))?;
        if schema != FAMILY_CATALOG_SCHEMA_V1 {
            return Err(FamilyCatalogErrorV1::UnknownSchema { found: schema.to_owned() });
        }

        let mut groups: Vec<FamilyGroupV1> = Vec::new();
        for (index, raw) in array(top, "groups", "the document")?.iter().enumerate() {
            let at = format!("group {index}");
            let group = exact_fields(raw, &GROUP_FIELDS, &[], &at)?;
            let id = non_empty_string(group, "id", &at)?;
            if groups.iter().any(|earlier| earlier.id == id) {
                return Err(FamilyCatalogErrorV1::DuplicateGroup { group: index, id });
            }
            let display_name = text(group, "display_name", &at)?;
            groups.push(FamilyGroupV1 { id, display_name });
        }

        let mut entries: Vec<FamilyEntryV1> = Vec::new();
        for (index, raw) in array(top, "entries", "the document")?.iter().enumerate() {
            let entry = Self::entry_from_value(index, raw, &groups)?;
            if entries
                .iter()
                .any(|earlier| earlier.world == entry.world && earlier.family == entry.family)
            {
                return Err(FamilyCatalogErrorV1::DuplicateEntry {
                    entry: index,
                    world: entry.world,
                    family: entry.family,
                });
            }
            entries.push(entry);
        }
        Ok(Self { groups, entries })
    }

    fn entry_from_value(
        index: usize,
        raw: &Value,
        groups: &[FamilyGroupV1],
    ) -> Result<FamilyEntryV1, FamilyCatalogErrorV1> {
        let at = format!("entry {index}");
        let entry = exact_fields(raw, &ENTRY_REQUIRED_FIELDS, &ENTRY_OPTIONAL_FIELDS, &at)?;
        let world = non_empty_string(entry, "world", &at)?;
        let family = non_empty_string(entry, "family", &at)?;
        let group = if entry.contains_key("group") {
            let group = non_empty_string(entry, "group", &at)?;
            if !groups.iter().any(|known| known.id == group) {
                return Err(FamilyCatalogErrorV1::UnknownGroup { entry: index, group });
            }
            Some(group)
        } else {
            None
        };
        let modality = match entry.get("modality") {
            None => None,
            Some(raw) => Some(match raw.as_str() {
                Some("video") => FamilyModalityV1::Video,
                Some("image") => FamilyModalityV1::Image,
                _ => return Err(malformed(&format!("{at} `modality` is not `video` or `image`"))),
            }),
        };
        match (world == FAMILY_CATALOG_MODALITY_WORLD_V1, modality) {
            (true, None) => return Err(FamilyCatalogErrorV1::MissingModality { entry: index }),
            (false, Some(_)) => {
                return Err(FamilyCatalogErrorV1::UnexpectedModality { entry: index });
            }
            _ => {}
        }
        Ok(FamilyEntryV1 {
            display_name: text(entry, "display_name", &at)?,
            description: text(entry, "description", &at)?,
            config_keys: text_map(entry, "config_keys", &at)?,
            credential_fields: text_map(entry, "credential_fields", &at)?,
            world,
            family,
            group,
            modality,
        })
    }

    /// The groups, in document order.
    #[must_use]
    pub fn groups(&self) -> &[FamilyGroupV1] {
        &self.groups
    }

    /// The group with `id`, if the catalog has one.
    #[must_use]
    pub fn group(&self, id: &str) -> Option<&FamilyGroupV1> {
        self.groups.iter().find(|group| group.id == id)
    }

    /// The entries, in document order.
    #[must_use]
    pub fn entries(&self) -> &[FamilyEntryV1] {
        &self.entries
    }

    /// The entry for `family` in `world`; `None` when the catalog does not describe it (a host
    /// then shows the family id itself).
    #[must_use]
    pub fn entry(&self, world: &str, family: &str) -> Option<&FamilyEntryV1> {
        self.entries.iter().find(|entry| entry.world == world && entry.family == family)
    }

    /// Checks the catalog against the families the packages of a release declare.
    ///
    /// Every disagreement is reported, in a stable order: missing entries (by world, then family),
    /// then entries no package declares, then undeclared keys, in entry order.
    ///
    /// # Errors
    ///
    /// Every disagreement found, as listed on [`FamilyCatalogCoverageErrorV1`].
    pub fn check_against(
        &self,
        declared: &[DeclaredFamilyV1],
    ) -> Result<(), Vec<FamilyCatalogCoverageErrorV1>> {
        let mut by_key: BTreeMap<(&str, &str), DeclaredKeys<'_>> = BTreeMap::new();
        for family in declared {
            let slot = by_key.entry((&family.world, &family.family)).or_default();
            slot.0.extend(family.config_keys.iter().map(String::as_str));
            slot.1.extend(family.credential_fields.iter().map(String::as_str));
        }
        let mut errors = Vec::new();
        for (world, family) in by_key.keys() {
            if FAMILY_CATALOG_REQUIRED_WORLDS_V1.contains(world)
                && self.entry(world, family).is_none()
            {
                errors.push(FamilyCatalogCoverageErrorV1::MissingEntry {
                    world: (*world).to_owned(),
                    family: (*family).to_owned(),
                });
            }
        }
        for entry in &self.entries {
            let Some((config_keys, credential_fields)) =
                by_key.get(&(entry.world.as_str(), entry.family.as_str()))
            else {
                errors.push(FamilyCatalogCoverageErrorV1::UndeclaredFamily {
                    world: entry.world.clone(),
                    family: entry.family.clone(),
                });
                continue;
            };
            for key in entry.config_keys.keys().filter(|key| !config_keys.contains(key.as_str())) {
                errors.push(FamilyCatalogCoverageErrorV1::UndeclaredConfigKey {
                    world: entry.world.clone(),
                    family: entry.family.clone(),
                    key: key.clone(),
                });
            }
            for field in entry
                .credential_fields
                .keys()
                .filter(|field| !credential_fields.contains(field.as_str()))
            {
                errors.push(FamilyCatalogCoverageErrorV1::UndeclaredCredentialField {
                    world: entry.world.clone(),
                    family: entry.family.clone(),
                    field: field.clone(),
                });
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }
}

fn malformed(reason: &str) -> FamilyCatalogErrorV1 {
    FamilyCatalogErrorV1::Malformed { reason: reason.to_owned() }
}

/// `raw` as an object whose keys are all of `required` and any of `optional`, nothing else.
fn exact_fields<'a>(
    raw: &'a Value,
    required: &[&str],
    optional: &[&str],
    at: &str,
) -> Result<&'a Map<String, Value>, FamilyCatalogErrorV1> {
    let object = raw.as_object().ok_or_else(|| malformed(&format!("{at} is not a JSON object")))?;
    if let Some(missing) = required.iter().find(|field| !object.contains_key(**field)) {
        return Err(malformed(&format!("{at} has no `{missing}`")));
    }
    if let Some(unknown) = object
        .keys()
        .find(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(malformed(&format!("{at} has the unknown field `{unknown}`")));
    }
    Ok(object)
}

fn non_empty_string(
    object: &Map<String, Value>,
    field: &str,
    at: &str,
) -> Result<String, FamilyCatalogErrorV1> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| malformed(&format!("{at} `{field}` is not a non-empty string")))
}

fn array<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    at: &str,
) -> Result<&'a Vec<Value>, FamilyCatalogErrorV1> {
    object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| malformed(&format!("{at} `{field}` is not an array")))
}

fn text(
    object: &Map<String, Value>,
    field: &str,
    at: &str,
) -> Result<LocalizedTextV1, FamilyCatalogErrorV1> {
    let raw = object.get(field).ok_or_else(|| malformed(&format!("{at} has no `{field}`")))?;
    LocalizedTextV1::from_value(raw, &format!("{at} `{field}`"))
}

fn text_map(
    object: &Map<String, Value>,
    field: &str,
    at: &str,
) -> Result<BTreeMap<String, LocalizedTextV1>, FamilyCatalogErrorV1> {
    let Some(raw) = object.get(field) else {
        return Ok(BTreeMap::new());
    };
    let map = raw
        .as_object()
        .ok_or_else(|| malformed(&format!("{at} `{field}` is not a JSON object")))?;
    map.iter()
        .map(|(key, value)| {
            if key.trim().is_empty() {
                return Err(malformed(&format!("{at} `{field}` has an empty key")));
            }
            Ok((
                key.clone(),
                LocalizedTextV1::from_value(value, &format!("{at} `{field}`.`{key}`"))?,
            ))
        })
        .collect()
}
