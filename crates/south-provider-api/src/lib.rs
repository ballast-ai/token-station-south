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
//! The manifest's `api_version` declares the world it was built against, and
//! must name a world this South knows ([`KNOWN_WORLDS`]); the suite name, the
//! capability vocabulary, and the auth arms are validated against that world
//! (2026-08-27 manifest-schema record, D1). A breaking ABI change ships a
//! `-v3` world alongside `-v2`; it never edits `-v2` in place, because
//! installed components are compiled artifacts that cannot be migrated.

mod config;
mod credentials;
mod instances;
mod manifest;
mod values;

pub use config::{ConfigErrorV1, ConfigKeyV1, EndpointValuesErrorV1, ValueSyntaxV1};
pub use credentials::{
    AttributeV1, CREDENTIAL_RECIPE_SCHEMA, ConstantV1, CredentialFieldV1, CredentialsV1,
    EncodingV1, ExtractV1, FieldRefV1, HOST_MAX_TTL_SECONDS, HOST_MIN_TTL_SECONDS, ImportRuleV1,
    JwtAlgorithmV1, JwtClaimV1, MAX_PRESENT_CANDIDATES, MAX_RECIPE_STEPS, PredicateV1,
    PresentCandidateV1, PresentFieldV1, PresentV1, RecipeV1, SeedClockV1, SeedV1, SelectRuleV1,
    SlotV1, StatusActionV1, StepAuthV1, StepKindV1, StepMethodV1, StepV1, ValueSourceV1,
    WithoutRefreshMaterialV1,
};

// B7a (query, quota, user-agent): provider instances declared in the manifest.
pub use instances::{
    DECLARED_QUERY_DENIED_FRAGMENTS, DECLARED_QUERY_DENIED_NAMES, MAX_DECLARED_QUERY_NAME_BYTES,
    MAX_DECLARED_QUERY_PARAMETERS, MAX_QUERY_ENUM_VALUES, MAX_QUOTA_HEADER_NAME_BYTES,
    MAX_USER_AGENT_BYTES, QUOTA_HEADER_DENIED_NAMES, QUOTA_METADATA_FIELDS,
    QueryParameterDeclarationV1, QueryValueSyntaxV1, QuotaHeaderDeclarationV1,
    SANCTIONED_QUERY_NAMES, is_user_agent_value,
};

pub use manifest::{
    COMPONENT_BEHAVIOR_SUITE, CompatibilityDeclarationV1, CompatibilityMismatchV1,
    CompatibilityMismatchV2, CompatibilityTupleV1, ComponentManifestV1, ComponentMetadataV1,
    ComponentPermissionsV1, ConformanceSpecV1, EMBEDDINGS_BEHAVIOR_SUITE, EMBEDDINGS_CAPABILITIES,
    EMBEDDINGS_WIT_PACKAGE, EMBEDDINGS_WORLD, EMBEDDINGS_WORLD_SCHEMA, HostExpectationsV1,
    HostRangeV1, KNOWN_WORLDS, MAX_OUTPUT_CAP_LOCATIONS, ManifestErrorV1, ModelLocationV1,
    PROVIDER_AUTH_ARMS, PROVIDER_CAPABILITIES, PROVIDER_WORLD, PROVIDER_WORLD_SCHEMA, RUNTIME_ABI,
    RequestFactsV1, SIGNED_HEADER_NAMES, SigningSchemeV1, SigningV1, StreamFramingV1,
    StreamLocationV1, TASK_AUTH_ARMS, TASK_BEHAVIOR_SUITE, TASK_BEHAVIOR_SUITE_V2,
    TASK_CAPABILITIES, TASK_REQUIRED_CAPABILITIES, TASK_WIT_PACKAGE, TASK_WIT_PACKAGE_V2,
    TASK_WORLD, TASK_WORLD_SCHEMA, TASK_WORLD_SCHEMA_V2, TASK_WORLD_V2, TemplateParamV1,
    UsageEvidenceV1, WIT_PACKAGE, WorldSchemaV1, compatibility_admits, compatibility_matches,
    known_world, validate_component_name, validate_package_relative_path,
};

// The component value channel (Q14, host-zero-vendor-boundary §13.8).
pub use values::{DeclaredValuesErrorV1, HOST_VALUE_ATTEMPT_ID, HOST_VALUES};

// Declared secret headers (B7a, host-zero-vendor-boundary §10).
pub use manifest::{
    MAX_SECRET_HEADER_NAME_BYTES, MAX_SECRET_HEADERS, UNDECLARABLE_SECRET_HEADER_NAMES,
    validate_secret_header_name,
};

/// The component ABI, as WIT source.
///
/// Embedded so hosts and component authors can hand it to `wit-bindgen`
/// without depending on this crate's source layout. Tests in this crate
/// assert the package name, the world name, the sandbox posture (no
/// `wasi:filesystem` / `wasi:sockets`) and the bytes-typed chunk entry point
/// never drift from the constants the manifest validates against.
pub const ADAPTER_WIT: &str = include_str!("../wit/provider-adapter.wit");

/// The task component ABI, as WIT source.
///
/// Embedded for the same reason as [`ADAPTER_WIT`], and kept in its own
/// package so the two worlds version independently (2026-09-18
/// task-adapter-world record, D1).
pub const TASK_ADAPTER_WIT: &str = include_str!("../wit/task-adapter.wit");

/// Pure task-v2 exports with bounded recovery facts and explicit render context.
pub const TASK_ADAPTER_V2_WIT: &str = include_str!("../wit/task-adapter-v2.wit");

/// The embeddings component ABI, as WIT source: pure exports and no host
/// import, in its own package so it versions independently of the chat and
/// task worlds (2026-09-30 embeddings-contract record, D2).
pub const EMBEDDINGS_ADAPTER_WIT: &str = include_str!("../wit/embeddings-adapter.wit");
