//! The model catalog data artifact, `south.model-catalog.v1`
//! (`docs/design/2026-09-30-host-zero-vendor-boundary.md` §7.5 and §13.11).
//!
//! A South release publishes one catalog document beside its component packages (source
//! `catalogs/model-catalog.json`, published as `model-catalog-<tag>.json` and listed in the release
//! index with its SHA-256). It describes models by **upstream model id**, not by provider family:
//! an entry is a list of match rules and a `capabilities` object.
//!
//! ```json
//! {
//!   "schema": "south.model-catalog.v1",
//!   "entries": [
//!     { "match": [{ "contains": "seedance-2-0-fast" }], "capabilities": { "params": {} } },
//!     { "match": [{ "exact": "gpt-image-2" }, { "prefix": "seedream-4-0" }], "capabilities": {} }
//!   ]
//! }
//! ```
//!
//! **What South owns is the document's shape**, and [`ModelCatalogV1::parse`] refuses any
//! document that breaks it: bytes that are not a JSON object, an unknown schema id, a missing or
//! unknown top-level or entry field, an entry without a match rule, a match rule that is not
//! exactly one of `exact` / `prefix` / `contains` with a non-empty string, a rule that repeats an
//! earlier one (the later copy could never decide a lookup), and a `capabilities` value that is not
//! a JSON object.
//!
//! **What South does not own is the meaning of `capabilities`.** Its vocabulary (parameter ranges,
//! media roles, features, constraints) is the host's admission policy, validated and enforced by
//! the host; South carries it as an opaque JSON object. A host-side vocabulary change therefore
//! needs no South contract change, and South never judges whether a capability is admissible. The
//! host's loader is the authority on those semantics and refuses a document whose capabilities it
//! cannot read.
//!
//! **Entry order is significant**: a model id takes the first entry with a rule that matches it
//! ([`ModelCatalogV1::entry_for`]), so a more specific rule comes before a broader one.
//!
//! The catalog lives in this crate, beside [`load_package_set`](crate::load_package_set), because
//! it is release data a host loads with the packages. It must not live in a crate the guests link
//! (`south-contracts`, `south-provider-api`, `south-component-conformance`): any source change
//! there moves every package's `component.wasm` (§13.11).

use serde_json::{Map, Value};
use thiserror::Error;

/// The schema id of the model catalog format this module reads and writes.
///
/// A change a reader of this version could not read correctly (a renamed or removed field, a new
/// match kind, a changed match meaning) takes a new schema id. Data changes (models added, removed
/// or re-described) keep the id and ship in an ordinary South release.
pub const MODEL_CATALOG_SCHEMA_V1: &str = "south.model-catalog.v1";

const TOP_LEVEL_FIELDS: [&str; 2] = ["schema", "entries"];
const ENTRY_FIELDS: [&str; 2] = ["match", "capabilities"];

/// A refusal of a model catalog document.
///
/// Positions are zero-based indexes into `entries` and into an entry's `match` list.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ModelCatalogErrorV1 {
    /// The bytes are not JSON.
    #[error("the model catalog is not JSON: {reason}")]
    NotJson {
        /// The JSON parser's description of the problem.
        reason: String,
    },
    /// The document, or an entry, is not a JSON object with exactly the format's fields.
    #[error("the model catalog is not a south.model-catalog.v1 document: {reason}")]
    Malformed {
        /// Which field is missing, unknown or of the wrong type.
        reason: String,
    },
    /// The `schema` field names another format.
    #[error("the model catalog schema is `{found}`, not `south.model-catalog.v1`")]
    UnknownSchema {
        /// The schema id the document declares.
        found: String,
    },
    /// An entry has an empty `match` list, so it could never apply to any model.
    #[error("model catalog entry {entry} has no match rule")]
    EmptyMatchList {
        /// The entry's index.
        entry: usize,
    },
    /// A match rule is not an object with exactly one key.
    #[error("model catalog entry {entry} match rule {rule} is not an object with exactly one key")]
    MatchRuleNotOneKey {
        /// The entry's index.
        entry: usize,
        /// The rule's index within the entry.
        rule: usize,
    },
    /// A match rule's key is not `exact`, `prefix` or `contains`.
    #[error(
        "model catalog entry {entry} match rule {rule} has kind `{kind}`, not exact, prefix or \
         contains"
    )]
    UnknownMatchKind {
        /// The entry's index.
        entry: usize,
        /// The rule's index within the entry.
        rule: usize,
        /// The key the rule used.
        kind: String,
    },
    /// A match rule's value is not a non-empty string. An empty `prefix` or `contains` would match
    /// every model id and shadow every later entry.
    #[error("model catalog entry {entry} match rule {rule} is not a non-empty string")]
    InvalidMatchValue {
        /// The entry's index.
        entry: usize,
        /// The rule's index within the entry.
        rule: usize,
    },
    /// A match rule repeats an earlier rule of the same kind and value, so it could never decide a
    /// lookup.
    #[error("model catalog entry {entry} match rule {rule} repeats a rule of entry {first_entry}")]
    DuplicateMatchRule {
        /// The entry's index.
        entry: usize,
        /// The rule's index within the entry.
        rule: usize,
        /// The entry that already carries the same rule.
        first_entry: usize,
    },
    /// An entry's `capabilities` is not a JSON object.
    #[error("model catalog entry {entry} capabilities is not a JSON object")]
    CapabilitiesNotObject {
        /// The entry's index.
        entry: usize,
    },
}

