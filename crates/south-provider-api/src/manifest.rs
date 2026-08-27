use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The WIT package this crate owns (compatibility tuple field 3).
pub const WIT_PACKAGE: &str = "token-station:adapter@2.0.0";

/// The provider world name, doubling as the manifest `api_version`
/// (compatibility tuple field 4).
pub const PROVIDER_WORLD: &str = "provider-adapter-v2";

/// The component-behavior conformance suite name (compatibility tuple 6).
///
/// Gate ② judges every provider component under this identifier. Frozen here
/// so the manifest validates exactly; S2 builds the suite under this name.
pub const COMPONENT_BEHAVIOR_SUITE: &str = "south.provider-component.v1";

/// A component world this South knows, and the properties gate ① validates a
/// manifest against once the manifest has declared which world it is for
/// (2026-08-27 manifest-schema record, D1).
///
/// The suite name, the capability vocabulary, and the auth arms are
/// properties of the declared world, not constants of the schema. Each
/// vocabulary is closed on purpose: a word a world does not know is a version
/// mismatch, not a request to honour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldSchemaV1 {
    /// The world name a manifest declares in `api_version` (tuple 4).
    pub world: &'static str,
    /// The WIT package that world's functions live in (tuple 3).
    pub wit_package: &'static str,
    /// The suite that world is judged by (tuple 6).
    pub behavior_suite: &'static str,
    /// That world's capability vocabulary.
    pub capabilities: &'static [&'static str],
    /// That world's auth arm vocabulary.
    pub auth_arms: &'static [&'static str],
}

/// The provider world's capability vocabulary.
///
/// `chat` and `stream` name world functions; `tool_call` and `json_schema`
/// name `ChatRequest` fields the component promises to translate. Closed by
/// construction — v2 defines no function or field beyond these.
pub const PROVIDER_CAPABILITIES: &[&str] = &["chat", "stream", "tool_call", "json_schema"];

/// The provider world's auth arm vocabulary.
///
/// - `bearer`: `Authorization: Bearer <resolved secret>`, including
///   host-minted (OAuth-shaped) credentials whose product is a bearer token.
/// - `header_secret`: the resolved secret travels verbatim in one sanctioned
///   provider header.
/// - `oauth`: the host exchanges the named grant for a token before the funds
///   marker and presents it as a bearer token; the component never sees the
///   exchange.
/// - `host_signed`: the host's request finalizer signs every request after
///   the component returns its descriptor; the descriptor itself carries no
///   auth, and the manifest's `emits` set is the contract the finalizer's
///   output is diffed against (2026-08-27 manifest-schema record, D2–D3).
pub const PROVIDER_AUTH_ARMS: &[&str] = &["bearer", "header_secret", "oauth", "host_signed"];

/// The signed-header vocabulary a `host_signed` manifest may name in `emits`.
///
/// The wire names of the host half's frozen `SignedHeaderV1` enum
/// (`south-contracts`), repeated here because this crate deliberately depends
/// on no other south crate; a conformance-crate test pins the two lists
/// together.
pub const SIGNED_HEADER_NAMES: &[&str] =
    &["authorization", "x-amz-date", "x-amz-content-sha256", "x-amz-security-token"];

/// The provider world, as gate ① validates it.
pub const PROVIDER_WORLD_SCHEMA: WorldSchemaV1 = WorldSchemaV1 {
    world: PROVIDER_WORLD,
    wit_package: WIT_PACKAGE,
    behavior_suite: COMPONENT_BEHAVIOR_SUITE,
    capabilities: PROVIDER_CAPABILITIES,
    auth_arms: PROVIDER_AUTH_ARMS,
};

/// Every world this South can admit. The task adapter world (#52) enters
/// here when its vocabulary ships.
pub const KNOWN_WORLDS: &[WorldSchemaV1] = &[PROVIDER_WORLD_SCHEMA];

/// Resolves a manifest's declared `api_version` to a world this South knows.
#[must_use]
pub fn known_world(api_version: &str) -> Option<&'static WorldSchemaV1> {
    KNOWN_WORLDS.iter().find(|schema| schema.world == api_version)
}

