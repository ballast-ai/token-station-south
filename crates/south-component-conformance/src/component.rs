//! The seam between the conformance suite and a provider component.
//!
//! The suite is written against these traits rather than against a WASM
//! runtime, exactly as the donor gates were: it lets gates ① and ② exist and
//! be proven to bite before `south-provider-runtime` can instantiate a
//! component, and it lets a component author run the same suite against a
//! native build in their own CI without a WASM toolchain. What the runtime
//! does with the boundary — serialize, call, deserialize, and turn a trap
//! into an [`ErrorEnvelope`] — is what makes it an implementation of these
//! traits (S3).
//!
//! Each method mirrors one function of the `provider-adapter-v2` world, with
//! the `json` payloads already parsed into the Canonical IR. `healthcheck` is
//! absent: it carries no fixture — a component exporting the world at all is
//! a load-time fact the runtime checks, not something a fixture can express.

use south_provider_api::ComponentMetadataV1;
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ModelCapability, ProviderConfig, StreamEvent,
};

/// What a component returns. The error is the component's own
/// [`ErrorEnvelope`], which is also what the runtime reports when a component
/// traps.
pub type ComponentResultV1<T> = Result<T, ErrorEnvelope>;

/// One provider stream, mid-parse.
///
/// Streaming is the only stateful part of the ABI. A chunk off the socket is
/// not a whole frame — SSE or binary eventstream alike — so a component must
/// hold the tail until the rest arrives. The v2 world expresses that as
/// instance state behind a plain `parse-stream-chunk` function, which means
/// the host instantiates a component per stream; this trait hands out a fresh
/// parser per stream rather than pretending the call is pure.
///
/// Chunks are raw bounded bytes (S0 ruling D2). A split may land inside a
/// UTF-8 sequence; a parser buffers bytes and decodes only complete frames.
pub trait StreamParserV1 {
    /// Consumes one fragment and emits whatever complete events it completed.
    ///
    /// Zero events is a normal answer: the fragment ended mid-frame.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>>;

    /// Flushes a clean transport EOF. The v2 world has no separate finish
    /// export, so the runtime represents EOF as an empty fragment, which a
    /// successful network read can never produce.
    ///
    /// # Errors
    ///
    /// Returns a typed protocol failure when buffered state cannot finish.
    fn finish(&mut self) -> ComponentResultV1<Vec<StreamEvent>> {
        self.parse_chunk(&[])
    }
}

/// Southbound: the Canonical IR, in and out of one provider's HTTP dialect.
pub trait ProviderComponentV1 {
    fn metadata(&self) -> ComponentMetadataV1;

    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ModelCapability>>;

    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor>;

    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with.
    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse>;

    /// Maps a failed upstream response onto the stable error catalog.
    ///
    /// # Errors
    ///
    /// Returns the envelope the caller should be answered with. A component
    /// that cannot classify a failure returns
    /// `Ok(ErrorEnvelope { code: Internal, .. })` rather than `Err`; `Err`
    /// here means the mapping itself broke.
    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope>;

    /// A parser for one stream. Called once per exchange.
    fn stream_parser(&self) -> Box<dyn StreamParserV1>;
}
