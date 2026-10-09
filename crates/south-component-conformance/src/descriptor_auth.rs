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

use south_contracts::media::{MediaAuthV1, MediaRequestDescriptorV1};
use south_contracts::{DeclaredSecretHeaderV1, SecretHeaderV1};
use south_provider_api::{ComponentManifestV1, SlotV1};
use token_station_protocol::{
    Auth, DescriptorError, HttpRequestDescriptor, ProviderConfig, SecretRef,
};

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
    /// The same secret twice: `Authorization: Bearer <secret>` and `<header>: <secret>`, in one
    /// sanctioned secret-bearing header (B7b, §4.3). A host maps it onto
    /// `RawAuthV1::BearerAndHeaderSecret`.
    BearerAndHeaderSecret(SecretHeaderV1),
    /// `<header>: <secret>`, in one header the manifest declares in `secret_headers` (B7a, §10).
    ///
    /// A host maps it onto `RawAuthV1::DeclaredHeaderSecret` and passes the manifest's whole
    /// declaration as the raw call's `secret_headers`.
    DeclaredHeaderSecret(DeclaredSecretHeaderV1),
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
    /// `Auth::BearerAndHeader`, protocol 0.5.0), an arm the manifest does not declare
    /// (`bearer_and_header_secret`).
    BearerAndHeaderNotDeclared,
    /// The descriptor names a header that is neither a sanctioned secret-bearing header nor one
    /// the manifest declares in `secret_headers`.
    HeaderNotSanctioned(String),
    /// The combined arm names a header outside the five sanctioned ones. A header the manifest
    /// declares in `secret_headers` is refused here too: auth contract 5 has no combined arm over
    /// a declared name (§16 Q35).
    CombinedHeaderNotSanctioned(String),
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
            Self::CombinedHeaderNotSanctioned(name) => write!(
                f,
                "`{name}` is not a sanctioned secret-bearing header, and the combined arm admits \
                 no other"
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
/// - `Auth::BearerAndHeader` requires the `bearer_and_header_secret` arm and one of the five
///   sanctioned header names (B7b, §4.3); without the arm it is refused with
///   [`DescriptorAuthErrorV1::BearerAndHeaderNotDeclared`], and a declared name is refused,
///   because the contract's combined arm is closed over the sanctioned set;
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
    let presentation = descriptor.auth.as_ref().map(|auth| match auth {
        Auth::Bearer { .. } => Presentation::Bearer,
        Auth::Header { name, .. } => Presentation::Header(name),
        Auth::BearerAndHeader { name, .. } => Presentation::BearerAndHeader(name),
        Auth::OAuth { secret, .. } => Presentation::OAuth(secret.as_str()),
    });
    admit_presentation(
        manifest,
        config,
        descriptor.headers.iter().map(|(name, _)| name.as_str()),
        presentation,
    )
}

