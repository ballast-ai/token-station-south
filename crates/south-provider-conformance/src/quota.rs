use std::fmt;

use south_contracts::{PROVIDER_QUOTA_METADATA_FIELD_COUNT, ProviderQuotaMetadataFieldV1};

use crate::{ProviderCallCountV1, ProviderCallInputV1};

/// The stable identifier for provider quota metadata conformance version one.
pub const PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_ID: &str = "south.provider-quota-metadata.v1";

/// The provider quota metadata conformance suite version.
pub const PROVIDER_QUOTA_METADATA_CONFORMANCE_SUITE_VERSION: u32 = 1;

/// The closed set of canonical provider quota metadata cases.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderQuotaMetadataCaseIdV1 {
    /// Every approved field is present with one distinct valid value.
    AllFields,
    /// No approved field is present.
    NoFields,
}

impl fmt::Debug for ProviderQuotaMetadataCaseIdV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AllFields => "AllFields",
            Self::NoFields => "NoFields",
        })
    }
}

/// Allocation-free raw provider quota metadata retained as synthetic static fixture data.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderQuotaMetadataRawV1 {
    values: [Option<&'static str>; PROVIDER_QUOTA_METADATA_FIELD_COUNT],
}

impl ProviderQuotaMetadataRawV1 {
    /// Returns one closed raw field when present.
    #[must_use]
    pub const fn value(&self, field: ProviderQuotaMetadataFieldV1) -> Option<&'static str> {
        self.values[field_index(field)]
    }

    /// Returns the number of present fields without exposing their values.
    #[must_use]
    pub fn present_field_count(&self) -> usize {
        self.values.iter().flatten().count()
    }
}

impl fmt::Debug for ProviderQuotaMetadataRawV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaMetadataRawV1")
            .field("present_field_count", &self.present_field_count())
            .field(
                "total_value_byte_count",
                &self.values.iter().flatten().map(|value| value.len()).sum::<usize>(),
            )
            .finish_non_exhaustive()
    }
}

/// Exact resolver and transport counts expected from one metadata case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderQuotaMetadataExpectedEvidenceV1 {
    resolver_calls: ProviderCallCountV1,
    transport_calls: ProviderCallCountV1,
}

impl ProviderQuotaMetadataExpectedEvidenceV1 {
    /// Returns the expected resolver call count category.
    #[must_use]
    pub const fn resolver_calls(&self) -> ProviderCallCountV1 {
        self.resolver_calls
    }

    /// Returns the expected transport call count category.
    #[must_use]
    pub const fn transport_calls(&self) -> ProviderCallCountV1 {
        self.transport_calls
    }
}

impl fmt::Debug for ProviderQuotaMetadataExpectedEvidenceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaMetadataExpectedEvidenceV1")
            .field("resolver_calls", &self.resolver_calls)
            .field("transport_calls", &self.transport_calls)
            .finish()
    }
}

/// One immutable assembled-call fixture for the quota metadata extension.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderQuotaMetadataFixtureV1 {
    case_id: ProviderQuotaMetadataCaseIdV1,
    input: ProviderCallInputV1,
    upstream_metadata: ProviderQuotaMetadataRawV1,
    expected_metadata: ProviderQuotaMetadataRawV1,
    expected_evidence: ProviderQuotaMetadataExpectedEvidenceV1,
}

impl ProviderQuotaMetadataFixtureV1 {
    /// Returns the stable case identifier.
    #[must_use]
    pub const fn case_id(&self) -> ProviderQuotaMetadataCaseIdV1 {
        self.case_id
    }

    /// Returns the canonical provider-call input.
    #[must_use]
    pub const fn input(&self) -> &ProviderCallInputV1 {
        &self.input
    }

    /// Returns the raw metadata produced by the fake upstream.
    #[must_use]
    pub const fn upstream_metadata(&self) -> &ProviderQuotaMetadataRawV1 {
        &self.upstream_metadata
    }

    /// Returns the exact metadata expected after the assembled host adapter.
    #[must_use]
    pub const fn expected_metadata(&self) -> &ProviderQuotaMetadataRawV1 {
        &self.expected_metadata
    }

    /// Returns the exact expected resolver and transport evidence.
    #[must_use]
    pub const fn expected_evidence(&self) -> &ProviderQuotaMetadataExpectedEvidenceV1 {
        &self.expected_evidence
    }
}

impl fmt::Debug for ProviderQuotaMetadataFixtureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProviderQuotaMetadataFixtureV1")
            .field("case_id", &self.case_id)
            .field("input", &self.input)
            .field("upstream_metadata", &self.upstream_metadata)
            .field("expected_metadata", &self.expected_metadata)
            .field("expected_evidence", &self.expected_evidence)
            .finish()
    }
}

const ALL_FIELDS: ProviderQuotaMetadataRawV1 = ProviderQuotaMetadataRawV1 {
    values: [
        Some("1000"),
        Some("900"),
        Some("10s"),
        Some("2000"),
        Some("1500"),
        Some("20s"),
        Some("3000"),
        Some("2500"),
        Some("1970-01-01T00:00:30Z"),
    ],
};

const NO_FIELDS: ProviderQuotaMetadataRawV1 =
    ProviderQuotaMetadataRawV1 { values: [None; PROVIDER_QUOTA_METADATA_FIELD_COUNT] };

const INPUT: ProviderCallInputV1 = ProviderCallInputV1 {
    endpoint: super::ENDPOINT,
    bound_credential_slot: super::BOUND_SLOT,
    requested_credential_slot: super::REQUESTED_SLOT,
    relative_path: super::PATH,
    json_body: super::REQUEST_BODY,
    headers: super::HEADERS,
};

const EXPECTED_EVIDENCE: ProviderQuotaMetadataExpectedEvidenceV1 =
    ProviderQuotaMetadataExpectedEvidenceV1 {
        resolver_calls: ProviderCallCountV1::One,
        transport_calls: ProviderCallCountV1::One,
    };

const FIXTURES: &[ProviderQuotaMetadataFixtureV1] = &[
    ProviderQuotaMetadataFixtureV1 {
        case_id: ProviderQuotaMetadataCaseIdV1::AllFields,
        input: INPUT,
        upstream_metadata: ALL_FIELDS,
        expected_metadata: ALL_FIELDS,
        expected_evidence: EXPECTED_EVIDENCE,
    },
    ProviderQuotaMetadataFixtureV1 {
        case_id: ProviderQuotaMetadataCaseIdV1::NoFields,
        input: INPUT,
        upstream_metadata: NO_FIELDS,
        expected_metadata: NO_FIELDS,
        expected_evidence: EXPECTED_EVIDENCE,
    },
];

/// Returns the immutable canonical provider quota metadata fixture table.
#[must_use]
pub const fn provider_quota_metadata_fixtures_v1() -> &'static [ProviderQuotaMetadataFixtureV1] {
    FIXTURES
}

const fn field_index(field: ProviderQuotaMetadataFieldV1) -> usize {
    match field {
        ProviderQuotaMetadataFieldV1::XRateLimitLimitTokens => 0,
        ProviderQuotaMetadataFieldV1::XRateLimitRemainingTokens => 1,
        ProviderQuotaMetadataFieldV1::XRateLimitResetTokens => 2,
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensLimit => 3,
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensRemaining => 4,
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitTokensReset => 5,
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedLimit => 6,
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedRemaining => 7,
        ProviderQuotaMetadataFieldV1::AnthropicRateLimitUnifiedReset => 8,
    }
}
