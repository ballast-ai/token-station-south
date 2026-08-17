use std::{
    future::pending,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{
    BearerAuthV1, CredentialSlotV1, JsonBodyV1, JsonPostRequestV1, PreparationErrorV1,
    ProviderEndpointV1, RelativePathV1, SafeHeaders, StreamChunkV1, StreamReadErrorV1,
    StreamRejectedV1, StreamingResponseHeadV1, TransportErrorV1,
};
use south_core::{
    AsyncStreamingTransport, CredentialResolutionErrorV1, CredentialResolutionFuture,
    CredentialResolver, OpenedByteStreamV1, PreparedHttpRequestV1, ProviderBindingV1,
    ProviderCallErrorV1, SecretValue, StreamByteSourceV1, StreamChunkFutureV1, StreamOpenErrorV1,
    StreamingCallV1, StreamingOpenFutureV1, open_streaming_provider_call_v1,
};
use static_assertions::assert_impl_all;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

assert_impl_all!(StreamingCallV1: Send);

const ENDPOINT_SENTINEL: &str = "stream-endpoint-sentinel.invalid";
const PATH_SENTINEL: &str = "stream-path-sentinel";
const SLOT_SENTINEL: &str = "stream-slot-sentinel";
const HEADER_SENTINEL: &str = "stream-header-sentinel";
const BODY_SENTINEL: &str = "stream-body-sentinel";
const SECRET_SENTINEL: &str = "stream-secret-sentinel";
const CONTENT_TYPE_SENTINEL: &str = "stream-content-type-sentinel";
const ERROR_BODY_SENTINEL: &str = "stream-error-body-sentinel";
const CHUNK_ONE: &[u8] = b"stream-chunk-one-sentinel";
const CHUNK_TWO: &[u8] = b"stream-chunk-two-sentinel";

fn binding() -> ProviderBindingV1 {
    ProviderBindingV1::new(
        ProviderEndpointV1::parse(&format!("https://{ENDPOINT_SENTINEL}/base"))
            .expect("fixture endpoint should be valid"),
        CredentialSlotV1::parse(SLOT_SENTINEL).expect("fixture slot should be valid"),
    )
}

fn request(slot: &str) -> JsonPostRequestV1 {
    JsonPostRequestV1::new(
        RelativePathV1::parse(PATH_SENTINEL).expect("fixture path should be valid"),
        SafeHeaders::try_from_iter([("x-test", HEADER_SENTINEL)])
            .expect("fixture header should be valid"),
        JsonBodyV1::parse(&format!(r#"{{"value":"{BODY_SENTINEL}"}}"#))
            .expect("fixture body should be valid"),
        BearerAuthV1::new(CredentialSlotV1::parse(slot).expect("fixture slot should be valid")),
    )
}

fn head(status: StatusCode) -> StreamingResponseHeadV1 {
    StreamingResponseHeadV1::try_from_parts(status, Some(CONTENT_TYPE_SENTINEL.to_owned()), None)
        .expect("fixture head should be valid")
}

async fn wait_for_start(started: oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(1), started)
        .await
        .expect("start handshake watchdog should not expire")
        .expect("operation should send its start handshake");
}

#[derive(Default)]
struct ImmediateResolver {
    calls: AtomicUsize,
    fail: bool,
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

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy)]
enum SourceEvent {
    Chunk(&'static [u8]),
    Error(StreamReadErrorV1),
    Pending,
}

/// Shared instrumentation handles for one scripted fake source.
#[derive(Clone, Default)]
struct SourceProbes {
    calls: Arc<AtomicUsize>,
    source_dropped: Arc<AtomicBool>,
    pending_future_dropped: Arc<AtomicBool>,
    pending_started: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

struct ScriptedSource {
    events: Vec<SourceEvent>,
    next_index: usize,
    probes: SourceProbes,
}

impl StreamByteSourceV1 for ScriptedSource {
    fn next_chunk(&mut self) -> StreamChunkFutureV1<'_> {
        self.probes.calls.fetch_add(1, Ordering::SeqCst);
        let event = self.events.get(self.next_index).copied();
        self.next_index += 1;
        let probes = self.probes.clone();
        Box::pin(async move {
            match event {
                Some(SourceEvent::Chunk(bytes)) => {
                    Some(StreamChunkV1::try_new(Bytes::from_static(bytes)))
                }
                Some(SourceEvent::Error(error)) => Some(Err(error)),
                Some(SourceEvent::Pending) => {
                    let _drop_flag = DropFlag(Arc::clone(&probes.pending_future_dropped));
                    let started = {
                        let mut sender = probes
                            .pending_started
                            .lock()
                            .expect("test start lock should be available");
                        sender.take()
                    };
                    if let Some(started) = started {
                        let _ = started.send(());
                    }
                    pending().await
                }
                None => None,
            }
        })
    }
}

impl Drop for ScriptedSource {
    fn drop(&mut self) {
        self.probes.source_dropped.store(true, Ordering::SeqCst);
    }
}

struct ScriptedTransport {
    calls: AtomicUsize,
    status: StatusCode,
    events: Vec<SourceEvent>,
    probes: SourceProbes,
}

impl ScriptedTransport {
    fn new(events: Vec<SourceEvent>) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            status: StatusCode::OK,
            events,
            probes: SourceProbes::default(),
        }
    }
}

