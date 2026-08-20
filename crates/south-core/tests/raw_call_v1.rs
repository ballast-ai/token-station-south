//! Public contract tests for the host-prelude raw-call scaffolding.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, ControlledUserAgentV1, CredentialSlotV1, QueryParameterV1,
    QueryStringV1, SecretHeaderV1, StreamChunkV1, StreamingResponseHeadV1, TransportErrorV1,
};
use south_core::raw::{
    BoundedResolverV1, PreparedSecretResolverV1, RawAuthV1, RawCallErrorV1, RawProviderCallErrorV1,
    RawProviderCallV1, execute_raw_call_v1, open_streaming_raw_call_v1, parse_raw_call,
    raw_call_parses,
};
use south_core::{
    AsyncHttpTransport, AsyncStreamingTransport, CredentialResolver, OpenedByteStreamV1,
    PreparedHttpRequestV1, ProviderCallErrorV1, SecretValue, StreamByteSourceV1,
    StreamChunkFutureV1, StreamOpenErrorV1, StreamingOpenFutureV1, TransportFuture,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const SECRET: &str = "prelude-secret-token";

const fn valid_raw<'a>(headers: &'a [(String, String)], body: &'a str) -> RawProviderCallV1<'a> {
    RawProviderCallV1 {
        endpoint: "https://provider.invalid",
        relative_path: "v1/chat/completions",
        bound_slot: "primary",
        requested_slot: "primary",
        headers,
        body,
        auth: RawAuthV1::Bearer,
        query: None,
        user_agent: None,
    }
}

struct CountingResolver {
    calls: AtomicUsize,
}

impl CountingResolver {
    const fn new() -> Self {
        Self { calls: AtomicUsize::new(0) }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl CredentialResolver for CountingResolver {
    fn resolve<'a>(
        &'a self,
        _slot: &'a CredentialSlotV1,
    ) -> south_core::CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(SecretValue::new(SECRET.to_owned())) })
    }
}

type RecordedAuth = Arc<Mutex<Option<(String, Vec<u8>, String, Option<String>)>>>;

struct RecordingTransport {
    calls: AtomicUsize,
    recorded: RecordedAuth,
}

impl RecordingTransport {
    fn new() -> Self {
        Self { calls: AtomicUsize::new(0), recorded: Arc::new(Mutex::new(None)) }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn record(&self, request: &PreparedHttpRequestV1<'_>) {
        let (auth_name, auth_value) = request.auth_header();
        *self.recorded.lock().unwrap() = Some((
            auth_name.to_owned(),
            auth_value.to_vec(),
            request.url().to_string(),
            request.user_agent().map(|agent| agent.as_str().to_owned()),
        ));
    }
}

impl AsyncHttpTransport for RecordingTransport {
    fn execute<'a>(
        &'a self,
        request: &'a PreparedHttpRequestV1<'_>,
        _remaining_timeout: Duration,
    ) -> TransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.record(request);
        Box::pin(async move {
            BufferedHttpResponseV1::try_from_parts(
                StatusCode::OK,
                b"{\"ok\":true}".to_vec(),
                Some("application/json".to_owned()),
                None,
            )
        })
    }
}

struct OneChunkStreamSource {
    chunk: Option<StreamChunkV1>,
}

impl StreamByteSourceV1 for OneChunkStreamSource {
    fn next_chunk(&mut self) -> StreamChunkFutureV1<'_> {
        let chunk = self.chunk.take();
        Box::pin(async move { chunk.map(Ok) })
    }
}

struct RecordingStreamingTransport {
    calls: AtomicUsize,
    recorded: RecordedAuth,
}

