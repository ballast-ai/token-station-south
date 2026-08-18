use south_contracts::{ProviderQuotaMetadataFieldV1, ProviderQuotaMetadataV1};
use south_provider_conformance::{
    PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID,
    PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION, ProviderCallCountV1,
    ProviderCallFailureCodeV1, ProviderQuotaMetadataCaseIdV1,
    ProviderQuotaMetadataExpectedOutcomeV1, ProviderQuotaMetadataFixtureV1,
    ProviderQuotaMetadataRawV1, ProviderQuotaMetadataUpstreamV1,
    provider_quota_metadata_fixtures_v1,
};

/// Returns the raw metadata a fixture serves, or `None` when its transport is never reached.
const fn upstream_metadata(
    fixture: &ProviderQuotaMetadataFixtureV1,
) -> Option<&ProviderQuotaMetadataRawV1> {
    match fixture.upstream() {
        ProviderQuotaMetadataUpstreamV1::Metadata(raw) => Some(raw),
        ProviderQuotaMetadataUpstreamV1::NotReached => None,
    }
}

/// Returns the raw metadata a fixture expects, or `None` when it expects a failure instead.
const fn expected_metadata(
    fixture: &ProviderQuotaMetadataFixtureV1,
) -> Option<&ProviderQuotaMetadataRawV1> {
    match fixture.expected_outcome() {
        ProviderQuotaMetadataExpectedOutcomeV1::Metadata(raw) => Some(raw),
        ProviderQuotaMetadataExpectedOutcomeV1::Failure { .. } => None,
    }
}

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
    let expected = [
        (
            ProviderQuotaMetadataCaseIdV1::AllFields,
            ProviderCallCountV1::One,
            ProviderCallCountV1::One,
        ),
        (
            ProviderQuotaMetadataCaseIdV1::NoFields,
            ProviderCallCountV1::One,
            ProviderCallCountV1::One,
        ),
        (
            ProviderQuotaMetadataCaseIdV1::CredentialSlotMismatch,
            ProviderCallCountV1::Zero,
            ProviderCallCountV1::Zero,
        ),
    ];
    // `zip` truncates silently, so a fixture added without extending the table above would go
    // unchecked rather than failing here.
    assert_eq!(fixtures.len(), expected.len());
    for (fixture, expected) in fixtures.iter().zip(expected) {
        assert_eq!(fixture.case_id(), expected.0);
        assert_eq!(fixture.expected_evidence().resolver_calls(), expected.1);
        assert_eq!(fixture.expected_evidence().transport_calls(), expected.2);
    }
}

/// The two original cases were both success paths expecting `(One, One)`, so an adapter reporting
/// the literal `(1, 1)` instead of reading its real counters passed the whole suite. Exactly one
/// case must expect zero calls at both boundaries; deleting it, or relaxing its expectation back
/// to a success path, fails here rather than silently reopening that hole.
#[test]
fn exactly_one_case_expects_a_failure_with_no_call_at_either_boundary() {
    let zero_call = provider_quota_metadata_fixtures_v1().iter().filter(|fixture| {
        let evidence = fixture.expected_evidence();
        evidence.resolver_calls() == ProviderCallCountV1::Zero
            && evidence.transport_calls() == ProviderCallCountV1::Zero
    });

    assert_eq!(zero_call.count(), 1);

    let failing = provider_quota_metadata_fixtures_v1()
        .iter()
        .filter(|fixture| expected_metadata(fixture).is_none())
        .collect::<Vec<_>>();

    assert_eq!(failing.len(), 1);
    assert_eq!(failing[0].case_id(), ProviderQuotaMetadataCaseIdV1::CredentialSlotMismatch);
    assert_eq!(
        *failing[0].expected_outcome(),
        ProviderQuotaMetadataExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::CredentialBindingMismatch,
        }
    );
    assert_eq!(*failing[0].upstream(), ProviderQuotaMetadataUpstreamV1::NotReached);
}

/// The zero-call case only refuses before any boundary because its requested slot differs from
/// the slot the binding grants. Everything else about the input stays canonical.
#[test]
fn only_the_zero_call_case_requests_a_slot_the_binding_does_not_grant() {
    let fixtures = provider_quota_metadata_fixtures_v1();
    for fixture in fixtures {
        let input = fixture.input();
        let mismatched = input.requested_credential_slot() != input.bound_credential_slot();
        assert_eq!(
            mismatched,
            fixture.case_id() == ProviderQuotaMetadataCaseIdV1::CredentialSlotMismatch
        );
        assert_eq!(input.endpoint(), fixtures[0].input().endpoint());
        assert_eq!(input.relative_path(), fixtures[0].input().relative_path());
        assert_eq!(input.json_body(), fixtures[0].input().json_body());
    }
}

#[test]
fn all_fields_are_distinct_bounded_and_no_fields_stays_empty() {
    let fixtures = provider_quota_metadata_fixtures_v1();
    let all = upstream_metadata(&fixtures[0]).expect("the all-fields case must serve metadata");
    let none = upstream_metadata(&fixtures[1]).expect("the no-fields case must serve metadata");
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
        let rendered =
            format!("{fixture:?} {:?} {:?}", fixture.upstream(), fixture.expected_outcome());
        for field in FIELDS {
            if let Some(value) = upstream_metadata(fixture).and_then(|raw| raw.value(field)) {
                assert!(!rendered.contains(value));
            }
        }
    }
}

#[test]
fn expected_metadata_exactly_matches_the_canonical_upstream() {
    for fixture in provider_quota_metadata_fixtures_v1() {
        match (upstream_metadata(fixture), expected_metadata(fixture)) {
            (Some(upstream), Some(expected)) => {
                for field in FIELDS {
                    assert_eq!(upstream.value(field), expected.value(field));
                }
            }
            // A case whose transport is never reached has no metadata to propagate.
            (None, None) => {}
            (upstream, expected) => panic!(
                "upstream and expected outcome must agree on whether metadata exists: \
                 upstream_present={}, expected_present={}",
                upstream.is_some(),
                expected.is_some()
            ),
        }
    }
}
