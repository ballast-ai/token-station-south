use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use south_contracts::{
    BearerAuthV1, CredentialSlotV1, JsonBodyV1, JsonPostRequestV1, MAX_STREAM_CHUNK_BYTES,
    MAX_STREAM_ERROR_BODY_BYTES, ProviderEndpointV1, RelativePathV1, SafeHeaders,
    StreamReadErrorV1, StreamTransportConfigV1, TransportErrorV1,
};
use south_core::{
    CredentialResolutionFuture, CredentialResolver, ProviderBindingV1, ProviderCallErrorV1,
    SecretValue, StreamingCallV1, open_streaming_provider_call_v1,
};
use south_transport_reqwest::ReqwestStreamingTransportV1;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const SECRET_SENTINEL: &str = "stream-transport-secret-sentinel";
const BODY_SENTINEL: &str = "stream-transport-body-sentinel";
const HEADER_SENTINEL: &str = "stream-transport-header-sentinel";
const CHUNK_ONE: &[u8] = b"data: stream-chunk-one\n\n";
const CHUNK_TWO: &[u8] = b"data: stream-chunk-two\n\n";

#[derive(Debug)]
struct ReceivedRequest {
    request_line: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> ReceivedRequest {
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk).await.expect("fixture request should read");
        assert_ne!(count, 0, "request headers should be complete");
        bytes.extend_from_slice(&chunk[..count]);
        assert!(bytes.len() <= 128 * 1024, "fixture request headers must stay bounded");
    };

    let header_text =
        std::str::from_utf8(&bytes[..header_end]).expect("fixture request headers should be UTF-8");
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().expect("request line should exist").to_owned();
    let headers = lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line.split_once(':').expect("fixture header should contain colon");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect::<BTreeMap<_, _>>();
    let body_length = headers
        .get("content-length")
        .expect("buffered request should have content-length")
        .parse::<usize>()
        .expect("fixture content-length should be numeric");
    while bytes.len() - header_end < body_length {
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk).await.expect("fixture body should read");
        assert_ne!(count, 0, "request body should be complete");
        bytes.extend_from_slice(&chunk[..count]);
    }

    ReceivedRequest {
        request_line,
        headers,
        body: bytes[header_end..header_end + body_length].to_vec(),
    }
}

async fn receive_without_advancing_time<T>(receiver: &mut oneshot::Receiver<T>) -> T {
    for _ in 0..100_000 {
        match receiver.try_recv() {
            Ok(value) => return value,
            Err(oneshot::error::TryRecvError::Empty) => tokio::task::yield_now().await,
            Err(oneshot::error::TryRecvError::Closed) => {
                panic!("server should report the synchronized event")
            }
        }
    }
    panic!("server did not report the synchronized event within the yield budget")
}

fn chunked_headers(extra_headers: &[(&str, &str)]) -> Vec<u8> {
    let mut response =
        "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n".to_owned();
    for (name, value) in extra_headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    response.into_bytes()
}

fn encoded_chunk(bytes: &[u8]) -> Vec<u8> {
    let mut encoded = format!("{:x}\r\n", bytes.len()).into_bytes();
    encoded.extend_from_slice(bytes);
    encoded.extend_from_slice(b"\r\n");
    encoded
}

fn buffered_response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status}\r\nconnection: close\r\n");
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    write!(response, "content-length: {}\r\n\r\n", body.len())
        .expect("writing to a String should not fail");
    let mut response = response.into_bytes();
    response.extend_from_slice(body);
    response
}

#[derive(Default)]
struct StaticResolver {
    calls: AtomicUsize,
}

impl CredentialResolver for StaticResolver {
    fn resolve<'a>(&'a self, _slot: &'a CredentialSlotV1) -> CredentialResolutionFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(SecretValue::new(SECRET_SENTINEL.to_owned())) })
    }
}

fn stream_config() -> StreamTransportConfigV1 {
    StreamTransportConfigV1::try_new(None, Duration::from_secs(5), Duration::from_secs(10))
        .expect("fixture stream timeouts should be valid")
}

