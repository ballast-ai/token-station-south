use std::{
    fmt::{Debug, Display},
    future::pending,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use http::{Method, StatusCode};
use south_contracts::{
    BearerAuthV1, BufferedBinaryResponseV1, BufferedHttpResponseV1, ControlledUserAgentV1,
    CredentialSlotV1, GetRequestV1, JsonBodyV1, JsonPostRequestV1, MultipartBodyV1,
    MultipartBoundaryV1, MultipartPostRequestV1, PreparationErrorV1, ProviderAuthV1,
    ProviderEndpointV1, QueryParameterV1, QueryStringV1, RelativePathV1, SafeHeaders,
    SecretHeaderV1, SignedHeaderSetV1, SignedHeaderV1, TransportErrorV1,
};
use south_core::{
    AsyncBinaryHttpTransport, AsyncHttpTransport, BinaryTransportFutureV1,
    CredentialResolutionErrorV1, CredentialResolutionFuture, CredentialResolver,
    PreparedHttpRequestV1, ProviderBindingV1, ProviderCallErrorV1, SecretValue, TransportFuture,
    execute_binary_call_v1, execute_get_call_v1, execute_multipart_call_v1,
    execute_provider_call_v1,
};
use static_assertions::{assert_impl_all, assert_not_impl_any};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use url::Url;

assert_impl_all!(ProviderBindingV1: Send, Sync);
assert_impl_all!(SecretValue: Send, Sync);
assert_not_impl_any!(SecretValue: Clone, Display, serde::Serialize, serde::de::DeserializeOwned);

const ENDPOINT_SENTINEL: &str = "endpoint-sentinel.invalid";
const PATH_SENTINEL: &str = "path-sentinel";
const SLOT_SENTINEL: &str = "slot-sentinel";
const HEADER_SENTINEL: &str = "header-sentinel";
const BODY_SENTINEL: &str = "body-sentinel";
const SECRET_SENTINEL: &str = "secret-sentinel";

fn binding(endpoint: &str, slot: &str) -> ProviderBindingV1 {
    ProviderBindingV1::new(
        ProviderEndpointV1::parse(endpoint).expect("fixture endpoint should be valid"),
        CredentialSlotV1::parse(slot).expect("fixture slot should be valid"),
    )
}

fn request(path: &str, slot: &str) -> JsonPostRequestV1 {
    JsonPostRequestV1::new(
        RelativePathV1::parse(path).expect("fixture path should be valid"),
        SafeHeaders::try_from_iter([("x-test", HEADER_SENTINEL)])
            .expect("fixture header should be valid"),
        JsonBodyV1::parse(&format!(r#"{{"value":"{BODY_SENTINEL}"}}"#))
            .expect("fixture body should be valid"),
        BearerAuthV1::new(CredentialSlotV1::parse(slot).expect("fixture slot should be valid")),
    )
}

fn header_secret_request(path: &str, slot: &str, header: SecretHeaderV1) -> JsonPostRequestV1 {
    JsonPostRequestV1::new(
        RelativePathV1::parse(path).expect("fixture path should be valid"),
        SafeHeaders::try_from_iter([("x-test", HEADER_SENTINEL)])
            .expect("fixture header should be valid"),
        JsonBodyV1::parse(&format!(r#"{{"value":"{BODY_SENTINEL}"}}"#))
            .expect("fixture body should be valid"),
        ProviderAuthV1::HeaderSecret {
            header,
            slot: BearerAuthV1::new(
                CredentialSlotV1::parse(slot).expect("fixture slot should be valid"),
            ),
        },
    )
}

fn response() -> BufferedHttpResponseV1 {
    BufferedHttpResponseV1::try_from_parts(
        StatusCode::CREATED,
        br#"{"ok":true}"#.to_vec(),
        Some("application/json".to_owned()),
        None,
    )
    .expect("fixture response should be valid")
}

async fn wait_for_start(started: oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(1), started)
        .await
        .expect("start handshake watchdog should not expire")
        .expect("operation should send its start handshake");
}

const fn assert_send<T: Send>(_: &T) {}

#[test]
fn binding_debug_redacts_endpoint_and_slot() {
    let debug =
        format!("{:?}", binding(&format!("https://{ENDPOINT_SENTINEL}/base"), SLOT_SENTINEL));

    assert!(!debug.contains(ENDPOINT_SENTINEL));
    assert!(!debug.contains(SLOT_SENTINEL));
}

#[test]
fn secret_debug_is_redacted() {
    let debug = format!("{:?}", SecretValue::new(SECRET_SENTINEL.to_owned()));

    assert!(!debug.contains(SECRET_SENTINEL));
    assert!(debug.contains("REDACTED"));
}

const fn assert_debug<T: Debug>() {}

#[test]
fn secret_retains_only_the_required_public_traits() {
    assert_debug::<SecretValue>();
}

#[derive(Default)]
struct ImmediateResolver {
    calls: AtomicUsize,
    fail: bool,
}

impl ImmediateResolver {
    const fn failing() -> Self {
        Self { calls: AtomicUsize::new(0), fail: true }
    }
}

impl CredentialResolver for ImmediateResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if self.fail {
                Err(CredentialResolutionErrorV1)
            } else {
                Ok(SecretValue::new(SECRET_SENTINEL.to_owned()))
            }
        })
    }
}