impl AsyncStreamingTransport for ScriptedTransport {
    fn open<'a>(&'a self, request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.bearer_secret(), SECRET_SENTINEL.as_bytes());
        let source = ScriptedSource {
            events: self.events.clone(),
            next_index: 0,
            probes: self.probes.clone(),
        };
        let status = self.status;
        Box::pin(async move {
            OpenedByteStreamV1::try_new(head(status), Box::new(source))
                .map_err(StreamOpenErrorV1::Transport)
        })
    }
}

struct RejectingTransport {
    calls: AtomicUsize,
}

impl AsyncStreamingTransport for RejectingTransport {
    fn open<'a>(&'a self, _request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            Err(StreamOpenErrorV1::Rejected(StreamRejectedV1::new(
                head(StatusCode::TOO_MANY_REQUESTS),
                ERROR_BODY_SENTINEL.as_bytes().to_vec(),
            )))
        })
    }
}

struct FailingTransport {
    calls: AtomicUsize,
    error: TransportErrorV1,
}

impl AsyncStreamingTransport for FailingTransport {
    fn open<'a>(&'a self, _request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let error = self.error;
        Box::pin(async move { Err(StreamOpenErrorV1::Transport(error)) })
    }
}

struct PendingOpenTransport {
    calls: AtomicUsize,
    started: Mutex<Option<oneshot::Sender<()>>>,
    dropped: Arc<AtomicBool>,
}

impl AsyncStreamingTransport for PendingOpenTransport {
    fn open<'a>(&'a self, _request: &'a PreparedHttpRequestV1<'_>) -> StreamingOpenFutureV1<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let started = self.started.lock().expect("test start lock should be available").take();
        let dropped = Arc::clone(&self.dropped);
        Box::pin(async move {
            let _drop_flag = DropFlag(dropped);
            if let Some(started) = started {
                let _ = started.send(());
            }
            pending().await
        })
    }
}

async fn open_with(
    resolver: &ImmediateResolver,
    transport: &dyn AsyncStreamingTransport,
    deadline: Option<tokio::time::Instant>,
    cancellation: &CancellationToken,
    slot: &str,
) -> Result<StreamingCallV1, ProviderCallErrorV1> {
    open_streaming_provider_call_v1(
        &binding(),
        &request(slot),
        resolver,
        transport,
        deadline,
        cancellation,
    )
    .await
}