fn request() -> JsonPostRequestV1 {
    JsonPostRequestV1::new(
        RelativePathV1::parse("v1/stream").expect("fixture path should be valid"),
        SafeHeaders::try_from_iter([
            ("accept", "text/event-stream"),
            ("content-type", "application/json"),
            ("x-test", HEADER_SENTINEL),
        ])
        .expect("fixture headers should be valid"),
        JsonBodyV1::parse(&format!(r#"{{"value":"{BODY_SENTINEL}"}}"#))
            .expect("fixture body should be valid"),
        BearerAuthV1::new(
            CredentialSlotV1::parse("primary").expect("fixture slot should be valid"),
        ),
    )
}

async fn open(
    endpoint: &str,
    transport: &ReqwestStreamingTransportV1,
    deadline: Option<tokio::time::Instant>,
    cancellation: &CancellationToken,
) -> Result<StreamingCallV1, ProviderCallErrorV1> {
    let resolver = StaticResolver::default();
    let binding = ProviderBindingV1::new(
        ProviderEndpointV1::parse(endpoint).expect("loopback endpoint should be valid"),
        CredentialSlotV1::parse("primary").expect("fixture slot should be valid"),
    );
    open_streaming_provider_call_v1(
        &binding,
        &request(),
        &resolver,
        transport,
        deadline,
        cancellation,
    )
    .await
}

struct Loopback {
    endpoint: String,
    request: oneshot::Receiver<ReceivedRequest>,
    task: JoinHandle<Vec<u8>>,
}

/// Serves one connection: reports the request, writes `first`, waits for `release` when present,
/// writes `second`, then drains the socket until the peer closes and returns the drained bytes.
fn scripted_loopback(
    first: Vec<u8>,
    release: Option<oneshot::Receiver<()>>,
    second: Vec<u8>,
    drain_after_writes: bool,
) -> Loopback {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback should bind");
    listener.set_nonblocking(true).expect("loopback listener should be nonblocking");
    let address = listener.local_addr().expect("loopback address should be available");
    let (request_tx, request_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let listener =
            TcpListener::from_std(listener).expect("loopback listener should convert to tokio");
        let (mut stream, _) = listener.accept().await.expect("one loopback request should connect");
        let received = read_request(&mut stream).await;
        request_tx.send(received).expect("test should retain its request receiver");
        stream.write_all(&first).await.expect("first fixture bytes should write");
        if let Some(release) = release {
            release.await.expect("test should release the fixture server");
        }
        stream.write_all(&second).await.expect("second fixture bytes should write");
        if drain_after_writes {
            let mut remaining = Vec::new();
            stream.read_to_end(&mut remaining).await.expect("fixture connection should close");
            remaining
        } else {
            stream.shutdown().await.expect("fixture connection should close");
            Vec::new()
        }
    });
    Loopback { endpoint: format!("http://{address}/base"), request: request_rx, task }
}

#[tokio::test]
async fn opens_exact_post_and_streams_chunks_to_clean_eof() {
    let (release_tx, release_rx) = oneshot::channel();
    let mut first = chunked_headers(&[("content-type", "text/event-stream"), ("retry-after", "7")]);
    first.extend_from_slice(&encoded_chunk(CHUNK_ONE));
    let mut second = encoded_chunk(CHUNK_TWO);
    second.extend_from_slice(b"0\r\n\r\n");
    let loopback = scripted_loopback(first, Some(release_rx), second, false);
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");

    let mut call = open(&loopback.endpoint, &transport, None, &CancellationToken::new())
        .await
        .expect("a 2xx upstream should open a stream");
    let received = loopback.request.await.expect("server should report the request");

    assert_eq!(received.request_line, "POST /base/v1/stream HTTP/1.1");
    assert_eq!(received.headers.get("accept").map(String::as_str), Some("text/event-stream"));
    assert_eq!(received.headers.get("x-test").map(String::as_str), Some(HEADER_SENTINEL));
    assert_eq!(
        received.headers.get("authorization").map(String::as_str),
        Some("Bearer stream-transport-secret-sentinel")
    );
    assert!(!received.headers.contains_key("accept-encoding"));
    assert_eq!(received.body, request().body().as_str().as_bytes());
    assert_eq!(call.head().status().as_u16(), 200);
    assert_eq!(call.head().content_type(), Some("text/event-stream"));
    assert_eq!(call.head().retry_after(), Some("7"));

    let mut delivered = Vec::new();
    let first_chunk = call
        .next_chunk()
        .await
        .expect("the first pull should yield bytes")
        .expect("the first pull should not fail");
    delivered.extend_from_slice(first_chunk.as_bytes());
    release_tx.send(()).expect("server should await its release signal");
    while let Some(result) = call.next_chunk().await {
        let chunk = result.expect("streamed pulls should not fail");
        delivered.extend_from_slice(chunk.as_bytes());
    }
    assert!(call.next_chunk().await.is_none(), "pulls after EOF must keep yielding None");

    let mut expected = CHUNK_ONE.to_vec();
    expected.extend_from_slice(CHUNK_TWO);
    assert_eq!(delivered, expected);
    loopback.task.await.expect("server task should finish");
}

#[tokio::test]
async fn dropping_an_in_flight_pull_future_loses_no_bytes() {
    let (release_tx, release_rx) = oneshot::channel();
    let mut first = chunked_headers(&[]);
    first.extend_from_slice(&encoded_chunk(CHUNK_ONE));
    let mut second = encoded_chunk(CHUNK_TWO);
    second.extend_from_slice(b"0\r\n\r\n");
    let loopback = scripted_loopback(first, Some(release_rx), second, false);
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");

    let mut call = open(&loopback.endpoint, &transport, None, &CancellationToken::new())
        .await
        .expect("a 2xx upstream should open a stream");
    let mut delivered = Vec::new();
    while delivered.len() < CHUNK_ONE.len() {
        let chunk = call
            .next_chunk()
            .await
            .expect("the first scripted chunk should yield bytes")
            .expect("the first scripted chunk should not fail");
        delivered.extend_from_slice(chunk.as_bytes());
    }
    assert_eq!(delivered, CHUNK_ONE);

    // A host heartbeat leg can win the select and drop an in-flight pull. Poll the pull once so
    // real read interest exists, drop it, and prove the resumed stream loses nothing.
    {
        let pull = call.next_chunk();
        tokio::pin!(pull);
        tokio::select! {
            biased;
            outcome = &mut pull => {
                panic!("no bytes should be in flight before the release: {outcome:?}")
            }
            () = std::future::ready(()) => {}
        }
    }

    release_tx.send(()).expect("server should await its release signal");
    let mut resumed = Vec::new();
    while let Some(result) = call.next_chunk().await {
        let chunk = result.expect("resumed pulls should not fail");
        resumed.extend_from_slice(chunk.as_bytes());
    }

    assert_eq!(resumed, CHUNK_TWO, "a dropped in-flight pull must not lose or repeat bytes");
    assert!(call.next_chunk().await.is_none(), "pulls after EOF must keep yielding None");
    loopback.task.await.expect("server task should finish");
}

#[tokio::test]
async fn oversized_network_reads_are_rechunked_to_the_delivery_bound() {
    let body = vec![b'x'; MAX_STREAM_CHUNK_BYTES + 40_000];
    let loopback = scripted_loopback(
        buffered_response("200 OK", &[("content-type", "application/octet-stream")], &body),
        None,
        Vec::new(),
        false,
    );
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");

    let mut call = open(&loopback.endpoint, &transport, None, &CancellationToken::new())
        .await
        .expect("a 2xx upstream should open a stream");
    loopback.request.await.expect("server should receive one request");

    let mut delivered = Vec::new();
    let mut chunk_count = 0_usize;
    while let Some(result) = call.next_chunk().await {
        let chunk = result.expect("streamed pulls should not fail");
        assert!(chunk.len() <= MAX_STREAM_CHUNK_BYTES, "chunks must respect the delivery bound");
        chunk_count += 1;
        delivered.extend_from_slice(chunk.as_bytes());
    }

    assert!(chunk_count >= 2, "an oversized upstream read must be split");
    assert_eq!(delivered, body);
    loopback.task.await.expect("server task should finish");
}

#[tokio::test(start_paused = true)]
async fn slow_drip_upstream_hits_the_idle_guard_mid_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback listener should bind");
    let address = listener.local_addr().expect("loopback address should exist");
    let (written_tx, mut written_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("one request should connect");
        read_request(&mut stream).await;
        let mut first = chunked_headers(&[]);
        first.extend_from_slice(&encoded_chunk(CHUNK_ONE));
        stream.write_all(&first).await.expect("headers and first chunk should write");
        written_tx.send(()).expect("test should retain its write receiver");
        let mut remaining = Vec::new();
        stream.read_to_end(&mut remaining).await.expect("stalled connection should close");
        remaining
    });
    let endpoint = format!("http://{address}/base");
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");
    let cancellation = CancellationToken::new();

    // The yield-loop keeps the paused runtime busy so no timer auto-advances before the
    // upstream has written its headers and first chunk.
    let opening = open(&endpoint, &transport, None, &cancellation);
    let sync = receive_without_advancing_time(&mut written_rx);
    let (opened, ()) = tokio::join!(opening, sync);
    let mut call = opened.expect("a 2xx upstream should open a stream");
    let first_chunk = call
        .next_chunk()
        .await
        .expect("the first pull should yield bytes")
        .expect("the first pull should not fail");
    assert_eq!(first_chunk.as_bytes(), CHUNK_ONE);

    let pull = call.next_chunk();
    let advance = async {
        tokio::time::advance(Duration::from_secs(11)).await;
    };
    let (outcome, ()) = tokio::join!(pull, advance);

    assert_eq!(
        outcome.expect("a stalled upstream must fail the pull"),
        Err(StreamReadErrorV1::StreamIdleTimeout)
    );
    assert!(call.next_chunk().await.is_none(), "pulls after a terminal error must yield None");
    drop(call);
    let remaining = server.await.expect("server task should finish after the drop");
    assert!(remaining.is_empty(), "an aborted stream must not leave detached writes");
}