/// What the sandbox must grant.
///
/// `network` and `filesystem` exist so a manifest can *ask*, and be refused
/// with a named reason; silently ignoring the request would let an author
/// believe it was granted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentPermissionsV1 {
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub filesystem: bool,
    /// Names of credentials, never credentials. Each must look like a
    /// reference name, which is what stops a key being pasted here.
    #[serde(default)]
    pub secrets: Vec<String>,
}

/// Where the component's conformance fixtures live, and which suite gates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceSpecV1 {
    pub required_suite: String,
    /// Directory inside the component package, e.g. `fixtures/`.
    pub fixtures: String,
}

/// The versions this component was built and verified against — the manifest
/// half of the compatibility tuple that is not already a top-level field.
///
/// Together with `version` (tuple 7), `api_version` (tuple 4) and
/// `conformance.required_suite` (tuple 6), this completes the seven-field
/// tuple of the S0 contract freeze. The runtime refuses any mismatch at load
/// time — refusal, never silent degradation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityDeclarationV1 {
    /// Tuple 1 — the IR revision, `token-station-protocol@<crate>/<kernel-tag>`,
    /// e.g. `token-station-protocol@0.3.0/v0.2.0`.
    pub ir_schema_id: String,
    /// Tuple 2a — the kernel distribution release, e.g. `0.2.0`.
    pub kernel_version: String,
    /// Tuple 2b — the kernel's mirrored upstream commit (40 lowercase hex).
    pub kernel_revision: String,
    /// Tuple 3 — must equal [`WIT_PACKAGE`].
    pub wit_package: String,
    /// Tuple 5 — the south runtime version the component was verified with.
    pub south_runtime: String,
}

/// A provider component package's `manifest.json`.
///
/// Untrusted third-party input, so it parses permissively and
/// [`ComponentManifestV1::validate`] rejects with an enumerable reason — a
/// registry has to record *why* it turned a package away. Unknown fields are
/// refused (`deny_unknown_fields`): a misspelt key must fail loudly, not
/// deserialize into a defaulted shape the record-keeping then lies about, and
/// a new manifest field must arrive through a schema bump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentManifestV1 {
    pub name: String,
    /// Tuple 7 — the component's own version, a `major.minor.patch` triple.
    pub version: String,
    /// Tuple 4 — the world this manifest is for; must name a world in
    /// [`KNOWN_WORLDS`].
    pub api_version: String,
    /// Provider dialect families this component translates, e.g.
    /// `openai-compatible`. Also names the usage cache convention the host's
    /// pricing folds (S0 ruling D1).
    pub providers: Vec<String>,
    /// Words from the declared world's capability vocabulary; a word the
    /// world does not know is refused with its name.
    pub capabilities: BTreeSet<String>,
    /// Auth arms the component's descriptors may use, from the declared
    /// world's vocabulary; empty means the component never attaches a
    /// credential (unauthenticated upstreams).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub auth_arms: BTreeSet<String>,
    /// The headers a `host_signed` component promises its host's finalizer
    /// will emit — the allow-list South diffs the finalizer's output against,
    /// in both directions. Required non-empty with the `host_signed` arm,
    /// refused without it. A `Vec`, not a set, so a duplicate is refused by
    /// name rather than silently collapsed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emits: Vec<String>,
    pub permissions: ComponentPermissionsV1,
    pub conformance: ConformanceSpecV1,
    pub compatibility: CompatibilityDeclarationV1,
}

/// The identity a loaded component must report back from `metadata()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentMetadataV1 {
    pub name: String,
    pub version: String,
    pub api_version: String,
}

/// A borrowed view of the complete seven-field compatibility tuple, in the
/// S0 contract order, for the runtime handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatibilityTupleV1<'m> {
    pub ir_schema_id: &'m str,
    pub kernel_version: &'m str,
    pub kernel_revision: &'m str,
    pub wit_package: &'m str,
    pub wit_world: &'m str,
    pub south_runtime: &'m str,
    pub conformance_suite: &'m str,
    pub component_version: &'m str,
}

