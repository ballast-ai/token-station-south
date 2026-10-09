//! Assembled-executor runner and reference executor for the media-binary suite.

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use http::StatusCode;
use south_contracts::{
    BearerAuthV1, BufferedBinaryResponseV1, CredentialSlotV1, MultipartBodyV1, MultipartBoundaryV1,
    MultipartPostRequestV1, ProviderAuthV1, ProviderEndpointV1, RelativePathV1, SafeHeaders,
    TextBodyV1, TextMediaTypeV1, TextPostRequestV1, TransportErrorV1,
};
use south_core::{
    AsyncBinaryHttpTransport, BinaryTransportFutureV1, CredentialResolutionFuture,
    CredentialResolver, PreparedHttpRequestV1, ProviderBindingV1, RequestBodyRefV1, SecretValue,
    execute_multipart_binary_call_v1, execute_text_binary_call_v1,
};
use south_provider_conformance::{
    FAKE_BEARER_SECRET_V1, FAKE_HEADER_SECRET_V1, PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID,
    PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION, ProviderCallCountV1,
    ProviderCallFailureCodeV1, ProviderMediaBinaryAuthArmV1, ProviderMediaBinaryCaseIdV1,
    ProviderMediaBinaryExpectedOutcomeV1, ProviderMediaBinaryFixtureV1, ProviderMediaBinaryInputV1,
    ProviderMediaBinaryRequestBodyV1, ProviderMediaBinaryUpstreamV1,
    provider_media_binary_fixtures_v1,
};
use tokio_util::sync::CancellationToken;

use crate::{
    map_canonical_fixture_header_invariant_failure, map_contract_error, map_provider_call_error,
};

/// Six cases multiplied by the eleven closed media-binary mismatch categories.
pub const MAX_PROVIDER_MEDIA_BINARY_MISMATCHES_V1: usize = 66;

/// A boxed, cancellation-safe assembled media-binary executor future.
pub type AssembledProviderMediaBinaryExecutionFutureV1<'a> =
    Pin<Box<dyn Future<Output = ProviderMediaBinaryObservationV1> + Send + 'a>>;

/// A host-assembled media-binary call path exercised by the public media-binary runner.
pub trait AssembledProviderMediaBinaryExecutorV1: Send + Sync {
    /// Executes one immutable canonical media-binary fixture.
    ///
    /// A fixture whose body is multipart must be driven through
    /// `execute_multipart_binary_call_v1`, and one whose body is text through
    /// `execute_text_binary_call_v1`; the `wire_binary_response_observed` evidence is what tells
    /// a bytes-reading seam from a UTF-8 one.
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderMediaBinaryFixtureV1,
    ) -> AssembledProviderMediaBinaryExecutionFutureV1<'a>;
}

/// Adapter-reported resolver, transport, and wire-shape boundary evidence.
///
/// The three wire-shape booleans are presence claims measured at the adapter's real boundary:
/// whether the rendered media type reached it byte for byte, whether the declared body bytes
/// reached it unmodified, and whether a bytes-reading transport seam carried the exchange. Like
/// every adapter-reported value, a passing report alone is insufficient for host verification;
/// the adoption review must confirm the wiring.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMediaBinaryEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
    wire_content_type_exact: bool,
    wire_request_body_exact: bool,
    wire_binary_response_observed: bool,
}

impl ProviderMediaBinaryEvidenceV1 {
    /// Constructs evidence and saturates both raw call counts.
    #[must_use]
    pub const fn new(
        resolver_calls: usize,
        transport_calls: usize,
        wire_content_type_exact: bool,
        wire_request_body_exact: bool,
        wire_binary_response_observed: bool,
    ) -> Self {
        Self {
            resolver_calls: ProviderCallCountV1::from_usize(resolver_calls),
            transport_calls: ProviderCallCountV1::from_usize(transport_calls),
            wire_content_type_exact,
            wire_request_body_exact,
            wire_binary_response_observed,
        }
    }

    /// Returns the saturated resolver call category.
    #[must_use]
    pub const fn resolver_calls(&self) -> ProviderCallCountV1 {
        self.resolver_calls
    }