#[tokio::test(start_paused = true)]
async fn header_stall_is_a_pre_stream_transport_timeout() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback listener should bind");
    let address = listener.local_addr().expect("loopback address should exist");
    let (request_tx, mut request_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("one request should connect");
        let received = read_request(&mut stream).await;
        request_tx.send(received).expect("test should retain request receiver");
        let mut remaining = Vec::new();
        stream.read_to_end(&mut remaining).await.expect("timed-out request should close");
    });
    let endpoint = format!("http://{address}/base");
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");
    let cancellation = CancellationToken::new();

    let opening = open(&endpoint, &transport, None, &cancellation);
    let advance_after_request = async {
        receive_without_advancing_time(&mut request_rx).await;
        tokio::time::advance(Duration::from_secs(11)).await;
    };
    let (result, ()) = tokio::join!(opening, advance_after_request);
    server.await.expect("server task should finish");

    let error = result.expect_err("a silent header phase must time out before any stream exists");
    assert_eq!(error.code(), "TRANSPORT_TIMEOUT");
    assert!(matches!(error, ProviderCallErrorV1::Transport(TransportErrorV1::TransportTimeout)));
}

#[tokio::test]
async fn mid_stream_disconnect_is_a_stable_read_failure() {
    let mut first = chunked_headers(&[]);
    first.extend_from_slice(&encoded_chunk(CHUNK_ONE));
    let loopback = scripted_loopback(first, None, Vec::new(), false);
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");

    let mut call = open(&loopback.endpoint, &transport, None, &CancellationToken::new())
        .await
        .expect("a 2xx upstream should open a stream");
    let first_chunk = call
        .next_chunk()
        .await
        .expect("the first pull should yield bytes")
        .expect("the first pull should not fail");
    assert_eq!(first_chunk.as_bytes(), CHUNK_ONE);
    loopback.task.await.expect("server task should finish");

    assert_eq!(
        call.next_chunk().await.expect("a broken upstream must fail the pull"),
        Err(StreamReadErrorV1::StreamReadFailed)
    );
    assert!(call.next_chunk().await.is_none(), "pulls after a terminal error must yield None");
}

