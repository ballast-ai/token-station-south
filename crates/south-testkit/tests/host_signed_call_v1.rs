//! The `south.host-signed.v1` case set (design record §4), run against real orchestration.
//!
//! Every case from the ruled design record is here, buffered and streaming. The subject is
//! South's seam — where the finalizer sits, what it sees, and how its output is held to the
//! declaration — never a signing algorithm: South ships none and this suite must not grow one.

use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use http::StatusCode;
use south_contracts::{
    BearerAuthV1, BufferedHttpResponseV1, ControlledUserAgentV1, CredentialSlotV1, JsonBodyV1,
    JsonPostRequestV1, ProviderAuthV1, ProviderEndpointV1, QueryParameterV1, QueryStringV1,
    RelativePathV1, SafeHeaders, SignedHeaderSetV1, SignedHeaderV1, StreamingResponseHeadV1,
};
use south_core::{
    AsyncHttpTransport, AsyncStreamingTransport, OpenedByteStreamV1, PreparedHttpRequestV1,
    ProviderBindingV1, ProviderCallErrorV1, StreamByteSourceV1, StreamChunkFutureV1,
    StreamOpenErrorV1, StreamingOpenFutureV1, TransportFuture, execute_signed_provider_call_v1,
    open_streaming_signed_provider_call_v1,
};
use south_testkit::{
    DeterministicRequestFinalizerV1, FinalizerBehaviorV1, HangingRequestFinalizerV1,
    ObservedFinalizeViewV1, expected_signature_v1,
};
use tokio_util::sync::CancellationToken;

const ENDPOINT: &str = "https://signed.invalid/base";
const SLOT: &str = "aws.bedrock.primary";
const PATH: &str = "model/invoke";
const AGENT: &str = "south-drill/1.0";

/// The declaration used by every case but one: three of the four permitted headers, so
/// `EmitsUndeclared` always has a fourth to reach for.
fn declared() -> SignedHeaderSetV1 {
    SignedHeaderSetV1::new(&[
        SignedHeaderV1::Authorization,
        SignedHeaderV1::XAmzDate,
        SignedHeaderV1::XAmzContentSha256,
    ])
    .expect("fixture declaration is valid")
}

fn binding() -> ProviderBindingV1 {
    ProviderBindingV1::new(
        ProviderEndpointV1::parse(ENDPOINT).expect("fixture endpoint"),
        CredentialSlotV1::parse(SLOT).expect("fixture slot"),
    )
}