    /// Returns the saturated transport call category.
    #[must_use]
    pub const fn transport_calls(&self) -> ProviderCallCountV1 {
        self.transport_calls
    }

    /// Returns whether the rendered media type reached the transport byte for byte.
    #[must_use]
    pub const fn wire_content_type_exact(&self) -> bool {
        self.wire_content_type_exact
    }

    /// Returns whether the declared body bytes reached the transport unmodified.
    #[must_use]
    pub const fn wire_request_body_exact(&self) -> bool {
        self.wire_request_body_exact
    }

    /// Returns whether a bytes-reading transport seam carried the exchange.
    #[must_use]
    pub const fn wire_binary_response_observed(&self) -> bool {
        self.wire_binary_response_observed
    }
}

impl fmt::Debug for ProviderMediaBinaryEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .field("wire_content_type_exact", &self.wire_content_type_exact)
            .field("wire_request_body_exact", &self.wire_request_body_exact)
            .field("wire_binary_response_observed", &self.wire_binary_response_observed)
            .finish()
    }
}

enum ProviderMediaBinaryObservedOutcomeV1 {
    Response(BufferedBinaryResponseV1),
    Failure(ProviderCallFailureCodeV1),
}

impl fmt::Debug for ProviderMediaBinaryObservedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Response(response) => formatter.debug_tuple("Response").field(response).finish(),
            Self::Failure(code) => formatter.debug_tuple("Failure").field(code).finish(),
        }
    }
}

/// An observed media-binary terminal shape plus adapter-reported evidence.
pub struct ProviderMediaBinaryObservationV1 {
    outcome: ProviderMediaBinaryObservedOutcomeV1,
    evidence: ProviderMediaBinaryEvidenceV1,
}

impl ProviderMediaBinaryObservationV1 {
    /// Constructs a buffered-response observation from an already bounded binary response.
    #[must_use]
    pub const fn response(
        response: BufferedBinaryResponseV1,
        evidence: ProviderMediaBinaryEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderMediaBinaryObservedOutcomeV1::Response(response), evidence }
    }

    /// Constructs a failed observation from a closed known failure code.
    #[must_use]
    pub const fn failure(
        code: ProviderCallFailureCodeV1,
        evidence: ProviderMediaBinaryEvidenceV1,
    ) -> Self {
        Self { outcome: ProviderMediaBinaryObservedOutcomeV1::Failure(code), evidence }
    }

    /// Returns the bounded response when this is a buffered-response observation.
    #[must_use]
    pub const fn response_value(&self) -> Option<&BufferedBinaryResponseV1> {
        match &self.outcome {
            ProviderMediaBinaryObservedOutcomeV1::Response(response) => Some(response),
            ProviderMediaBinaryObservedOutcomeV1::Failure(_) => None,
        }
    }

    /// Returns the closed code when this is a failure observation.
    #[must_use]
    pub const fn failure_code(&self) -> Option<ProviderCallFailureCodeV1> {
        match &self.outcome {
            ProviderMediaBinaryObservedOutcomeV1::Failure(code) => Some(*code),
            ProviderMediaBinaryObservedOutcomeV1::Response(_) => None,
        }
    }

    /// Returns adapter-reported boundary evidence.
    #[must_use]
    pub const fn evidence(&self) -> &ProviderMediaBinaryEvidenceV1 {
        &self.evidence
    }
}

impl fmt::Debug for ProviderMediaBinaryObservationV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryObservationV1")
            .field("outcome", &self.outcome)
            .field("evidence", &self.evidence)
            .finish()
    }
}

/// The closed reasons why an observed media-binary case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderMediaBinaryMismatchCategoryV1 {
    /// Response versus failure differed.
    OutcomeKind,
    /// Stable failure code differed.
    ErrorCode,
    /// Response status differed.
    Status,
    /// Response body bytes differed.
    Body,
    /// `content-type` value or presence differed.
    ContentType,
    /// `retry-after` value or presence differed.
    RetryAfter,
    /// Resolver call category differed.
    ResolverCallCount,
    /// Transport call category differed.
    TransportCallCount,
    /// Rendered-media-type wire evidence differed.
    WireContentType,
    /// Request-body wire evidence differed.
    WireRequestBody,
    /// Bytes-reading-seam wire evidence differed.
    WireBinaryResponse,
}

