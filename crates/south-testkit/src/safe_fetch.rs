//! Fake network ports, runner and reference executor for the safe fetch host suite.
//!
//! Unlike the provider-call suites, the adapter reports nothing here: the runner builds a fake
//! resolver, a fake connector and a fake proxy environment from each fixture
//! ([`SafeFetchPortsV1`]), hands them to the host's executor, and reads every piece of boundary
//! evidence off those fakes itself. A host plugs in by driving its real executor — its URL check,
//! address check, pinning, header policy, limits and timeout — over these ports in place of its
//! system resolver and socket connector.

use std::{
    cmp::min,
    collections::VecDeque,
    fmt,
    future::{Future, pending},
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{Mutex, PoisonError},
    time::Duration,
};

use bytes::Bytes;
use south_contracts::{
    MAX_BINARY_RESPONSE_BODY_BYTES,
    media::{ArtifactUrlV1, is_forbidden_egress_address},
};
use south_provider_conformance::{
    ProviderCallCountV1, SAFE_FETCH_CONFORMANCE_SUITE_ID, SAFE_FETCH_CONFORMANCE_SUITE_VERSION,
    SafeFetchCaseIdV1, SafeFetchEnvironmentV1, SafeFetchExpectedOutcomeV1, SafeFetchFailureCodeV1,
    SafeFetchFixtureV1, SafeFetchInputV1, SafeFetchResolutionV1, SafeFetchUpstreamBodyV1,
    SafeFetchUpstreamResponseV1, SafeFetchWireTargetV1, safe_fetch_fixtures_v1,
};

/// Twenty-six cases multiplied by the ten closed safe fetch mismatch categories.
pub const MAX_SAFE_FETCH_MISMATCHES_V1: usize = 260;

/// How long past a case's total timeout the runner waits before it records the case as overrun
/// and drops the executor's future.
pub const SAFE_FETCH_RUNNER_GRACE_V1: Duration = Duration::from_secs(1);

/// A boxed, cancellation-safe safe fetch future.
pub type SafeFetchFutureV1<'a> =
    Pin<Box<dyn Future<Output = Result<SafeFetchArtifactV1, SafeFetchFailureCodeV1>> + Send + 'a>>;

/// A host's safe fetch executor, driven over the runner's fake network.
///
/// The executor must resolve through [`SafeFetchPortsV1::resolve`] and connect through
/// [`SafeFetchPortsV1::exchange`] only, and must treat [`SafeFetchPortsV1::system_proxy`] as the
/// process proxy environment it would otherwise read: an executor that honours a system proxy
/// routes through it here, and is caught. The total timeout is the executor's own; the runner's
/// watchdog only bounds a case that ignores it.
pub trait SafeFetchExecutorV1: Send + Sync {
    /// Fetches `input` over `ports`.
    fn fetch<'a>(
        &'a self,
        input: &'a SafeFetchInputV1,
        ports: &'a SafeFetchPortsV1,
    ) -> SafeFetchFutureV1<'a>;
}

/// A fetched artifact.
pub struct SafeFetchArtifactV1 {
    media_type: String,
    body: Vec<u8>,
    upstream_content_type: Option<String>,
}

impl SafeFetchArtifactV1 {
    /// Constructs an artifact: the media type the component declared, the bytes, and the upstream
    /// `content-type` as recorded.
    #[must_use]
    pub const fn new(
        media_type: String,
        body: Vec<u8>,
        upstream_content_type: Option<String>,
    ) -> Self {
        Self { media_type, body, upstream_content_type }
    }

    /// Returns the artifact's media type.
    #[must_use]
    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    /// Returns the artifact's bytes.
    #[must_use]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Returns the upstream `content-type`, recorded and never acted on.
    #[must_use]
    pub fn upstream_content_type(&self) -> Option<&str> {
        self.upstream_content_type.as_deref()
    }
}

impl fmt::Debug for SafeFetchArtifactV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchArtifactV1")
            .field("media_type_byte_count", &self.media_type.len())
            .field("body_byte_count", &self.body.len())
            .field("has_upstream_content_type", &self.upstream_content_type.is_some())
            .finish()
    }
}

/// Why the fake resolver gave no answer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SafeFetchResolveErrorV1 {
    /// The name is not in the case's resolver table.
    NotFound,
}