/// Admits a media descriptor's auth (image record §6.3, §18.4): the twin of
/// [`admit_descriptor_auth`] for [`MediaRequestDescriptorV1`], built on the same rule so the two
/// cannot drift.
///
/// A media descriptor has no absolute URL — its path is relative, so it cannot leave the
/// configured endpoint — but its slot is checked exactly as [`ProviderConfig::authorize`] checks
/// a kernel descriptor's: a slot on an upstream configured without one, a missing slot on one
/// configured with one, and a slot other than the configured one are refused. Then the shared
/// rule: the arm must be declared, the header must be sanctioned or declared in
/// `secret_headers`, no ordinary header may carry a declared secret header's name, and a slot a
/// credential recipe mints is presented as Bearer.
///
/// # Errors
///
/// Returns the first [`DescriptorAuthErrorV1`] found.
pub fn admit_media_descriptor_auth(
    manifest: &ComponentManifestV1,
    config: &ProviderConfig,
    descriptor: &MediaRequestDescriptorV1,
) -> Result<AdmittedAuthV1, DescriptorAuthErrorV1> {
    let named = descriptor.auth().map(|auth| SecretRef::new(auth.slot()));
    match (config.auth.as_ref(), named) {
        (None, Some(named)) => {
            return Err(DescriptorAuthErrorV1::NotAuthorized(
                DescriptorError::UnexpectedCredential { named },
            ));
        }
        (Some(_), None) => {
            return Err(DescriptorAuthErrorV1::NotAuthorized(DescriptorError::MissingCredential));
        }
        (Some(slot), Some(named)) if &named != slot => {
            return Err(DescriptorAuthErrorV1::NotAuthorized(
                DescriptorError::UndeclaredCredential { named, declared: slot.clone() },
            ));
        }
        (None, None) | (Some(_), Some(_)) => {}
    }
    let presentation = descriptor.auth().map(|auth| match auth {
        MediaAuthV1::Bearer { slot } if slot_is_minted(manifest, config, slot) => {
            Presentation::OAuth(slot.as_str())
        }
        MediaAuthV1::Bearer { .. } => Presentation::Bearer,
        MediaAuthV1::HeaderSecret { header, .. } => Presentation::Header(header.as_str()),
    });
    admit_presentation(
        manifest,
        config,
        descriptor.headers().iter().map(|(name, _)| name),
        presentation,
    )
}

/// How a descriptor asks for its credential to be presented, independent of the descriptor's own
/// shape.
#[derive(Clone, Copy)]
enum Presentation<'a> {
    Bearer,
    Header(&'a str),
    BearerAndHeader(&'a str),
    /// A slot a credential recipe may mint, presented as Bearer.
    OAuth(&'a str),
}

/// The rule both descriptor shapes share, after their slot checks.
fn admit_presentation<'a>(
    manifest: &ComponentManifestV1,
    config: &ProviderConfig,
    mut ordinary_headers: impl Iterator<Item = &'a str>,
    presentation: Option<Presentation<'_>>,
) -> Result<AdmittedAuthV1, DescriptorAuthErrorV1> {
    if let Some(name) = ordinary_headers.find(|name| declared(manifest, name).is_some()) {
        return Err(DescriptorAuthErrorV1::SecretHeaderOnOrdinaryChannel(name.to_owned()));
    }
    if manifest.auth_arms.contains("host_signed") {
        return match presentation {
            None => Ok(AdmittedAuthV1::HostSigned),
            Some(_) => Err(DescriptorAuthErrorV1::HostSignedCarriesAuth),
        };
    }
    match presentation {
        None => Ok(AdmittedAuthV1::None),
        Some(Presentation::Bearer) if manifest.auth_arms.contains("bearer") => {
            Ok(AdmittedAuthV1::Bearer)
        }
        Some(Presentation::Bearer) => Err(DescriptorAuthErrorV1::BearerNotDeclared),
        Some(Presentation::Header(name)) => {
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
                .ok_or_else(|| DescriptorAuthErrorV1::HeaderNotSanctioned(name.to_owned()))
        }
        Some(Presentation::BearerAndHeader(name)) => {
            if !manifest.auth_arms.contains("bearer_and_header_secret") {
                return Err(DescriptorAuthErrorV1::BearerAndHeaderNotDeclared);
            }
            sanctioned(name)
                .map(AdmittedAuthV1::BearerAndHeaderSecret)
                .ok_or_else(|| DescriptorAuthErrorV1::CombinedHeaderNotSanctioned(name.to_owned()))
        }
        Some(Presentation::OAuth(slot)) => {
            if slot_is_minted(manifest, config, slot) {
                Ok(AdmittedAuthV1::Bearer)
            } else {
                Err(DescriptorAuthErrorV1::OAuthNotAdmitted)
            }
        }
    }
}

/// Whether the section that applies to this family mints `slot` (§13.5 D2).
fn slot_is_minted(manifest: &ComponentManifestV1, config: &ProviderConfig, slot: &str) -> bool {
    manifest
        .credentials_for(&config.provider)
        .is_some_and(|credentials| matches!(credentials.slots.get(slot), Some(SlotV1::Minted(_))))
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
