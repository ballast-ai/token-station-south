//! The host-side inverse of [`abi`](crate::abi): a typed
//! [`ProviderComponentV1`] over a sandboxed `.wasm` instance.
//!
//! This is where the Canonical IR meets the runtime's deliberately opaque
//! JSON face — the runtime crate never parses the IR (its layering
//! obligation), so the typed seam lives here, in the crate that already owns
//! the sanctioned IR edge. Gate ② can therefore judge a sandboxed component
//! exactly as it judges a native one, which is the S3 acceptance criterion:
//! same suite, same fixtures, byte-identical outputs.
//!
//! Behind the `sandbox` cargo feature so that guests (which depend on this
//! crate for the [`abi`](crate::abi) shims and compile to `wasm32-wasip2`)
//! never pull wasmtime into their build.

use south_provider_runtime::{CallErrorV1, ComponentStreamV1, LoadedComponentV1};
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorCode, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ModelCapability, ProviderConfig, StreamEvent,
};

use crate::abi::parse_error_envelope;
use south_contracts::{HostMintedValuesV1, TaskObservationV1};

use crate::component::{
    ComponentResultV1, ProviderComponentV1, StreamParserV1, SubmitOutcomeV1, TaskComponentV1,
};
use crate::task_json::{ObservationInput, observation_json};
use south_provider_api::ComponentMetadataV1;

/// A sandboxed component presented through the typed seam.
#[derive(Debug)]
pub struct SandboxedComponentV1 {
    component: LoadedComponentV1,
}

impl SandboxedComponentV1 {
    #[must_use]
    pub const fn new(component: LoadedComponentV1) -> Self {
        Self { component }
    }

    /// The loaded component, for callers that need the JSON face too.
    #[must_use]
    pub const fn inner(&self) -> &LoadedComponentV1 {
        &self.component
    }
}

fn internal(detail: impl std::fmt::Display) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, detail.to_string())
}

/// Maps a runtime failure to the seam's error shape. The component's own
/// error channel is parsed here — the one place the opaque payload becomes
/// typed.
fn seam_error(error: &CallErrorV1) -> ErrorEnvelope {
    match error {
        CallErrorV1::Component(error_json) => parse_error_envelope(error_json),
        other => internal(other),
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> ComponentResultV1<String> {
    serde_json::to_string(value).map_err(|error| internal(format_args!("serialize: {error}")))
}

fn from_json<T: for<'de> serde::Deserialize<'de>>(json: &str) -> ComponentResultV1<T> {
    serde_json::from_str(json).map_err(|error| {
        internal(format_args!("component returned JSON that is not the canonical form: {error}"))
    })
}

impl ProviderComponentV1 for SandboxedComponentV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        self.component.metadata()
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ModelCapability>> {
        let out = self
            .component
            .call_model_capabilities(&to_json(config)?)
            .map_err(|error| seam_error(&error))?;
        from_json(&out)
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        let out = self
            .component
            .call_build_http_request(&to_json(request)?, &to_json(config)?)
            .map_err(|error| seam_error(&error))?;
        from_json(&out)
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        let out = self
            .component
            .call_parse_response(&to_json(parts)?)
            .map_err(|error| seam_error(&error))?;
        from_json(&out)
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        let out = self
            .component
            .call_map_provider_error(&to_json(parts)?)
            .map_err(|error| seam_error(&error))?;
        from_json(&out)
    }

    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        match self.component.open_stream() {
            Ok(stream) => Box::new(SandboxedStreamParser { stream }),
            // A stream whose instance could not be created fails every chunk
            // with the open error instead of panicking mid-stream.
            Err(error) => Box::new(BrokenStreamParser { envelope: seam_error(&error) }),
        }
    }
}

struct SandboxedStreamParser {
    stream: ComponentStreamV1,
}

impl StreamParserV1 for SandboxedStreamParser {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        let out = self.stream.parse_chunk(chunk).map_err(|error| seam_error(&error))?;
        from_json(&out)
    }
}

struct BrokenStreamParser {
    envelope: ErrorEnvelope,
}

impl StreamParserV1 for BrokenStreamParser {
    fn parse_chunk(&mut self, _: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        Err(self.envelope.clone())
    }
}

/// Renders host-minted values in the shape the guest parses.
fn minted_json(minted: &HostMintedValuesV1) -> String {
    minted
        .callback_url()
        .map_or_else(
            || serde_json::json!({ "task_id": minted.task_id() }),
            |url| serde_json::json!({ "task_id": minted.task_id(), "callback_url": url }),
        )
        .to_string()
}

/// Reads back an observation the guest rendered.
fn parse_observation_json(json: &str) -> ComponentResultV1<TaskObservationV1> {
    let wire: ObservationInput = serde_json::from_str(json).map_err(|error| {
        internal(format_args!("component returned an observation that is not the form: {error}"))
    })?;
    wire.build().map_err(internal)
}

