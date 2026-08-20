//! A loaded, gated provider component and its JSON-face calls.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use south_provider_api::{ComponentManifestV1, ComponentMetadataV1};
use wasmtime::Store;
use wasmtime::component::{Component, Linker};

use crate::bindings::ProviderAdapterV2;
use crate::bindings::token_station::adapter::host as wit_host;
use crate::loader::{CallErrorV1, Ctx, LoadErrorV1, parse_package, read_package};
use crate::runtime::{ComponentRuntimeV1, StreamPermit};

/// Resolves a *declared* credential name into a signature.
///
/// The runtime has already checked the name against the manifest before this
/// is called, so an implementation never learns that an undeclared name was
/// asked for. What it must still decide is whether the named credential
/// exists and whether the algorithm is supported.
pub trait SecretSignerV1: Send + 'static {
    /// # Errors
    ///
    /// A message safe to hand back to the guest: it must not contain key
    /// material, because the guest will see it verbatim.
    fn sign(&self, secret_ref: &str, payload: &[u8], algorithm: &str) -> Result<Vec<u8>, String>;
}

/// A signer for hosts with no signing credentials configured. Refuses
/// politely.
pub struct NoSecretsV1;

impl SecretSignerV1 for NoSecretsV1 {
    fn sign(&self, _: &str, _: &[u8], _: &str) -> Result<Vec<u8>, String> {
        Err("this host has no signing credentials configured".to_owned())
    }
}

impl wit_host::Host for Ctx {
    fn sign(
        &mut self,
        secret_ref: String,
        payload: Vec<u8>,
        algorithm: String,
    ) -> Result<Vec<u8>, String> {
        // The manifest boundary, enforced before any signer sees the request.
        if !self.declared_secrets.contains(&secret_ref) {
            return Err(format!(
                "secret `{secret_ref}` is not declared in this component's manifest"
            ));
        }
        self.signer.sign(&secret_ref, &payload, &algorithm)
    }
}

struct InstanceHandle {
    store: Store<Ctx>,
    instance: ProviderAdapterV2,
}

/// A loaded, gated provider component, exposing the world's functions as
/// `json in → json out` (bytes in for the stream chunk).
///
/// Deliberately not a typed adapter: the typed seam lives in the conformance
/// crate's sandbox module and in host adapter layers, so this crate never
/// consumes the Canonical IR (the layering's S3 obligation, enforced by the
/// repository boundary gate).
pub struct LoadedComponentV1 {
    runtime: ComponentRuntimeV1,
    component: Component,
    linker: Arc<Linker<Ctx>>,
    manifest: ComponentManifestV1,
    signer: Arc<dyn SecretSignerV1 + Sync>,
    /// The instance regular calls go through. Streams get their own; see
    /// [`LoadedComponentV1::open_stream`].
    main: Mutex<InstanceHandle>,
}

impl LoadedComponentV1 {
    /// Loads `manifest.json` and `component.wasm` from `dir` and runs the
    /// gates in order: manifest, import scan, instantiate + identity.
    ///
    /// # Errors
    ///
    /// The first [`LoadErrorV1`] encountered, in gate order.
    pub fn load(
        runtime: &ComponentRuntimeV1,
        dir: &Path,
        signer: impl SecretSignerV1 + Sync,
    ) -> Result<Self, LoadErrorV1> {
        let (manifest, component) = read_package(runtime, dir)?;
        Self::admit(runtime, manifest, component, Arc::new(signer))
    }

    /// [`LoadedComponentV1::load`] for package bytes already in memory: same
    /// gates, same order, no filesystem.
    ///
    /// # Errors
    ///
    /// The first [`LoadErrorV1`] encountered, in gate order.
    pub fn load_embedded(
        runtime: &ComponentRuntimeV1,
        manifest_source: &str,
        wasm: &[u8],
        signer: impl SecretSignerV1 + Sync,
    ) -> Result<Self, LoadErrorV1> {
        let (manifest, component) = parse_package(runtime, manifest_source, wasm)?;
        Self::admit(runtime, manifest, component, Arc::new(signer))
    }