struct ControlledResolver {
    calls: AtomicUsize,
    started: Mutex<Option<oneshot::Sender<()>>>,
    release: Mutex<Option<oneshot::Receiver<()>>>,
}

impl CredentialResolver for ControlledResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let started = self.started.lock().expect("test start lock should be available").take();
        let release = self.release.lock().expect("test release lock should be available").take();
        Box::pin(async move {
            if let Some(started) = started {
                let _ = started.send(());
            }
            release
                .expect("controlled resolver should have one release receiver")
                .await
                .expect("test should release the controlled resolver");
            Ok(SecretValue::new(SECRET_SENTINEL.to_owned()))
        })
    }
}

#[derive(Default)]
struct RecordingTransport {
    calls: AtomicUsize,
    observation: Mutex<Option<Observation>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Observation {
    method: Method,
    url: Url,
    header: String,
    body: String,
    auth_header_name: String,
    auth_header_value: Vec<u8>,
    user_agent: Option<&'static str>,
    remaining_timeout: Duration,
    prepared_debug: String,
}

impl AsyncHttpTransport for RecordingTransport {
    fn execute<'a>(
        &'a self,
        prepared: &'a PreparedHttpRequestV1<'_>,
        remaining_timeout: Duration,
    ) -> TransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (auth_header_name, auth_header_value) = prepared
            .auth_headers()
            .next()
            .expect("both credential arms bind exactly one auth header");
        let observation = Observation {
            method: prepared.method().clone(),
            url: prepared.url().clone(),
            header: prepared
                .headers()
                .get("x-test")
                .expect("prepared fixture should retain its ordinary header")
                .to_owned(),
            body: prepared.body().map_or_else(String::new, |body| {
                String::from_utf8_lossy(body.as_bytes()).into_owned()
            }),
            auth_header_name: auth_header_name.to_owned(),
            auth_header_value: auth_header_value.to_vec(),
            user_agent: prepared.user_agent().map(ControlledUserAgentV1::as_str),
            remaining_timeout,
            prepared_debug: format!("{prepared:?}"),
        };
        *self.observation.lock().expect("test observation lock should be available") =
            Some(observation);
        Box::pin(async { Ok(response()) })
    }
}

