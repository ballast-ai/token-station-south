use super::*;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[derive(Default)]
struct Capture(Mutex<Vec<(&'static str, u64)>>);
impl TransportObserver for Capture {
    fn event(&self, _id: &str, stage: &'static str, elapsed: Duration) {
        self.0.lock().unwrap().push((stage, u64::try_from(elapsed.as_micros()).unwrap()));
    }
}
#[tokio::test]
async fn mock_headers_and_body_are_separate_and_bytes_preserved() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (release, wait) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut raw = Vec::new();
        loop {
            let mut buf = [0; 1024];
            let n = s.read(&mut buf).await.unwrap();
            assert_ne!(n, 0);
            raw.extend_from_slice(&buf[..n]);
            assert!(raw.len() <= 8192);
            if raw.ends_with(b"{\"effort\":\"low\"}") {
                break;
            }
        }
        let text = String::from_utf8(raw).unwrap();
        assert!(text.to_lowercase().contains("content-length: 16"));
        assert!(!text.to_lowercase().contains("transfer-encoding"));
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        wait.await.unwrap();
        s.write_all(b"{\"ok\":true}").await.unwrap();
    });
    let sink = Arc::new(Capture::default());
    let context = ObserveContext::new("req_mock".into(), sink.clone());
    let client =
        reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build().unwrap();
    let body = Bytes::from_static(b"{\"effort\":\"low\"}");
    let builder = client
        .post(format!("http://{address}/mock"))
        .body(reqwest::Body::wrap(ObservedBody::new(body, context.clone())));
    let response = send_observed(builder, Some(context.clone())).await.unwrap();
    assert!(sink.0.lock().unwrap().iter().any(|(s, _)| *s == "south_headers_ready"));
    assert!(!sink.0.lock().unwrap().iter().any(|(s, _)| *s == "south_body_first_chunk"));
    release.send(()).unwrap();
    let data = read_bounded_body_observed(response, 100, Some(context)).await.unwrap();
    assert_eq!(data, b"{\"ok\":true}");
    server.await.unwrap();
    let es = sink.0.lock().unwrap().clone();
    let stages: Vec<_> = es.iter().map(|e| e.0).collect();
    for required in [
        "south_send_start",
        "south_body_first_poll",
        "south_body_handed_off",
        "south_headers_ready",
        "south_body_first_chunk",
        "south_body_complete",
    ] {
        assert!(stages.contains(&required));
    }
    assert!(es.windows(2).all(|w| w[0].1 <= w[1].1));
}
#[test]
fn exact_size_hint_is_preserved() {
    use http_body::Body;
    let sink = Arc::new(Capture::default());
    let b =
        ObservedBody::new(Bytes::from_static(b"abc"), ObserveContext::new("req_mock".into(), sink));
    assert_eq!(b.size_hint().exact(), Some(3));
    assert!(!b.is_end_stream());
}
#[tokio::test]
async fn missing_observer_keeps_response_bytes() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = [0; 1024];
        let _ = s.read(&mut buf).await.unwrap();
        s.write_all(b"HTTP/1.1 503 Mock\r\nContent-Length: 3\r\nConnection: close\r\n\r\nerr")
            .await
            .unwrap();
    });
    let c = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .retry(reqwest::retry::never())
        .build()
        .unwrap();
    let r = send_observed(c.get(format!("http://{addr}")), None).await.unwrap();
    assert_eq!(r.status().as_u16(), 503);
    assert_eq!(read_bounded_body_observed(r, 100, None).await.unwrap(), b"err");
    server.await.unwrap();
}
#[tokio::test]
async fn response_limit_never_becomes_success() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut buf = [0; 1024];
        let _ = s.read(&mut buf).await.unwrap();
        s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef")
            .await
            .unwrap();
    });
    let c = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build().unwrap();
    let sink = Arc::new(Capture::default());
    let context = ObserveContext::new("req_mock".into(), sink.clone());
    let r = send_observed(c.get(format!("http://{addr}")), Some(context.clone())).await.unwrap();
    assert!(read_bounded_body_observed(r, 3, Some(context)).await.is_err());
    assert!(!sink.0.lock().unwrap().iter().any(|(s, _)| *s == "south_body_complete"));
    server.await.unwrap();
}

#[derive(Default)]
struct Connections(Mutex<Vec<(u64, bool, String)>>);
impl TransportObserver for Connections {
    fn event(&self, _: &str, _: &'static str, _: Duration) {}
    fn connection(&self, _: &str, _: Duration, fingerprint: u64, reused: bool, protocol: &str) {
        self.0.lock().unwrap().push((fingerprint, reused, protocol.into()));
    }
}
#[tokio::test]
async fn keepalive_reuse_is_distinguished_from_new_client() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for count in [2, 1] {
            let (mut socket, _) = listener.accept().await.unwrap();
            for _ in 0..count {
                let mut raw = Vec::new();
                while !raw.ends_with(b"\r\n\r\n") {
                    let mut b = [0; 1];
                    assert_eq!(socket.read(&mut b).await.unwrap(), 1);
                    raw.extend_from_slice(&b);
                }
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok").await.unwrap();
            }
        }
    });
    let sink = Arc::new(Connections::default());
    let tracker = observation::ConnectionTracker::default();
    let client =
        reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build().unwrap();
    for id in ["one", "two"] {
        let context = ObserveContext::new(id.into(), sink.clone()).with_tracker(tracker.clone());
        let response =
            send_observed(client.get(format!("http://{addr}")), Some(context)).await.unwrap();
        assert_eq!(read_bounded_body(response, 100).await.unwrap(), b"ok");
    }
    let fresh =
        reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(2)).build().unwrap();
    let context = ObserveContext::new("fresh".into(), sink.clone()).with_tracker(tracker);
    let response = send_observed(fresh.get(format!("http://{addr}")), Some(context)).await.unwrap();
    assert_eq!(read_bounded_body(response, 100).await.unwrap(), b"ok");
    server.await.unwrap();
    let rows = sink.0.lock().unwrap().clone();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].0, rows[1].0);
    assert_ne!(rows[1].0, rows[2].0);
    assert_eq!(rows.iter().map(|r| r.1).collect::<Vec<_>>(), [false, true, false]);
    assert!(rows.iter().all(|r| r.2 == "HTTP/1.1"));
}