fixed_debug!(ProviderMediaBinaryMismatchCategoryV1 {
    OutcomeKind => "OutcomeKind",
    ErrorCode => "ErrorCode",
    Status => "Status",
    Body => "Body",
    ContentType => "ContentType",
    RetryAfter => "RetryAfter",
    ResolverCallCount => "ResolverCallCount",
    TransportCallCount => "TransportCallCount",
    WireContentType => "WireContentType",
    WireRequestBody => "WireRequestBody",
    WireBinaryResponse => "WireBinaryResponse",
});

/// One case/category mismatch without expected or observed payload values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderMediaBinaryMismatchV1 {
    case_id: ProviderMediaBinaryCaseIdV1,
    category: ProviderMediaBinaryMismatchCategoryV1,
}

impl ProviderMediaBinaryMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> ProviderMediaBinaryCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> ProviderMediaBinaryMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for ProviderMediaBinaryMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete canonical media-binary suite.
pub struct ProviderMediaBinaryConformanceReportV1 {
    passed_case_ids: Vec<ProviderMediaBinaryCaseIdV1>,
}

impl ProviderMediaBinaryConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[ProviderMediaBinaryCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for ProviderMediaBinaryConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryConformanceReportV1")
            .field("suite_id", &PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the evaluated canonical media-binary suite.
pub struct ProviderMediaBinaryConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<ProviderMediaBinaryMismatchV1>,
}

impl ProviderMediaBinaryConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[ProviderMediaBinaryMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for ProviderMediaBinaryConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderMediaBinaryConformanceFailureV1")
            .field("suite_id", &PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_ID)
            .field("suite_version", &PROVIDER_MEDIA_BINARY_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs all canonical media-binary cases sequentially without failing fast.
///
/// Every caller must wrap the entire runner in an outer watchdog. This function intentionally has
/// no internal timeout, so a broken assembled executor may remain pending forever. The watchdog
/// must own the complete structured future tree so timeout drops all in-progress executor work
/// without leaving a detached task.
pub async fn run_provider_media_binary_conformance_v1(
    executor: &dyn AssembledProviderMediaBinaryExecutorV1,
) -> Result<ProviderMediaBinaryConformanceReportV1, ProviderMediaBinaryConformanceFailureV1> {
    let fixtures = provider_media_binary_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_PROVIDER_MEDIA_BINARY_MISMATCHES_V1);

    for fixture in fixtures {
        let mismatch_count_before_case = mismatches.len();
        let observation = executor.execute_case(fixture).await;
        compare_media_binary_outcome(fixture, &observation, &mut mismatches);
        compare_media_binary_evidence(fixture, &observation, &mut mismatches);
        if mismatches.len() == mismatch_count_before_case {
            passed_case_ids.push(fixture.case_id());
        }
    }

    if mismatches.is_empty() {
        Ok(ProviderMediaBinaryConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_PROVIDER_MEDIA_BINARY_MISMATCHES_V1);
        Err(ProviderMediaBinaryConformanceFailureV1 {
            evaluated_case_count: fixtures.len(),
            mismatches,
        })
    }
}

fn compare_media_binary_outcome(
    fixture: &ProviderMediaBinaryFixtureV1,
    observation: &ProviderMediaBinaryObservationV1,
    mismatches: &mut Vec<ProviderMediaBinaryMismatchV1>,
) {
    match (fixture.expected().outcome(), &observation.outcome) {
        (
            ProviderMediaBinaryExpectedOutcomeV1::Response {
                status,
                body,
                content_type,
                retry_after,
            },
            ProviderMediaBinaryObservedOutcomeV1::Response(response),
        ) => {
            record_if(
                response.status().as_u16() != *status,
                fixture,
                ProviderMediaBinaryMismatchCategoryV1::Status,
                mismatches,
            );
            record_if(
                response.body() != *body,
                fixture,
                ProviderMediaBinaryMismatchCategoryV1::Body,
                mismatches,
            );
            record_if(
                response.content_type() != *content_type,
                fixture,
                ProviderMediaBinaryMismatchCategoryV1::ContentType,
                mismatches,
            );
            record_if(
                response.retry_after() != *retry_after,
                fixture,
                ProviderMediaBinaryMismatchCategoryV1::RetryAfter,
                mismatches,
            );
        }
        (
            ProviderMediaBinaryExpectedOutcomeV1::Failure { code: expected },
            ProviderMediaBinaryObservedOutcomeV1::Failure(observed),
        ) => record_if(
            expected != observed,
            fixture,
            ProviderMediaBinaryMismatchCategoryV1::ErrorCode,
            mismatches,
        ),
        _ => record(fixture, ProviderMediaBinaryMismatchCategoryV1::OutcomeKind, mismatches),
    }
}

fn compare_media_binary_evidence(
    fixture: &ProviderMediaBinaryFixtureV1,
    observation: &ProviderMediaBinaryObservationV1,
    mismatches: &mut Vec<ProviderMediaBinaryMismatchV1>,
) {
    let expected = fixture.expected().evidence();
    let observed = observation.evidence();
    record_if(
        expected.resolver_calls() != observed.resolver_calls(),
        fixture,
        ProviderMediaBinaryMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        expected.transport_calls() != observed.transport_calls(),
        fixture,
        ProviderMediaBinaryMismatchCategoryV1::TransportCallCount,
        mismatches,
    );
    record_if(
        expected.wire_content_type_exact() != observed.wire_content_type_exact(),
        fixture,
        ProviderMediaBinaryMismatchCategoryV1::WireContentType,
        mismatches,
    );
    record_if(
        expected.wire_request_body_exact() != observed.wire_request_body_exact(),
        fixture,
        ProviderMediaBinaryMismatchCategoryV1::WireRequestBody,
        mismatches,
    );
    record_if(
        expected.wire_binary_response_observed() != observed.wire_binary_response_observed(),
        fixture,
        ProviderMediaBinaryMismatchCategoryV1::WireBinaryResponse,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &ProviderMediaBinaryFixtureV1,
    category: ProviderMediaBinaryMismatchCategoryV1,
    mismatches: &mut Vec<ProviderMediaBinaryMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &ProviderMediaBinaryFixtureV1,
    category: ProviderMediaBinaryMismatchCategoryV1,
    mismatches: &mut Vec<ProviderMediaBinaryMismatchV1>,
) {
    mismatches.push(ProviderMediaBinaryMismatchV1 { case_id: fixture.case_id(), category });
}

/// A parsed canonical media-binary request: which shape it is selects the entry point.
pub enum ProviderMediaBinaryRequestV1 {
    /// Driven through `execute_multipart_binary_call_v1`.
    Multipart(MultipartPostRequestV1),
    /// Driven through `execute_text_binary_call_v1`.
    Text(TextPostRequestV1),
}

impl fmt::Debug for ProviderMediaBinaryRequestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Multipart(request) => formatter.debug_tuple("Multipart").field(request).finish(),
            Self::Text(request) => formatter.debug_tuple("Text").field(request).finish(),
        }
    }
}