fn signed_request(slot: &str) -> JsonPostRequestV1 {
    JsonPostRequestV1::new(
        RelativePathV1::parse(PATH).expect("fixture path"),
        SafeHeaders::try_from_iter([("x-test", "header-sentinel")]).expect("fixture header"),
        JsonBodyV1::parse(r#"{"value":"body-sentinel"}"#).expect("fixture body"),
        ProviderAuthV1::HostSigned {
            slot: BearerAuthV1::new(CredentialSlotV1::parse(slot).expect("fixture slot")),
            emits: declared(),
        },
    )
    .with_query(
        QueryStringV1::try_from_iter([(QueryParameterV1::ApiVersion, "2024-01-01")])
            .expect("fixture query"),
    )
    .with_user_agent(ControlledUserAgentV1::try_from_static(AGENT).expect("fixture agent"))
}

fn bearer_request() -> JsonPostRequestV1 {
    JsonPostRequestV1::new(
        RelativePathV1::parse(PATH).expect("fixture path"),
        SafeHeaders::default(),
        JsonBodyV1::parse("{}").expect("fixture body"),
        ProviderAuthV1::Bearer(BearerAuthV1::new(
            CredentialSlotV1::parse(SLOT).expect("fixture slot"),
        )),
    )
}

fn response() -> BufferedHttpResponseV1 {
    BufferedHttpResponseV1::try_from_parts(StatusCode::OK, b"{}".to_vec(), None, None)
        .expect("fixture response")
}

type WireHeaders = Vec<(String, Vec<u8>)>;

/// What one call put on the transport boundary: url, auth headers, body, user agent.
struct WireRecord {
    url: String,
    auth_headers: WireHeaders,
    body: Vec<u8>,
    user_agent: Option<String>,
}

/// Records what actually reached the transport boundary.
#[derive(Default)]
struct RecordingTransport {
    calls: AtomicUsize,
    wire: Mutex<Option<WireRecord>>,
}

impl RecordingTransport {
    fn record(&self, request: &PreparedHttpRequestV1<'_>) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let headers =
            request.auth_headers().map(|(name, value)| (name.to_owned(), value.to_vec())).collect();
        if let Ok(mut wire) = self.wire.lock() {
            *wire = Some(WireRecord {
                url: request.url().to_string(),
                auth_headers: headers,
                body: request.body().as_str().as_bytes().to_vec(),
                user_agent: request.user_agent().map(|agent| agent.as_str().to_owned()),
            });
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn auth_headers(&self) -> WireHeaders {
        self.read(|record| record.auth_headers.clone()).unwrap_or_default()
    }

    fn url(&self) -> String {
        self.read(|record| record.url.clone()).unwrap_or_default()
    }

    /// The body and user agent that reached the boundary, for the fixture that asserts the
    /// transport was handed the same bytes the signer saw.
    fn body_and_agent(&self) -> Option<(Vec<u8>, Option<String>)> {
        self.read(|record| (record.body.clone(), record.user_agent.clone()))
    }

    fn read<T>(&self, project: impl FnOnce(&WireRecord) -> T) -> Option<T> {
        self.wire.lock().ok().and_then(|wire| wire.as_ref().map(project))
    }
}

impl AsyncHttpTransport for RecordingTransport {
    fn execute<'a>(
        &'a self,
        request: &'a PreparedHttpRequestV1<'a>,
        _timeout: Duration,
    ) -> TransportFuture<'a> {
        self.record(request);
        Box::pin(async { Ok(response()) })
    }
}

struct EmptySource;

impl StreamByteSourceV1 for EmptySource {
    fn next_chunk(&mut self) -> StreamChunkFutureV1<'_> {
        Box::pin(async { None })
    }
}

impl AsyncStreamingTransport for RecordingTransport {
    fn open<'a>(&'a self, request: &'a PreparedHttpRequestV1<'a>) -> StreamingOpenFutureV1<'a> {
        self.record(request);
        Box::pin(async {
            let head = StreamingResponseHeadV1::try_from_parts(StatusCode::OK, None, None)
                .map_err(|_| {
                    StreamOpenErrorV1::Transport(south_contracts::TransportErrorV1::RequestFailed)
                })?;
            OpenedByteStreamV1::try_new(head, Box::new(EmptySource))
                .map_err(StreamOpenErrorV1::Transport)
        })
    }
}

fn code(error: &ProviderCallErrorV1) -> &'static str {
    match error {
        ProviderCallErrorV1::Preparation(error) => error.code(),
        other => panic!("expected a preparation error, got {other:?}"),
    }
}

/// The view a correct South must have shown the finalizer, built independently of South.
fn expected_view() -> ObservedFinalizeViewV1 {
    ObservedFinalizeViewV1 {
        method: "POST".to_owned(),
        url: format!("{ENDPOINT}/{PATH}?api-version=2024-01-01"),
        headers: vec![("x-test".to_owned(), "header-sentinel".to_owned())],
        body: br#"{"value":"body-sentinel"}"#.to_vec(),
        user_agent: Some(AGENT.to_owned()),
        slot: SLOT.to_owned(),
        emits: declared().headers().to_vec(),
    }
}

