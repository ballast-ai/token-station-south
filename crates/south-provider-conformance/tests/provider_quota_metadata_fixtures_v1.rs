use south_contracts::{ProviderQuotaMetadataFieldV1, ProviderQuotaMetadataV1};
use south_provider_conformance::{
    PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID,
    PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION, ProviderCallCountV1,
    ProviderQuotaMetadataCaseIdV1, provider_quota_metadata_fixtures_v1,
};

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

#[test]
fn suite_identity_case_order_and_evidence_are_frozen() {
    assert_eq!(PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID, "south.provider-quota-metadata.v1");
    assert_eq!(PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION, 1);

    let fixtures = provider_quota_metadata_fixtures_v1();
    assert_eq!(fixtures.len(), 2);
    assert_eq!(fixtures[0].case_id(), ProviderQuotaMetadataCaseIdV1::AllFields);
    assert_eq!(fixtures[1].case_id(), ProviderQuotaMetadataCaseIdV1::NoFields);
    for fixture in fixtures {
        assert_eq!(fixture.expected_evidence().resolver_calls(), ProviderCallCountV1::One);
        assert_eq!(fixture.expected_evidence().transport_calls(), ProviderCallCountV1::One);
    }
}

#[test]
fn all_fields_are_distinct_bounded_and_no_fields_stays_empty() {
    let fixtures = provider_quota_metadata_fixtures_v1();
    let all = fixtures[0].upstream_metadata();
    let none = fixtures[1].upstream_metadata();
    let bounded = ProviderQuotaMetadataV1::try_from_iter(
        FIELDS
            .into_iter()
            .filter_map(|field| all.value(field).map(|value| (field, value.to_owned()))),
    )
    .expect("canonical raw values must pass the production contract");

    assert_eq!(bounded.present_field_count(), 9);
    assert_eq!(FIELDS.map(|field| none.value(field)), [None; 9]);
    let values = FIELDS
        .into_iter()
        .map(|field| all.value(field).expect("all-fields fixture must populate every field"))
        .collect::<Vec<_>>();
    for (index, value) in values.iter().enumerate() {
        assert!(
            values
                .iter()
                .enumerate()
                .all(|(other_index, other)| { index == other_index || value != other }),
            "each field needs a distinct sentinel so swapped mappings fail"
        );
    }
}

#[test]
fn raw_expected_and_fixture_debug_output_redacts_every_value() {
    for fixture in provider_quota_metadata_fixtures_v1() {
        let rendered = format!(
            "{fixture:?} {:?} {:?}",
            fixture.upstream_metadata(),
            fixture.expected_metadata()
        );
        for field in FIELDS {
            if let Some(value) = fixture.upstream_metadata().value(field) {
                assert!(!rendered.contains(value));
            }
        }
    }
}

#[test]
fn expected_metadata_exactly_matches_the_canonical_upstream() {
    for fixture in provider_quota_metadata_fixtures_v1() {
        for field in FIELDS {
            assert_eq!(
                fixture.upstream_metadata().value(field),
                fixture.expected_metadata().value(field)
            );
        }
    }
}