/// Reads back a submit outcome the guest rendered.
fn parse_submit_outcome(json: &str) -> ComponentResultV1<SubmitOutcomeV1> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| internal(format_args!("submit outcome is not json: {error}")))?;
    match value.get("outcome").and_then(serde_json::Value::as_str) {
        Some("accepted") => value
            .get("upstream_task_id")
            .and_then(serde_json::Value::as_str)
            .map(|id| SubmitOutcomeV1::Accepted(id.to_owned()))
            .ok_or_else(|| internal("an accepted outcome carries no upstream task id")),
        Some("accepted-terminal") => Ok(SubmitOutcomeV1::AcceptedTerminal(
            value.get("body").cloned().unwrap_or(serde_json::Value::Null),
        )),
        Some("rejected") => {
            let envelope = value
                .get("error")
                .cloned()
                .ok_or_else(|| internal("a rejected outcome carries no error"))?;
            Ok(SubmitOutcomeV1::Rejected(serde_json::from_value(envelope).map_err(|error| {
                internal(format_args!("a rejected outcome's error is not an envelope: {error}"))
            })?))
        }
        Some("unknown") => Ok(SubmitOutcomeV1::Unknown),
        other => Err(internal(format_args!("`{other:?}` is not a submit outcome"))),
    }
}

/// A sandboxed **task** component presented through the typed seam.
///
/// Constructed only from a component whose declared world is the task world
/// (2026-09-19 runtime-second-world record, D2): a wrong-world call then
/// cannot be written, rather than failing at runtime. The manifest already
/// refuses a world mismatch at admission; this keeps the same discipline one
/// layer up.
#[derive(Debug)]
pub struct SandboxedTaskComponentV1 {
    component: LoadedComponentV1,
}

impl SandboxedTaskComponentV1 {
    /// Wraps a loaded component, or returns it untouched when it exports the
    /// other world.
    ///
    /// # Errors
    ///
    /// The component itself (boxed — it is a large value, and the error path
    /// should not widen every `Result` that carries it), so a caller that
    /// guessed wrong can still use it as what it is.
    pub fn new(component: LoadedComponentV1) -> Result<Self, Box<LoadedComponentV1>> {
        if component.manifest().api_version == south_provider_api::TASK_WORLD {
            Ok(Self { component })
        } else {
            Err(Box::new(component))
        }
    }

    /// The loaded component, for callers that need the JSON face too.
    #[must_use]
    pub const fn inner(&self) -> &LoadedComponentV1 {
        &self.component
    }
}

impl TaskComponentV1 for SandboxedTaskComponentV1 {
    fn metadata(&self) -> ComponentMetadataV1 {
        self.component.metadata()
    }

    fn build_submit_request(
        &self,
        config: &ProviderConfig,
        request: &serde_json::Value,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        let json = self
            .component
            .call_build_submit_request(&to_json(config)?, &to_json(request)?, &minted_json(minted))
            .map_err(|error| seam_error(&error))?;
        from_json(&json)
    }

    fn parse_submit_response(
        &self,
        parts: &HttpResponseParts,
    ) -> ComponentResultV1<SubmitOutcomeV1> {
        let json = self
            .component
            .call_parse_submit_response(&to_json(parts)?)
            .map_err(|error| seam_error(&error))?;
        parse_submit_outcome(&json)
    }

    fn build_observe_request(
        &self,
        config: &ProviderConfig,
        upstream_model: &str,
        upstream_task_id: &str,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        let json = self
            .component
            .call_build_observe_request(&to_json(config)?, upstream_model, upstream_task_id)
            .map_err(|error| seam_error(&error))?;
        from_json(&json)
    }

    fn parse_observation(&self, parts: &HttpResponseParts) -> ComponentResultV1<TaskObservationV1> {
        let json = self
            .component
            .call_parse_observation(&to_json(parts)?)
            .map_err(|error| seam_error(&error))?;
        parse_observation_json(&json)
    }

    fn build_artifact_request(
        &self,
        config: &ProviderConfig,
        observation: &TaskObservationV1,
    ) -> ComponentResultV1<Option<HttpRequestDescriptor>> {
        let json = self
            .component
            .call_build_artifact_request(
                &to_json(config)?,
                &observation_json(observation).to_string(),
            )
            .map_err(|error| seam_error(&error))?;
        // `null` is "already have it", not "not supported".
        if json.trim() == "null" {
            return Ok(None);
        }
        from_json(&json).map(Some)
    }

    fn render_success(
        &self,
        observation: &TaskObservationV1,
        fetched: Option<&HttpResponseParts>,
        minted: &HostMintedValuesV1,
    ) -> ComponentResultV1<serde_json::Value> {
        let fetched = fetched.map(to_json).transpose()?;
        let json = self
            .component
            .call_render_success(
                &observation_json(observation).to_string(),
                fetched.as_deref(),
                &minted_json(minted),
            )
            .map_err(|error| seam_error(&error))?;
        from_json(&json)
    }

    fn map_terminal_failure(
        &self,
        observation: &TaskObservationV1,
    ) -> ComponentResultV1<ErrorEnvelope> {
        let json = self
            .component
            .call_map_terminal_failure(&observation_json(observation).to_string())
            .map_err(|error| seam_error(&error))?;
        from_json(&json)
    }
}