#[tokio::test]
async fn wrong_slot_precedes_an_already_cancelled_preflight() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = execute_provider_call_v1(
        &binding("https://provider.invalid/base", "bound-slot"),
        &request("v1/call", "other-slot"),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    )
    .await
    .expect_err("a mismatched credential slot must fail");

    assert_eq!(error.code(), PreparationErrorV1::CredentialBindingMismatch.code());
    assert!(matches!(
        error,
        ProviderCallErrorV1::Preparation(PreparationErrorV1::CredentialBindingMismatch)
    ));
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn already_cancelled_stops_before_resolver_and_transport() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = execute_provider_call_v1(
        &binding("https://provider.invalid/base", "bound-slot"),
        &request("v1/call", "bound-slot"),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    )
    .await
    .expect_err("an already cancelled request must fail");

    assert_eq!(error.code(), PreparationErrorV1::Cancelled.code());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn already_expired_stops_before_resolver_and_transport() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();

    let error = execute_provider_call_v1(
        &binding("https://provider.invalid/base", "bound-slot"),
        &request("v1/call", "bound-slot"),
        &resolver,
        &transport,
        tokio::time::Instant::now(),
        &CancellationToken::new(),
    )
    .await
    .expect_err("an already expired request must fail");

    assert_eq!(error.code(), PreparationErrorV1::DeadlineExceeded.code());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resolver_failure_is_stable_and_stops_before_transport() {
    let resolver = ImmediateResolver::failing();
    let transport = RecordingTransport::default();

    let error = execute_provider_call_v1(
        &binding("https://provider.invalid/base", "bound-slot"),
        &request("v1/call", "bound-slot"),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("resolver failure must map to the frozen preparation error");

    assert_eq!(error.code(), PreparationErrorV1::CredentialResolutionFailed.code());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn success_prepares_exactly_one_post_for_the_transport() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let request = request(PATH_SENTINEL, SLOT_SENTINEL);

    let result = execute_provider_call_v1(
        &binding(&format!("https://{ENDPOINT_SENTINEL}/base"), SLOT_SENTINEL),
        &request,
        &resolver,
        &transport,
        deadline,
        &CancellationToken::new(),
    )
    .await
    .expect("a valid prepared call should succeed");

    assert_eq!(result.status(), StatusCode::CREATED);
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let observation = transport
        .observation
        .lock()
        .expect("test observation lock should be available")
        .as_ref()
        .expect("transport should record the prepared request")
        .clone();
    assert_eq!(
        &observation,
        &Observation {
            method: Method::POST,
            url: Url::parse(&format!("https://{ENDPOINT_SENTINEL}/base/{PATH_SENTINEL}"))
                .expect("expected prepared URL should be valid"),
            header: HEADER_SENTINEL.to_owned(),
            body: format!(r#"{{"value":"{BODY_SENTINEL}"}}"#),
            auth_header_name: "authorization".to_owned(),
            auth_header_value: format!("Bearer {SECRET_SENTINEL}").into_bytes(),
            user_agent: None,
            remaining_timeout: Duration::from_secs(30),
            prepared_debug: format!(
                "PreparedHttpRequestV1 {{ method: POST, header_count: 1, body_byte_count: {}, .. }}",
                request.body().len()
            ),
        }
    );
    for sentinel in
        [ENDPOINT_SENTINEL, PATH_SENTINEL, HEADER_SENTINEL, BODY_SENTINEL, SECRET_SENTINEL]
    {
        assert!(!observation.prepared_debug.contains(sentinel));
    }
}

#[tokio::test(start_paused = true)]
async fn header_secret_prepares_the_sanctioned_header_with_the_verbatim_secret() {
    for header in SecretHeaderV1::ALL {
        let resolver = ImmediateResolver::default();
        let transport = RecordingTransport::default();

        execute_provider_call_v1(
            &binding(&format!("https://{ENDPOINT_SENTINEL}/base"), SLOT_SENTINEL),
            &header_secret_request(PATH_SENTINEL, SLOT_SENTINEL, header),
            &resolver,
            &transport,
            tokio::time::Instant::now() + Duration::from_secs(30),
            &CancellationToken::new(),
        )
        .await
        .expect("a valid header-secret call should succeed");

        let observation = transport
            .observation
            .lock()
            .expect("test observation lock should be available")
            .as_ref()
            .expect("transport should record the prepared request")
            .clone();
        assert_eq!(observation.auth_header_name, header.header_name());
        assert_eq!(observation.auth_header_value, SECRET_SENTINEL.as_bytes());
        assert_ne!(observation.auth_header_name, "authorization");
    }
}

#[tokio::test]
async fn header_secret_slot_mismatch_stops_before_resolver_and_transport() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();

    let error = execute_provider_call_v1(
        &binding("https://provider.invalid/base", "bound-slot"),
        &header_secret_request("v1/call", "other-slot", SecretHeaderV1::XApiKey),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("a mismatched header-secret slot must fail");

    assert_eq!(error.code(), PreparationErrorV1::CredentialBindingMismatch.code());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn resolver_time_is_subtracted_from_the_transport_budget() {
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let resolver = ControlledResolver {
        calls: AtomicUsize::new(0),
        started: Mutex::new(Some(started_tx)),
        release: Mutex::new(Some(release_rx)),
    };
    let transport = RecordingTransport::default();
    let call_binding = binding("https://provider.invalid/base", "bound-slot");
    let call_request = request("v1/call", "bound-slot");
    let cancellation = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

    let call = execute_provider_call_v1(
        &call_binding,
        &call_request,
        &resolver,
        &transport,
        deadline,
        &cancellation,
    );
    let advance_then_release = async {
        wait_for_start(started_rx).await;
        tokio::time::advance(Duration::from_secs(7)).await;
        release_tx.send(()).expect("resolver should still be waiting for release");
    };
    let (result, ()) = tokio::join!(call, advance_then_release);

    result.expect("resolver should finish before the absolute deadline");
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        transport
            .observation
            .lock()
            .expect("test observation lock should be available")
            .as_ref()
            .expect("transport should record the prepared request")
            .remaining_timeout,
        Duration::from_secs(23)
    );
}

#[tokio::test]
async fn dynamic_ports_produce_a_send_executor_future() {
    let concrete_resolver = ImmediateResolver::default();
    let concrete_transport = RecordingTransport::default();
    let resolver: &dyn CredentialResolver = &concrete_resolver;
    let transport: &dyn AsyncHttpTransport = &concrete_transport;
    let call_binding = binding("https://provider.invalid/base", "bound-slot");
    let call_request = request("v1/call", "bound-slot");
    let cancellation = CancellationToken::new();

    let future = execute_provider_call_v1(
        &call_binding,
        &call_request,
        resolver,
        transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    );
    assert_send(&future);
    let result = future.await.expect("dynamic resolver and transport ports should execute");

    assert_eq!(result.status(), StatusCode::CREATED);
    assert_eq!(concrete_resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(concrete_transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn simultaneous_cancellation_completion_and_deadline_prefer_cancellation() {
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let resolver = ControlledResolver {
        calls: AtomicUsize::new(0),
        started: Mutex::new(Some(started_tx)),
        release: Mutex::new(Some(release_rx)),
    };
    let transport = RecordingTransport::default();
    let call_binding = binding("https://provider.invalid/base", "bound-slot");
    let call_request = request("v1/call", "bound-slot");
    let cancellation = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

    let call = execute_provider_call_v1(
        &call_binding,
        &call_request,
        &resolver,
        &transport,
        deadline,
        &cancellation,
    );
    let make_every_branch_ready = async {
        wait_for_start(started_rx).await;
        tokio::time::sleep_until(deadline).await;
        cancellation.cancel();
        release_tx.send(()).expect("resolver should still be waiting for release");
    };
    let ((), result) = tokio::join!(biased; make_every_branch_ready, call);

    assert_eq!(
        result.expect_err("biased selection must prefer cancellation").code(),
        PreparationErrorV1::Cancelled.code()
    );
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct PendingResolver {
    calls: AtomicUsize,
    started: Mutex<Option<oneshot::Sender<()>>>,
    dropped: Arc<AtomicBool>,
}

impl CredentialResolver for PendingResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let started = self.started.lock().expect("test start lock should be available").take();
        let dropped = Arc::clone(&self.dropped);
        Box::pin(async move {
            let _drop_flag = DropFlag(dropped);
            if let Some(started) = started {
                let _ = started.send(());
            }
            pending::<Result<SecretValue, CredentialResolutionErrorV1>>().await
        })
    }
}

fn pending_resolver() -> (PendingResolver, oneshot::Receiver<()>, Arc<AtomicBool>) {
    let (started_tx, started_rx) = oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    (
        PendingResolver {
            calls: AtomicUsize::new(0),
            started: Mutex::new(Some(started_tx)),
            dropped: Arc::clone(&dropped),
        },
        started_rx,
        dropped,
    )
}

#[tokio::test(start_paused = true)]
async fn cancellation_drops_a_pending_resolver_without_calling_transport() {
    let (resolver, started, dropped) = pending_resolver();
    let transport = RecordingTransport::default();
    let cancellation = CancellationToken::new();
    let call_binding = binding("https://provider.invalid/base", "bound-slot");
    let call_request = request("v1/call", "bound-slot");

    let call = execute_provider_call_v1(
        &call_binding,
        &call_request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    );
    let cancel = async {
        wait_for_start(started).await;
        cancellation.cancel();
    };
    let (result, ()) = tokio::join!(call, cancel);

    assert_eq!(
        result.expect_err("cancellation must stop a pending resolver").code(),
        PreparationErrorV1::Cancelled.code()
    );
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn deadline_drops_a_pending_resolver_without_calling_transport() {
    let (resolver, started, dropped) = pending_resolver();
    let transport = RecordingTransport::default();
    let call_binding = binding("https://provider.invalid/base", "bound-slot");
    let call_request = request("v1/call", "bound-slot");
    let cancellation = CancellationToken::new();

    let call = execute_provider_call_v1(
        &call_binding,
        &call_request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    );
    let expire = async {
        wait_for_start(started).await;
        tokio::time::advance(Duration::from_secs(30)).await;
    };
    let (result, ()) = tokio::join!(call, expire);

    assert_eq!(
        result.expect_err("deadline must stop a pending resolver").code(),
        PreparationErrorV1::DeadlineExceeded.code()
    );
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

struct PendingTransport {
    calls: AtomicUsize,
    started: Mutex<Option<oneshot::Sender<()>>>,
    dropped: Arc<AtomicBool>,
}

impl AsyncHttpTransport for PendingTransport {
    fn execute<'a>(
        &'a self,
        _prepared: &'a PreparedHttpRequestV1<'_>,
        _remaining_timeout: Duration,
    ) -> TransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let started = self.started.lock().expect("test start lock should be available").take();
        let dropped = Arc::clone(&self.dropped);
        Box::pin(async move {
            let _drop_flag = DropFlag(dropped);
            if let Some(started) = started {
                let _ = started.send(());
            }
            pending::<Result<BufferedHttpResponseV1, TransportErrorV1>>().await
        })
    }
}

fn pending_transport() -> (PendingTransport, oneshot::Receiver<()>, Arc<AtomicBool>) {
    let (started_tx, started_rx) = oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    (
        PendingTransport {
            calls: AtomicUsize::new(0),
            started: Mutex::new(Some(started_tx)),
            dropped: Arc::clone(&dropped),
        },
        started_rx,
        dropped,
    )
}

#[tokio::test(start_paused = true)]
async fn cancellation_drops_a_pending_transport() {
    let resolver = ImmediateResolver::default();
    let (transport, started, dropped) = pending_transport();
    let cancellation = CancellationToken::new();
    let call_binding = binding("https://provider.invalid/base", "bound-slot");
    let call_request = request("v1/call", "bound-slot");

    let call = execute_provider_call_v1(
        &call_binding,
        &call_request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    );
    let cancel = async {
        wait_for_start(started).await;
        cancellation.cancel();
    };
    let (result, ()) = tokio::join!(call, cancel);

    assert_eq!(
        result.expect_err("cancellation must stop a pending transport").code(),
        PreparationErrorV1::Cancelled.code()
    );
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn deadline_drops_a_pending_transport() {
    let resolver = ImmediateResolver::default();
    let (transport, started, dropped) = pending_transport();
    let call_binding = binding("https://provider.invalid/base", "bound-slot");
    let call_request = request("v1/call", "bound-slot");
    let cancellation = CancellationToken::new();

    let call = execute_provider_call_v1(
        &call_binding,
        &call_request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    );
    let expire = async {
        wait_for_start(started).await;
        tokio::time::advance(Duration::from_secs(30)).await;
    };
    let (result, ()) = tokio::join!(call, expire);

    assert_eq!(
        result.expect_err("deadline must stop a pending transport").code(),
        PreparationErrorV1::DeadlineExceeded.code()
    );
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn composite_errors_never_include_request_or_secret_sentinels() {
    let errors = [
        ProviderCallErrorV1::Preparation(PreparationErrorV1::CredentialResolutionFailed),
        ProviderCallErrorV1::Transport(TransportErrorV1::RequestFailed),
    ];
    for error in errors {
        let rendered = format!("{error:?} {error}");
        for sentinel in [
            ENDPOINT_SENTINEL,
            PATH_SENTINEL,
            SLOT_SENTINEL,
            HEADER_SENTINEL,
            BODY_SENTINEL,
            SECRET_SENTINEL,
        ] {
            assert!(!rendered.contains(sentinel));
        }
    }
}

// ───────────────── controlled query (HTTP contract v2) ─────────────────

#[tokio::test]
async fn declared_query_reaches_the_transport_url_intact() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let query = QueryStringV1::try_from_iter([
        (QueryParameterV1::ApiVersion, "2024-10-21"),
        (QueryParameterV1::Alt, "sse"),
    ])
    .expect("both parameters are sanctioned");
    let request = request("v1/chat/completions", SLOT_SENTINEL).with_query(query);

    execute_provider_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("a sanctioned query must not block preparation");

    let observation = transport
        .observation
        .lock()
        .expect("observation lock")
        .clone()
        .expect("transport must be reached");
    // Canonical declaration order, not the order the caller declared them in.
    assert_eq!(observation.url.query(), Some("api-version=2024-10-21&alt=sse"));
    // The query must not have leaked into the path: `set_path` would have percent-encoded `?`
    // into `%3F` and produced a literal segment instead.
    assert_eq!(observation.url.path(), "/base/v1/chat/completions");
    assert!(observation.url.path().starts_with("/base/"), "binding prefix must survive");
}

#[tokio::test]
async fn a_request_without_a_query_still_reaches_the_wire_query_free() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();

    execute_provider_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request("v1/chat/completions", SLOT_SENTINEL),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("contract version one shape stays valid");

    let observation = transport
        .observation
        .lock()
        .expect("observation lock")
        .clone()
        .expect("transport must be reached");
    assert_eq!(observation.url.query(), None, "no query declared means no query on the wire");
}

// ─────────────── controlled user-agent (HTTP contract v3) ───────────────

#[tokio::test]
async fn declared_user_agent_reaches_the_transport_boundary_intact() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let user_agent = ControlledUserAgentV1::try_from_static("aws-sdk-js/1.0.0 KiroIDE")
        .expect("an audited inventory value must be accepted");
    let request = request("v1/chat/completions", SLOT_SENTINEL).with_user_agent(user_agent);

    execute_provider_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("a sanctioned user-agent must not block preparation");

    let observation = transport
        .observation
        .lock()
        .expect("observation lock")
        .clone()
        .expect("transport must be reached");
    assert_eq!(observation.user_agent, Some("aws-sdk-js/1.0.0 KiroIDE"));
    // The declaration is a header concern only: it must not have touched the URL or the auth
    // channel.
    assert_eq!(observation.url.as_str(), "https://example.com/base/v1/chat/completions");
    assert_eq!(observation.auth_header_name, "authorization");
}

#[tokio::test]
async fn a_request_without_a_user_agent_prepares_none() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();

    execute_provider_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request("v1/chat/completions", SLOT_SENTINEL),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("contract version two shape stays valid");

    let observation = transport
        .observation
        .lock()
        .expect("observation lock")
        .clone()
        .expect("transport must be reached");
    assert_eq!(
        observation.user_agent, None,
        "no user-agent declared means none at the prepared boundary"
    );
}

// ─────────────────── buffered GET request (HTTP contract v6) ───────────────────

fn get_request(path: &str, slot: &str) -> GetRequestV1 {
    GetRequestV1::new(
        RelativePathV1::parse(path).expect("fixture path should be valid"),
        SafeHeaders::try_from_iter([("x-test", HEADER_SENTINEL)])
            .expect("fixture headers should be valid"),
        BearerAuthV1::new(CredentialSlotV1::parse(slot).expect("fixture slot should be valid")),
    )
}

fn observed(transport: &RecordingTransport) -> Observation {
    transport
        .observation
        .lock()
        .expect("test observation lock should be available")
        .as_ref()
        .expect("transport should record the prepared request")
        .clone()
}

#[tokio::test(start_paused = true)]
async fn get_success_prepares_exactly_one_body_less_get_for_the_transport() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

    let result = execute_get_call_v1(
        &binding(&format!("https://{ENDPOINT_SENTINEL}/base"), SLOT_SENTINEL),
        &get_request(PATH_SENTINEL, SLOT_SENTINEL),
        &resolver,
        &transport,
        deadline,
        &CancellationToken::new(),
    )
    .await
    .expect("a valid prepared GET should succeed");

    assert_eq!(result.status(), StatusCode::CREATED);
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let observation = observed(&transport);
    assert_eq!(
        &observation,
        &Observation {
            method: Method::GET,
            url: Url::parse(&format!("https://{ENDPOINT_SENTINEL}/base/{PATH_SENTINEL}"))
                .expect("expected prepared URL should be valid"),
            header: HEADER_SENTINEL.to_owned(),
            // No body slot at all: the recorder's fallback for `None`, not an empty JSON body.
            body: String::new(),
            auth_header_name: "authorization".to_owned(),
            auth_header_value: format!("Bearer {SECRET_SENTINEL}").into_bytes(),
            user_agent: None,
            remaining_timeout: Duration::from_secs(30),
            prepared_debug:
                "PreparedHttpRequestV1 { method: GET, header_count: 1, body_byte_count: 0, .. }"
                    .to_owned(),
        }
    );
}

#[tokio::test]
async fn get_slot_mismatch_stops_before_resolver_and_transport() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();

    let error = execute_get_call_v1(
        &binding(&format!("https://{ENDPOINT_SENTINEL}/base"), SLOT_SENTINEL),
        &get_request(PATH_SENTINEL, "other-slot"),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("a slot outside the binding must be refused");

    assert!(matches!(
        error,
        ProviderCallErrorV1::Preparation(PreparationErrorV1::CredentialBindingMismatch)
    ));
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn get_task_id_query_reaches_the_transport_url_intact() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let query = QueryStringV1::try_from_iter([(QueryParameterV1::TaskId, "276843862449040")])
        .expect("the task id fixture satisfies its grammar");
    let request = get_request("v1/query/video_generation", SLOT_SENTINEL).with_query(query);

    execute_get_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("a sanctioned task id must not block preparation");

    let observation = observed(&transport);
    assert_eq!(observation.method, Method::GET);
    assert_eq!(observation.url.query(), Some("task_id=276843862449040"));
    assert_eq!(observation.url.path(), "/base/v1/query/video_generation");
    assert!(observation.body.is_empty(), "a poll carries its id in the URL, never a body");
}

#[tokio::test]
async fn get_declared_user_agent_and_header_secret_arm_reach_the_boundary() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let request = GetRequestV1::new(
        RelativePathV1::parse("v1/videos/276843862449040").expect("fixture path"),
        SafeHeaders::try_from_iter([("x-test", HEADER_SENTINEL)]).expect("fixture headers"),
        ProviderAuthV1::HeaderSecret {
            header: SecretHeaderV1::XApiKey,
            slot: BearerAuthV1::new(CredentialSlotV1::parse(SLOT_SENTINEL).expect("slot")),
        },
    )
    .with_user_agent(ControlledUserAgentV1::try_from_static("south-poll/1.0").expect("agent"));

    execute_get_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("the header-secret arm binds on a GET exactly as on a POST");

    let observation = observed(&transport);
    assert_eq!(observation.auth_header_name, "x-api-key");
    assert_eq!(observation.auth_header_value, SECRET_SENTINEL.as_bytes());
    assert_eq!(observation.user_agent, Some("south-poll/1.0"));
}