impl ComponentManifestV1 {
    /// The identity a loaded component must report back from `metadata()`.
    #[must_use]
    pub fn metadata(&self) -> ComponentMetadataV1 {
        ComponentMetadataV1 {
            name: self.name.clone(),
            version: self.version.clone(),
            api_version: self.api_version.clone(),
        }
    }

    /// The complete compatibility tuple this manifest declares.
    #[must_use]
    pub fn compatibility_tuple(&self) -> CompatibilityTupleV1<'_> {
        CompatibilityTupleV1 {
            ir_schema_id: &self.compatibility.ir_schema_id,
            kernel_version: &self.compatibility.kernel_version,
            kernel_revision: &self.compatibility.kernel_revision,
            wit_package: &self.compatibility.wit_package,
            wit_world: &self.api_version,
            south_runtime: &self.compatibility.south_runtime,
            conformance_suite: &self.conformance.required_suite,
            component_version: &self.version,
        }
    }

    /// Checks everything gate ① requires before a package may be admitted.
    ///
    /// Order is deliberate: identity first (which resolves the declared
    /// world), then the sandbox, then the world's vocabulary, then role
    /// coherence, then conformance, then the compatibility declaration — so a
    /// package missing a name is not reported as a tuple violation.
    ///
    /// # Errors
    ///
    /// Returns the first [`ManifestErrorV1`] found.
    pub fn validate(&self) -> Result<(), ManifestErrorV1> {
        let world = self.validate_identity()?;
        self.validate_sandbox()?;
        self.validate_vocabulary(world)?;
        self.validate_signing()?;
        self.validate_role(world)?;
        self.validate_conformance(world)?;
        self.validate_compatibility(world)
    }

    fn validate_identity(&self) -> Result<&'static WorldSchemaV1, ManifestErrorV1> {
        if self.name.is_empty() {
            return Err(ManifestErrorV1::MissingName);
        }
        validate_component_name(&self.name)?;
        if !is_semver_triple(&self.version) {
            return Err(ManifestErrorV1::InvalidVersion(self.version.clone()));
        }
        known_world(&self.api_version)
            .ok_or_else(|| ManifestErrorV1::ApiVersionIsNotAKnownWorld(self.api_version.clone()))
    }

    fn validate_sandbox(&self) -> Result<(), ManifestErrorV1> {
        if self.permissions.network {
            return Err(ManifestErrorV1::NetworkPermissionDenied);
        }
        if self.permissions.filesystem {
            return Err(ManifestErrorV1::FilesystemPermissionDenied);
        }
        for secret in &self.permissions.secrets {
            if !is_secret_ref_name(secret) {
                return Err(ManifestErrorV1::SecretIsNotAReferenceName(secret.clone()));
            }
        }
        Ok(())
    }

    fn validate_vocabulary(&self, world: &WorldSchemaV1) -> Result<(), ManifestErrorV1> {
        for capability in &self.capabilities {
            if !world.capabilities.contains(&capability.as_str()) {
                return Err(ManifestErrorV1::CapabilityIsNotInTheWorldVocabulary {
                    capability: capability.clone(),
                    world: world.world.to_owned(),
                });
            }
        }
        for auth_arm in &self.auth_arms {
            if !world.auth_arms.contains(&auth_arm.as_str()) {
                return Err(ManifestErrorV1::AuthArmIsNotInTheWorldVocabulary {
                    auth_arm: auth_arm.clone(),
                    world: world.world.to_owned(),
                });
            }
        }
        Ok(())
    }

    /// The `host_signed` coherence rules (2026-08-27 manifest-schema record,
    /// D2–D3).
    ///
    /// A signed request's descriptor carries no auth, so the manifest
    /// declaration is the only thing telling the host these requests are
    /// finalized. Making that indistinguishability unreachable means the arm
    /// admits no mixture: `host_signed` stands alone, its `emits` allow-list
    /// is non-empty, duplicate-free, and drawn from the frozen signed-header
    /// vocabulary — the same shapes, refused with the same words, as the host
    /// half's `SignedHeaderSetV1`.
    fn validate_signing(&self) -> Result<(), ManifestErrorV1> {
        if !self.auth_arms.contains("host_signed") {
            if let Some(header) = self.emits.first() {
                return Err(ManifestErrorV1::EmitsRequireTheHostSignedArm(header.clone()));
            }
            return Ok(());
        }
        if self.auth_arms.len() > 1 {
            return Err(ManifestErrorV1::HostSignedAdmitsNoOtherArm);
        }
        if self.emits.is_empty() {
            return Err(ManifestErrorV1::HostSignedNamesNoHeader);
        }
        let mut seen = BTreeSet::new();
        for header in &self.emits {
            if !SIGNED_HEADER_NAMES.contains(&header.as_str()) {
                return Err(ManifestErrorV1::EmitIsNotASignedHeader(header.clone()));
            }
            if !seen.insert(header.as_str()) {
                return Err(ManifestErrorV1::HostSignedNamesAHeaderTwice(header.clone()));
            }
        }
        Ok(())
    }

    fn validate_role(&self, world: &WorldSchemaV1) -> Result<(), ManifestErrorV1> {
        if world.world == PROVIDER_WORLD {
            if !self.capabilities.contains("chat") {
                return Err(ManifestErrorV1::ChatCapabilityRequired);
            }
            if self.providers.is_empty() {
                return Err(ManifestErrorV1::ProviderFamilyRequired);
            }
        }
        for provider in &self.providers {
            validate_component_name(provider)
                .map_err(|_| ManifestErrorV1::InvalidProviderFamily(provider.clone()))?;
        }
        Ok(())
    }

    fn validate_conformance(&self, world: &WorldSchemaV1) -> Result<(), ManifestErrorV1> {
        if self.conformance.required_suite != world.behavior_suite {
            return Err(ManifestErrorV1::ConformanceSuiteIsNotTheWorldSuite {
                declared: self.conformance.required_suite.clone(),
                world: world.world.to_owned(),
                expected: world.behavior_suite.to_owned(),
            });
        }
        if self.conformance.fixtures.is_empty() {
            return Err(ManifestErrorV1::MissingFixtures);
        }
        validate_package_relative_path(&self.conformance.fixtures)?;
        Ok(())
    }

    fn validate_compatibility(&self, world: &WorldSchemaV1) -> Result<(), ManifestErrorV1> {
        if self.compatibility.wit_package != world.wit_package {
            return Err(ManifestErrorV1::WitPackageIsNotTheWorldPackage {
                declared: self.compatibility.wit_package.clone(),
                world: world.world.to_owned(),
                expected: world.wit_package.to_owned(),
            });
        }
        if !is_ir_schema_id(&self.compatibility.ir_schema_id) {
            return Err(ManifestErrorV1::InvalidIrSchemaId(
                self.compatibility.ir_schema_id.clone(),
            ));
        }
        if !is_semver_triple(&self.compatibility.kernel_version) {
            return Err(ManifestErrorV1::InvalidKernelVersion(
                self.compatibility.kernel_version.clone(),
            ));
        }
        if !is_commit_hash(&self.compatibility.kernel_revision) {
            return Err(ManifestErrorV1::InvalidKernelRevision(
                self.compatibility.kernel_revision.clone(),
            ));
        }
        if !is_semver_triple(&self.compatibility.south_runtime) {
            return Err(ManifestErrorV1::InvalidSouthRuntimeVersion(
                self.compatibility.south_runtime.clone(),
            ));
        }
        Ok(())
    }
}