#[tokio::test]
async fn non_2xx_collapses_into_a_bounded_rejection_without_a_stream() {
    let loopback = scripted_loopback(
        buffered_response(
            "429 Too Many Requests",
            &[("content-type", "application/json"), ("retry-after", "11")],
            b"denied-sentinel",
        ),
        None,
        Vec::new(),
        false,
    );
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");

    let error = open(&loopback.endpoint, &transport, None, &CancellationToken::new())
        .await
        .expect_err("a non-2xx upstream must not open a stream");
    loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(error.code(), "UPSTREAM_REJECTED");
    let ProviderCallErrorV1::Rejected(rejected) = error else {
        panic!("a non-2xx upstream must use the Rejected variant");
    };
    assert_eq!(rejected.head().status().as_u16(), 429);
    assert_eq!(rejected.head().content_type(), Some("application/json"));
    assert_eq!(rejected.head().retry_after(), Some("11"));
    assert_eq!(rejected.body(), b"denied-sentinel");
}

#[tokio::test]
async fn oversized_rejection_body_is_truncated_at_the_bound() {
    let body = vec![b'x'; MAX_STREAM_ERROR_BODY_BYTES + 4096];
    let loopback = scripted_loopback(
        buffered_response("502 Bad Gateway", &[], &body),
        None,
        Vec::new(),
        false,
    );
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");

    let error = open(&loopback.endpoint, &transport, None, &CancellationToken::new())
        .await
        .expect_err("a non-2xx upstream must not open a stream");
    loopback.request.await.expect("server should receive one request");

    let ProviderCallErrorV1::Rejected(rejected) = error else {
        panic!("a non-2xx upstream must use the Rejected variant");
    };
    assert_eq!(rejected.body().len(), MAX_STREAM_ERROR_BODY_BYTES);
    assert!(rejected.body().iter().all(|byte| *byte == b'x'));
    loopback.task.abort();
}