impl RecordingStreamingTransport {
    fn new() -> Self {
        Self { calls: AtomicUsize::new(0), recorded: Arc::new(Mutex::new(None)) }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl AsyncStreamingTransport for RecordingStreamingTransport {
    fn open<'a>(&'a self, request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (auth_name, auth_value) = request.auth_header();
        *self.recorded.lock().unwrap() = Some((
            auth_name.to_owned(),
            auth_value.to_vec(),
            request.url().to_string(),
            request.user_agent().map(|agent| agent.as_str().to_owned()),
        ));
        Box::pin(async move {
            let head = StreamingResponseHeadV1::try_from_parts(StatusCode::OK, None, None)
                .map_err(StreamOpenErrorV1::Transport)?;
            let source = OneChunkStreamSource {
                chunk: Some(
                    StreamChunkV1::try_new(bytes::Bytes::from_static(b"data: {}\n\n")).map_err(
                        |_| StreamOpenErrorV1::Transport(TransportErrorV1::RequestFailed),
                    )?,
                ),
            };
            OpenedByteStreamV1::try_new(head, Box::new(source))
                .map_err(StreamOpenErrorV1::Transport)
        })
    }
}

fn far_deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}

#[test]
fn precheck_agrees_with_parse_for_valid_and_invalid_inputs() {
    let headers = vec![("x-request-id".to_owned(), "req-1".to_owned())];
    let valid = valid_raw(&headers, "{}");
    assert!(raw_call_parses(&valid));
    assert!(parse_raw_call(&valid).is_ok());

    let invalid = RawProviderCallV1 { body: "not json", ..valid_raw(&headers, "{}") };
    assert!(!raw_call_parses(&invalid));
    assert!(parse_raw_call(&invalid).is_err());
}

#[test]
fn parse_error_names_the_failing_field_and_keeps_the_wrapped_code() {
    let headers: Vec<(String, String)> = Vec::new();

    let bad_endpoint = RawProviderCallV1 { endpoint: "not-a-url", ..valid_raw(&headers, "{}") };
    let error = parse_raw_call(&bad_endpoint).unwrap_err();
    assert_eq!(error.field(), "endpoint");
    assert_eq!(error.code(), "INVALID_ENDPOINT");

    let bad_bound_slot = RawProviderCallV1 { bound_slot: "", ..valid_raw(&headers, "{}") };
    let error = parse_raw_call(&bad_bound_slot).unwrap_err();
    assert_eq!(error.field(), "bound_slot");
    assert_eq!(error.code(), "INVALID_CREDENTIAL_SLOT");

    let bad_requested_slot = RawProviderCallV1 { requested_slot: "", ..valid_raw(&headers, "{}") };
    let error = parse_raw_call(&bad_requested_slot).unwrap_err();
    assert_eq!(error.field(), "requested_slot");
    assert_eq!(error.code(), "INVALID_CREDENTIAL_SLOT");

    let bad_path = RawProviderCallV1 { relative_path: "../up", ..valid_raw(&headers, "{}") };
    let error = parse_raw_call(&bad_path).unwrap_err();
    assert_eq!(error.field(), "relative_path");
    assert_eq!(error.code(), "INVALID_RELATIVE_PATH");

    let bad_body = RawProviderCallV1 { body: "not json", ..valid_raw(&headers, "{}") };
    let error = parse_raw_call(&bad_body).unwrap_err();
    assert_eq!(error.field(), "body");
    assert_eq!(error.code(), "INVALID_JSON_BODY");

    let reserved = vec![("authorization".to_owned(), "Bearer smuggled".to_owned())];
    let bad_headers = valid_raw(&reserved, "{}");
    let error = parse_raw_call(&bad_headers).unwrap_err();
    assert_eq!(error.field(), "headers");
    assert_eq!(error.code(), "RESERVED_HEADER_FORBIDDEN");
    assert!(matches!(error, RawCallErrorV1::Headers(_)));
}