/// Validates the one-component package identity: lowercase ASCII kebab-case,
/// alphanumeric at both ends, at most 64 bytes.
///
/// # Errors
///
/// Returns [`ManifestErrorV1::InvalidName`] otherwise.
pub fn validate_component_name(value: &str) -> Result<(), ManifestErrorV1> {
    let bytes = value.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= 64
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
    if valid { Ok(()) } else { Err(ManifestErrorV1::InvalidName(value.to_owned())) }
}

/// Validates a normalized package-relative path without consulting the file
/// system. Runtime walkers still reject symlinks and special files.
///
/// # Errors
///
/// Returns [`ManifestErrorV1::InvalidFixturesPath`] for absolute,
/// parent-relative, platform-ambiguous, empty-component, control-character,
/// or over-deep paths.
pub fn validate_package_relative_path(value: &str) -> Result<(), ManifestErrorV1> {
    let normalized = value.strip_suffix('/').unwrap_or(value);
    let components: Vec<_> = normalized.split('/').collect();
    let invalid = value.is_empty()
        || value.len() > 256
        || value.starts_with('/')
        || value.ends_with("//")
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || components.len() > 16
        || components
            .iter()
            .any(|component| component.is_empty() || matches!(*component, "." | ".."));
    if invalid { Err(ManifestErrorV1::InvalidFixturesPath(value.to_owned())) } else { Ok(()) }
}