fixed_debug!(SafeFetchResolveErrorV1 { NotFound => "NotFound" });

/// Why the fake connector gave no response.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SafeFetchTransportErrorV1 {
    /// No server answers to the named server.
    ConnectionRefused,
}

fixed_debug!(SafeFetchTransportErrorV1 { ConnectionRefused => "ConnectionRefused" });

/// One request as the executor puts it on the wire.
///
/// `server_name` is the TLS server name and `host`; `headers` is every other request header the
/// executor sends, names and values as given.
pub struct SafeFetchWireRequestV1 {
    server_name: String,
    path_and_query: String,
    headers: Vec<(String, String)>,
}

impl SafeFetchWireRequestV1 {
    /// Constructs a wire request.
    #[must_use]
    pub const fn new(
        server_name: String,
        path_and_query: String,
        headers: Vec<(String, String)>,
    ) -> Self {
        Self { server_name, path_and_query, headers }
    }

    /// Returns the TLS server name.
    #[must_use]
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Returns the request target.
    #[must_use]
    pub fn path_and_query(&self) -> &str {
        &self.path_and_query
    }

    /// Returns every header besides `host`.
    #[must_use]
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }
}

impl fmt::Debug for SafeFetchWireRequestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchWireRequestV1")
            .field("server_name_byte_count", &self.server_name.len())
            .field("path_and_query_byte_count", &self.path_and_query.len())
            .field("header_count", &self.headers.len())
            .finish()
    }
}

/// A response body read chunk by chunk, so the executor's byte limit is what bounds memory.
pub struct SafeFetchWireBodyV1 {
    source: WireBodySource,
}

enum WireBodySource {
    Chunks(VecDeque<Bytes>),
    Repeated { chunk: Bytes, remaining: usize },
    StallAfter(Option<Bytes>),
}

impl SafeFetchWireBodyV1 {
    fn from_fixture(body: SafeFetchUpstreamBodyV1) -> Self {
        let source = match body {
            SafeFetchUpstreamBodyV1::Chunks(chunks) => WireBodySource::Chunks(
                chunks.iter().map(|chunk| Bytes::from_static(chunk)).collect(),
            ),
            SafeFetchUpstreamBodyV1::Repeated { chunk_bytes, total_bytes } => {
                WireBodySource::Repeated {
                    chunk: Bytes::from(vec![0xa5; chunk_bytes.max(1)]),
                    remaining: total_bytes,
                }
            }
            SafeFetchUpstreamBodyV1::StallAfter(first) => {
                WireBodySource::StallAfter(Some(Bytes::from_static(first)))
            }
        };
        Self { source }
    }

    /// Returns the next chunk, or `None` at the end of the body. A stalling body never completes.
    pub async fn next_chunk(&mut self) -> Option<Bytes> {
        match &mut self.source {
            WireBodySource::Chunks(chunks) => chunks.pop_front(),
            WireBodySource::Repeated { chunk, remaining } => {
                if *remaining == 0 {
                    return None;
                }
                let length = min(chunk.len(), *remaining);
                *remaining -= length;
                Some(chunk.slice(..length))
            }
            WireBodySource::StallAfter(first) => match first.take() {
                Some(chunk) => Some(chunk),
                None => pending().await,
            },
        }
    }
}

impl fmt::Debug for SafeFetchWireBodyV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("SafeFetchWireBodyV1").finish_non_exhaustive()
    }
}

/// A response head and its unread body.
pub struct SafeFetchWireResponseV1 {
    status: u16,
    content_type: Option<&'static str>,
    location: Option<&'static str>,
    body: SafeFetchWireBodyV1,
}

impl SafeFetchWireResponseV1 {
    fn from_fixture(response: &SafeFetchUpstreamResponseV1) -> Self {
        Self {
            status: response.status(),
            content_type: response.content_type(),
            location: response.location(),
            body: SafeFetchWireBodyV1::from_fixture(response.body()),
        }
    }

    /// Returns the HTTP status.
    #[must_use]
    pub const fn status(&self) -> u16 {
        self.status
    }

    /// Returns the upstream `content-type`, if any.
    #[must_use]
    pub const fn content_type(&self) -> Option<&'static str> {
        self.content_type
    }

    /// Returns the upstream `location`, if any.
    #[must_use]
    pub const fn location(&self) -> Option<&'static str> {
        self.location
    }

    /// Returns the unread body.
    pub const fn body_mut(&mut self) -> &mut SafeFetchWireBodyV1 {
        &mut self.body
    }
}

