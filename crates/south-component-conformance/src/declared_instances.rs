//! The provider instances a manifest declares, as the contract types a host executes with (B7a,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §10).
//!
//! Gate ① lives in `south-provider-api` and the contract types in `south-contracts`; neither crate
//! depends on the other. This is where the two meet, the same way [`crate::admit_descriptor_auth`]
//! joins a manifest to a descriptor: a host builds [`DeclaredInstancesV1`] once per admitted
//! package, and it is the sanctioned way to obtain a [`DeclaredUserAgentV1`] — the manifest is
//! validated here before any value is read, so a value always comes from a manifest that passed
//! gate ①.

use std::{collections::BTreeMap, error::Error, fmt};

use south_contracts::{
    ContractErrorV1, DeclaredQueryParameterV1, DeclaredUserAgentV1, ProviderQuotaHeaderMapV1,
    ProviderQuotaMetadataFieldV1, QueryParameterV1, QueryValueSyntaxV1,
};
use south_provider_api::{ComponentManifestV1, ManifestErrorV1};

/// Why a manifest's declarations could not be turned into contract types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredInstancesErrorV1 {
    /// Gate ① refused the manifest.
    Manifest(ManifestErrorV1),
    /// Gate ① admitted a declaration the contract refuses. The shared lists are pinned by test, so
    /// this means the two halves drifted; the package must not be used.
    Contract(ContractErrorV1),
}

impl fmt::Display for DeclaredInstancesErrorV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(error) => error.fmt(f),
            Self::Contract(error) => write!(f, "gate ① and the contract disagree: {error}"),
        }
    }
}

impl Error for DeclaredInstancesErrorV1 {}

impl From<ManifestErrorV1> for DeclaredInstancesErrorV1 {
    fn from(error: ManifestErrorV1) -> Self {
        Self::Manifest(error)
    }
}

/// One admitted package's declared query parameters, quota headers and per-family user-agents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredInstancesV1 {
    query_parameters: Vec<DeclaredQueryParameterV1>,
    quota_headers: ProviderQuotaHeaderMapV1,
    user_agents: BTreeMap<String, DeclaredUserAgentV1>,
}

impl DeclaredInstancesV1 {
    /// Validates `manifest` (gate ①) and converts its declarations.
    ///
    /// A manifest without `quota_headers` gets [`ProviderQuotaHeaderMapV1::canonical`], which is
    /// what every transport captured before quota metadata contract version two.
    ///
    /// # Errors
    ///
    /// Returns [`DeclaredInstancesErrorV1`] when gate ① refuses the manifest or, impossibly while
    /// the pins hold, the contract refuses a declaration gate ① admitted.
    pub fn from_manifest(manifest: &ComponentManifestV1) -> Result<Self, DeclaredInstancesErrorV1> {
        manifest.validate()?;
        let query_parameters = manifest
            .query_parameters
            .iter()
            .map(|declaration| {
                DeclaredQueryParameterV1::try_new(
                    &declaration.name,
                    contract_query_syntax(&declaration.syntax),
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(DeclaredInstancesErrorV1::Contract)?;
        let quota_headers = if manifest.quota_headers.is_empty() {
            ProviderQuotaHeaderMapV1::canonical()
        } else {
            let entries = manifest
                .quota_headers
                .iter()
                .map(|declaration| {
                    ProviderQuotaMetadataFieldV1::from_header_name(&declaration.field)
                        .map(|field| (declaration.header.as_str(), field))
                        .ok_or(ContractErrorV1::InvalidQuotaHeaderDeclaration)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(DeclaredInstancesErrorV1::Contract)?;
            ProviderQuotaHeaderMapV1::try_from_iter(entries)
                .map_err(DeclaredInstancesErrorV1::Contract)?
        };
        let user_agents = manifest
            .user_agent
            .iter()
            .map(|(family, value)| {
                DeclaredUserAgentV1::from_manifest_value(value).map(|agent| (family.clone(), agent))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(DeclaredInstancesErrorV1::Contract)?;
        Ok(Self { query_parameters, quota_headers, user_agents })
    }

    /// Every declared query parameter, in manifest order.
    #[must_use]
    pub fn query_parameters(&self) -> &[DeclaredQueryParameterV1] {
        &self.query_parameters
    }

    /// The parameter a query pair names: a sanctioned one, else one this package declared, else
    /// `None` (the pair is not admissible for this package).
    #[must_use]
    pub fn query_parameter(&self, wire_name: &str) -> Option<QueryParameterV1> {
        QueryParameterV1::ALL
            .into_iter()
            .find(|sanctioned| sanctioned.wire_name() == wire_name)
            .or_else(|| {
                self.query_parameters
                    .iter()
                    .find(|declared| declared.name() == wire_name)
                    .cloned()
                    .map(QueryParameterV1::Declared)
            })
    }

    /// Which response headers the package's transport captures as quota metadata.
    #[must_use]
    pub const fn quota_headers(&self) -> &ProviderQuotaHeaderMapV1 {
        &self.quota_headers
    }

    /// The user-agent `family` declares, if any.
    #[must_use]
    pub fn user_agent(&self, family: &str) -> Option<&DeclaredUserAgentV1> {
        self.user_agents.get(family)
    }
}

/// The contract syntax for a manifest query syntax. Both sets are closed and name the same four
/// mechanisms; a new syntax is a contract change on both sides.
#[must_use]
pub fn contract_query_syntax(
    syntax: &south_provider_api::QueryValueSyntaxV1,
) -> QueryValueSyntaxV1 {
    match syntax {
        south_provider_api::QueryValueSyntaxV1::Digits => QueryValueSyntaxV1::Digits,
        south_provider_api::QueryValueSyntaxV1::Token => QueryValueSyntaxV1::Token,
        south_provider_api::QueryValueSyntaxV1::Date => QueryValueSyntaxV1::Date,
        south_provider_api::QueryValueSyntaxV1::Enum(values) => {
            QueryValueSyntaxV1::Enum(values.clone())
        }
    }
}