#[tokio::test]
async fn wrong_slot_stops_before_resolver_and_transport() {
    let resolver = ImmediateResolver::default();
    let transport = ScriptedTransport::new(Vec::new());

    let error = open_with(&resolver, &transport, None, &CancellationToken::new(), "other-slot")
        .await
        .expect_err("a mismatched credential slot must fail");

    assert_eq!(error.code(), PreparationErrorV1::CredentialBindingMismatch.code());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn already_cancelled_stops_before_resolver_and_transport() {
    let resolver = ImmediateResolver::default();
    let transport = ScriptedTransport::new(Vec::new());
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = open_with(&resolver, &transport, None, &cancellation, SLOT_SENTINEL)
        .await
        .expect_err("an already cancelled open must fail");

    assert_eq!(error.code(), PreparationErrorV1::Cancelled.code());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn already_expired_optional_deadline_stops_before_resolver_and_transport() {
    let resolver = ImmediateResolver::default();
    let transport = ScriptedTransport::new(Vec::new());

    let error = open_with(
        &resolver,
        &transport,
        Some(tokio::time::Instant::now()),
        &CancellationToken::new(),
        SLOT_SENTINEL,
    )
    .await
    .expect_err("an already expired open must fail");

    assert_eq!(error.code(), PreparationErrorV1::DeadlineExceeded.code());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 0);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resolver_failure_is_stable_and_stops_before_transport() {
    let resolver = ImmediateResolver { calls: AtomicUsize::new(0), fail: true };
    let transport = ScriptedTransport::new(Vec::new());

    let error = open_with(&resolver, &transport, None, &CancellationToken::new(), SLOT_SENTINEL)
        .await
        .expect_err("resolver failure must map to the frozen preparation error");

    assert_eq!(error.code(), PreparationErrorV1::CredentialResolutionFailed.code());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test(start_paused = true)]
async fn deadline_drops_a_pending_open_transport() {
    let resolver = ImmediateResolver::default();
    let (started_tx, started_rx) = oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let transport = PendingOpenTransport {
        calls: AtomicUsize::new(0),
        started: Mutex::new(Some(started_tx)),
        dropped: Arc::clone(&dropped),
    };
    let cancellation = CancellationToken::new();

    let open = open_with(
        &resolver,
        &transport,
        Some(tokio::time::Instant::now() + Duration::from_secs(30)),
        &cancellation,
        SLOT_SENTINEL,
    );
    let expire = async {
        wait_for_start(started_rx).await;
        tokio::time::advance(Duration::from_secs(30)).await;
    };
    let (result, ()) = tokio::join!(open, expire);

    assert_eq!(
        result.expect_err("deadline must stop a pending open").code(),
        PreparationErrorV1::DeadlineExceeded.code()
    );
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancellation_drops_a_pending_open_transport() {
    let resolver = ImmediateResolver::default();
    let (started_tx, started_rx) = oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let transport = PendingOpenTransport {
        calls: AtomicUsize::new(0),
        started: Mutex::new(Some(started_tx)),
        dropped: Arc::clone(&dropped),
    };
    let cancellation = CancellationToken::new();

    let open = open_with(&resolver, &transport, None, &cancellation, SLOT_SENTINEL);
    let cancel = async {
        wait_for_start(started_rx).await;
        cancellation.cancel();
    };
    let (result, ()) = tokio::join!(open, cancel);

    assert_eq!(
        result.expect_err("cancellation must stop a pending open").code(),
        PreparationErrorV1::Cancelled.code()
    );
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn successful_stream_yields_identical_chunks_then_clean_eof_with_no_deadline() {
    let resolver = ImmediateResolver::default();
    let transport =
        ScriptedTransport::new(vec![SourceEvent::Chunk(CHUNK_ONE), SourceEvent::Chunk(CHUNK_TWO)]);

    let mut call = open_with(&resolver, &transport, None, &CancellationToken::new(), SLOT_SENTINEL)
        .await
        .expect("a valid streaming open should succeed");

    assert_eq!(call.head().status(), StatusCode::OK);
    assert_eq!(call.head().content_type(), Some(CONTENT_TYPE_SENTINEL));
    let first = call
        .next_chunk()
        .await
        .expect("first pull should yield a chunk")
        .expect("first chunk should be delivered");
    assert_eq!(first.as_bytes(), CHUNK_ONE);
    let second = call
        .next_chunk()
        .await
        .expect("second pull should yield a chunk")
        .expect("second chunk should be delivered");
    assert_eq!(second.as_bytes(), CHUNK_TWO);
    assert!(call.next_chunk().await.is_none(), "clean upstream EOF must yield None");
    assert!(call.next_chunk().await.is_none(), "pulls after EOF must keep yielding None");
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn next_chunk_composes_with_a_caller_owned_select_loop() {
    let resolver = ImmediateResolver::default();
    let transport = ScriptedTransport::new(vec![SourceEvent::Chunk(CHUNK_ONE)]);
    let heartbeat = tokio::sync::Notify::new();

    let mut call = open_with(&resolver, &transport, None, &CancellationToken::new(), SLOT_SENTINEL)
        .await
        .expect("a valid streaming open should succeed");

    let mut delivered = Vec::new();
    loop {
        tokio::select! {
            chunk = call.next_chunk() => {
                match chunk {
                    Some(Ok(chunk)) => delivered.extend_from_slice(chunk.as_bytes()),
                    Some(Err(error)) => panic!("unexpected stream error: {}", error.code()),
                    None => break,
                }
            }
            () = heartbeat.notified() => unreachable!("heartbeat leg must stay pending"),
        }
    }

    assert_eq!(delivered, CHUNK_ONE);
}

#[tokio::test]
async fn upstream_rejection_is_a_terminal_error_without_a_stream_object() {
    let resolver = ImmediateResolver::default();
    let transport = RejectingTransport { calls: AtomicUsize::new(0) };

    let error = open_with(&resolver, &transport, None, &CancellationToken::new(), SLOT_SENTINEL)
        .await
        .expect_err("a rejected exchange must not produce a stream object");

    assert_eq!(error.code(), "UPSTREAM_REJECTED");
    let ProviderCallErrorV1::Rejected(rejected) = error else {
        panic!("a rejected exchange must use the Rejected variant");
    };
    assert_eq!(rejected.head().status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(rejected.body(), ERROR_BODY_SENTINEL.as_bytes());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn open_transport_failures_keep_their_frozen_codes() {
    let resolver = ImmediateResolver::default();
    let transport =
        FailingTransport { calls: AtomicUsize::new(0), error: TransportErrorV1::ConnectFailed };

    let error = open_with(&resolver, &transport, None, &CancellationToken::new(), SLOT_SENTINEL)
        .await
        .expect_err("an open transport failure must fail the call");

    assert_eq!(error.code(), "CONNECT_FAILED");
    assert!(matches!(error, ProviderCallErrorV1::Transport(TransportErrorV1::ConnectFailed)));
}

#[tokio::test]
async fn a_non_success_head_cannot_become_an_opened_stream() {
    let source =
        ScriptedSource { events: Vec::new(), next_index: 0, probes: SourceProbes::default() };

    let error = OpenedByteStreamV1::try_new(head(StatusCode::BAD_REQUEST), Box::new(source))
        .expect_err("a non-2xx head must not become an opened stream");

    assert_eq!(error, TransportErrorV1::RequestFailed);
}

#[tokio::test]
async fn cancellation_mid_pull_drops_the_in_flight_source_future() {
    let resolver = ImmediateResolver::default();
    let (started_tx, started_rx) = oneshot::channel();
    let transport =
        ScriptedTransport::new(vec![SourceEvent::Chunk(CHUNK_ONE), SourceEvent::Pending]);
    *transport.probes.pending_started.lock().expect("test start lock should be available") =
        Some(started_tx);
    let cancellation = CancellationToken::new();

    let mut call = open_with(&resolver, &transport, None, &cancellation, SLOT_SENTINEL)
        .await
        .expect("a valid streaming open should succeed");
    let first = call.next_chunk().await.expect("first pull should yield a chunk");
    assert_eq!(first.expect("first chunk should be delivered").as_bytes(), CHUNK_ONE);

    let pull = call.next_chunk();
    let cancel = async {
        wait_for_start(started_rx).await;
        cancellation.cancel();
    };
    let (outcome, ()) = tokio::join!(pull, cancel);

    assert_eq!(
        outcome.expect("a cancelled pull should report a terminal error"),
        Err(StreamReadErrorV1::StreamCancelled)
    );
    assert!(transport.probes.pending_future_dropped.load(Ordering::SeqCst));
    assert!(call.next_chunk().await.is_none(), "pulls after a terminal error must yield None");
}

#[tokio::test(start_paused = true)]
async fn deadline_mid_pull_drops_the_in_flight_source_future() {
    let resolver = ImmediateResolver::default();
    let (started_tx, started_rx) = oneshot::channel();
    let transport = ScriptedTransport::new(vec![SourceEvent::Pending]);
    *transport.probes.pending_started.lock().expect("test start lock should be available") =
        Some(started_tx);

    let mut call = open_with(
        &resolver,
        &transport,
        Some(tokio::time::Instant::now() + Duration::from_secs(30)),
        &CancellationToken::new(),
        SLOT_SENTINEL,
    )
    .await
    .expect("a valid streaming open should succeed");

    let pull = call.next_chunk();
    let expire = async {
        wait_for_start(started_rx).await;
        tokio::time::advance(Duration::from_secs(30)).await;
    };
    let (outcome, ()) = tokio::join!(pull, expire);

    assert_eq!(
        outcome.expect("an expired pull should report a terminal error"),
        Err(StreamReadErrorV1::StreamDeadlineExceeded)
    );
    assert!(transport.probes.pending_future_dropped.load(Ordering::SeqCst));
    assert!(call.next_chunk().await.is_none(), "pulls after a terminal error must yield None");
}

#[tokio::test(start_paused = true)]
async fn expired_deadline_fails_the_pull_even_when_a_chunk_is_ready() {
    let resolver = ImmediateResolver::default();
    let transport =
        ScriptedTransport::new(vec![SourceEvent::Chunk(CHUNK_ONE), SourceEvent::Chunk(CHUNK_TWO)]);

    let mut call = open_with(
        &resolver,
        &transport,
        Some(tokio::time::Instant::now() + Duration::from_secs(30)),
        &CancellationToken::new(),
        SLOT_SENTINEL,
    )
    .await
    .expect("a valid streaming open should succeed");
    let first = call.next_chunk().await.expect("first pull should yield a chunk");
    assert_eq!(first.expect("first chunk should be delivered").as_bytes(), CHUNK_ONE);
    let polls_before_expiry = transport.probes.calls.load(Ordering::SeqCst);

    tokio::time::advance(Duration::from_secs(30)).await;

    assert_eq!(
        call.next_chunk().await.expect("an expired pull must fail immediately"),
        Err(StreamReadErrorV1::StreamDeadlineExceeded)
    );
    assert_eq!(
        transport.probes.calls.load(Ordering::SeqCst),
        polls_before_expiry,
        "an expired deadline must be observed before the source is pulled again"
    );
    assert!(call.next_chunk().await.is_none(), "pulls after a terminal error must yield None");
}

#[tokio::test]
async fn source_errors_are_terminal_and_the_source_is_never_polled_again() {
    let resolver = ImmediateResolver::default();
    let transport = ScriptedTransport::new(vec![
        SourceEvent::Chunk(CHUNK_ONE),
        SourceEvent::Error(StreamReadErrorV1::StreamReadFailed),
        SourceEvent::Chunk(CHUNK_TWO),
    ]);

    let mut call = open_with(&resolver, &transport, None, &CancellationToken::new(), SLOT_SENTINEL)
        .await
        .expect("a valid streaming open should succeed");
    let first = call.next_chunk().await.expect("first pull should yield a chunk");
    assert_eq!(first.expect("first chunk should be delivered").as_bytes(), CHUNK_ONE);
    assert_eq!(
        call.next_chunk().await.expect("second pull should report the upstream break"),
        Err(StreamReadErrorV1::StreamReadFailed)
    );

    let polls_before = transport.probes.calls.load(Ordering::SeqCst);
    assert!(call.next_chunk().await.is_none());
    assert!(call.next_chunk().await.is_none());
    assert_eq!(
        transport.probes.calls.load(Ordering::SeqCst),
        polls_before,
        "a terminal stream must never poll its source again"
    );
}

#[tokio::test]
async fn dropping_the_call_drops_the_transport_source() {
    let resolver = ImmediateResolver::default();
    let transport = ScriptedTransport::new(vec![SourceEvent::Chunk(CHUNK_ONE)]);

    let call = open_with(&resolver, &transport, None, &CancellationToken::new(), SLOT_SENTINEL)
        .await
        .expect("a valid streaming open should succeed");

    assert!(!transport.probes.source_dropped.load(Ordering::SeqCst));
    drop(call);
    assert!(
        transport.probes.source_dropped.load(Ordering::SeqCst),
        "dropping the call must abort the transport exchange"
    );
}

#[tokio::test]
async fn streaming_debug_output_is_redacted() {
    let resolver = ImmediateResolver::default();
    let transport = ScriptedTransport::new(vec![SourceEvent::Chunk(CHUNK_ONE)]);
    let rejecting = RejectingTransport { calls: AtomicUsize::new(0) };

    let call = open_with(&resolver, &transport, None, &CancellationToken::new(), SLOT_SENTINEL)
        .await
        .expect("a valid streaming open should succeed");
    let rejection =
        open_with(&resolver, &rejecting, None, &CancellationToken::new(), SLOT_SENTINEL)
            .await
            .expect_err("the rejecting transport must fail the call");

    let rendered = format!("{call:?} {rejection:?} {rejection}");
    for sentinel in [
        ENDPOINT_SENTINEL,
        PATH_SENTINEL,
        SLOT_SENTINEL,
        HEADER_SENTINEL,
        BODY_SENTINEL,
        SECRET_SENTINEL,
        CONTENT_TYPE_SENTINEL,
        ERROR_BODY_SENTINEL,
    ] {
        assert!(!rendered.contains(sentinel), "debug output leaked sentinel: {rendered}");
    }
}