/// A reference name is lowercase alphanumeric with underscores, starting with
/// a letter. Deliberately narrow: a real credential — `sk-live-abc`, a base64
/// blob, a JWT — cannot satisfy it, so pasting one fails at admission rather
/// than leaking into a registry.
fn is_secret_ref_name(value: &str) -> bool {
    let mut chars = value.chars();
    let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    starts_with_letter && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_semver_triple(value: &str) -> bool {
    let mut parts = value.split('.');
    let triple = [parts.next(), parts.next(), parts.next()];
    parts.next().is_none()
        && triple.iter().all(|part| {
            part.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// `token-station-protocol@<major.minor.patch>/v<major.minor.patch>` — crate
/// revision, then the kernel distribution tag it rode in on.
fn is_ir_schema_id(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("token-station-protocol@") else {
        return false;
    };
    let Some((crate_version, tag)) = rest.split_once('/') else {
        return false;
    };
    let Some(tag_version) = tag.strip_prefix('v') else {
        return false;
    };
    is_semver_triple(crate_version) && is_semver_triple(tag_version)
}

fn is_commit_hash(value: &str) -> bool {
    value.len() == 40
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Why gate ① refused a manifest.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ManifestErrorV1 {
    #[error("manifest declares no name")]
    MissingName,
    #[error("component name `{0}` must be one lowercase kebab-case component of at most 64 bytes")]
    InvalidName(String),
    #[error("version `{0}` is not a `major.minor.patch` triple")]
    InvalidVersion(String),
    #[error("api_version `{0}` is not a world this South knows")]
    ApiVersionIsNotAKnownWorld(String),
    #[error("capability `{capability}` is not in the `{world}` world's vocabulary")]
    CapabilityIsNotInTheWorldVocabulary { capability: String, world: String },
    #[error("auth arm `{auth_arm}` is not in the `{world}` world's vocabulary")]
    AuthArmIsNotInTheWorldVocabulary { auth_arm: String, world: String },
    #[error("emits names `{0}` but the manifest does not declare the `host_signed` arm")]
    EmitsRequireTheHostSignedArm(String),
    #[error(
        "a host-signed component's requests are all finalized; `host_signed` admits no other arm"
    )]
    HostSignedAdmitsNoOtherArm,
    #[error("a host-signed declaration must name at least one header")]
    HostSignedNamesNoHeader,
    #[error("`{0}` is not a signed header the finalizer vocabulary permits")]
    EmitIsNotASignedHeader(String),
    #[error("a host-signed declaration must not name the same header twice; `{0}` repeats")]
    HostSignedNamesAHeaderTwice(String),
    #[error("components have no network; the host makes every request")]
    NetworkPermissionDenied,
    #[error("components have no file system")]
    FilesystemPermissionDenied,
    #[error("`{0}` is not a credential reference name; declare a name, never a credential")]
    SecretIsNotAReferenceName(String),
    #[error("every provider component must support `chat`")]
    ChatCapabilityRequired,
    #[error("a provider component must declare at least one provider family")]
    ProviderFamilyRequired,
    #[error("provider family `{0}` must be one lowercase kebab-case component")]
    InvalidProviderFamily(String),
    #[error(
        "conformance.required_suite `{declared}` is not the suite the `{world}` world is judged \
         by (`{expected}`)"
    )]
    ConformanceSuiteIsNotTheWorldSuite { declared: String, world: String, expected: String },
    #[error("conformance.fixtures is empty")]
    MissingFixtures,
    #[error("conformance.fixtures `{0}` must be a normalized relative package path")]
    InvalidFixturesPath(String),
    #[error(
        "compatibility.wit_package `{declared}` is not the `{world}` world's package \
         (`{expected}`)"
    )]
    WitPackageIsNotTheWorldPackage { declared: String, world: String, expected: String },
    #[error("compatibility.ir_schema_id `{0}` is not `token-station-protocol@<x.y.z>/v<x.y.z>`")]
    InvalidIrSchemaId(String),
    #[error("compatibility.kernel_version `{0}` is not a `major.minor.patch` triple")]
    InvalidKernelVersion(String),
    #[error("compatibility.kernel_revision `{0}` is not a 40-hex commit")]
    InvalidKernelRevision(String),
    #[error("compatibility.south_runtime `{0}` is not a `major.minor.patch` triple")]
    InvalidSouthRuntimeVersion(String),
}