#[test]
fn parse_carries_auth_arm_query_and_user_agent() {
    let headers: Vec<(String, String)> = Vec::new();
    let query = QueryStringV1::try_from_iter([(QueryParameterV1::Alt, "sse")]).unwrap();
    let user_agent = ControlledUserAgentV1::try_from_static("prelude-test/1.0").unwrap();
    let raw = RawProviderCallV1 {
        auth: RawAuthV1::HeaderSecret(SecretHeaderV1::XApiKey),
        query: Some(query.clone()),
        user_agent: Some(user_agent),
        ..valid_raw(&headers, "{\"model\":\"m\"}")
    };

    let (_binding, request) = parse_raw_call(&raw).unwrap();
    assert_eq!(request.query().map(QueryStringV1::as_str), Some(query.as_str()));
    assert_eq!(request.user_agent().map(ControlledUserAgentV1::as_str), Some("prelude-test/1.0"));
    match request.auth() {
        south_contracts::ProviderAuthV1::HeaderSecret { header, .. } => {
            assert_eq!(header.header_name(), "x-api-key");
        }
        _ => panic!("auth arm did not survive parsing"),
    }
}

#[tokio::test]
async fn parse_failure_returns_before_resolver_or_transport_is_invoked() {
    let headers: Vec<(String, String)> = Vec::new();
    let invalid = RawProviderCallV1 { body: "not json", ..valid_raw(&headers, "{}") };
    let resolver = CountingResolver::new();
    let transport = RecordingTransport::new();
    let cancellation = CancellationToken::new();

    let result =
        execute_raw_call_v1(&invalid, &resolver, &transport, far_deadline(), &cancellation).await;

    let error = result.unwrap_err();
    assert!(matches!(error, RawProviderCallErrorV1::Parse(RawCallErrorV1::Body(_))));
    assert_eq!(error.code(), "INVALID_JSON_BODY");
    assert_eq!(resolver.calls(), 0);
    assert_eq!(transport.calls(), 0);

    let streaming = RecordingStreamingTransport::new();
    let result =
        open_streaming_raw_call_v1(&invalid, &resolver, &streaming, None, &cancellation).await;
    assert!(matches!(result.unwrap_err(), RawProviderCallErrorV1::Parse(_)));
    assert_eq!(resolver.calls(), 0);
    assert_eq!(streaming.calls(), 0);
}

#[tokio::test]
async fn execute_delegates_to_orchestration_and_binds_the_bearer_header() {
    let headers = vec![("x-request-id".to_owned(), "req-1".to_owned())];
    let raw = valid_raw(&headers, "{\"model\":\"m\"}");
    let resolver = CountingResolver::new();
    let transport = RecordingTransport::new();
    let cancellation = CancellationToken::new();

    let response = execute_raw_call_v1(&raw, &resolver, &transport, far_deadline(), &cancellation)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(resolver.calls(), 1);
    assert_eq!(transport.calls(), 1);
    let recorded = transport.recorded.lock().unwrap().clone().unwrap();
    assert_eq!(recorded.0, "authorization");
    assert_eq!(recorded.1, format!("Bearer {SECRET}").into_bytes());
    assert!(recorded.2.starts_with("https://provider.invalid/v1/chat/completions"));
}

#[tokio::test]
async fn execute_binds_the_header_secret_arm_verbatim() {
    let headers: Vec<(String, String)> = Vec::new();
    let raw = RawProviderCallV1 {
        auth: RawAuthV1::HeaderSecret(SecretHeaderV1::XApiKey),
        ..valid_raw(&headers, "{}")
    };
    let resolver = CountingResolver::new();
    let transport = RecordingTransport::new();
    let cancellation = CancellationToken::new();

    execute_raw_call_v1(&raw, &resolver, &transport, far_deadline(), &cancellation).await.unwrap();

    let recorded = transport.recorded.lock().unwrap().clone().unwrap();
    assert_eq!(recorded.0, "x-api-key");
    assert_eq!(recorded.1, SECRET.as_bytes());
}

#[tokio::test]
async fn slot_mismatch_surfaces_as_credential_binding_mismatch() {
    let headers: Vec<(String, String)> = Vec::new();
    let raw = RawProviderCallV1 { requested_slot: "other", ..valid_raw(&headers, "{}") };
    let resolver = CountingResolver::new();
    let transport = RecordingTransport::new();
    let cancellation = CancellationToken::new();

    let error = execute_raw_call_v1(&raw, &resolver, &transport, far_deadline(), &cancellation)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "CREDENTIAL_BINDING_MISMATCH");
    assert!(matches!(error, RawProviderCallErrorV1::Call(ProviderCallErrorV1::Preparation(_))));
    assert_eq!(resolver.calls(), 0);
    assert_eq!(transport.calls(), 0);
}