impl fmt::Debug for SafeFetchWireResponseV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchWireResponseV1")
            .field("status", &self.status)
            .field("has_content_type", &self.content_type.is_some())
            .field("has_location", &self.location.is_some())
            .finish_non_exhaustive()
    }
}

/// What the ports observed during one case. Values only, never header values or URLs.
#[derive(Clone, PartialEq, Eq)]
struct PortRecord {
    resolver_calls: usize,
    queries_per_entry: Vec<usize>,
    connected_to: Vec<SocketAddr>,
    every_exchange_sent_only_accept: bool,
    every_exchange_named_target: bool,
}

/// The fake resolver, connector and proxy environment of one case, recording what the executor
/// did through them.
pub struct SafeFetchPortsV1 {
    environment: &'static SafeFetchEnvironmentV1,
    wire_target: &'static SafeFetchWireTargetV1,
    record: Mutex<PortRecord>,
}

impl SafeFetchPortsV1 {
    /// Builds the fake network of `fixture`. The runner builds one per case; a host may build its
    /// own to debug a single case.
    #[must_use]
    pub fn new(fixture: &'static SafeFetchFixtureV1) -> Self {
        let environment = fixture.environment();
        Self {
            environment,
            wire_target: fixture.wire_target(),
            record: Mutex::new(PortRecord {
                resolver_calls: 0,
                queries_per_entry: vec![0; environment.dns().len()],
                connected_to: Vec::new(),
                every_exchange_sent_only_accept: true,
                every_exchange_named_target: true,
            }),
        }
    }

    /// Resolves `name` through the case's resolver table, ASCII case-insensitively. A stalling
    /// entry never completes; the executor's timeout must end it.
    pub async fn resolve(&self, name: &str) -> Result<Vec<IpAddr>, SafeFetchResolveErrorV1> {
        let answer = self.answer(name);
        match answer {
            Some(answer) => answer,
            None => pending().await,
        }
    }

    fn answer(&self, name: &str) -> Option<Result<Vec<IpAddr>, SafeFetchResolveErrorV1>> {
        let mut record = self.record.lock().unwrap_or_else(PoisonError::into_inner);
        record.resolver_calls += 1;
        let Some(index) =
            self.environment.dns().iter().position(|entry| entry.name().eq_ignore_ascii_case(name))
        else {
            return Some(Err(SafeFetchResolveErrorV1::NotFound));
        };
        let earlier_queries = record.queries_per_entry[index];
        record.queries_per_entry[index] += 1;
        drop(record);
        match self.environment.dns()[index].resolution() {
            SafeFetchResolutionV1::Addresses(addresses) => Some(Ok(addresses.to_vec())),
            SafeFetchResolutionV1::Rebinding { first, later } => {
                Some(Ok(if earlier_queries == 0 { first } else { later }.to_vec()))
            }
            SafeFetchResolutionV1::Stall => None,
        }
    }

    /// Connects to exactly `connect_to` and performs one exchange.
    ///
    /// The fake server is found by the request's server name, not by the address, so an exchange
    /// sent to the wrong address still gets an answer: the address it went to is the evidence.
    pub async fn exchange(
        &self,
        connect_to: SocketAddr,
        request: SafeFetchWireRequestV1,
    ) -> Result<SafeFetchWireResponseV1, SafeFetchTransportErrorV1> {
        self.record_exchange(connect_to, &request);
        // A real connector yields here; so does this one, so a timeout the executor wraps around
        // the exchange can interleave with it.
        tokio::task::yield_now().await;
        self.environment
            .servers()
            .iter()
            .find(|server| server.server_name().eq_ignore_ascii_case(request.server_name()))
            .map(|server| SafeFetchWireResponseV1::from_fixture(server.response()))
            .ok_or(SafeFetchTransportErrorV1::ConnectionRefused)
    }

