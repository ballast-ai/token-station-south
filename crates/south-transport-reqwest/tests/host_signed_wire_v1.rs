//! The transport's byte promise for the host-signed arm (design record §3.3, D5).
//!
//! A signature is only as good as the transport's restraint: the signer commits to a request, and
//! anything the transport adds, renames, re-encodes, or reorders afterwards invalidates it. The
//! promise is checked here on a real socket, because it is a claim about bytes on the wire and
//! only the wire can settle it.

use std::{collections::BTreeMap, time::Duration};

use south_contracts::{
    BearerAuthV1, ControlledUserAgentV1, CredentialSlotV1, JsonBodyV1, JsonPostRequestV1,
    ProviderAuthV1, ProviderEndpointV1, QueryParameterV1, QueryStringV1, RelativePathV1,
    SafeHeaders, SignedHeaderSetV1, SignedHeaderV1,
};
use south_core::{ProviderBindingV1, execute_signed_provider_call_v1};
use south_testkit::{DeterministicRequestFinalizerV1, expected_signature_v1};
use south_transport_reqwest::{
    ReqwestTransportConfigV1, ReqwestTransportV1, TRANSPORT_ADDED_HEADERS_V1,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
use tokio_util::sync::CancellationToken;

const SLOT: &str = "aws.bedrock.primary";
const AGENT: &str = "south-wire/1.0";
const BODY: &str = r#"{"value":"wire-body-sentinel"}"#;

#[derive(Debug)]
struct Received {
    request_line: String,
    headers: BTreeMap<String, String>,
    header_names: Vec<String>,
    body: Vec<u8>,
}

async fn read_request(stream: &mut TcpStream) -> Received {
    let mut bytes = Vec::new();
    let header_end = loop {
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk).await.expect("fixture request should read");
        assert_ne!(count, 0, "request headers should be complete");
        bytes.extend_from_slice(&chunk[..count]);
    };
    let text = std::str::from_utf8(&bytes[..header_end]).expect("headers should be UTF-8");
    let mut lines = text.split("\r\n");
    let request_line = lines.next().expect("request line").to_owned();
    let pairs: Vec<(String, String)> = lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line.split_once(':').expect("header should contain colon");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let header_names = pairs.iter().map(|(name, _)| name.clone()).collect();
    let headers: BTreeMap<String, String> = pairs.into_iter().collect();
    let length: usize = headers
        .get("content-length")
        .expect("buffered request should carry content-length")
        .parse()
        .expect("content-length should be numeric");
    while bytes.len() - header_end < length {
        let mut chunk = [0_u8; 1024];
        let count = stream.read(&mut chunk).await.expect("fixture body should read");
        assert_ne!(count, 0, "request body should be complete");
        bytes.extend_from_slice(&chunk[..count]);
    }
    Received {
        request_line,
        headers,
        header_names,
        body: bytes[header_end..header_end + length].to_vec(),
    }
}

fn declared() -> SignedHeaderSetV1 {
    SignedHeaderSetV1::new(&[
        SignedHeaderV1::Authorization,
        SignedHeaderV1::XAmzDate,
        SignedHeaderV1::XAmzContentSha256,
        SignedHeaderV1::XAmzSecurityToken,
    ])
    .expect("fixture declaration")
}

#[tokio::test]
async fn the_transport_adds_exactly_its_declared_header_set_and_nothing_else() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback should bind");
    let address = listener.local_addr().expect("loopback address");
    let (tx, rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("one request should connect");
        let received = read_request(&mut stream).await;
        tx.send(received).expect("test retains its receiver");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}")
            .await
            .expect("fixture response writes");
        stream.shutdown().await.expect("fixture connection closes");
    });

    let endpoint = format!("http://{address}/base");
    let binding = ProviderBindingV1::new(
        ProviderEndpointV1::parse(&endpoint).expect("fixture endpoint"),
        CredentialSlotV1::parse(SLOT).expect("fixture slot"),
    );
    let request = JsonPostRequestV1::new(
        RelativePathV1::parse("model/invoke").expect("fixture path"),
        SafeHeaders::try_from_iter([
            ("content-type", "application/json"),
            ("x-test", "wire-header-sentinel"),
        ])
        .expect("fixture headers"),
        JsonBodyV1::parse(BODY).expect("fixture body"),
        ProviderAuthV1::HostSigned {
            slot: BearerAuthV1::new(CredentialSlotV1::parse(SLOT).expect("fixture slot")),
            emits: declared(),
        },
    )
    .with_query(
        QueryStringV1::try_from_iter([(QueryParameterV1::ApiVersion, "2024-01-01")])
            .expect("fixture query"),
    )
    .with_user_agent(ControlledUserAgentV1::try_from_static(AGENT).expect("fixture agent"));

    let finalizer = DeterministicRequestFinalizerV1::correct();
    let transport = ReqwestTransportV1::new(
        ReqwestTransportConfigV1::try_new(
            Duration::from_secs(30),
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .expect("fixture timeouts"),
    )
    .expect("transport should build");

    execute_signed_provider_call_v1(
        &binding,
        &request,
        &finalizer,
        &transport,
        tokio::time::Instant::now() + Duration::from_secs(30),
        &CancellationToken::new(),
    )
    .await
    .expect("the signed call should complete");
    server.await.expect("server task should finish");
    let received = rx.await.expect("server should report the request");

    // The URL the signer saw is the URL on the wire, query included.
    assert_eq!(received.request_line, "POST /base/model/invoke?api-version=2024-01-01 HTTP/1.1");

    // Byte-identical body. `content-length` is derived from it and from nothing else.
    assert_eq!(received.body, BODY.as_bytes());
    assert_eq!(received.headers.get("content-length").map(String::as_str), Some("30"));
    assert_eq!(received.body.len(), 30);

    // Every emitted signature is on the wire, byte for byte, under its declared name.
    let observed = finalizer.observed().expect("the finalizer recorded its view");
    for header in declared().headers() {
        assert_eq!(
            received.headers.get(header.header_name()).map(String::as_str),
            Some(expected_signature_v1(&observed, *header).as_str()),
            "{} must reach the wire unchanged",
            header.header_name()
        );
    }

    // Nothing else. The wire set is exactly: ordinary headers ∪ signed headers ∪ the typed
    // user-agent ∪ `TRANSPORT_ADDED_HEADERS_V1`. Anything beyond that list is a byte the signer
    // did not see, and a signature computed without it is a signature over a different request.
    //
    // This assertion is why the added-header list exists at all: its first run reported `accept`,
    // a `reqwest` client default nobody had counted, and the design record's promise said the
    // transport adds "only `host` and `content-length`". The promise was wrong about the world,
    // not the world about the promise.
    let mut permitted: Vec<String> =
        vec!["content-type".to_owned(), "x-test".to_owned(), "user-agent".to_owned()];
    permitted.extend(TRANSPORT_ADDED_HEADERS_V1.iter().map(|name| (*name).to_owned()));
    permitted.extend(declared().headers().iter().map(|h| h.header_name().to_owned()));
    permitted.sort_unstable();
    let mut arrived = received.header_names.clone();
    arrived.sort_unstable();
    assert_eq!(arrived, permitted, "the transport added or dropped a header");

    // `host` is a pure function of the URL the signer saw — which is what makes it legal for a
    // finalizer to include `host` in its signed-headers list.
    assert_eq!(
        received.headers.get("host").map(String::as_str),
        Some(address.to_string().as_str())
    );
    assert_eq!(received.headers.get("user-agent").map(String::as_str), Some(AGENT));
}