#[tokio::test]
async fn redirect_is_denied_before_any_stream_exists() {
    let loopback = scripted_loopback(
        buffered_response("302 Found", &[("location", "http://127.0.0.1:9/escaped")], b"redirect"),
        None,
        Vec::new(),
        false,
    );
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");

    let error = open(&loopback.endpoint, &transport, None, &CancellationToken::new())
        .await
        .expect_err("a redirect must be denied");
    loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(error.code(), "REDIRECT_DENIED");
    assert!(matches!(error, ProviderCallErrorV1::Transport(TransportErrorV1::RedirectDenied)));
}

#[tokio::test]
async fn cancellation_mid_pull_aborts_the_stream_without_detached_writes() {
    let mut first = chunked_headers(&[]);
    first.extend_from_slice(&encoded_chunk(CHUNK_ONE));
    let loopback = scripted_loopback(first, None, Vec::new(), true);
    let transport =
        ReqwestStreamingTransportV1::new(stream_config()).expect("transport should build");
    let cancellation = CancellationToken::new();

    let mut call = open(&loopback.endpoint, &transport, None, &cancellation)
        .await
        .expect("a 2xx upstream should open a stream");
    let first_chunk = call
        .next_chunk()
        .await
        .expect("the first pull should yield bytes")
        .expect("the first pull should not fail");
    assert_eq!(first_chunk.as_bytes(), CHUNK_ONE);

    let pull = call.next_chunk();
    let cancel = async {
        cancellation.cancel();
    };
    let (outcome, ()) = tokio::join!(pull, cancel);

    assert_eq!(
        outcome.expect("a cancelled pull must fail"),
        Err(StreamReadErrorV1::StreamCancelled)
    );
    assert!(call.next_chunk().await.is_none(), "pulls after a terminal error must yield None");
    drop(call);
    let remaining = loopback.task.await.expect("server task should finish after the drop");
    assert!(remaining.is_empty(), "a cancelled stream must not leave detached writes");
}
