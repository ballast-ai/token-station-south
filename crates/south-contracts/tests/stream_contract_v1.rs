use std::{fmt::Display, time::Duration};

use bytes::Bytes;
use http::StatusCode;
use south_contracts::{
    MAX_STREAM_CHUNK_BYTES, MAX_STREAM_ERROR_BODY_BYTES, MAX_TRANSPORT_TIMEOUT,
    STREAM_CONTRACT_VERSION, StreamChunkV1, StreamReadErrorV1, StreamRejectedV1,
    StreamTransportConfigV1, StreamingResponseHeadV1, TransportErrorV1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(StreamingResponseHeadV1: Display);
assert_not_impl_any!(StreamRejectedV1: Display);
assert_not_impl_any!(StreamChunkV1: Display);

const CONTENT_TYPE_SENTINEL: &str = "stream-content-type-sentinel";
const RETRY_AFTER_SENTINEL: &str = "stream-retry-after-sentinel";
const ERROR_BODY_SENTINEL: &str = "stream-error-body-sentinel";
const CHUNK_SENTINEL: &str = "stream-chunk-sentinel";

fn head(status: StatusCode) -> StreamingResponseHeadV1 {
    StreamingResponseHeadV1::try_from_parts(
        status,
        Some(CONTENT_TYPE_SENTINEL.to_owned()),
        Some(RETRY_AFTER_SENTINEL.to_owned()),
    )
    .expect("fixture head should be valid")
}

#[test]
fn stream_contract_version_and_limits_are_frozen() {
    assert_eq!(STREAM_CONTRACT_VERSION, Some(1));
    assert_eq!(MAX_STREAM_ERROR_BODY_BYTES, 64 * 1024);
    assert_eq!(MAX_STREAM_CHUNK_BYTES, 64 * 1024);
}

#[test]
fn head_preserves_status_and_the_two_allowed_metadata_fields() {
    let head = head(StatusCode::OK);

    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.content_type(), Some(CONTENT_TYPE_SENTINEL));
    assert_eq!(head.retry_after(), Some(RETRY_AFTER_SENTINEL));

    let absent = StreamingResponseHeadV1::try_from_parts(StatusCode::BAD_GATEWAY, None, None)
        .expect("absent metadata should be valid");
    assert_eq!(absent.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(absent.content_type(), None);
    assert_eq!(absent.retry_after(), None);
}

#[test]
fn head_refuses_redirect_statuses() {
    for status in [StatusCode::MOVED_PERMANENTLY, StatusCode::FOUND, StatusCode::TEMPORARY_REDIRECT]
    {
        let error = StreamingResponseHeadV1::try_from_parts(status, None, None)
            .expect_err("redirect statuses must not become a streaming head");
        assert_eq!(error, TransportErrorV1::RedirectDenied);
        assert_eq!(error.code(), "REDIRECT_DENIED");
    }
}

#[test]
fn head_enforces_the_existing_metadata_bounds() {
    let oversized_content_type = "x".repeat(south_contracts::MAX_RESPONSE_CONTENT_TYPE_BYTES + 1);
    let oversized_retry_after = "x".repeat(south_contracts::MAX_RESPONSE_RETRY_AFTER_BYTES + 1);
    let invalid_metadata = "invalid\rmetadata".to_owned();

    for (content_type, retry_after) in [
        (Some(oversized_content_type), None),
        (None, Some(oversized_retry_after)),
        (Some(invalid_metadata.clone()), None),
        (None, Some(invalid_metadata)),
    ] {
        let error =
            StreamingResponseHeadV1::try_from_parts(StatusCode::OK, content_type, retry_after)
                .expect_err("invalid metadata must be rejected");
        assert_eq!(error, TransportErrorV1::ResponseMetadataInvalid);
    }
}

#[test]
fn head_debug_is_redacted() {
    let debug = format!("{:?}", head(StatusCode::OK));

    assert!(!debug.contains(CONTENT_TYPE_SENTINEL));
    assert!(!debug.contains(RETRY_AFTER_SENTINEL));
}

#[test]
fn rejected_keeps_a_bounded_error_body_exactly_at_the_limit() {
    let body = vec![b'x'; MAX_STREAM_ERROR_BODY_BYTES];
    let rejected = StreamRejectedV1::new(head(StatusCode::TOO_MANY_REQUESTS), body.clone());

    assert_eq!(rejected.head().status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(rejected.body(), body.as_slice());
}

#[test]
fn rejected_truncates_an_oversized_error_body_without_failing() {
    let mut body = vec![b'x'; MAX_STREAM_ERROR_BODY_BYTES];
    body.push(b'y');
    let rejected = StreamRejectedV1::new(head(StatusCode::BAD_GATEWAY), body);

    assert_eq!(rejected.body().len(), MAX_STREAM_ERROR_BODY_BYTES);
    assert!(rejected.body().iter().all(|byte| *byte == b'x'));
}

#[test]
fn rejected_debug_is_redacted() {
    let rejected = StreamRejectedV1::new(
        head(StatusCode::BAD_REQUEST),
        ERROR_BODY_SENTINEL.as_bytes().to_vec(),
    );
    let debug = format!("{rejected:?}");

    assert!(!debug.contains(ERROR_BODY_SENTINEL));
    assert!(!debug.contains(CONTENT_TYPE_SENTINEL));
    assert!(!debug.contains(RETRY_AFTER_SENTINEL));
}

#[test]
fn chunk_shares_the_transport_allocation_up_to_the_limit() {
    let bytes = Bytes::from(vec![b'x'; MAX_STREAM_CHUNK_BYTES]);
    let pointer = bytes.as_ref().as_ptr();

    let chunk = StreamChunkV1::try_new(bytes).expect("a chunk at the limit should be accepted");

    assert_eq!(chunk.len(), MAX_STREAM_CHUNK_BYTES);
    assert!(!chunk.is_empty());
    assert_eq!(chunk.as_bytes().as_ptr(), pointer);
    assert_eq!(chunk.into_bytes().as_ref().as_ptr(), pointer);
}

#[test]
fn chunk_above_the_limit_is_not_deliverable() {
    let error = StreamChunkV1::try_new(Bytes::from(vec![b'x'; MAX_STREAM_CHUNK_BYTES + 1]))
        .expect_err("an oversized chunk must be rejected");

    assert_eq!(error, StreamReadErrorV1::ChunkNotDeliverable);
    assert_eq!(error.code(), "STREAM_CHUNK_INVALID");
}

#[test]
fn empty_chunk_remains_representable() {
    let chunk = StreamChunkV1::try_new(Bytes::new()).expect("an empty chunk should be accepted");

    assert!(chunk.is_empty());
    assert_eq!(chunk.len(), 0);
}

#[test]
fn chunk_debug_is_redacted() {
    let chunk = StreamChunkV1::try_new(Bytes::from_static(CHUNK_SENTINEL.as_bytes()))
        .expect("fixture chunk should be valid");

    assert!(!format!("{chunk:?}").contains(CHUNK_SENTINEL));
}

#[test]
fn stream_read_error_codes_are_frozen() {
    let codes = [
        StreamReadErrorV1::StreamReadFailed,
        StreamReadErrorV1::StreamIdleTimeout,
        StreamReadErrorV1::StreamDeadlineExceeded,
        StreamReadErrorV1::StreamCancelled,
        StreamReadErrorV1::ChunkNotDeliverable,
    ]
    .map(StreamReadErrorV1::code);

    assert_eq!(
        codes,
        [
            "STREAM_READ_FAILED",
            "STREAM_IDLE_TIMEOUT",
            "STREAM_DEADLINE_EXCEEDED",
            "STREAM_CANCELLED",
            "STREAM_CHUNK_INVALID",
        ]
    );
}

#[test]
fn stream_transport_config_accepts_the_unbounded_production_shape() {
    let config =
        StreamTransportConfigV1::try_new(None, Duration::from_secs(5), Duration::from_secs(30))
            .expect("an unbounded total with explicit guards should be accepted");

    assert_eq!(config.total_timeout(), None);
    assert_eq!(config.connect_timeout(), Duration::from_secs(5));
    assert_eq!(config.idle_timeout(), Duration::from_secs(30));
}

#[test]
fn stream_transport_config_accepts_an_explicit_bounded_total() {
    let config = StreamTransportConfigV1::try_new(
        Some(MAX_TRANSPORT_TIMEOUT),
        Duration::from_secs(5),
        Duration::from_secs(30),
    )
    .expect("a bounded total at the transport cap should be accepted");

    assert_eq!(config.total_timeout(), Some(MAX_TRANSPORT_TIMEOUT));
}

#[test]
fn stream_transport_config_rejects_zero_or_inconsistent_timeouts() {
    let invalid = [
        (None, Duration::ZERO, Duration::from_secs(1)),
        (None, Duration::from_secs(1), Duration::ZERO),
        (Some(Duration::ZERO), Duration::from_secs(1), Duration::from_secs(1)),
        (
            Some(MAX_TRANSPORT_TIMEOUT + Duration::from_secs(1)),
            Duration::from_secs(1),
            Duration::from_secs(1),
        ),
        (Some(Duration::from_secs(1)), Duration::from_secs(2), Duration::from_secs(1)),
        (Some(Duration::from_secs(1)), Duration::from_secs(1), Duration::from_secs(2)),
    ];

    for (total, connect, idle) in invalid {
        let error = StreamTransportConfigV1::try_new(total, connect, idle)
            .expect_err("invalid stream transport timeouts must be rejected");
        assert_eq!(error, TransportErrorV1::ClientBuildFailed);
        assert_eq!(error.code(), "CLIENT_BUILD_FAILED");
    }
}