    /// The identity gate and onward, shared by both load paths.
    fn admit(
        runtime: &ComponentRuntimeV1,
        manifest: ComponentManifestV1,
        component: Component,
        signer: Arc<dyn SecretSignerV1 + Sync>,
    ) -> Result<Self, LoadErrorV1> {
        let mut linker: Linker<Ctx> = Linker::new(runtime.engine());
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(LoadErrorV1::NotAComponent)?;
        wit_host::add_to_linker::<Ctx, wasmtime::component::HasSelf<Ctx>>(&mut linker, |ctx| ctx)
            .map_err(LoadErrorV1::NotAComponent)?;
        let linker = Arc::new(linker);

        let ctx = component_ctx(runtime, &manifest, &signer);
        let mut handle =
            instantiate(runtime, &component, &linker, ctx).map_err(LoadErrorV1::Probe)?;
        let reported = call_metadata(runtime, &mut handle).map_err(LoadErrorV1::Probe)?;
        if reported != manifest.metadata() {
            return Err(LoadErrorV1::IdentityMismatch {
                declared: Box::new(manifest.metadata()),
                reported: Box::new(reported),
            });
        }

        Ok(Self {
            runtime: runtime.clone(),
            component,
            linker,
            manifest,
            signer,
            main: Mutex::new(handle),
        })
    }

    /// The manifest this component was admitted under.
    #[must_use]
    pub const fn manifest(&self) -> &ComponentManifestV1 {
        &self.manifest
    }

    /// The identity gated at load; equal to what the component reports.
    #[must_use]
    pub fn metadata(&self) -> ComponentMetadataV1 {
        self.manifest.metadata()
    }

    /// `provider-config` JSON → `list<ModelCapability>` JSON.
    ///
    /// # Errors
    ///
    /// See [`CallErrorV1`].
    pub fn call_model_capabilities(&self, config_json: &str) -> Result<String, CallErrorV1> {
        self.bounded(&[config_json])?;
        let config_json = config_json.to_owned();
        self.call(|handle| {
            handle
                .instance
                .token_station_adapter_provider_adapter()
                .call_model_capabilities(&mut handle.store, &config_json)
        })
    }

    /// (`ChatRequest`, `ProviderConfig`) JSON → `HttpRequestDescriptor` JSON.
    ///
    /// # Errors
    ///
    /// See [`CallErrorV1`].
    pub fn call_build_http_request(
        &self,
        request_json: &str,
        config_json: &str,
    ) -> Result<String, CallErrorV1> {
        self.bounded(&[request_json, config_json])?;
        let request_json = request_json.to_owned();
        let config_json = config_json.to_owned();
        self.call(|handle| {
            handle.instance.token_station_adapter_provider_adapter().call_build_http_request(
                &mut handle.store,
                &request_json,
                &config_json,
            )
        })
    }

    /// `HttpResponseParts` JSON → `ChatResponse` JSON.
    ///
    /// # Errors
    ///
    /// See [`CallErrorV1`].
    pub fn call_parse_response(&self, parts_json: &str) -> Result<String, CallErrorV1> {
        self.bounded(&[parts_json])?;
        let parts_json = parts_json.to_owned();
        self.call(|handle| {
            handle
                .instance
                .token_station_adapter_provider_adapter()
                .call_parse_response(&mut handle.store, &parts_json)
        })
    }

    /// `HttpResponseParts` JSON → `ErrorEnvelope` JSON.
    ///
    /// # Errors
    ///
    /// See [`CallErrorV1`].
    pub fn call_map_provider_error(&self, parts_json: &str) -> Result<String, CallErrorV1> {
        self.bounded(&[parts_json])?;
        let parts_json = parts_json.to_owned();
        self.call(|handle| {
            handle
                .instance
                .token_station_adapter_provider_adapter()
                .call_map_provider_error(&mut handle.store, &parts_json)
        })
    }

    /// Opens one stream on its own instance.
    ///
    /// One instance per stream: `parse-stream-chunk` holds the unparsed tail
    /// as instance state, so sharing an instance across streams would
    /// interleave two providers' bodies. Live streams are capped per runtime;
    /// hitting the cap is [`CallErrorV1::StreamLimit`].
    ///
    /// # Errors
    ///
    /// See [`CallErrorV1`].
    pub fn open_stream(&self) -> Result<ComponentStreamV1, CallErrorV1> {
        let Some(permit) = self.runtime.try_acquire_stream() else {
            return Err(CallErrorV1::StreamLimit);
        };
        let ctx = component_ctx(&self.runtime, &self.manifest, &self.signer);
        let handle = instantiate(&self.runtime, &self.component, &self.linker, ctx)
            .map_err(|error| classify_trap(&error))?;
        Ok(ComponentStreamV1 { runtime: self.runtime.clone(), handle, _permit: permit })
    }

    fn bounded(&self, payloads: &[&str]) -> Result<(), CallErrorV1> {
        let limit = self.runtime.limits().max_payload_bytes;
        if payloads.iter().any(|payload| payload.len() > limit) {
            return Err(CallErrorV1::PayloadTooLarge { limit });
        }
        Ok(())
    }

