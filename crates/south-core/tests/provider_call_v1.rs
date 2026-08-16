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
    BearerAuthV1, BufferedHttpResponseV1, CredentialSlotV1, JsonBodyV1, JsonPostRequestV1,
    PreparationErrorV1, ProviderEndpointV1, RelativePathV1, SafeHeaders, TransportErrorV1,
};
use south_core::{
    AsyncHttpTransport, CredentialResolutionErrorV1, CredentialResolutionFuture,
    CredentialResolver, PreparedHttpRequestV1, ProviderBindingV1, ProviderCallErrorV1, SecretValue,
    TransportFuture, execute_provider_call_v1,
};
use static_assertions::{assert_impl_all, assert_not_impl_any};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

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

fn response() -> BufferedHttpResponseV1 {
    BufferedHttpResponseV1::try_from_parts(
        StatusCode::CREATED,
        br#"{"ok":true}"#.to_vec(),
        Some("application/json".to_owned()),
        None,
    )
    .expect("fixture response should be valid")
}

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

#[derive(Default)]
struct RecordingTransport {
    calls: AtomicUsize,
    observation: Mutex<Option<Observation>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Observation {
    method: Method,
    url: String,
    header: String,
    body: String,
    secret: String,
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
        let observation = Observation {
            method: prepared.method().clone(),
            url: prepared.url().to_owned(),
            header: prepared
                .headers()
                .get("x-test")
                .expect("prepared fixture should retain its ordinary header")
                .to_owned(),
            body: prepared.body().as_str().to_owned(),
            secret: prepared.bearer_secret().to_owned(),
            remaining_timeout,
            prepared_debug: format!("{prepared:?}"),
        };
        *self.observation.lock().expect("test observation lock should be available") =
            Some(observation);
        Box::pin(async { Ok(response()) })
    }
}

#[tokio::test]
async fn wrong_slot_stops_before_resolver_and_transport() {
    let resolver = ImmediateResolver::default();
    let transport = RecordingTransport::default();

    let error = execute_provider_call_v1(
        &binding("https://provider.invalid/base", "bound-slot"),
        &request("v1/call", "other-slot"),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
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
            url: format!("https://{ENDPOINT_SENTINEL}/base/{PATH_SENTINEL}"),
            header: HEADER_SENTINEL.to_owned(),
            body: format!(r#"{{"value":"{BODY_SENTINEL}"}}"#),
            secret: SECRET_SENTINEL.to_owned(),
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

#[tokio::test]
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
        started.await.expect("resolver should start");
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
        started.await.expect("resolver should start");
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

#[tokio::test]
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
        started.await.expect("transport should start");
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
        started.await.expect("transport should start");
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