/// Parses one canonical media-binary input through the production contract, attaching the
/// fixture's arm.
///
/// Public so a host's own executor can reuse the exact parse the reference executor uses. The
/// smuggling case fails here rather than at a boundary, which is the point: a `content-type` in
/// the ordinary header channel of a text request is refused while the request is still a value.
///
/// # Errors
///
/// Returns the closed failure code the fixture table expects for a refused input.
pub fn parse_reference_media_binary_input(
    input: &ProviderMediaBinaryInputV1,
    auth_arm: ProviderMediaBinaryAuthArmV1,
) -> Result<(ProviderBindingV1, ProviderMediaBinaryRequestV1), ProviderCallFailureCodeV1> {
    let endpoint = ProviderEndpointV1::parse(input.endpoint()).map_err(map_contract_error)?;
    let bound_slot =
        CredentialSlotV1::parse(input.bound_credential_slot()).map_err(map_contract_error)?;
    let requested_slot =
        CredentialSlotV1::parse(input.requested_credential_slot()).map_err(map_contract_error)?;
    let relative_path = RelativePathV1::parse(input.relative_path()).map_err(map_contract_error)?;
    let headers = SafeHeaders::try_from_iter(input.headers().iter().copied())
        .map_err(map_canonical_fixture_header_invariant_failure)?;
    let slot = BearerAuthV1::new(requested_slot);
    let auth = match auth_arm {
        ProviderMediaBinaryAuthArmV1::Bearer => ProviderAuthV1::Bearer(slot),
        ProviderMediaBinaryAuthArmV1::HeaderSecret(header) => {
            ProviderAuthV1::HeaderSecret { header, slot }
        }
    };
    let request = match *input.body() {
        ProviderMediaBinaryRequestBodyV1::Multipart { bytes, boundary } => {
            let boundary = MultipartBoundaryV1::parse(boundary).map_err(map_contract_error)?;
            let body =
                MultipartBodyV1::parse(bytes.to_vec(), boundary).map_err(map_contract_error)?;
            ProviderMediaBinaryRequestV1::Multipart(
                MultipartPostRequestV1::try_new(relative_path, headers, body, auth)
                    .map_err(map_contract_error)?,
            )
        }
        ProviderMediaBinaryRequestBodyV1::Text { text, media_type } => {
            let media_type = TextMediaTypeV1::parse(media_type).map_err(map_contract_error)?;
            let body =
                TextBodyV1::try_new(text.to_owned(), media_type).map_err(map_contract_error)?;
            ProviderMediaBinaryRequestV1::Text(
                TextPostRequestV1::try_new(relative_path, headers, body, auth)
                    .map_err(map_contract_error)?,
            )
        }
    };
    Ok((ProviderBindingV1::new(endpoint, bound_slot), request))
}