    fn record_exchange(&self, connect_to: SocketAddr, request: &SafeFetchWireRequestV1) {
        let only_accept = matches!(
            request.headers(),
            [(name, _)] if name.eq_ignore_ascii_case("accept")
        );
        let named_target =
            request.server_name().eq_ignore_ascii_case(self.wire_target.server_name())
                && request.path_and_query() == self.wire_target.path_and_query();
        let mut record = self.record.lock().unwrap_or_else(PoisonError::into_inner);
        record.connected_to.push(connect_to);
        record.every_exchange_sent_only_accept &= only_accept;
        record.every_exchange_named_target &= named_target;
    }

    /// Returns the system proxy the case's environment configures, which a correct executor never
    /// uses.
    #[must_use]
    pub const fn system_proxy(&self) -> Option<&'static str> {
        self.environment.system_proxy()
    }

    fn observed(&self) -> PortRecord {
        self.record.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl fmt::Debug for SafeFetchPortsV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchPortsV1")
            .field("environment", self.environment)
            .finish_non_exhaustive()
    }
}

/// The closed reasons why an observed safe fetch case can differ from its fixture.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SafeFetchMismatchCategoryV1 {
    /// Artifact versus failure differed.
    OutcomeKind,
    /// Stable failure code differed.
    ErrorCode,
    /// Artifact bytes differed.
    Body,
    /// Artifact media type was not the declared one.
    MediaType,
    /// The recorded upstream `content-type` differed in value or presence.
    UpstreamContentType,
    /// The executor did not finish within its total timeout plus the runner's grace.
    Deadline,
    /// Resolver query category differed.
    ResolverCallCount,
    /// The addresses connected to differed from the one checked address, or from none.
    ConnectedAddresses,
    /// An exchange sent a header other than `accept`, or not exactly one.
    WireHeaders,
    /// An exchange named a server or request target other than the URL's.
    WireTarget,
}

fixed_debug!(SafeFetchMismatchCategoryV1 {
    OutcomeKind => "OutcomeKind",
    ErrorCode => "ErrorCode",
    Body => "Body",
    MediaType => "MediaType",
    UpstreamContentType => "UpstreamContentType",
    Deadline => "Deadline",
    ResolverCallCount => "ResolverCallCount",
    ConnectedAddresses => "ConnectedAddresses",
    WireHeaders => "WireHeaders",
    WireTarget => "WireTarget",
});

/// One case/category mismatch without expected or observed payload values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SafeFetchMismatchV1 {
    case_id: SafeFetchCaseIdV1,
    category: SafeFetchMismatchCategoryV1,
}

impl SafeFetchMismatchV1 {
    /// Returns the canonical case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> SafeFetchCaseIdV1 {
        self.case_id
    }

    /// Returns the closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> SafeFetchMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for SafeFetchMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete canonical safe fetch suite.
pub struct SafeFetchConformanceReportV1 {
    passed_case_ids: Vec<SafeFetchCaseIdV1>,
}

impl SafeFetchConformanceReportV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        SAFE_FETCH_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        SAFE_FETCH_CONFORMANCE_SUITE_VERSION
    }

    /// Returns all passed cases in canonical table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[SafeFetchCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for SafeFetchConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchConformanceReportV1")
            .field("suite_id", &SAFE_FETCH_CONFORMANCE_SUITE_ID)
            .field("suite_version", &SAFE_FETCH_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// A complete bounded mismatch report for the evaluated canonical safe fetch suite.
pub struct SafeFetchConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<SafeFetchMismatchV1>,
}

impl SafeFetchConformanceFailureV1 {
    /// Returns the stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        SAFE_FETCH_CONFORMANCE_SUITE_ID
    }

    /// Returns the suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        SAFE_FETCH_CONFORMANCE_SUITE_VERSION
    }

    /// Returns how many canonical cases completed evaluation.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Returns every case/category mismatch in canonical evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[SafeFetchMismatchV1] {
        &self.mismatches
    }
}

