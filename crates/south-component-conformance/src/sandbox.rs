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
use crate::component::{ComponentResultV1, ProviderComponentV1, StreamParserV1};
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