/// A deterministic assembled media-binary executor built from real `south-core` orchestration and
/// fake ports.
///
/// Multipart rows run through `execute_multipart_binary_call_v1` and text rows through
/// `execute_text_binary_call_v1`. The wire-shape booleans are measured on the prepared request at
/// a fake bytes-reading transport boundary, mirroring what a real adapter must measure on its
/// wire: the rendered media type, the body bytes, and which transport seam carried the exchange.
pub struct ReferenceAssembledProviderMediaBinaryExecutorV1;

impl ReferenceAssembledProviderMediaBinaryExecutorV1 {
    /// Creates an independent reference media-binary executor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ReferenceAssembledProviderMediaBinaryExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl AssembledProviderMediaBinaryExecutorV1 for ReferenceAssembledProviderMediaBinaryExecutorV1 {
    fn execute_case<'a>(
        &'a self,
        fixture: &'a ProviderMediaBinaryFixtureV1,
    ) -> AssembledProviderMediaBinaryExecutionFutureV1<'a> {
        Box::pin(async move { execute_reference_media_binary_case(fixture).await })
    }
}

async fn execute_reference_media_binary_case(
    fixture: &ProviderMediaBinaryFixtureV1,
) -> ProviderMediaBinaryObservationV1 {
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let transport_calls = Arc::new(AtomicUsize::new(0));
    let probe = Arc::new(WireShapeProbe::default());

    let (binding, request) =
        match parse_reference_media_binary_input(fixture.input(), fixture.auth_arm()) {
            Ok(parsed) => parsed,
            Err(code) => {
                return ProviderMediaBinaryObservationV1::failure(
                    code,
                    ProviderMediaBinaryEvidenceV1::new(0, 0, false, false, false),
                );
            }
        };

    let resolver =
        ArmSecretResolver { calls: Arc::clone(&resolver_calls), arm: fixture.auth_arm() };
    let transport = WireRecordingBinaryTransport {
        calls: Arc::clone(&transport_calls),
        upstream: fixture.upstream(),
        expected_content_type: fixture.input().expected_content_type(),
        expected_body: fixture.input().body().bytes(),
        probe: Arc::clone(&probe),
    };
    let cancellation = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let result = match &request {
        ProviderMediaBinaryRequestV1::Multipart(request) => {
            execute_multipart_binary_call_v1(
                &binding,
                request,
                &resolver,
                &transport,
                deadline,
                &cancellation,
            )
            .await
        }
        ProviderMediaBinaryRequestV1::Text(request) => {
            execute_text_binary_call_v1(
                &binding,
                request,
                &resolver,
                &transport,
                deadline,
                &cancellation,
            )
            .await
        }
    };
    let evidence = ProviderMediaBinaryEvidenceV1::new(
        resolver_calls.load(Ordering::SeqCst),
        transport_calls.load(Ordering::SeqCst),
        probe.content_type_exact.load(Ordering::SeqCst),
        probe.request_body_exact.load(Ordering::SeqCst),
        probe.binary_response_observed.load(Ordering::SeqCst),
    );
    match result {
        Ok(response) => ProviderMediaBinaryObservationV1::response(response, evidence),
        Err(error) => {
            ProviderMediaBinaryObservationV1::failure(map_provider_call_error(&error), evidence)
        }
    }
}

