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

use south_contracts::{DeclaredSecretHeaderV1, SecretHeaderV1};
use south_provider_api::{ComponentManifestV1, SlotV1};
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
    /// `<header>: <secret>`, in one header the manifest declares in `secret_headers` (B7a, §10).
    ///
    /// A host maps it onto `RawAuthV1::DeclaredHeaderSecret` and passes the manifest's whole
    /// declaration as the raw call's `secret_headers`.
    DeclaredHeaderSecret(DeclaredSecretHeaderV1),
    /// `Authorization: Bearer <secret>` and `<header>: <secret>` from one resolution, in one
    /// sanctioned secret-bearing header (B7b, host-zero-vendor-boundary §4.3).
    ///
    /// A host maps it onto `RawAuthV1::BearerAndHeaderSecret`. The contract's arm is closed over
    /// [`SecretHeaderV1`], so a declared secret header is never presented this way.
    BearerAndHeaderSecret(SecretHeaderV1),
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
    /// The descriptor presents the same credential as Bearer and in a header (the kernel's
    /// `Auth::BearerAndHeader`, protocol 0.5.0), which the manifest does not declare
    /// (`bearer_and_header_secret`).
    BearerAndHeaderNotDeclared,
    /// The descriptor names a header that is neither a sanctioned secret-bearing header nor one
    /// the manifest declares in `secret_headers`. The combined arm admits sanctioned headers
    /// only.
    HeaderNotSanctioned(String),
    /// The descriptor sends a header the manifest declares as secret-bearing through the ordinary
    /// header channel, where nothing redacts it (B7a, §10).
    SecretHeaderOnOrdinaryChannel(String),
    /// The descriptor asks for an OAuth exchange on a slot the manifest's credential recipes do
    /// not mint (§3.3, §4.2).
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
            Self::BearerAndHeaderNotDeclared => f.write_str(
                "the descriptor presents bearer auth and a credential header together, which the \
                 manifest does not declare",
            ),
            Self::HeaderNotSanctioned(name) => write!(
                f,
                "`{name}` is neither a sanctioned secret-bearing header nor declared in secret_headers"
            ),
            Self::SecretHeaderOnOrdinaryChannel(name) => write!(
                f,
                "`{name}` is a declared secret header and may only be presented as the credential"
            ),
            Self::OAuthNotAdmitted => {
                f.write_str("an OAuth descriptor names a slot no credential recipe mints")
            }
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
/// - `Auth::Header` requires the `header_secret` arm and a sanctioned header name, or one the
///   manifest declares in `secret_headers` (B7a, §10), which is admitted as
///   [`AdmittedAuthV1::DeclaredHeaderSecret`];
/// - `Auth::BearerAndHeader` requires the `bearer_and_header_secret` arm and a sanctioned header
///   name, and is admitted as [`AdmittedAuthV1::BearerAndHeaderSecret`]; a header declared in
///   `secret_headers` is refused, since the contract's combined arm is closed over
///   [`SecretHeaderV1`];
/// - no ordinary descriptor header may carry a declared secret header's name;
/// - `Auth::OAuth` is admitted, as Bearer, only on a slot a credential recipe mints (§3.3) in the
///   section that applies to the configured family (§13.5 D2); the host's recipe executor produces
///   the value, and nothing in the descriptor names an exchange;
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
    if let Some((name, _)) =
        descriptor.headers.iter().find(|(name, _)| declared(manifest, name).is_some())
    {
        return Err(DescriptorAuthErrorV1::SecretHeaderOnOrdinaryChannel(name.clone()));
    }
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
        Some(Auth::BearerAndHeader { name, .. }) => {
            if !manifest.auth_arms.contains("bearer_and_header_secret") {
                return Err(DescriptorAuthErrorV1::BearerAndHeaderNotDeclared);
            }
            sanctioned(name)
                .map(AdmittedAuthV1::BearerAndHeaderSecret)
                .ok_or_else(|| DescriptorAuthErrorV1::HeaderNotSanctioned(name.clone()))
        }
        Some(Auth::Header { name, .. }) => {
            if !manifest.auth_arms.contains("header_secret") {
                return Err(DescriptorAuthErrorV1::HeaderSecretNotDeclared);
            }
            if let Some(header) = sanctioned(name) {
                return Ok(AdmittedAuthV1::HeaderSecret(header));
            }
            // Since protocol 0.5.0 the kernel's `Auth::header` admits any lowercase field name
            // outside its never-credential list, so a component's descriptor can name a header
            // outside `CREDENTIAL_HEADERS`; this check decides whether the package may use it.
            declared(manifest, name)
                .map(AdmittedAuthV1::DeclaredHeaderSecret)
                .ok_or_else(|| DescriptorAuthErrorV1::HeaderNotSanctioned(name.clone()))
        }
        Some(Auth::OAuth { secret, .. }) => {
            // Only the section that applies to this family mints (§13.5 D2).
            let minted = manifest.credentials_for(&config.provider).is_some_and(|credentials| {
                matches!(credentials.slots.get(secret.as_str()), Some(SlotV1::Minted(_)))
            });
            if minted {
                Ok(AdmittedAuthV1::Bearer)
            } else {
                Err(DescriptorAuthErrorV1::OAuthNotAdmitted)
            }
        }
    }
}

/// The sanctioned secret-bearing header matching `name` without case.
fn sanctioned(name: &str) -> Option<SecretHeaderV1> {
    SecretHeaderV1::ALL.into_iter().find(|header| header.header_name().eq_ignore_ascii_case(name))
}

/// The manifest's declared secret header matching `name` without case, when it passes the
/// contract's rules. Gate ① already applied them; a manifest that skipped gate ① declares nothing.
fn declared(manifest: &ComponentManifestV1, name: &str) -> Option<DeclaredSecretHeaderV1> {
    manifest
        .secret_headers
        .iter()
        .find(|declared| declared.eq_ignore_ascii_case(name))
        .and_then(|declared| DeclaredSecretHeaderV1::parse(declared).ok())
}
