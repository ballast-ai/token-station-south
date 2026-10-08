//! The component value channel (Q14, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §13.8).
//!
//! Kernel protocol 0.5.0 gives a component two typed maps it may read, both under the kernel's
//! `ComponentValues` grammar (keys of 1 to 64 bytes of `[a-z0-9_]`, values of 1 to 4096 bytes of
//! printable ASCII):
//!
//! - `ProviderConfig.declared`, per provider row and per attempt: the family's `config_schema`
//!   keys and the attributes the applying `credentials` section exports. The two share one flat
//!   namespace, so gate ① refuses a package that uses one name for both (D8). An attribute belongs
//!   to one credential, so a host builds the map after it selects the credential, for each attempt,
//!   and never caches it per row.
//! - `ChatRequest.host_values`, per request: values only the host can mint, from the closed
//!   vocabulary [`HOST_VALUES`] (D7). A new word is a generic host feature, never a per-vendor
//!   value.
//!
//! A host passes only the keys a package declares, and a component reads only those; gate ②
//! checks that a component ignores any other key (`undeclared_values_ignored`). Nothing secret
//! travels in either map: an attribute comes only from a field declared non-secret (§3.4 rule 4).

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::{ComponentManifestV1, ConfigErrorV1, ManifestErrorV1, PROVIDER_WORLD, WorldSchemaV1};

/// A host-minted identifier for one upstream attempt: a UUID (RFC 9562 version 4, lowercase and
/// hyphenated), fresh for every attempt, so a failover attempt gets a new one.
pub const HOST_VALUE_ATTEMPT_ID: &str = "attempt_id";

/// The closed vocabulary of `ChatRequest.host_values` keys a manifest may declare (D7).
pub const HOST_VALUES: &[&str] = &[HOST_VALUE_ATTEMPT_ID];

/// Why [`ComponentManifestV1::declared_values`] built no map.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DeclaredValuesErrorV1 {
    /// The operator's config values do not fit the family's `config_schema`.
    #[error(transparent)]
    Config(#[from] ConfigErrorV1),
    /// The section that applies to the family exports no attribute of this name.
    #[error("`{0}` is not an attribute this family's credentials export")]
    UndeclaredAttribute(String),
    /// The value is not one the attribute's declaration admits.
    #[error("the value of attribute `{0}` is not one its declaration admits")]
    InvalidAttribute(String),
}

impl ComponentManifestV1 {
    /// Gate ① for the value channel: `host_values` from the closed vocabulary, once each;
    /// `host_values` and credential attributes only in the provider world; and no family whose
    /// config key and exported attribute share a name.
    pub(crate) fn validate_component_values(
        &self,
        world: &WorldSchemaV1,
    ) -> Result<(), ManifestErrorV1> {
        if world.world != PROVIDER_WORLD {
            if !self.host_values.is_empty() {
                return Err(ManifestErrorV1::InstanceIsAProviderWorldDeclaration(
                    "host_values".to_owned(),
                ));
            }
            // No task host path builds `declared` from a recipe; a word nothing honors would be a
            // promise (as for the combined auth arm, §16 Q36).
            if self
                .credentials
                .as_ref()
                .is_some_and(|section| !section.attribute_names().is_empty())
            {
                return Err(ManifestErrorV1::InstanceIsAProviderWorldDeclaration(
                    "credential attributes".to_owned(),
                ));
            }
            return Ok(());
        }
        let mut seen = BTreeSet::new();
        for value in &self.host_values {
            if !HOST_VALUES.contains(&value.as_str()) {
                return Err(ManifestErrorV1::HostValueIsNotInTheVocabulary(value.clone()));
            }
            if !seen.insert(value.as_str()) {
                return Err(ManifestErrorV1::HostValueDeclaredTwice(value.clone()));
            }
        }
        for family in &self.providers {
            let (Some(keys), Some(section)) =
                (self.config_schema.get(family), self.credentials_for(family))
            else {
                continue;
            };
            if let Some(name) =
                section.attribute_names().into_iter().find(|name| keys.contains_key(*name))
            {
                return Err(ManifestErrorV1::DeclaredValueNameCollision {
                    family: family.clone(),
                    name: name.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// The keys a host may place in `ProviderConfig.declared` for `family`: its `config_schema`
    /// keys and the attributes of the `credentials` section that applies to it. Empty for a family
    /// the manifest does not declare.
    #[must_use]
    pub fn declared_keys(&self, family: &str) -> BTreeSet<&str> {
        if !self.providers.iter().any(|provider| provider == family) {
            return BTreeSet::new();
        }
        let config = self.config_schema.get(family).into_iter().flat_map(BTreeMap::keys);
        let attributes = self.credentials_for(family).map(crate::CredentialsV1::attribute_names);
        config.map(String::as_str).chain(attributes.into_iter().flatten()).collect()
    }

    /// Builds `ProviderConfig.declared` for one attempt on `family`: the operator's validated
    /// `config` values (a key's `default` standing in for an absent one, an optional key without
    /// one left out) and the selected credential's exported `attributes`, which the host's recipe
    /// executor produced.
    ///
    /// Every value satisfies the kernel's `ComponentValues` grammar, because every value syntax
    /// lies inside it.
    ///
    /// # Errors
    ///
    /// The first [`DeclaredValuesErrorV1`] found: config values that break the family's schema, an
    /// attribute the applying section does not export, or a value its declaration does not admit.
    pub fn declared_values(
        &self,
        family: &str,
        config: &BTreeMap<String, String>,
        attributes: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, DeclaredValuesErrorV1> {
        self.validate_config_values(family, config)?;
        let mut declared = BTreeMap::new();
        for (key, declaration) in self.config_schema.get(family).into_iter().flatten() {
            if let Some(value) = config.get(key).or(declaration.default.as_ref()) {
                declared.insert(key.clone(), value.clone());
            }
        }
        let section = self.credentials_for(family);
        for (name, value) in attributes {
            match section.and_then(|section| section.admits_attribute(name, value)) {
                None => return Err(DeclaredValuesErrorV1::UndeclaredAttribute(name.clone())),
                Some(false) => return Err(DeclaredValuesErrorV1::InvalidAttribute(name.clone())),
                Some(true) => {
                    declared.insert(name.clone(), value.clone());
                }
            }
        }
        Ok(declared)
    }
}
