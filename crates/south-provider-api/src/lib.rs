#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Provider component API and WIT ownership boundary.
//!
//! This crate owns the southbound component ABI: the WIT package
//! `token-station:adapter@2.0.0` with its single `provider-adapter-v2` world,
//! and the `manifest.json` schema every provider component ships (gate ① of
//! the conformance layering). The design record is
//! `docs/design/2026-08-21-provider-api-promotion.md`; the frozen boundary
//! contract it implements is `docs/design/2026-08-21-canonical-ir-inventory.md`
//! (S0).
//!
//! # What crosses the boundary
//!
//! Component functions take and return JSON documents named by canonical type,
//! not WIT records — the Canonical IR is defined once, in the community
//! `crates/protocol`, and distributed at fixed revisions by the kernel mirror;
//! mirroring it into WIT would create a second definition to keep in step, and
//! WIT cannot express the open JSON the IR carries. The one exception is the
//! stream-chunk entry point, which takes raw bounded bytes (S0 ruling D2) so
//! binary eventstream dialects cross the boundary without a base64 tax on
//! every SSE chunk.
//!
//! This crate therefore depends on no IR crate and no other south crate: it
//! knows the *names* of the documents, not their contents. Typed judgement is
//! the conformance suite's job (gate ②), through a fixed kernel revision.
//!
//! # Versioning
//!
//! The manifest's `api_version` must equal the world it was built against
//! ([`PROVIDER_WORLD`]). A breaking ABI change ships a `-v3` world alongside
//! `-v2`; it never edits `-v2` in place, because installed components are
//! compiled artifacts that cannot be migrated.

mod manifest;

pub use manifest::{
    AuthArmV1, COMPONENT_BEHAVIOR_SUITE, CompatibilityDeclarationV1, CompatibilityTupleV1,
    ComponentCapabilityV1, ComponentManifestV1, ComponentMetadataV1, ComponentPermissionsV1,
    ConformanceSpecV1, ManifestErrorV1, PROVIDER_WORLD, WIT_PACKAGE, validate_component_name,
    validate_package_relative_path,
};

/// The component ABI, as WIT source.
///
/// Embedded so hosts and component authors can hand it to `wit-bindgen`
/// without depending on this crate's source layout. Tests in this crate
/// assert the package name, the world name, the sandbox posture (no
/// `wasi:filesystem` / `wasi:sockets`) and the bytes-typed chunk entry point
/// never drift from the constants the manifest validates against.
pub const ADAPTER_WIT: &str = include_str!("../wit/provider-adapter.wit");
