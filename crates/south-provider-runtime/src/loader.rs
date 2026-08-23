//! The load gates, the sandbox context, and the runtime's own stable errors.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use south_provider_api::{
    CompatibilityMismatchV1, ComponentManifestV1, ComponentMetadataV1, HostExpectationsV1,
    ManifestErrorV1, compatibility_matches,
};
use thiserror::Error;
use wasmtime::component::{Component, ResourceTable};
use wasmtime::{StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use crate::component::SecretSignerV1;
use crate::runtime::ComponentRuntimeV1;

const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_COMPONENT_BYTES: u64 = 64 * 1024 * 1024;

/// Interface prefixes no component may import.
///
/// The rest of WASI gets a locked-down implementation because a std-compiled
/// guest imports it whether its author wanted to or not. The network is
/// different: no protocol component has any business even *asking*, and an
/// empty implementation would leave "did we actually cut this off" to be
/// re-audited on every wasmtime upgrade. Refusing the import makes the
/// question moot.
const FORBIDDEN_IMPORTS: &[&str] = &["wasi:sockets/", "wasi:http/"];

/// Everything one store carries: the locked-down WASI, the resource limits,
/// and the credential boundary for `host.sign`.
pub struct Ctx {
    wasi: WasiCtx,
    table: ResourceTable,
    pub(crate) limits: StoreLimits,
    /// The manifest's `permissions.secrets`, the only names `sign` may see.
    pub(crate) declared_secrets: BTreeSet<String>,
    pub(crate) signer: Arc<dyn SecretSignerV1 + Sync>,
}

impl Ctx {
    pub(crate) fn new(
        memory_bytes: usize,
        declared_secrets: BTreeSet<String>,
        signer: Arc<dyn SecretSignerV1 + Sync>,
    ) -> Self {
        // No preopened directories, no environment, no arguments, no
        // inherited stdio: WASI is provided so a std-compiled guest can
        // instantiate, and it opens onto nothing.
        Self {
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new().memory_size(memory_bytes).build(),
            declared_secrets,
            signer,
        }
    }
}

impl WasiView for Ctx {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

/// Why a file the loader needed could not be read. Typed rather than
/// pre-formatted so a caller can treat "missing" and "too large" differently.
#[derive(Debug, Error)]
pub enum UnreadableReasonV1 {
    #[error("{0}")]
    Io(std::io::Error),
    #[error("file exceeds the {limit} byte limit")]
    TooLarge { limit: u64 },
    #[error("{0}")]
    NotUtf8(std::string::FromUtf8Error),
}

/// Why a component package was refused at load.
///
/// Ordered like the gates: a package that fails early is reported for the
/// early failure, so a broken manifest is not reported as a sandbox
/// violation.
#[derive(Debug, Error)]
pub enum LoadErrorV1 {
    #[error("cannot read `{path}`: {reason}")]
    Unreadable { path: PathBuf, reason: UnreadableReasonV1 },
    #[error("manifest.json is not a manifest: {0}")]
    ManifestSyntax(serde_json::Error),
    #[error("manifest refused: {0}")]
    Manifest(ManifestErrorV1),
    #[error("component is not compatible with this host: {0}")]
    Incompatible(CompatibilityMismatchV1),
    /// The bytes are not a WASM component, or do not export the provider
    /// world — the ABI-mismatch load failure.
    #[error("not a provider component: {0}")]
    NotAComponent(wasmtime::Error),
    #[error("component imports `{0}`, which the sandbox will never provide")]
    ForbiddenImport(String),
    #[error(
        "component reports {}/{} but its manifest declares {}/{}; the package is not what it \
         claims to be",
        reported.name,
        reported.version,
        declared.name,
        declared.version
    )]
    IdentityMismatch { declared: Box<ComponentMetadataV1>, reported: Box<ComponentMetadataV1> },
    #[error("component failed while being probed at load: {0}")]
    Probe(wasmtime::Error),
}

/// A failure of one guest call, in the runtime's own stable vocabulary.
///
/// Deliberately IR-free: the component's own error channel is carried as an
/// opaque JSON string (`Component`), because parsing it would make this crate
/// a consumer of the Canonical IR, which the layering forbids. The typed
/// interpretation happens in the conformance crate's sandbox seam or in a
/// host's adapter layer.
#[derive(Debug, Error)]
pub enum CallErrorV1 {
    /// The component answered on its error channel. By contract the payload
    /// is a `protocol::ErrorEnvelope` as JSON; this crate does not look.
    #[error("component returned an error payload")]
    Component(String),
    /// The call exceeded its wall-clock deadline and was interrupted.
    #[error("component call exceeded its deadline")]
    Deadline,
    /// The component trapped: a panic, an out-of-memory growth failure, or
    /// any other fault. To the caller they are the same thing — a component
    /// that did not answer.
    #[error("component trapped: {0}")]
    Trap(String),
    /// A payload crossing the boundary exceeded the configured ceiling.
    #[error("payload exceeds the {limit} byte boundary limit")]
    PayloadTooLarge { limit: usize },
    /// The per-runtime cap on concurrently live stream instances is reached.
    #[error("stream instance limit reached")]
    StreamLimit,
}