impl fmt::Debug for SafeFetchConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeFetchConformanceFailureV1")
            .field("suite_id", &SAFE_FETCH_CONFORMANCE_SUITE_ID)
            .field("suite_version", &SAFE_FETCH_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs all canonical safe fetch cases sequentially without failing fast.
///
/// Two rows end only by the executor's own timeout, so the runner bounds each case itself: a case
/// still pending at its total timeout plus [`SAFE_FETCH_RUNNER_GRACE_V1`] is recorded as a
/// [`SafeFetchMismatchCategoryV1::Deadline`] mismatch and its future is dropped. That bounds only
/// an executor that yields; callers should still wrap the runner in an outer watchdog, as for
/// every other suite.
pub async fn run_safe_fetch_conformance_v1(
    executor: &dyn SafeFetchExecutorV1,
) -> Result<SafeFetchConformanceReportV1, SafeFetchConformanceFailureV1> {
    let fixtures = safe_fetch_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::with_capacity(MAX_SAFE_FETCH_MISMATCHES_V1);

    for fixture in fixtures {
        let mismatch_count_before_case = mismatches.len();
        let ports = SafeFetchPortsV1::new(fixture);
        let watchdog = fixture.input().total_timeout().saturating_add(SAFE_FETCH_RUNNER_GRACE_V1);
        let outcome =
            tokio::time::timeout(watchdog, executor.fetch(fixture.input(), &ports)).await.ok();
        compare_safe_fetch_outcome(fixture, outcome.as_ref(), &mut mismatches);
        compare_safe_fetch_evidence(fixture, &ports.observed(), &mut mismatches);
        if mismatches.len() == mismatch_count_before_case {
            passed_case_ids.push(fixture.case_id());
        }
    }

    if mismatches.is_empty() {
        Ok(SafeFetchConformanceReportV1 { passed_case_ids })
    } else {
        debug_assert!(mismatches.len() <= MAX_SAFE_FETCH_MISMATCHES_V1);
        Err(SafeFetchConformanceFailureV1 { evaluated_case_count: fixtures.len(), mismatches })
    }
}

fn compare_safe_fetch_outcome(
    fixture: &SafeFetchFixtureV1,
    outcome: Option<&Result<SafeFetchArtifactV1, SafeFetchFailureCodeV1>>,
    mismatches: &mut Vec<SafeFetchMismatchV1>,
) {
    let Some(outcome) = outcome else {
        record(fixture, SafeFetchMismatchCategoryV1::Deadline, mismatches);
        return;
    };
    match (fixture.expected().outcome(), outcome) {
        (
            SafeFetchExpectedOutcomeV1::Artifact { body, media_type, upstream_content_type },
            Ok(artifact),
        ) => {
            record_if(
                artifact.body() != *body,
                fixture,
                SafeFetchMismatchCategoryV1::Body,
                mismatches,
            );
            record_if(
                artifact.media_type() != *media_type,
                fixture,
                SafeFetchMismatchCategoryV1::MediaType,
                mismatches,
            );
            record_if(
                artifact.upstream_content_type() != *upstream_content_type,
                fixture,
                SafeFetchMismatchCategoryV1::UpstreamContentType,
                mismatches,
            );
        }
        (SafeFetchExpectedOutcomeV1::Failure { code: expected }, Err(observed)) => record_if(
            expected != observed,
            fixture,
            SafeFetchMismatchCategoryV1::ErrorCode,
            mismatches,
        ),
        _ => record(fixture, SafeFetchMismatchCategoryV1::OutcomeKind, mismatches),
    }
}

fn compare_safe_fetch_evidence(
    fixture: &SafeFetchFixtureV1,
    observed: &PortRecord,
    mismatches: &mut Vec<SafeFetchMismatchV1>,
) {
    let expected = fixture.expected().evidence();
    record_if(
        ProviderCallCountV1::from_usize(observed.resolver_calls) != expected.resolver_calls(),
        fixture,
        SafeFetchMismatchCategoryV1::ResolverCallCount,
        mismatches,
    );
    record_if(
        observed.connected_to.as_slice() != expected.connected_to().as_slice(),
        fixture,
        SafeFetchMismatchCategoryV1::ConnectedAddresses,
        mismatches,
    );
    record_if(
        !observed.every_exchange_sent_only_accept,
        fixture,
        SafeFetchMismatchCategoryV1::WireHeaders,
        mismatches,
    );
    record_if(
        !observed.every_exchange_named_target,
        fixture,
        SafeFetchMismatchCategoryV1::WireTarget,
        mismatches,
    );
}

fn record_if(
    condition: bool,
    fixture: &SafeFetchFixtureV1,
    category: SafeFetchMismatchCategoryV1,
    mismatches: &mut Vec<SafeFetchMismatchV1>,
) {
    if condition {
        record(fixture, category, mismatches);
    }
}

fn record(
    fixture: &SafeFetchFixtureV1,
    category: SafeFetchMismatchCategoryV1,
    mismatches: &mut Vec<SafeFetchMismatchV1>,
) {
    mismatches.push(SafeFetchMismatchV1 { case_id: fixture.case_id(), category });
}

/// A safe fetch executor built on south's two pure halves, [`ArtifactUrlV1::parse`] and
/// [`is_forbidden_egress_address`], and nothing else.
///
/// In order, under one total timeout: parse the URL; take a literal address as is or resolve the
/// name once; refuse if any address is forbidden; connect to the first checked address, never
/// resolving again, never consulting the system proxy, sending only `accept` (the declared media
/// type); refuse any 3xx and any other non-2xx; read the body under the smaller of the host limit
/// and [`MAX_BINARY_RESPONSE_BODY_BYTES`]; refuse an empty body; return the declared media type
/// and record the upstream one.
pub struct ReferenceSafeFetchExecutorV1;

impl ReferenceSafeFetchExecutorV1 {
    /// Creates a reference safe fetch executor.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ReferenceSafeFetchExecutorV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl SafeFetchExecutorV1 for ReferenceSafeFetchExecutorV1 {
    fn fetch<'a>(
        &'a self,
        input: &'a SafeFetchInputV1,
        ports: &'a SafeFetchPortsV1,
    ) -> SafeFetchFutureV1<'a> {
        Box::pin(async move {
            tokio::time::timeout(input.total_timeout(), reference_fetch(input, ports))
                .await
                .unwrap_or(Err(SafeFetchFailureCodeV1::Timeout))
        })
    }
}