#[tokio::test]
async fn the_unsigned_get_entry_point_refuses_the_host_signed_arm() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let request = GetRequestV1::new(
        RelativePathV1::parse(PATH_SENTINEL).expect("fixture path"),
        SafeHeaders::default(),
        ProviderAuthV1::HostSigned {
            slot: BearerAuthV1::new(CredentialSlotV1::parse(SLOT_SENTINEL).expect("slot")),
            emits: SignedHeaderSetV1::new(&[SignedHeaderV1::Authorization]).expect("declaration"),
        },
    );

    let error = execute_get_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("the unsigned GET path cannot serve a signed request");

    assert!(matches!(
        error,
        ProviderCallErrorV1::Preparation(PreparationErrorV1::UnsupportedAuthShape)
    ));
    // Same seam as the POST twin: the shape is refused at assembly, after resolution and before
    // the transport, so the dangerous half — an unauthenticated request on the wire — cannot
    // happen.
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

// ───────────────── multipart POST (HTTP contract v7) ─────────────────

const MULTIPART_BOUNDARY: &str = "core-test-boundary";

fn multipart_body() -> MultipartBodyV1 {
    let boundary = MultipartBoundaryV1::parse(MULTIPART_BOUNDARY).expect("fixture boundary");
    let bytes = format!(
        "--{MULTIPART_BOUNDARY}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\n{BODY_SENTINEL}\r\n--{MULTIPART_BOUNDARY}--\r\n"
    )
    .into_bytes();
    MultipartBodyV1::parse(bytes, boundary).expect("fixture body is delimited by its boundary")
}