/// Gates ① (manifest) and the import scan: everything decidable before
/// instantiation, for a package read from a directory.
pub fn read_package(
    runtime: &ComponentRuntimeV1,
    dir: &Path,
    expectations: &HostExpectationsV1,
) -> Result<(ComponentManifestV1, Component), LoadErrorV1> {
    let manifest_path = dir.join("manifest.json");
    let manifest_bytes = read_file_limited(&manifest_path, MAX_MANIFEST_BYTES)
        .map_err(|reason| LoadErrorV1::Unreadable { path: manifest_path.clone(), reason })?;
    let manifest_source = String::from_utf8(manifest_bytes).map_err(|error| {
        LoadErrorV1::Unreadable { path: manifest_path, reason: UnreadableReasonV1::NotUtf8(error) }
    })?;
    let manifest = gate_manifest(&manifest_source)?;
    // Refused here the component's bytes are never even read, which is the
    // cheapest possible answer to "this package was built for another host".
    compatibility_matches(&manifest, expectations).map_err(LoadErrorV1::Incompatible)?;

    let wasm_path = dir.join("component.wasm");
    let wasm = read_file_limited(&wasm_path, MAX_COMPONENT_BYTES)
        .map_err(|reason| LoadErrorV1::Unreadable { path: wasm_path, reason })?;
    let component = gate_component(runtime, &wasm)?;

    Ok((manifest, component))
}

/// The filesystem-free twin of [`read_package`]: same gates, same order, for
/// package bytes that were compiled in or fetched elsewhere. Embedded
/// packages earn no shortcut.
pub fn parse_package(
    runtime: &ComponentRuntimeV1,
    manifest_source: &str,
    wasm: &[u8],
    expectations: &HostExpectationsV1,
) -> Result<(ComponentManifestV1, Component), LoadErrorV1> {
    let manifest = gate_manifest(manifest_source)?;
    // The tuple handshake sits between the manifest and the Wasm on purpose: a
    // component built against another host is refused before its bytes are
    // opened, so a stale package cannot reach the import scan or the engine.
    compatibility_matches(&manifest, expectations).map_err(LoadErrorV1::Incompatible)?;
    let component = gate_component(runtime, wasm)?;
    Ok((manifest, component))
}

/// Gate ①, judged before any wasm is even opened. The compatibility-tuple
/// handshake against host expectations is the admitting layer's job (it holds
/// the expected values); this gate covers everything manifest-local.
fn gate_manifest(manifest_source: &str) -> Result<ComponentManifestV1, LoadErrorV1> {
    let manifest: ComponentManifestV1 =
        serde_json::from_str(manifest_source).map_err(LoadErrorV1::ManifestSyntax)?;
    manifest.validate().map_err(LoadErrorV1::Manifest)?;
    Ok(manifest)
}

/// The import scan. Compilation is cached by the engine, so a repeated start
/// deserializes rather than recompiling.
fn gate_component(runtime: &ComponentRuntimeV1, wasm: &[u8]) -> Result<Component, LoadErrorV1> {
    let component = Component::new(runtime.engine(), wasm).map_err(LoadErrorV1::NotAComponent)?;

    for (name, _) in component.component_type().imports(runtime.engine()) {
        if FORBIDDEN_IMPORTS.iter().any(|prefix| name.starts_with(prefix)) {
            return Err(LoadErrorV1::ForbiddenImport(name.to_owned()));
        }
    }

    Ok(component)
}

fn read_file_limited(path: &Path, limit: u64) -> Result<Vec<u8>, UnreadableReasonV1> {
    let file = fs::File::open(path).map_err(UnreadableReasonV1::Io)?;
    let metadata = file.metadata().map_err(UnreadableReasonV1::Io)?;
    if metadata.len() > limit {
        return Err(UnreadableReasonV1::TooLarge { limit });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(limit + 1).read_to_end(&mut bytes).map_err(UnreadableReasonV1::Io)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(UnreadableReasonV1::TooLarge { limit });
    }
    Ok(bytes)
}