/// Observed wire shape at the fake transport boundary. All three are presence claims, so all
/// start `false` and only a transport call can raise them.
#[derive(Default)]
struct WireShapeProbe {
    content_type_exact: AtomicBool,
    request_body_exact: AtomicBool,
    binary_response_observed: AtomicBool,
}

struct ArmSecretResolver {
    calls: Arc<AtomicUsize>,
    arm: ProviderMediaBinaryAuthArmV1,
}

impl CredentialResolver for ArmSecretResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let secret = match self.arm {
            ProviderMediaBinaryAuthArmV1::Bearer => FAKE_BEARER_SECRET_V1,
            ProviderMediaBinaryAuthArmV1::HeaderSecret(_) => FAKE_HEADER_SECRET_V1,
        };
        Box::pin(async move { Ok(SecretValue::new(secret.to_owned())) })
    }
}

struct WireRecordingBinaryTransport<'fixture> {
    calls: Arc<AtomicUsize>,
    upstream: &'fixture ProviderMediaBinaryUpstreamV1,
    expected_content_type: String,
    expected_body: &'static [u8],
    probe: Arc<WireShapeProbe>,
}

impl WireRecordingBinaryTransport<'_> {
    fn record_wire_shape(&self, request: &PreparedHttpRequestV1<'_>) {
        // The media type the transport is about to emit, compared against the value rebuilt from
        // the fixture's raw input — not against South's answer, so this measures agreement with
        // the contract's rule rather than self-consistency.
        self.probe.content_type_exact.store(
            request.content_type() == Some(self.expected_content_type.as_str()),
            Ordering::SeqCst,
        );
        // The bytes the transport is about to send, compared against the fixture's raw bytes:
        // neither body is re-encoded by South on its way to the wire.
        self.probe.request_body_exact.store(
            request.body().map(RequestBodyRefV1::as_bytes) == Some(self.expected_body),
            Ordering::SeqCst,
        );
    }
}

impl AsyncBinaryHttpTransport for WireRecordingBinaryTransport<'_> {
    fn execute_binary<'a>(
        &'a self,
        request: &'a PreparedHttpRequestV1<'_>,
        _remaining_timeout: Duration,
    ) -> BinaryTransportFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.record_wire_shape(request);
        // Raised at the seam itself: the claim is about *which* transport trait carried the
        // exchange, which is what separates these entry points from the UTF-8 multipart one.
        self.probe.binary_response_observed.store(true, Ordering::SeqCst);
        match self.upstream {
            ProviderMediaBinaryUpstreamV1::Response(raw) => {
                let status = StatusCode::from_u16(raw.status());
                let body = raw.body().to_vec();
                let content_type = raw.content_type().map(str::to_owned);
                let retry_after = raw.retry_after().map(str::to_owned);
                Box::pin(async move {
                    let status = status.map_err(|_| TransportErrorV1::ResponseMetadataInvalid)?;
                    BufferedBinaryResponseV1::try_from_parts(
                        status,
                        body,
                        content_type,
                        retry_after,
                    )
                })
            }
            // `NotReached` must not reach any boundary. Fail closed with the context-free request
            // code rather than panicking.
            ProviderMediaBinaryUpstreamV1::NotReached => {
                Box::pin(async { Err(TransportErrorV1::RequestFailed) })
            }
        }
    }
}