fn multipart_request(path: &str, slot: &str) -> MultipartPostRequestV1 {
    MultipartPostRequestV1::try_new(
        RelativePathV1::parse(path).expect("fixture path should be valid"),
        SafeHeaders::try_from_iter([("x-test", HEADER_SENTINEL)])
            .expect("fixture headers should be valid"),
        multipart_body(),
        BearerAuthV1::new(CredentialSlotV1::parse(slot).expect("fixture slot should be valid")),
    )
    .expect("fixture headers carry no content-type")
}

#[tokio::test(start_paused = true)]
async fn multipart_success_prepares_one_post_whose_media_type_south_renders() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();
    let request = multipart_request(PATH_SENTINEL, SLOT_SENTINEL);

    let result = execute_multipart_call_v1(
        &binding(&format!("https://{ENDPOINT_SENTINEL}/base"), SLOT_SENTINEL),
        &request,
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("a valid prepared multipart call should succeed");

    assert_eq!(result.status(), StatusCode::CREATED);
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let observation = observed(&transport);
    assert_eq!(observation.method, Method::POST);
    assert_eq!(observation.auth_header_name, "authorization");
    // The bytes reach the boundary unmodified — South re-encodes nothing.
    assert_eq!(observation.body.as_bytes(), multipart_body().as_bytes());
    assert_eq!(
        observation.prepared_debug,
        format!(
            "PreparedHttpRequestV1 {{ method: POST, header_count: 1, body_byte_count: {}, .. }}",
            multipart_body().len()
        )
    );
    for sentinel in [ENDPOINT_SENTINEL, PATH_SENTINEL, HEADER_SENTINEL, SECRET_SENTINEL] {
        assert!(!observation.prepared_debug.contains(sentinel));
    }
}