async fn reference_fetch(
    input: &SafeFetchInputV1,
    ports: &SafeFetchPortsV1,
) -> Result<SafeFetchArtifactV1, SafeFetchFailureCodeV1> {
    let url = ArtifactUrlV1::parse(input.url())
        .map_err(SafeFetchFailureCodeV1::from_artifact_url_error)?;
    let addresses = match literal_address(url.host_str()) {
        // A literal was already checked by the parse; the check below repeats it harmlessly.
        Some(address) => vec![address],
        None => ports
            .resolve(url.host_str())
            .await
            .map_err(|_| SafeFetchFailureCodeV1::ResolutionFailed)?,
    };
    let Some(&checked) = addresses.first() else {
        return Err(SafeFetchFailureCodeV1::ResolutionFailed);
    };
    if addresses.iter().copied().any(is_forbidden_egress_address) {
        return Err(SafeFetchFailureCodeV1::ForbiddenAddress);
    }

    let request = SafeFetchWireRequestV1::new(
        url.host_str().to_owned(),
        path_and_query(&url),
        vec![("accept".to_owned(), input.declared_media_type().to_owned())],
    );
    let mut response = ports
        .exchange(SocketAddr::new(checked, url.port()), request)
        .await
        .map_err(|_| SafeFetchFailureCodeV1::TransportFailed)?;
    match response.status() {
        200..=299 => {}
        300..=399 => return Err(SafeFetchFailureCodeV1::Redirect),
        _ => return Err(SafeFetchFailureCodeV1::UpstreamStatus),
    }

    let limit = min(input.host_byte_limit(), MAX_BINARY_RESPONSE_BODY_BYTES);
    let mut body = Vec::new();
    while let Some(chunk) = response.body_mut().next_chunk().await {
        if chunk.len() > limit - body.len() {
            return Err(SafeFetchFailureCodeV1::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    if body.is_empty() {
        return Err(SafeFetchFailureCodeV1::EmptyBody);
    }
    Ok(SafeFetchArtifactV1::new(
        input.declared_media_type().to_owned(),
        body,
        response.content_type().map(str::to_owned),
    ))
}

fn literal_address(host: &str) -> Option<IpAddr> {
    host.strip_prefix('[').and_then(|host| host.strip_suffix(']')).unwrap_or(host).parse().ok()
}

/// The request target of an artifact URL: everything from the first `/` after the authority, up
/// to any fragment. The parse has already refused userinfo, so the authority holds no `/`.
fn path_and_query(url: &ArtifactUrlV1) -> String {
    let text = url.as_str();
    let after_scheme = text.strip_prefix("https://").unwrap_or(text);
    let target = after_scheme.find('/').map_or("/", |start| &after_scheme[start..]);
    target.split('#').next().unwrap_or(target).to_owned()
}