// -- compatibility admission -------------------------------------------------

/// What the admitting host was built against, for the tuple handshake.
///
/// The manifest-side constants (`wit_package`, world name, suite name) are
/// already exact-validated by `accepts_manifest` (in the conformance crate,
/// which this one deliberately does not depend on); these four are the values
/// only a live host knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostExpectationsV1 {
    pub ir_schema_id: String,
    pub kernel_version: String,
    pub kernel_revision: String,
    pub south_runtime: String,
}

/// Why the compatibility handshake refused a component.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CompatibilityMismatchV1 {
    #[error("component was built against IR `{declared}`; this host distributes `{expected}`")]
    IrSchema { declared: String, expected: String },
    #[error("component was built against kernel `{declared}`; this host distributes `{expected}`")]
    KernelVersion { declared: String, expected: String },
    #[error("component pins kernel revision `{declared}`; this host distributes `{expected}`")]
    KernelRevision { declared: String, expected: String },
    #[error("component was verified with south runtime `{declared}`; this host runs `{expected}`")]
    SouthRuntime { declared: String, expected: String },
}

/// Gate ①, tuple half: the manifest's compatibility declaration must equal
/// what the admitting host was built against. Refusal, never silent
/// degradation, and never a partial acceptance.
///
/// # Errors
///
/// Returns the first [`CompatibilityMismatchV1`] found, in tuple order.
pub fn compatibility_matches(
    manifest: &ComponentManifestV1,
    expectations: &HostExpectationsV1,
) -> Result<(), CompatibilityMismatchV1> {
    let declared = manifest.compatibility_tuple();
    if declared.ir_schema_id != expectations.ir_schema_id {
        return Err(CompatibilityMismatchV1::IrSchema {
            declared: declared.ir_schema_id.to_owned(),
            expected: expectations.ir_schema_id.clone(),
        });
    }
    if declared.kernel_version != expectations.kernel_version {
        return Err(CompatibilityMismatchV1::KernelVersion {
            declared: declared.kernel_version.to_owned(),
            expected: expectations.kernel_version.clone(),
        });
    }
    if declared.kernel_revision != expectations.kernel_revision {
        return Err(CompatibilityMismatchV1::KernelRevision {
            declared: declared.kernel_revision.to_owned(),
            expected: expectations.kernel_revision.clone(),
        });
    }
    if declared.south_runtime != expectations.south_runtime {
        return Err(CompatibilityMismatchV1::SouthRuntime {
            declared: declared.south_runtime.to_owned(),
            expected: expectations.south_runtime.clone(),
        });
    }
    Ok(())
}