/// The media type is South's, rendered from the boundary the contract validated, and the JSON and
/// GET shapes report none — which is what keeps their wire bytes identical to version six.
#[tokio::test]
async fn only_the_multipart_shape_reports_a_content_type_to_the_transport() {
    /// One slot per transport call, so "never reached" and "reached, reported none" stay
    /// distinguishable without nesting two `Option`s.
    #[derive(Default)]
    struct ContentTypeProbe {
        seen: Mutex<Vec<Option<String>>>,
    }

    impl AsyncHttpTransport for ContentTypeProbe {
        fn execute<'a>(
            &'a self,
            prepared: &'a PreparedHttpRequestV1<'_>,
            _remaining_timeout: Duration,
        ) -> TransportFuture<'a> {
            self.seen.lock().expect("probe lock").push(prepared.content_type().map(str::to_owned));
            Box::pin(async { Ok(response()) })
        }
    }

    let deadline = || tokio::time::Instant::now() + Duration::from_secs(30);
    let bound = binding("https://example.com/base/", SLOT_SENTINEL);

    let probe = ContentTypeProbe::default();
    execute_multipart_call_v1(
        &bound,
        &multipart_request("v1/audio/transcriptions", SLOT_SENTINEL),
        &ImmediateResolver::default(),
        &probe,
        deadline(),
        &CancellationToken::new(),
    )
    .await
    .expect("multipart succeeds");
    assert_eq!(
        probe.seen.lock().expect("probe lock").as_slice(),
        [Some(format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"))],
        "the multipart shape renders its own media type"
    );

    let probe = ContentTypeProbe::default();
    execute_provider_call_v1(
        &bound,
        &request("v1/chat/completions", SLOT_SENTINEL),
        &ImmediateResolver::default(),
        &probe,
        deadline(),
        &CancellationToken::new(),
    )
    .await
    .expect("json succeeds");
    assert_eq!(
        probe.seen.lock().expect("probe lock").as_slice(),
        [None],
        "the JSON shape's content-type stays an ordinary header the host declares"
    );

    let probe = ContentTypeProbe::default();
    execute_get_call_v1(
        &bound,
        &get_request("v1/videos/1", SLOT_SENTINEL),
        &ImmediateResolver::default(),
        &probe,
        deadline(),
        &CancellationToken::new(),
    )
    .await
    .expect("get succeeds");
    assert_eq!(probe.seen.lock().expect("probe lock").as_slice(), [None]);
}

#[tokio::test]
async fn multipart_slot_mismatch_stops_before_resolver_and_transport() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();

    let error = execute_multipart_call_v1(
        &binding(&format!("https://{ENDPOINT_SENTINEL}/base"), SLOT_SENTINEL),
        &multipart_request(PATH_SENTINEL, "other-slot"),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("a slot outside the binding must be refused");

    assert!(matches!(
        error,
        ProviderCallErrorV1::Preparation(PreparationErrorV1::CredentialBindingMismatch)
    ));
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

/// A host-signed multipart request has no signed entry point to reach (multipart record, D5), so
/// the unsigned one must refuse it rather than send it unauthenticated.
#[tokio::test]
async fn the_multipart_entry_point_refuses_the_host_signed_arm() {
    let transport = RecordingTransport::default();
    let request = MultipartPostRequestV1::try_new(
        RelativePathV1::parse(PATH_SENTINEL).expect("fixture path"),
        SafeHeaders::default(),
        multipart_body(),
        ProviderAuthV1::HostSigned {
            slot: BearerAuthV1::new(CredentialSlotV1::parse(SLOT_SENTINEL).expect("slot")),
            emits: SignedHeaderSetV1::new(&[SignedHeaderV1::Authorization]).expect("declaration"),
        },
    )
    .expect("valid");

    let error = execute_multipart_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request,
        &ImmediateResolver::default(),
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("the unsigned multipart path cannot serve a signed request");

    assert!(matches!(
        error,
        ProviderCallErrorV1::Preparation(PreparationErrorV1::UnsupportedAuthShape)
    ));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

/// A byte sequence no UTF-8 decoder accepts, shaped like the head of an MP3 frame.
const BINARY_SENTINEL: &[u8] = &[0xff, 0xfb, 0x90, 0x80, 0x00, 0x80];

/// The binary twin of [`RecordingTransport`], recording the same request-side facts.
///
/// Deliberately records what the *request* looked like rather than what the response was: the
/// point of these cases is that the binary arm changes nothing before the wire. One type may
/// implement both transport traits, and the shipped reqwest transport does.
#[derive(Default)]
struct BinaryRecordingTransport {
    calls: AtomicUsize,
    observation: Mutex<Option<Observation>>,
}

impl AsyncBinaryHttpTransport for BinaryRecordingTransport {
    fn execute_binary<'a>(
        &'a self,
        prepared: &'a PreparedHttpRequestV1<'_>,
        remaining_timeout: Duration,
    ) -> BinaryTransportFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (auth_header_name, auth_header_value) = prepared
            .auth_headers()
            .next()
            .expect("both credential arms bind exactly one auth header");
        let observation = Observation {
            method: prepared.method().clone(),
            url: prepared.url().clone(),
            header: prepared
                .headers()
                .get("x-test")
                .expect("prepared fixture should retain its ordinary header")
                .to_owned(),
            body: prepared.body().map_or_else(String::new, |body| {
                String::from_utf8_lossy(body.as_bytes()).into_owned()
            }),
            auth_header_name: auth_header_name.to_owned(),
            auth_header_value: auth_header_value.to_vec(),
            user_agent: prepared.user_agent().map(ControlledUserAgentV1::as_str),
            remaining_timeout,
            prepared_debug: format!("{prepared:?}"),
        };
        *self.observation.lock().expect("test observation lock should be available") =
            Some(observation);
        Box::pin(async {
            Ok(BufferedBinaryResponseV1::try_from_parts(
                StatusCode::OK,
                BINARY_SENTINEL.to_vec(),
                Some("audio/mpeg".to_owned()),
                None,
            )
            .expect("fixture response is valid"))
        })
    }
}

#[tokio::test]
async fn the_binary_entry_point_delivers_bytes_no_utf8_decoder_would_accept() {
    let transport = BinaryRecordingTransport::default();

    let response = execute_binary_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request(PATH_SENTINEL, SLOT_SENTINEL),
        &ImmediateResolver::default(),
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("a binary response body is not required to be UTF-8");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.body(), BINARY_SENTINEL);
    assert_eq!(response.content_type(), Some("audio/mpeg"));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_binary_arm_prepares_exactly_what_the_utf8_arm_prepares() {
    // The load-bearing claim of D6's "JSON POST only": this is the same request type going
    // through the same preparation, and the only thing that differs is which transport trait
    // receives it. If the binary arm ever grew its own request handling, this test is what
    // notices.
    let binding = binding("https://example.com/base/", SLOT_SENTINEL);
    let request = request(PATH_SENTINEL, SLOT_SENTINEL);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

    let text_transport = RecordingTransport::default();
    execute_provider_call_v1(
        &binding,
        &request,
        &ImmediateResolver::default(),
        &text_transport,
        deadline,
        &CancellationToken::new(),
    )
    .await
    .expect("the UTF-8 arm succeeds");

    let binary_transport = BinaryRecordingTransport::default();
    execute_binary_call_v1(
        &binding,
        &request,
        &ImmediateResolver::default(),
        &binary_transport,
        deadline,
        &CancellationToken::new(),
    )
    .await
    .expect("the binary arm succeeds");

    let text = text_transport.observation.lock().expect("lock").clone().expect("observed");
    let binary = binary_transport.observation.lock().expect("lock").clone().expect("observed");

    assert_eq!(text.method, binary.method);
    assert_eq!(text.url, binary.url);
    assert_eq!(text.header, binary.header);
    assert_eq!(text.body, binary.body);
    assert_eq!(text.auth_header_name, binary.auth_header_name);
    assert_eq!(text.auth_header_value, binary.auth_header_value);
    assert_eq!(text.user_agent, binary.user_agent);
    assert_eq!(text.prepared_debug, binary.prepared_debug);
}

#[tokio::test]
async fn the_binary_entry_point_refuses_a_slot_mismatch_before_the_transport() {
    let transport = BinaryRecordingTransport::default();

    let error = execute_binary_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request(PATH_SENTINEL, "different-slot-sentinel"),
        &ImmediateResolver::default(),
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("a requested slot that is not the bound one cannot reach a transport");

    assert!(matches!(
        error,
        ProviderCallErrorV1::Preparation(PreparationErrorV1::CredentialBindingMismatch)
    ));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn the_binary_entry_point_refuses_the_host_signed_arm() {
    let transport = BinaryRecordingTransport::default();
    let request = JsonPostRequestV1::new(
        RelativePathV1::parse(PATH_SENTINEL).expect("fixture path"),
        SafeHeaders::try_from_iter([("x-test", HEADER_SENTINEL)]).expect("fixture header"),
        JsonBodyV1::parse(&format!("{{\"value\":\"{BODY_SENTINEL}\"}}")).expect("fixture body"),
        ProviderAuthV1::HostSigned {
            slot: BearerAuthV1::new(CredentialSlotV1::parse(SLOT_SENTINEL).expect("slot")),
            emits: SignedHeaderSetV1::new(&[SignedHeaderV1::Authorization]).expect("declaration"),
        },
    );

    let error = execute_binary_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request,
        &ImmediateResolver::default(),
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("the unsigned binary path cannot serve a signed request");

    assert!(matches!(
        error,
        ProviderCallErrorV1::Preparation(PreparationErrorV1::UnsupportedAuthShape)
    ));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_cancelled_token_pre_empts_the_binary_arm_before_the_transport() {
    // The binary arm shares one flow with the UTF-8 arm, so cancellation precedence is inherited
    // rather than reimplemented. Pinned here because "inherited" is exactly the kind of claim a
    // later refactor breaks silently.
    let transport = BinaryRecordingTransport::default();
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = execute_binary_call_v1(
        &binding("https://example.com/base/", SLOT_SENTINEL),
        &request(PATH_SENTINEL, SLOT_SENTINEL),
        &ImmediateResolver::default(),
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    )
    .await
    .expect_err("a cancelled call never reaches a transport");

    assert!(matches!(error, ProviderCallErrorV1::Preparation(PreparationErrorV1::Cancelled)));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}
