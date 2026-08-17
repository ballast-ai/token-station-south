use std::fmt::Display;

use http::StatusCode;
use south_contracts::{
    BufferedHttpResponseV1, MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES,
    MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES, PROVIDER_QUOTA_METADATA_CONTRACT_VERSION,
    PROVIDER_QUOTA_METADATA_FIELD_COUNT, ProviderQuotaMetadataFieldV1, ProviderQuotaMetadataV1,
    StreamingResponseHeadV1, TransportErrorV1,
};
use static_assertions::assert_not_impl_any;

const FIELDS: [ProviderQuotaMetadataFieldV1; 9] = [
    ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens,
    ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens,
    ProviderQuotaMetadataFieldV1::XRateLimitResetTokens,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensLimit,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensRemaining,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensReset,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedLimit,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedRemaining,
    ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedReset,
];

const HEADER_NAMES: [&str; 9] = [
    "x-ratelimit-limit-tokens",
    "x-ratelimit-remaining-tokens",
    "x-ratelimit-reset-tokens",
    "anthropic-ratelimit-tokens-limit",
    "anthropic-ratelimit-tokens-remaining",
    "anthropic-ratelimit-tokens-reset",
    "anthropic-ratelimit-unified-limit",
    "anthropic-ratelimit-unified-remaining",
    "anthropic-ratelimit-unified-reset",
];

assert_not_impl_any!(ProviderQuotaMetadataV1: Display, serde::Serialize, serde::Deserialize<'static>);

fn all_metadata() -> ProviderQuotaMetadataV1 {
    ProviderQuotaMetadataV1::try_from_iter(
        FIELDS
            .into_iter()
            .enumerate()
            .map(|(index, field)| (field, format!("quota-value-{index}"))),
    )
    .expect("the nine bounded fixture values should be valid")
}

#[test]
fn contract_version_vocabulary_and_limits_are_exact() {
    assert_eq!(PROVIDER_QUOTA_METADATA_CONTRACT_VERSION, 1);
    assert_eq!(PROVIDER_QUOTA_METADATA_FIELD_COUNT, 9);
    assert_eq!(MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES, 256);
    assert_eq!(MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES, 2_304);
    assert_eq!(FIELDS.len(), PROVIDER_QUOTA_METADATA_FIELD_COUNT);
    assert_eq!(FIELDS.map(ProviderQuotaMetadataFieldV1::as_header_name), HEADER_NAMES);
}

#[test]
fn empty_and_complete_metadata_expose_only_closed_read_only_values() {
    let empty = ProviderQuotaMetadataV1::default();
    assert_eq!(empty.present_field_count(), 0);
    for field in FIELDS {
        assert_eq!(empty.value(field), None);
    }

    let metadata = all_metadata();
    assert_eq!(metadata.present_field_count(), 9);
    assert_eq!(metadata.x_ratelimit_limit_tokens(), Some("quota-value-0"));
    assert_eq!(metadata.x_ratelimit_remaining_tokens(), Some("quota-value-1"));
    assert_eq!(metadata.x_ratelimit_reset_tokens(), Some("quota-value-2"));
    assert_eq!(metadata.anthropic_ratelimit_tokens_limit(), Some("quota-value-3"));
    assert_eq!(metadata.anthropic_ratelimit_tokens_remaining(), Some("quota-value-4"));
    assert_eq!(metadata.anthropic_ratelimit_tokens_reset(), Some("quota-value-5"));
    assert_eq!(metadata.anthropic_ratelimit_unified_limit(), Some("quota-value-6"));
    assert_eq!(metadata.anthropic_ratelimit_unified_remaining(), Some("quota-value-7"));
    assert_eq!(metadata.anthropic_ratelimit_unified_reset(), Some("quota-value-8"));
}

#[test]
fn metadata_rejects_duplicates_overlong_values_and_invalid_http_text() {
    let duplicate = ProviderQuotaMetadataV1::try_from_iter([
        (ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens, "1".to_owned()),
        (ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens, "1".to_owned()),
    ]);
    assert_eq!(duplicate, Err(TransportErrorV1::ResponseMetadataInvalid));

    let overlong = ProviderQuotaMetadataV1::try_from_iter([(
        ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens,
        "x".repeat(MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES + 1),
    )]);
    assert_eq!(overlong, Err(TransportErrorV1::ResponseMetadataInvalid));

    let invalid = ProviderQuotaMetadataV1::try_from_iter([(
        ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens,
        "invalid\r\nmetadata".to_owned(),
    )]);
    assert_eq!(invalid, Err(TransportErrorV1::ResponseMetadataInvalid));
}

#[test]
fn metadata_accepts_every_field_at_the_exact_combined_bound() {
    let metadata = ProviderQuotaMetadataV1::try_from_iter(
        FIELDS
            .into_iter()
            .map(|field| (field, "x".repeat(MAX_PROVIDER_QUOTA_METADATA_VALUE_BYTES))),
    )
    .expect("nine maximum-sized values should equal the total bound");

    assert_eq!(metadata.present_field_count(), PROVIDER_QUOTA_METADATA_FIELD_COUNT);
    assert_eq!(
        FIELDS
            .into_iter()
            .map(|field| metadata.value(field).expect("every field should be present").len())
            .sum::<usize>(),
        MAX_PROVIDER_QUOTA_METADATA_TOTAL_BYTES
    );
}

#[test]
fn old_response_constructors_default_quota_metadata_to_empty() {
    let buffered =
        BufferedHttpResponseV1::try_from_parts(StatusCode::OK, b"{}".to_vec(), None, None)
            .expect("the legacy buffered constructor should remain valid");
    assert_eq!(buffered.provider_quota_metadata().present_field_count(), 0);

    let streaming = StreamingResponseHeadV1::try_from_parts(StatusCode::OK, None, None)
        .expect("the legacy streaming constructor should remain valid");
    assert_eq!(streaming.provider_quota_metadata().present_field_count(), 0);
}

#[test]
fn explicit_response_constructors_preserve_bounded_quota_metadata() {
    let buffered = BufferedHttpResponseV1::try_from_parts_with_provider_quota_metadata(
        StatusCode::CREATED,
        b"{}".to_vec(),
        Some("application/json".to_owned()),
        None,
        all_metadata(),
    )
    .expect("the buffered response should retain quota metadata");
    assert_eq!(
        buffered.provider_quota_metadata().x_ratelimit_limit_tokens(),
        Some("quota-value-0")
    );

    let streaming = StreamingResponseHeadV1::try_from_parts_with_provider_quota_metadata(
        StatusCode::OK,
        Some("text/event-stream".to_owned()),
        None,
        all_metadata(),
    )
    .expect("the streaming head should retain quota metadata");
    assert_eq!(
        streaming.provider_quota_metadata().anthropic_ratelimit_unified_reset(),
        Some("quota-value-8")
    );
}

#[test]
fn debug_output_never_exposes_quota_values() {
    let metadata = all_metadata();
    let metadata_debug = format!("{metadata:?}");
    assert!(metadata_debug.contains("present_field_count"));
    for index in 0..FIELDS.len() {
        assert!(!metadata_debug.contains(&format!("quota-value-{index}")));
    }

    let response = BufferedHttpResponseV1::try_from_parts_with_provider_quota_metadata(
        StatusCode::OK,
        b"{}".to_vec(),
        None,
        None,
        metadata,
    )
    .expect("the response should be valid");
    let response_debug = format!("{response:?}");
    for index in 0..FIELDS.len() {
        assert!(!response_debug.contains(&format!("quota-value-{index}")));
    }
}
