use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use http::StatusCode;
use south_contracts::{
    BearerAuthV1, BufferedHttpResponseV1, CredentialSlotV1, JsonBodyV1, JsonPostRequestV1,
    MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES, MAX_RESPONSE_BODY_BYTES,
    MAX_RESPONSE_CONTENT_TYPE_BYTES, MAX_RESPONSE_RETRY_AFTER_BYTES, ProviderEndpointV1,
    RelativePathV1, SafeHeaders, TransportErrorV1,
};
use south_core::{
    CredentialResolutionFuture, CredentialResolver, ProviderBindingV1, ProviderCallErrorV1,
    SecretValue, execute_provider_call_v1,
};
use south_transport_reqwest::{ReqwestTransportConfigV1, ReqwestTransportV1};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

const SECRET_SENTINEL: &str = "transport-secret-sentinel";
const BODY_SENTINEL: &str = "transport-body-sentinel";
const HEADER_SENTINEL: &str = "transport-header-sentinel";

#[derive(Debug)]
struct ReceivedRequest {
    request_line: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

struct Loopback {
    endpoint: String,
    request: oneshot::Receiver<ReceivedRequest>,
    task: JoinHandle<()>,
}

async fn loopback_once(response: Vec<u8>) -> Loopback {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback listener should bind");
    let address = listener.local_addr().expect("loopback address should be available");
    let (request_tx, request_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("one loopback request should connect");
        let request = read_request(&mut stream).await;
        request_tx.send(request).expect("test should retain its request receiver");
        stream.write_all(&response).await.expect("fixture response should write");
        stream.shutdown().await.expect("fixture connection should close");
    });
    Loopback { endpoint: format!("http://{address}/base"), request: request_rx, task }
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

async fn receive_without_advancing_time(
    receiver: &mut oneshot::Receiver<ReceivedRequest>,
) -> ReceivedRequest {
    for _ in 0..100_000 {
        match receiver.try_recv() {
            Ok(request) => return request,
            Err(oneshot::error::TryRecvError::Empty) => tokio::task::yield_now().await,
            Err(oneshot::error::TryRecvError::Closed) => {
                panic!("server should report the synchronized request")
            }
        }
    }
    panic!("server did not report the synchronized request within the yield budget")
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

fn config() -> ReqwestTransportConfigV1 {
    ReqwestTransportConfigV1::try_new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(10),
    )
    .expect("fixture timeouts should be valid")
}

fn request() -> JsonPostRequestV1 {
    JsonPostRequestV1::new(
        RelativePathV1::parse("v1/call").expect("fixture path should be valid"),
        SafeHeaders::try_from_iter([
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

fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
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

async fn call(
    endpoint: &str,
    transport: &ReqwestTransportV1,
    deadline: tokio::time::Instant,
    cancellation: &CancellationToken,
) -> Result<BufferedHttpResponseV1, ProviderCallErrorV1> {
    let resolver = StaticResolver::default();
    let binding = ProviderBindingV1::new(
        ProviderEndpointV1::parse(endpoint).expect("loopback endpoint should be valid"),
        CredentialSlotV1::parse("primary").expect("fixture slot should be valid"),
    );
    execute_provider_call_v1(&binding, &request(), &resolver, transport, deadline, cancellation)
        .await
}

#[test]
fn config_accepts_explicit_sane_timeouts() {
    let config = ReqwestTransportConfigV1::try_new(
        Duration::from_secs(30),
        Duration::from_secs(5),
        Duration::from_secs(10),
    )
    .expect("explicit sane timeouts should be accepted");

    assert_eq!(config.total_timeout(), Duration::from_secs(30));
    assert_eq!(config.connect_timeout(), Duration::from_secs(5));
    assert_eq!(config.read_timeout(), Duration::from_secs(10));
}

#[test]
fn config_rejects_zero_or_unbounded_timeouts_without_context() {
    let invalid = [
        (Duration::ZERO, Duration::from_secs(1), Duration::from_secs(1)),
        (Duration::from_secs(1), Duration::ZERO, Duration::from_secs(1)),
        (Duration::from_secs(1), Duration::from_secs(1), Duration::ZERO),
        (Duration::from_secs(1), Duration::from_secs(2), Duration::from_secs(1)),
        (Duration::from_secs(1), Duration::from_secs(1), Duration::from_secs(2)),
        (Duration::from_secs(86_401), Duration::from_secs(1), Duration::from_secs(1)),
    ];

    for (total, connect, read) in invalid {
        let error = ReqwestTransportConfigV1::try_new(total, connect, read)
            .expect_err("invalid transport timeouts must be rejected");
        assert_eq!(error, TransportErrorV1::ClientBuildFailed);
        assert_eq!(error.code(), "CLIENT_BUILD_FAILED");
        assert!(!format!("{error:?} {error}").contains("86401"));
    }
}

#[test]
fn dedicated_transport_is_send_sync_and_debug_is_context_free() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ReqwestTransportV1>();

    let transport = ReqwestTransportV1::new(
        ReqwestTransportConfigV1::try_new(
            Duration::from_secs(30),
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .expect("fixture timeouts should be valid"),
    )
    .expect("a dedicated hardened client should build");

    assert_eq!(format!("{transport:?}"), "ReqwestTransportV1 { .. }");
}

#[tokio::test]
async fn sends_exact_post_and_preserves_created_response() {
    let loopback = loopback_once(response(
        "201 Created",
        &[("content-type", "application/json"), ("retry-after", "7")],
        br#"{"ok":true}"#,
    ))
    .await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");
    let resolver = StaticResolver::default();
    let binding = ProviderBindingV1::new(
        ProviderEndpointV1::parse(&loopback.endpoint).expect("loopback endpoint should be valid"),
        CredentialSlotV1::parse("primary").expect("fixture slot should be valid"),
    );

    let result = execute_provider_call_v1(
        &binding,
        &request(),
        &resolver,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("created response should succeed");
    let received = loopback.request.await.expect("server should report the request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(received.request_line, "POST /base/v1/call HTTP/1.1");
    assert_eq!(received.headers.get("x-test").map(String::as_str), Some(HEADER_SENTINEL));
    assert_eq!(
        received.headers.get("authorization").map(String::as_str),
        Some("Bearer transport-secret-sentinel")
    );
    assert_eq!(received.headers.get("content-type").map(String::as_str), Some("application/json"));
    assert_eq!(received.body, request().body().as_str().as_bytes());
    assert_eq!(resolver.calls.load(Ordering::SeqCst), 1);
    assert_eq!(result.status().as_u16(), 201);
    assert_eq!(result.body(), r#"{"ok":true}"#);
    assert_eq!(result.content_type(), Some("application/json"));
    assert_eq!(result.retry_after(), Some("7"));
}

#[tokio::test]
async fn client_error_and_server_error_are_normal_transport_outcomes() {
    for (status_line, status_code, body) in [
        ("429 Too Many Requests", 429, br#"{"error":"rate"}"#.as_slice()),
        ("500 Internal Server Error", 500, br#"{"error":"upstream"}"#.as_slice()),
    ] {
        let loopback = loopback_once(response(
            status_line,
            &[("content-type", "application/json"), ("retry-after", "11")],
            body,
        ))
        .await;
        let transport = ReqwestTransportV1::new(config()).expect("transport should build");

        let result = call(
            &loopback.endpoint,
            &transport,
            tokio::time::Instant::now() + Duration::from_secs(30),
            &CancellationToken::new(),
        )
        .await
        .expect("HTTP error status should remain a normal outcome");
        loopback.request.await.expect("server should receive one request");
        loopback.task.await.expect("server task should finish");

        assert_eq!(result.status().as_u16(), status_code);
        assert_eq!(result.body().as_bytes(), body);
        assert_eq!(result.content_type(), Some("application/json"));
        assert_eq!(result.retry_after(), Some("11"));
    }
}

#[tokio::test]
async fn redirect_is_denied_without_contacting_its_target() {
    let target_listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("redirect target should bind");
    target_listener.set_nonblocking(true).expect("redirect target should be nonblocking");
    let target = target_listener.local_addr().expect("redirect target address should exist");
    let location = format!("http://{target}/escaped");
    let loopback =
        loopback_once(response("302 Found", &[("location", location.as_str())], b"redirect")).await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let error = call(
        &loopback.endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("redirect must be denied");
    loopback.request.await.expect("origin should receive one request");
    loopback.task.await.expect("origin task should finish");

    assert_eq!(error.code(), "REDIRECT_DENIED");
    assert!(matches!(error, ProviderCallErrorV1::Transport(TransportErrorV1::RedirectDenied)));
    let accept_error = target_listener.accept().expect_err("redirect target must stay untouched");
    assert_eq!(accept_error.kind(), std::io::ErrorKind::WouldBlock);
}

async fn oversize_chunked_loopback() -> Loopback {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback listener should bind");
    let address = listener.local_addr().expect("loopback address should be available");
    let (request_tx, request_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("one loopback request should connect");
        let request = read_request(&mut stream).await;
        request_tx.send(request).expect("test should retain its request receiver");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
            )
            .await
            .expect("chunked response headers should write");
        let chunk = [b'x'; 8 * 1024];
        for _ in 0..(MAX_RESPONSE_BODY_BYTES / chunk.len()) {
            stream.write_all(b"2000\r\n").await.expect("chunk size should write");
            stream.write_all(&chunk).await.expect("chunk should write");
            stream.write_all(b"\r\n").await.expect("chunk delimiter should write");
        }
        stream.write_all(b"1\r\nx\r\n0\r\n\r\n").await.expect("limit byte should write");
    });
    Loopback { endpoint: format!("http://{address}/base"), request: request_rx, task }
}

#[tokio::test]
async fn streamed_chunked_response_stops_at_the_body_limit_plus_one() {
    let loopback = oversize_chunked_loopback().await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let error = call(
        &loopback.endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("oversized response should fail without truncation");
    loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(error.code(), "RESPONSE_BODY_TOO_LARGE");
}

#[tokio::test]
async fn invalid_utf8_response_has_a_stable_context_free_error() {
    let loopback = loopback_once(response("200 OK", &[], &[0xff, 0xfe])).await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let error = call(
        &loopback.endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("non-UTF-8 response should fail");
    loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(error.code(), "RESPONSE_BODY_NOT_UTF8");
    let rendered = format!("{error:?} {error}");
    for sentinel in [SECRET_SENTINEL, BODY_SENTINEL, HEADER_SENTINEL] {
        assert!(!rendered.contains(sentinel));
    }
}

#[tokio::test]
async fn captures_exactly_the_nine_approved_quota_metadata_fields() {
    let loopback = loopback_once(response(
        "200 OK",
        &[
            ("X-RateLimit-Limit-Tokens", "1000"),
            ("x-ratelimit-remaining-tokens", "900"),
            ("x-ratelimit-reset-tokens", "10s"),
            ("anthropic-ratelimit-tokens-limit", "2000"),
            ("anthropic-ratelimit-tokens-remaining", "1500"),
            ("anthropic-ratelimit-tokens-reset", "20s"),
            ("anthropic-ratelimit-unified-limit", "3000"),
            ("anthropic-ratelimit-unified-remaining", "2500"),
            ("anthropic-ratelimit-unified-reset", "30s"),
            ("x-unapproved-quota-sentinel", "must-not-be-retained"),
        ],
        b"safe",
    ))
    .await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let response = call(
        &loopback.endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("valid quota metadata must preserve the response");
    loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    let quota = response.provider_quota_metadata();
    assert_eq!(quota.present_field_count(), 9);
    assert_eq!(quota.x_ratelimit_limit_tokens(), Some("1000"));
    assert_eq!(quota.x_ratelimit_remaining_tokens(), Some("900"));
    assert_eq!(quota.x_ratelimit_reset_tokens(), Some("10s"));
    assert_eq!(quota.anthropic_ratelimit_tokens_limit(), Some("2000"));
    assert_eq!(quota.anthropic_ratelimit_tokens_remaining(), Some("1500"));
    assert_eq!(quota.anthropic_ratelimit_tokens_reset(), Some("20s"));
    assert_eq!(quota.anthropic_ratelimit_unified_limit(), Some("3000"));
    assert_eq!(quota.anthropic_ratelimit_unified_remaining(), Some("2500"));
    assert_eq!(quota.anthropic_ratelimit_unified_reset(), Some("30s"));
    assert!(!format!("{response:?}").contains("must-not-be-retained"));
}

#[tokio::test]
async fn malformed_optional_quota_metadata_is_omitted_without_failing_the_response() {
    let oversized = "x".repeat(MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES + 1);
    let mut non_utf8 = b"HTTP/1.1 200 OK\r\nx-ratelimit-reset-tokens: ".to_vec();
    non_utf8.push(0xff);
    non_utf8.extend_from_slice(
        b"\r\nanthropic-ratelimit-unified-limit: 77\r\ncontent-length: 4\r\nconnection: close\r\n\r\nsafe",
    );
    let fixtures = [
        response(
            "200 OK",
            &[
                ("x-ratelimit-limit-tokens", "1"),
                ("x-ratelimit-limit-tokens", "1"),
                ("anthropic-ratelimit-unified-limit", "77"),
            ],
            b"safe",
        ),
        response(
            "200 OK",
            &[
                ("x-ratelimit-remaining-tokens", &oversized),
                ("anthropic-ratelimit-unified-limit", "77"),
            ],
            b"safe",
        ),
        non_utf8,
    ];

    for raw in fixtures {
        let loopback = loopback_once(raw).await;
        let transport = ReqwestTransportV1::new(config()).expect("transport should build");
        let response = call(
            &loopback.endpoint,
            &transport,
            tokio::time::Instant::now() + Duration::from_secs(30),
            &CancellationToken::new(),
        )
        .await
        .expect("malformed optional quota metadata must not fail a valid response");
        loopback.request.await.expect("server should receive one request");
        loopback.task.await.expect("server task should finish");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), "safe");
        let quota = response.provider_quota_metadata();
        assert_eq!(quota.present_field_count(), 1);
        assert_eq!(quota.x_ratelimit_limit_tokens(), None);
        assert_eq!(quota.x_ratelimit_remaining_tokens(), None);
        assert_eq!(quota.x_ratelimit_reset_tokens(), None);
        assert_eq!(quota.anthropic_ratelimit_unified_limit(), Some("77"));
    }
}

#[tokio::test]
async fn oversized_allowed_metadata_is_rejected_before_body_buffering() {
    for (name, limit) in [
        ("content-type", MAX_RESPONSE_CONTENT_TYPE_BYTES),
        ("retry-after", MAX_RESPONSE_RETRY_AFTER_BYTES),
    ] {
        let value = "x".repeat(limit + 1);
        let loopback = loopback_once(response("200 OK", &[(name, &value)], b"safe")).await;
        let transport = ReqwestTransportV1::new(config()).expect("transport should build");

        let error = call(
            &loopback.endpoint,
            &transport,
            tokio::time::Instant::now() + Duration::from_secs(30),
            &CancellationToken::new(),
        )
        .await
        .expect_err("oversized response metadata should fail");
        loopback.request.await.expect("server should receive one request");
        loopback.task.await.expect("server task should finish");
        assert_eq!(error.code(), "RESPONSE_METADATA_INVALID");
    }
}

#[tokio::test]
async fn non_utf8_allowed_metadata_is_rejected() {
    let mut raw = b"HTTP/1.1 200 OK\r\ncontent-type: text/".to_vec();
    raw.push(0xff);
    raw.extend_from_slice(b"\r\ncontent-length: 4\r\nconnection: close\r\n\r\nsafe");
    let loopback = loopback_once(raw).await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let error = call(
        &loopback.endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("non-UTF-8 response metadata should fail");
    loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(error.code(), "RESPONSE_METADATA_INVALID");
}

#[tokio::test]
async fn duplicate_single_value_metadata_is_rejected() {
    for duplicate_headers in [
        "content-type: application/json\r\ncontent-type: text/plain",
        "retry-after: 1\r\nretry-after: 2",
    ] {
        let raw = format!(
            "HTTP/1.1 200 OK\r\n{duplicate_headers}\r\ncontent-length: 4\r\nconnection: close\r\n\r\nsafe"
        )
        .into_bytes();
        let loopback = loopback_once(raw).await;
        let transport = ReqwestTransportV1::new(config()).expect("transport should build");

        let error = call(
            &loopback.endpoint,
            &transport,
            tokio::time::Instant::now() + Duration::from_secs(30),
            &CancellationToken::new(),
        )
        .await
        .expect_err("single-value response metadata should reject duplicates");
        loopback.request.await.expect("server should receive one request");
        loopback.task.await.expect("server task should finish");

        assert_eq!(error.code(), "RESPONSE_METADATA_INVALID");
    }
}

#[tokio::test]
async fn declared_response_body_above_the_limit_fails_before_streaming() {
    let raw = format!(
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        MAX_RESPONSE_BODY_BYTES + 1
    )
    .into_bytes();
    let loopback = loopback_once(raw).await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let error = call(
        &loopback.endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("an oversized declared body should fail before streaming");
    loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(error.code(), "RESPONSE_BODY_TOO_LARGE");
}

#[tokio::test(start_paused = true)]
async fn transport_owned_timeout_precedes_the_later_caller_deadline_and_drops_io() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback listener should bind");
    let address = listener.local_addr().expect("loopback address should exist");
    let (request_tx, mut request_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("one request should connect");
        let request = read_request(&mut stream).await;
        request_tx.send(request).expect("test should retain request receiver");
        let mut remaining = Vec::new();
        stream.read_to_end(&mut remaining).await.expect("timed-out request should close");
        remaining
    });
    let endpoint = format!("http://{address}/base");
    let transport = ReqwestTransportV1::new(
        ReqwestTransportConfigV1::try_new(
            Duration::from_secs(1),
            Duration::from_millis(500),
            Duration::from_millis(500),
        )
        .expect("fixture timeout should be valid"),
    )
    .expect("transport should build");
    let cancellation = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

    let provider_call = call(&endpoint, &transport, deadline, &cancellation);
    let advance_after_request = async {
        let request = receive_without_advancing_time(&mut request_rx).await;
        tokio::time::advance(Duration::from_millis(600)).await;
        request
    };
    let (result, observed) = tokio::join!(provider_call, advance_after_request);
    let remaining = server.await.expect("server task should finish after future drop");

    let error = result.expect_err("transport timeout should fail");
    assert_eq!(observed.request_line, "POST /base/v1/call HTTP/1.1");
    assert_eq!(error.code(), "TRANSPORT_TIMEOUT");
    assert!(remaining.is_empty(), "dropped request must not leave detached writes");
}

#[tokio::test]
async fn refused_loopback_connection_has_a_stable_connect_error() {
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    // TCP port zero cannot be occupied by a listener, so this has no released-port race.
    let error = call(
        "http://127.0.0.1:0/base",
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("refused loopback connection should fail");

    assert_eq!(error.code(), "CONNECT_FAILED");
    assert_eq!(format!("{error:?} {error}"), "Transport(ConnectFailed) HTTP connection failed");
}

#[tokio::test]
async fn truncated_response_body_has_a_stable_read_error() {
    let loopback = loopback_once(
        b"HTTP/1.1 200 OK\r\ncontent-length: 10\r\nconnection: close\r\n\r\nshort".to_vec(),
    )
    .await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let error = call(
        &loopback.endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("truncated response body should fail");
    loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(error.code(), "RESPONSE_READ_FAILED");
}

#[tokio::test]
async fn malformed_response_source_is_not_retained_in_request_error() {
    const RAW_SOURCE_SENTINEL: &str = "raw-reqwest-source-sentinel";
    let loopback =
        loopback_once(format!("NOT-HTTP {RAW_SOURCE_SENTINEL}\r\n\r\n").into_bytes()).await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let error = call(
        &loopback.endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("malformed response should fail the request");
    loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(error.code(), "REQUEST_FAILED");
    assert!(!format!("{error:?} {error}").contains(RAW_SOURCE_SENTINEL));
}

#[tokio::test]
async fn compression_is_not_advertised_or_implicitly_decoded() {
    const GZIP_BODY: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x4b, 0x49, 0x4d, 0xce, 0x4f,
        0x49, 0x4d, 0xd1, 0x2d, 0x49, 0xad, 0x28, 0x01, 0x00, 0x45, 0x49, 0xdc, 0xf5, 0x0c, 0x00,
        0x00, 0x00,
    ];
    let loopback =
        loopback_once(response("200 OK", &[("content-encoding", "gzip")], GZIP_BODY)).await;
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let error = call(
        &loopback.endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect_err("raw compressed bytes should not be decoded into UTF-8");
    let received = loopback.request.await.expect("server should receive one request");
    loopback.task.await.expect("server task should finish");

    assert_eq!(error.code(), "RESPONSE_BODY_NOT_UTF8");
    assert!(!received.headers.contains_key("accept-encoding"));
}

#[tokio::test]
async fn disabled_retry_policy_sends_only_one_request_after_protocol_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback listener should bind");
    let address = listener.local_addr().expect("loopback address should exist");
    let (call_done_tx, call_done_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.expect("first request should connect");
        read_request(&mut first).await;
        drop(first);
        tokio::select! {
            second = listener.accept() => {
                second.expect("second accept should succeed");
                2_usize
            }
            result = call_done_rx => {
                result.expect("test should signal call completion");
                1_usize
            }
        }
    });
    let endpoint = format!("http://{address}/base");
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");

    let result = call(
        &endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await;
    call_done_tx.send(()).expect("server should await completion signal");
    let connection_count = server.await.expect("server task should finish");

    assert!(result.is_err(), "closed connection should fail");
    assert_eq!(connection_count, 1, "transport must not retry the request");
}

#[tokio::test]
async fn cancellation_drops_real_reqwest_io_without_detaching_work() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback listener should bind");
    let address = listener.local_addr().expect("loopback address should exist");
    let (request_tx, request_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("one request should connect");
        let request = read_request(&mut stream).await;
        request_tx.send(request).expect("test should retain request receiver");
        let mut remaining = Vec::new();
        stream.read_to_end(&mut remaining).await.expect("cancelled request should close");
        remaining
    });
    let endpoint = format!("http://{address}/base");
    let transport = ReqwestTransportV1::new(config()).expect("transport should build");
    let cancellation = CancellationToken::new();

    let provider_call = call(
        &endpoint,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &cancellation,
    );
    let cancel_after_request = async {
        request_rx.await.expect("server should observe one request");
        cancellation.cancel();
    };
    let (result, ()) = tokio::join!(provider_call, cancel_after_request);
    let remaining = server.await.expect("server task should finish after cancellation");

    assert_eq!(result.expect_err("cancelled call should fail").code(), "CANCELLED");
    assert!(remaining.is_empty(), "cancelled request must not leave detached writes");
}