#[tokio::test]
async fn open_streaming_delegates_and_pulls_the_stream() {
    let headers: Vec<(String, String)> = Vec::new();
    let raw = valid_raw(&headers, "{}");
    let resolver = CountingResolver::new();
    let transport = RecordingStreamingTransport::new();
    let cancellation = CancellationToken::new();

    let mut stream =
        open_streaming_raw_call_v1(&raw, &resolver, &transport, None, &cancellation).await.unwrap();

    assert_eq!(stream.head().status(), StatusCode::OK);
    assert_eq!(resolver.calls(), 1);
    assert_eq!(transport.calls(), 1);
    let chunk = stream.next_chunk().await.unwrap().unwrap();
    assert_eq!(chunk.as_bytes(), b"data: {}\n\n");
    assert!(stream.next_chunk().await.is_none());
}

#[tokio::test]
async fn prepared_secret_resolver_yields_the_same_secret_repeatedly() {
    let resolver = PreparedSecretResolverV1::new(SECRET.to_owned());
    let slot = CredentialSlotV1::parse("primary").unwrap();

    for _ in 0..2 {
        let headers: Vec<(String, String)> = Vec::new();
        let raw = valid_raw(&headers, "{}");
        let transport = RecordingTransport::new();
        let cancellation = CancellationToken::new();
        execute_raw_call_v1(&raw, &resolver, &transport, far_deadline(), &cancellation)
            .await
            .unwrap();
        let recorded = transport.recorded.lock().unwrap().clone().unwrap();
        assert_eq!(recorded.1, format!("Bearer {SECRET}").into_bytes());
    }

    assert!(CredentialResolver::resolve(&resolver, &slot).await.is_ok());
}

#[tokio::test]
async fn prepared_secret_resolver_optionally_checks_the_slot() {
    let slot = CredentialSlotV1::parse("primary").unwrap();
    let other = CredentialSlotV1::parse("other").unwrap();
    let resolver = PreparedSecretResolverV1::new(SECRET.to_owned()).expecting_slot(slot.clone());

    assert!(CredentialResolver::resolve(&resolver, &slot).await.is_ok());
    assert!(CredentialResolver::resolve(&resolver, &other).await.is_err());
}

#[tokio::test]
async fn bounded_resolver_enforces_the_host_supplied_cap() {
    let slot = CredentialSlotV1::parse("primary").unwrap();

    let at_cap =
        BoundedResolverV1::new(PreparedSecretResolverV1::new(SECRET.to_owned()), SECRET.len());
    assert!(CredentialResolver::resolve(&at_cap, &slot).await.is_ok());

    let over_cap =
        BoundedResolverV1::new(PreparedSecretResolverV1::new(SECRET.to_owned()), SECRET.len() - 1);
    assert!(CredentialResolver::resolve(&over_cap, &slot).await.is_err());
}

#[test]
fn debug_output_redacts_the_prepared_secret() {
    let resolver = PreparedSecretResolverV1::new(SECRET.to_owned());
    let rendered = format!("{resolver:?}");
    assert!(!rendered.contains(SECRET));
    assert!(rendered.contains("[REDACTED]"));

    let bounded = BoundedResolverV1::new(PreparedSecretResolverV1::new(SECRET.to_owned()), 64);
    let rendered = format!("{bounded:?}");
    assert!(!rendered.contains(SECRET));

    let headers: Vec<(String, String)> = Vec::new();
    let raw = valid_raw(&headers, "{\"secret-ish\":true}");
    let rendered = format!("{raw:?}");
    assert!(!rendered.contains("secret-ish"));
    assert!(!rendered.contains("provider.invalid"));
}
