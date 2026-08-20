//! Public contract tests for the host-prelude transport pair.

use std::time::Duration;

use south_transport_reqwest::{ReqwestTransportConfigV1, TransportPairV1};

#[test]
fn builds_both_hardened_transports_from_one_buffered_config() {
    let config = ReqwestTransportConfigV1::try_new(
        Duration::from_secs(30),
        Duration::from_secs(10),
        Duration::from_secs(30),
    )
    .unwrap();

    let pair = TransportPairV1::try_new(config).unwrap();

    let rendered = format!("{pair:?}");
    assert!(rendered.contains("buffered"));
    assert!(rendered.contains("streaming"));
}

#[test]
fn config_validation_rejects_before_the_pair_can_exist() {
    assert!(
        ReqwestTransportConfigV1::try_new(
            Duration::ZERO,
            Duration::from_secs(10),
            Duration::from_secs(30),
        )
        .is_err()
    );
}