async fn run_buffered(
    finalizer: &DeterministicRequestFinalizerV1,
    transport: &RecordingTransport,
) -> Result<BufferedHttpResponseV1, ProviderCallErrorV1> {
    execute_signed_provider_call_v1(
        &binding(),
        &signed_request(SLOT),
        finalizer,
        transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
}

// ── emits_exactly_declared / view_is_final / called_once / after_assemble_before_send ─────

#[tokio::test]
async fn a_correct_finalizer_signs_the_final_request_exactly_once_and_emits_what_it_declared() {
    let finalizer = DeterministicRequestFinalizerV1::correct();
    let transport = RecordingTransport::default();
    run_buffered(&finalizer, &transport).await.expect("a correct signer completes the call");

    // called_once
    assert_eq!(finalizer.calls(), 1, "the finalizer runs once per call");

    // view_is_final: the URL carries the binding-resolved path *and* the sanctioned query, and
    // the body bytes are the ones the transport will write. Compared against a view built here,
    // not against what South handed over — otherwise this asserts South equals itself.
    let observed = finalizer.observed().expect("the finalizer recorded its view");
    assert_eq!(observed, expected_view());

    // emits_exactly_declared, in the declaration's canonical order, byte for byte.
    let expected: WireHeaders = declared()
        .headers()
        .iter()
        .map(|header| {
            (
                header.header_name().to_owned(),
                expected_signature_v1(&expected_view(), *header).into_bytes(),
            )
        })
        .collect();
    assert_eq!(transport.auth_headers(), expected);

    // after_assemble_before_send: the transport was reached once, and only after signing — and
    // it was handed the same bytes the signer saw, which is the whole point of signing last.
    assert_eq!(transport.calls(), 1);
    assert_eq!(transport.body_and_agent(), Some((observed.body.clone(), observed.user_agent)));
}

#[tokio::test]
async fn the_streaming_entry_point_signs_the_same_request_the_same_way() {
    // streaming_parity: one seam, one position, one set of rules. A streaming path that signed
    // a different request would be a second contract nobody documented.
    let finalizer = DeterministicRequestFinalizerV1::correct();
    let transport = RecordingTransport::default();
    open_streaming_signed_provider_call_v1(
        &binding(),
        &signed_request(SLOT),
        &finalizer,
        &transport,
        None,
        &CancellationToken::new(),
    )
    .await
    .expect("a correct signer opens the stream");

    assert_eq!(finalizer.calls(), 1);
    assert_eq!(finalizer.observed().expect("recorded view"), expected_view());
    let expected: WireHeaders = declared()
        .headers()
        .iter()
        .map(|header| {
            (
                header.header_name().to_owned(),
                expected_signature_v1(&expected_view(), *header).into_bytes(),
            )
        })
        .collect();
    assert_eq!(transport.auth_headers(), expected);
    assert_eq!(transport.url(), expected_view().url);
}

// ── the four rejections, each proving the transport is never reached ──────────────────────

#[tokio::test]
async fn every_way_a_signer_breaks_its_declaration_is_rejected_before_the_network() {
    for behavior in [
        FinalizerBehaviorV1::EmitsUndeclared,
        FinalizerBehaviorV1::OmitsDeclared,
        FinalizerBehaviorV1::EmitsEmptyValue,
        FinalizerBehaviorV1::EmitsDuplicate,
    ] {
        let finalizer = DeterministicRequestFinalizerV1::new(behavior);
        let transport = RecordingTransport::default();
        let error = run_buffered(&finalizer, &transport)
            .await
            .expect_err("a signer outside its declaration must not reach the network");

        assert_eq!(code(&error), "REQUEST_FINALIZATION_REJECTED", "{behavior:?}");
        assert_eq!(transport.calls(), 0, "{behavior:?} must not reach the transport");
    }
}

#[tokio::test]
async fn a_failing_finalizer_is_a_preparation_error_and_never_reaches_the_network() {
    let finalizer = DeterministicRequestFinalizerV1::new(FinalizerBehaviorV1::Fails);
    let transport = RecordingTransport::default();
    let error = run_buffered(&finalizer, &transport).await.expect_err("a failing signer fails");

    assert_eq!(code(&error), "REQUEST_FINALIZATION_FAILED");
    assert_eq!(transport.calls(), 0);
}

// ── cancellation and deadline pre-empt a slow signer ──────────────────────────────────────

#[tokio::test]
async fn cancellation_observed_inside_the_finalizer_pre_empts_it() {
    let finalizer = HangingRequestFinalizerV1::new();
    let transport = RecordingTransport::default();
    let cancellation = CancellationToken::new();
    let token = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        token.cancel();
    });

    let error = execute_signed_provider_call_v1(
        &binding(),
        &signed_request(SLOT),
        &finalizer,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    )
    .await
    .expect_err("a cancelled call cannot succeed");

    assert_eq!(code(&error), "CANCELLED");
    assert_eq!(finalizer.calls(), 1, "the signer was entered, then pre-empted");
    assert_eq!(transport.calls(), 0);
}