/// One match rule: how an entry recognizes an upstream model id. Comparison is case-sensitive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModelMatchV1 {
    /// The id equals the value.
    Exact(String),
    /// The id starts with the value (upstream ids that carry a date or version suffix).
    Prefix(String),
    /// The id contains the value (upstream ids with both a prefix and a suffix).
    Contains(String),
}

impl ModelMatchV1 {
    /// Whether this rule recognizes `upstream_model`.
    #[must_use]
    pub fn hits(&self, upstream_model: &str) -> bool {
        match self {
            Self::Exact(id) => upstream_model == id,
            Self::Prefix(prefix) => upstream_model.starts_with(prefix.as_str()),
            Self::Contains(part) => upstream_model.contains(part.as_str()),
        }
    }

    const fn kind(&self) -> &'static str {
        match self {
            Self::Exact(_) => "exact",
            Self::Prefix(_) => "prefix",
            Self::Contains(_) => "contains",
        }
    }

    fn value(&self) -> &str {
        match self {
            Self::Exact(value) | Self::Prefix(value) | Self::Contains(value) => value,
        }
    }

    fn from_rule(entry: usize, rule: usize, raw: &Value) -> Result<Self, ModelCatalogErrorV1> {
        let not_one_key = ModelCatalogErrorV1::MatchRuleNotOneKey { entry, rule };
        let object = raw
            .as_object()
            .filter(|object| object.len() == 1)
            .ok_or_else(|| not_one_key.clone())?;
        let (kind, value) = object.iter().next().ok_or(not_one_key)?;
        let constructor: fn(String) -> Self = match kind.as_str() {
            "exact" => Self::Exact,
            "prefix" => Self::Prefix,
            "contains" => Self::Contains,
            _ => {
                return Err(ModelCatalogErrorV1::UnknownMatchKind {
                    entry,
                    rule,
                    kind: kind.clone(),
                });
            }
        };
        // An empty string is refused by `ModelCatalogV1::new`, which judges every rule.
        value
            .as_str()
            .map(|text| constructor(text.to_owned()))
            .ok_or(ModelCatalogErrorV1::InvalidMatchValue { entry, rule })
    }

    fn to_rule(&self) -> Value {
        let mut object = Map::new();
        object.insert(self.kind().to_owned(), Value::String(self.value().to_owned()));
        Value::Object(object)
    }
}

/// One catalog entry: the rules that select it and the capabilities it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalogEntryV1 {
    matches: Vec<ModelMatchV1>,
    capabilities: Map<String, Value>,
}

impl ModelCatalogEntryV1 {
    /// An entry of `matches` and `capabilities`; [`ModelCatalogV1::new`] checks the rules.
    #[must_use]
    pub const fn new(matches: Vec<ModelMatchV1>, capabilities: Map<String, Value>) -> Self {
        Self { matches, capabilities }
    }

    /// The entry's match rules, in document order.
    #[must_use]
    pub fn matches(&self) -> &[ModelMatchV1] {
        &self.matches
    }

    /// The entry's capabilities, carried verbatim. Their vocabulary is the host's (module docs).
    #[must_use]
    pub const fn capabilities(&self) -> &Map<String, Value> {
        &self.capabilities
    }

    /// Whether any of the entry's rules recognizes `upstream_model`.
    #[must_use]
    pub fn hits(&self, upstream_model: &str) -> bool {
        self.matches.iter().any(|rule| rule.hits(upstream_model))
    }
}

/// A validated `south.model-catalog.v1` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCatalogV1 {
    entries: Vec<ModelCatalogEntryV1>,
}

impl ModelCatalogV1 {
    /// Parses and validates a catalog document.
    ///
    /// # Errors
    ///
    /// The first shape violation found, as listed on [`ModelCatalogErrorV1`].
    pub fn parse(bytes: &[u8]) -> Result<Self, ModelCatalogErrorV1> {
        let document: Value = serde_json::from_slice(bytes)
            .map_err(|error| ModelCatalogErrorV1::NotJson { reason: error.to_string() })?;
        Self::from_value(&document)
    }

