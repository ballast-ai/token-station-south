//! Public construction contract for opt-in transport observation.
use south_transport_reqwest::{ReqwestTransportConfigV1, TransportObserver, TransportPairV1};
use std::{sync::Arc, time::Duration};
struct Sink;
impl TransportObserver for Sink {
    fn event(&self, _: &str, _: &'static str, _: Duration) {}
}
#[test]
fn observation_is_optional_and_works_with_stock_reqwest() {
    let config = ReqwestTransportConfigV1::try_new(
        Duration::from_secs(10),
        Duration::from_secs(2),
        Duration::from_secs(5),
    )
    .unwrap();
    let pair = TransportPairV1::try_new_observed(config, Arc::new(Sink)).unwrap();
    assert!(format!("{pair:?}").contains("buffered"));
}

#[derive(Default)]
struct Capture(std::sync::Mutex<Vec<(String, &'static str)>>);
impl TransportObserver for Capture {
    fn event(&self, id: &str, stage: &'static str, _: Duration) {
        self.0.lock().unwrap().push((id.to_owned(), stage));
    }
}
struct Resolver;
impl south_core::CredentialResolver for Resolver {
    fn resolve<'a>(
        &'a self,
        _: &'a south_contracts::CredentialSlotV1,
    ) -> south_core::CredentialResolutionFuture<'a> {
        Box::pin(async { Ok(south_core::SecretValue::new("fixture-secret".to_owned())) })
    }
}
#[tokio::test]
async fn public_execution_observes_only_bounded_opaque_ids_without_changing_wire() {
    use south_contracts::{
        BearerAuthV1, CredentialSlotV1, JsonBodyV1, JsonPostRequestV1, ProviderEndpointV1,
        ProviderQuotaHeaderMapV1, RelativePathV1, SafeHeaders,
    };
    use south_core::{ProviderBindingV1, execute_provider_call_v1};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for (id, observed) in [
        ("req_public_123".to_owned(), true),
        ("user@example.invalid".to_owned(), false),
        (" req_public_123".to_owned(), false),
        ("req_public_123 ".to_owned(), false),
        ("x".repeat(129), false),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut raw = Vec::new();
            while !raw.ends_with(b"\r\n\r\n{}") {
                let mut bytes = [0; 1024];
                let n = stream.read(&mut bytes).await.unwrap();
                assert_ne!(n, 0);
                raw.extend_from_slice(&bytes[..n]);
                assert!(raw.len() < 8192);
            }
            let wire = String::from_utf8(raw).unwrap().to_ascii_lowercase();
            assert!(wire.contains("content-length: 2"));
            assert!(!wire.contains("transfer-encoding"));
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").await.unwrap();
        });
        let config = ReqwestTransportConfigV1::try_new(
            Duration::from_secs(2),
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
        .unwrap();
        let capture = Arc::new(Capture::default());
        let pair = TransportPairV1::try_new_observed(config, capture.clone()).unwrap();
        let transport = pair.buffered.with_quota_headers(ProviderQuotaHeaderMapV1::canonical());
        let slot = CredentialSlotV1::parse("primary").unwrap();
        let request = JsonPostRequestV1::new(
            RelativePathV1::parse("call").unwrap(),
            SafeHeaders::try_from_iter([("x-request-id", id.as_str())]).unwrap(),
            JsonBodyV1::parse("{}").unwrap(),
            BearerAuthV1::new(slot.clone()),
        );
        let binding = ProviderBindingV1::new(
            ProviderEndpointV1::parse(&format!("http://{address}")).unwrap(),
            slot,
        );
        let result = execute_provider_call_v1(
            &binding,
            &request,
            &Resolver,
            &transport,
            tokio::time::Instant::now() + Duration::from_secs(2),
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert!(result.is_ok());
        server.await.unwrap();
        let events = capture.0.lock().unwrap().clone();
        assert_eq!(!events.is_empty(), observed);
        if observed {
            assert!(events.iter().all(|(actual, _)| actual == &id));
            for required in [
                "south_send_start",
                "south_body_handed_off",
                "south_headers_ready",
                "south_body_complete",
                "south_upstream_boundaries_unknown",
            ] {
                assert!(events.iter().any(|(_, stage)| *stage == required));
            }
            assert!(!events.iter().any(|(_, stage)| stage.contains("flush_complete")));
        }
    }
}
