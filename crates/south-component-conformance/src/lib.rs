#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Component-package and component-behavior conformance gates (gates ① and ②
//! of the four-gate layering).
//!
//! - **Gate ① (package admission)**: everything decidable before loading any
//!   code — [`accepts_manifest`] and the identity comparison
//!   [`reported_identity_matches`]. The manifest schema and the
//!   compatibility-tuple handshake live in `south-provider-api`, because the
//!   loader now *requires* the handshake: it is production admission, not a
//!   fixture, and a host cannot be left free to forget it.
//! - **Gate ② (component behavior)**: [`run_provider_component_suite_v1`] —
//!   typed decode, fixture-pinned translation, determinism, byte-level stream
//!   incrementality, endpoint confinement and error-catalog discipline,
//!   judged against the [`ProviderComponentV1`] seam so it exists and bites
//!   before any runtime can instantiate a component, and so a component
//!   author can run it against a native build without a WASM toolchain.
//! - Gate ③ (host integration) is the three frozen suites in
//!   `south-provider-conformance`, unchanged. Gate ④ (runtime enforcement)
//!   is a property of the S3 runtime's construction; no fixture here
//!   pretends to check it.
//!
//! This crate is the one sanctioned typed consumer of the Canonical IR in
//! this repository (S0 invariant 6): it takes `token-station-protocol` at a
//! **fixed kernel revision** (the distribution channel), so gate ② can judge
//! typed decode and byte-exact serialization. Production crates
//! (`south-provider-api`, `south-provider-runtime`) never gain this
//! dependency.
//!
//! The design record is `docs/design/2026-08-21-component-conformance.md`;
//! the contract it enforces is the S0 canonical IR inventory.

pub mod abi;
mod component;
mod fixture;
pub mod reference;
pub mod reference_anthropic;
pub mod reference_gemini;
mod report;
#[cfg(feature = "sandbox")]
pub mod sandbox;
mod suite;

pub use component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
pub use fixture::{CaseV1, FIXTURE_KIND_V1, FixtureErrorV1, FixturePackV1, ProviderFamilyV1};
pub use report::{CheckV1, OutcomeV1, ReportV1, VerdictV1};
pub use suite::{PROVIDER_COMPONENT_SUITE_V1, run_provider_component_suite_v1};

use south_provider_api::{ComponentManifestV1, ComponentMetadataV1, ManifestErrorV1};

/// Gate ①, first half: everything the host can decide before loading any
/// code.
///
/// # Errors
///
/// Returns the [`ManifestErrorV1`] that disqualified the package.
pub fn accepts_manifest(manifest: &ComponentManifestV1) -> Result<(), ManifestErrorV1> {
    manifest.validate()
}

/// Gate ①, second half: what a loaded component reports must be what it
/// declared.
///
/// A package whose `metadata()` disagrees with its `manifest.json` has either
/// been repackaged around its vetting, or been built from different source
/// than it claims. Either way the registry's record of what is installed
/// would be wrong, and every later decision keyed on that record — canary by
/// version, rollback, capability dispatch — would act on a fiction.
#[must_use]
pub fn reported_identity_matches(
    reported: &ComponentMetadataV1,
    manifest: &ComponentManifestV1,
) -> bool {
    *reported == manifest.metadata()
}