    fn call(
        &self,
        operation: impl FnOnce(&mut InstanceHandle) -> wasmtime::Result<Result<String, String>>,
    ) -> Result<String, CallErrorV1> {
        // A poisoned component stays poisoned; report it as a trap rather
        // than panicking in the host.
        let Ok(mut handle) = self.main.lock() else {
            return Err(CallErrorV1::Trap("component state is poisoned".to_owned()));
        };
        handle.store.set_epoch_deadline(self.runtime.deadline_ticks());

        finish_call(self.runtime.limits().max_payload_bytes, operation(&mut handle))
    }
}

impl fmt::Debug for LoadedComponentV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedComponentV1")
            .field("metadata", &self.manifest.metadata())
            .finish_non_exhaustive()
    }
}

/// One live stream, backed by its own component instance.
pub struct ComponentStreamV1 {
    runtime: ComponentRuntimeV1,
    handle: InstanceHandle,
    _permit: StreamPermit,
}

impl ComponentStreamV1 {
    /// One raw chunk in, `list<StreamEvent>` JSON out. EOF is an empty chunk.
    ///
    /// # Errors
    ///
    /// See [`CallErrorV1`].
    pub fn parse_chunk(&mut self, chunk: &[u8]) -> Result<String, CallErrorV1> {
        let limit = self.runtime.limits().max_payload_bytes;
        if chunk.len() > limit {
            return Err(CallErrorV1::PayloadTooLarge { limit });
        }
        self.handle.store.set_epoch_deadline(self.runtime.deadline_ticks());

        let outcome = self
            .handle
            .instance
            .token_station_adapter_provider_adapter()
            .call_parse_stream_chunk(&mut self.handle.store, chunk);
        finish_call(limit, outcome)
    }
}

impl fmt::Debug for ComponentStreamV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentStreamV1").finish_non_exhaustive()
    }
}

fn component_ctx(
    runtime: &ComponentRuntimeV1,
    manifest: &ComponentManifestV1,
    signer: &Arc<dyn SecretSignerV1 + Sync>,
) -> Ctx {
    Ctx::new(
        runtime.limits().memory_bytes,
        manifest.permissions.secrets.iter().cloned().collect::<BTreeSet<_>>(),
        Arc::clone(signer),
    )
}

fn instantiate(
    runtime: &ComponentRuntimeV1,
    component: &Component,
    linker: &Linker<Ctx>,
    ctx: Ctx,
) -> wasmtime::Result<InstanceHandle> {
    let mut store = Store::new(runtime.engine(), ctx);
    store.limiter(|ctx| &mut ctx.limits);
    store.set_epoch_deadline(runtime.deadline_ticks());

    let instance = ProviderAdapterV2::instantiate(&mut store, component, linker)?;
    Ok(InstanceHandle { store, instance })
}

fn call_metadata(
    runtime: &ComponentRuntimeV1,
    handle: &mut InstanceHandle,
) -> wasmtime::Result<ComponentMetadataV1> {
    handle.store.set_epoch_deadline(runtime.deadline_ticks());
    let reported = handle
        .instance
        .token_station_adapter_provider_adapter()
        .call_metadata(&mut handle.store)?;
    Ok(ComponentMetadataV1 {
        name: reported.name,
        version: reported.version,
        api_version: reported.api_version,
    })
}

/// Maps one raw guest outcome to the runtime's stable vocabulary, bounding
/// both channels of guest-produced text.
fn finish_call(
    limit: usize,
    outcome: wasmtime::Result<Result<String, String>>,
) -> Result<String, CallErrorV1> {
    match outcome {
        Ok(Ok(payload)) if payload.len() > limit => Err(CallErrorV1::PayloadTooLarge { limit }),
        Ok(Ok(payload)) => Ok(payload),
        Ok(Err(error_json)) if error_json.len() > limit => {
            Err(CallErrorV1::PayloadTooLarge { limit })
        }
        Ok(Err(error_json)) => Err(CallErrorV1::Component(error_json)),
        Err(trap) => Err(classify_trap(&trap)),
    }
}

/// A deadline interrupt is the one trap the caller can act on differently
/// (extend the budget, shed load); everything else is a component that did
/// not answer.
fn classify_trap(error: &wasmtime::Error) -> CallErrorV1 {
    if let Some(trap) = error.downcast_ref::<wasmtime::Trap>()
        && *trap == wasmtime::Trap::Interrupt
    {
        return CallErrorV1::Deadline;
    }
    CallErrorV1::Trap(format!("{error:#}"))
}