#[tokio::test]
async fn the_deadline_pre_empts_a_signer_that_never_returns() {
    let finalizer = HangingRequestFinalizerV1::new();
    let transport = RecordingTransport::default();

    let error = execute_signed_provider_call_v1(
        &binding(),
        &signed_request(SLOT),
        &finalizer,
        &transport,
        tokio::time::Instant::now() + Duration::from_millis(20),
        &CancellationToken::new(),
    )
    .await
    .expect_err("a signer that never returns cannot succeed");

    assert_eq!(code(&error), "DEADLINE_EXCEEDED");
    assert_eq!(transport.calls(), 0);
}

// ── the two entry points refuse each other's arms ─────────────────────────────────────────

#[tokio::test]
async fn the_signed_entry_point_refuses_a_credential_arm() {
    let finalizer = DeterministicRequestFinalizerV1::correct();
    let transport = RecordingTransport::default();
    let error = execute_signed_provider_call_v1(
        &binding(),
        &bearer_request(),
        &finalizer,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("a Bearer request has nothing to sign");

    assert_eq!(code(&error), "UNSUPPORTED_AUTH_SHAPE");
    assert_eq!(finalizer.calls(), 0, "a signer must not see a request it cannot sign");
    assert_eq!(transport.calls(), 0);
}

#[tokio::test]
async fn the_unsigned_entry_point_refuses_the_host_signed_arm() {
    // The dangerous direction: an unsigned path that "worked" would send an unauthenticated
    // request and surface as a confusing upstream 403 rather than a wiring error here.
    use south_core::{CredentialResolutionFuture, CredentialResolver, SecretValue};

    struct NeverResolver;
    impl CredentialResolver for NeverResolver {
        fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
            Box::pin(async { Ok(SecretValue::new("unused".to_owned())) })
        }
    }

    let transport = RecordingTransport::default();
    let error = south_core::execute_provider_call_v1(
        &binding(),
        &signed_request(SLOT),
        &NeverResolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("the unsigned path cannot serve a signed request");

    assert_eq!(code(&error), "UNSUPPORTED_AUTH_SHAPE");
    assert_eq!(transport.calls(), 0);
}

// ── the binding still governs which identity may be signed with ───────────────────────────

#[tokio::test]
async fn a_slot_outside_the_binding_is_refused_before_the_signer_sees_anything() {
    let finalizer = DeterministicRequestFinalizerV1::correct();
    let transport = RecordingTransport::default();
    let error = execute_signed_provider_call_v1(
        &binding(),
        &signed_request("aws.bedrock.other"),
        &finalizer,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("an unbound slot cannot be signed");

    assert_eq!(code(&error), "CREDENTIAL_BINDING_MISMATCH");
    assert_eq!(finalizer.calls(), 0);
    assert_eq!(transport.calls(), 0);
}

// ── plain_channel_still_rejects ───────────────────────────────────────────────────────────

#[test]
fn no_signed_header_can_travel_through_the_plain_channel() {
    for header in SignedHeaderV1::ALL {
        let error = SafeHeaders::try_from_iter([(header.header_name(), "smuggled")])
            .expect_err("a signed header name must stay reserved");
        assert_eq!(error.code(), "RESERVED_HEADER_FORBIDDEN");
    }
}