    /// Validates a catalog document already parsed as JSON.
    ///
    /// # Errors
    ///
    /// The first shape violation found, as listed on [`ModelCatalogErrorV1`].
    pub fn from_value(document: &Value) -> Result<Self, ModelCatalogErrorV1> {
        let top = exact_fields(document, &TOP_LEVEL_FIELDS, "the document")?;
        let schema = top
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| malformed("`schema` is not a string"))?;
        if schema != MODEL_CATALOG_SCHEMA_V1 {
            return Err(ModelCatalogErrorV1::UnknownSchema { found: schema.to_owned() });
        }
        let raw_entries = top
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| malformed("`entries` is not an array"))?;
        let mut entries = Vec::with_capacity(raw_entries.len());
        for (entry_index, raw_entry) in raw_entries.iter().enumerate() {
            let entry = exact_fields(raw_entry, &ENTRY_FIELDS, &format!("entry {entry_index}"))?;
            let rules = entry.get("match").and_then(Value::as_array).ok_or_else(|| {
                malformed(&format!("entry {entry_index} `match` is not an array"))
            })?;
            let matches = rules
                .iter()
                .enumerate()
                .map(|(rule_index, rule)| ModelMatchV1::from_rule(entry_index, rule_index, rule))
                .collect::<Result<Vec<_>, _>>()?;
            let Some(Value::Object(capabilities)) = entry.get("capabilities") else {
                return Err(ModelCatalogErrorV1::CapabilitiesNotObject { entry: entry_index });
            };
            entries.push(ModelCatalogEntryV1::new(matches, capabilities.clone()));
        }
        Self::new(entries)
    }

    /// A catalog of `entries`, in lookup order.
    ///
    /// # Errors
    ///
    /// [`ModelCatalogErrorV1::EmptyMatchList`] for an entry without rules,
    /// [`ModelCatalogErrorV1::InvalidMatchValue`] for an empty rule value, and
    /// [`ModelCatalogErrorV1::DuplicateMatchRule`] when a rule repeats an earlier one.
    pub fn new(entries: Vec<ModelCatalogEntryV1>) -> Result<Self, ModelCatalogErrorV1> {
        let mut seen: Vec<(&ModelMatchV1, usize)> = Vec::new();
        for (entry_index, entry) in entries.iter().enumerate() {
            if entry.matches.is_empty() {
                return Err(ModelCatalogErrorV1::EmptyMatchList { entry: entry_index });
            }
            for (rule_index, rule) in entry.matches.iter().enumerate() {
                if rule.value().is_empty() {
                    return Err(ModelCatalogErrorV1::InvalidMatchValue {
                        entry: entry_index,
                        rule: rule_index,
                    });
                }
                if let Some((_, first_entry)) = seen.iter().find(|(earlier, _)| *earlier == rule) {
                    return Err(ModelCatalogErrorV1::DuplicateMatchRule {
                        entry: entry_index,
                        rule: rule_index,
                        first_entry: *first_entry,
                    });
                }
                seen.push((rule, entry_index));
            }
        }
        Ok(Self { entries })
    }

    /// The entries, in lookup order.
    #[must_use]
    pub fn entries(&self) -> &[ModelCatalogEntryV1] {
        &self.entries
    }

    /// The first entry with a rule that recognizes `upstream_model`; `None` when the catalog does
    /// not describe the model (which is "no declaration", not "no capability").
    #[must_use]
    pub fn entry_for(&self, upstream_model: &str) -> Option<&ModelCatalogEntryV1> {
        self.entries.iter().find(|entry| entry.hits(upstream_model))
    }

    /// The document form: `{"schema": ..., "entries": [...]}`, which [`ModelCatalogV1::from_value`]
    /// reads back to an equal catalog.
    #[must_use]
    pub fn to_value(&self) -> Value {
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                let mut object = Map::new();
                object.insert(
                    "match".to_owned(),
                    Value::Array(entry.matches.iter().map(ModelMatchV1::to_rule).collect()),
                );
                object.insert("capabilities".to_owned(), Value::Object(entry.capabilities.clone()));
                Value::Object(object)
            })
            .collect();
        let mut document = Map::new();
        document.insert("schema".to_owned(), Value::String(MODEL_CATALOG_SCHEMA_V1.to_owned()));
        document.insert("entries".to_owned(), Value::Array(entries));
        Value::Object(document)
    }
}

fn malformed(reason: &str) -> ModelCatalogErrorV1 {
    ModelCatalogErrorV1::Malformed { reason: reason.to_owned() }
}

/// `value` as an object holding exactly `fields`: none missing, none unknown.
fn exact_fields<'a>(
    value: &'a Value,
    fields: &[&str],
    what: &str,
) -> Result<&'a Map<String, Value>, ModelCatalogErrorV1> {
    let object =
        value.as_object().ok_or_else(|| malformed(&format!("{what} is not a JSON object")))?;
    if let Some(unknown) = object.keys().find(|key| !fields.contains(&key.as_str())) {
        return Err(malformed(&format!("{what} has unknown field `{unknown}`")));
    }
    if let Some(missing) = fields.iter().find(|field| !object.contains_key(**field)) {
        return Err(malformed(&format!("{what} has no `{missing}`")));
    }
    Ok(object)
}
