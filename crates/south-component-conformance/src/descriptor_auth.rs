//! Descriptor auth admission: what a component's descriptor may present, judged against what its
//! manifest declared (B2, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §4.2).
//!
//! The component chooses how a credential is presented — `Authorization: Bearer` for one dialect,
//! `x-api-key` for another — and the manifest's `auth_arms` lists the arms its descriptors may
//! use. Until now nothing compared the two, so a host looked the presentation up in a table keyed
//! by provider type and the component's choice never reached the wire. This function is the
//! comparison, shared by both hosts' text and task paths and by gate ②'s
//! `DescriptorAuthWithinManifest`, so that a host can present strictly what the descriptor says.

use std::error::Error;
use std::fmt;

use south_contracts::SecretHeaderV1;
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{Auth, DescriptorError, HttpRequestDescriptor, ProviderConfig};

/// The presentation an admitted descriptor asks the host for.
///
/// A host maps it onto its raw-call auth arm; the slot itself is the one
/// [`ProviderConfig::authorize`] already matched against the configured upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmittedAuthV1 {
    /// No credential: the upstream is configured without one.
    None,
    /// `Authorization: Bearer <secret>`.
    Bearer,
    /// `<header>: <secret>`, in one sanctioned secret-bearing header.
    HeaderSecret(SecretHeaderV1),
    /// The package is `host_signed`: the descriptor names no credential and the host's finalizer
    /// signs the request.
    HostSigned,
}

/// Why a descriptor's auth was refused. Every refusal happens before the host resolves a
/// credential or calls the upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DescriptorAuthErrorV1 {
    /// The descriptor failed the kernel's own gate: addressed outside the configured endpoint, or
    /// naming a slot the upstream does not have, or missing one it needs.
    NotAuthorized(DescriptorError),
    /// The descriptor presents Bearer, which the manifest does not declare.
    BearerNotDeclared,
    /// The descriptor presents a header, which the manifest does not declare (`header_secret`).
    HeaderSecretNotDeclared,
    /// The descriptor names a header that is not a sanctioned secret-bearing header.
    HeaderNotSanctioned(String),
    /// The descriptor asks for an OAuth exchange. Admitted only once credential recipes declare
    /// the minted slot (§3, phase B4).
    OAuthNotAdmitted,
    /// A `host_signed` package's descriptor names a credential; its requests are signed by the
    /// host and carry none.
    HostSignedCarriesAuth,
}

impl fmt::Display for DescriptorAuthErrorV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAuthorized(error) => write!(f, "the descriptor was not authorized: {error}"),
            Self::BearerNotDeclared => f.write_str(
                "the descriptor presents bearer auth, which the manifest does not declare",
            ),
            Self::HeaderSecretNotDeclared => f.write_str(
                "the descriptor presents a credential header, which the manifest does not declare",
            ),
            Self::HeaderNotSanctioned(name) => {
                write!(f, "`{name}` is not a sanctioned secret-bearing header")
            }
            Self::OAuthNotAdmitted => f.write_str(
                "OAuth descriptors are not admitted until a credential recipe declares the slot",
            ),
            Self::HostSignedCarriesAuth => f.write_str(
                "a host-signed package's descriptor must not name a credential; the host signs it",
            ),
        }
    }
}

impl Error for DescriptorAuthErrorV1 {}

/// Admits a descriptor's auth against the package's manifest and the configured upstream.
///
/// The kernel's [`ProviderConfig::authorize`] runs first — the endpoint check is the one that
/// keeps a credential from leaving for a host the operator did not configure — and then:
///
/// - a `host_signed` package's descriptor must carry no auth (the host passes no slot for it);
/// - `Auth::Bearer` requires the `bearer` arm;
/// - `Auth::Header` requires the `header_secret` arm and a sanctioned header name;
/// - `Auth::OAuth` is refused until phase B4;
/// - no auth is admitted when, and only when, the upstream is configured without a slot (which
///   `authorize` has already judged).
///
/// # Errors
///
/// Returns the first [`DescriptorAuthErrorV1`] found.
pub fn admit_descriptor_auth(
    manifest: &ComponentManifestV1,
    config: &ProviderConfig,
    descriptor: &HttpRequestDescriptor,
) -> Result<AdmittedAuthV1, DescriptorAuthErrorV1> {
    config.authorize(descriptor).map_err(DescriptorAuthErrorV1::NotAuthorized)?;
    if manifest.auth_arms.contains("host_signed") {
        return match descriptor.auth {
            None => Ok(AdmittedAuthV1::HostSigned),
            Some(_) => Err(DescriptorAuthErrorV1::HostSignedCarriesAuth),
        };
    }
    match &descriptor.auth {
        None => Ok(AdmittedAuthV1::None),
        Some(Auth::Bearer { .. }) if manifest.auth_arms.contains("bearer") => {
            Ok(AdmittedAuthV1::Bearer)
        }
        Some(Auth::Bearer { .. }) => Err(DescriptorAuthErrorV1::BearerNotDeclared),
        Some(Auth::Header { name, .. }) => {
            if !manifest.auth_arms.contains("header_secret") {
                return Err(DescriptorAuthErrorV1::HeaderSecretNotDeclared);
            }
            SecretHeaderV1::ALL
                .into_iter()
                .find(|header| header.header_name().eq_ignore_ascii_case(name))
                .map(AdmittedAuthV1::HeaderSecret)
                .ok_or_else(|| DescriptorAuthErrorV1::HeaderNotSanctioned(name.clone()))
        }
        Some(Auth::OAuth { .. }) => Err(DescriptorAuthErrorV1::OAuthNotAdmitted),
    }
}
