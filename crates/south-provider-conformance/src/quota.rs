use std::fmt;

use south_contracts::{PROVIDER_QUOTA_METADATA_FIELD_COUNT, ProviderQuotaMetadataFieldV1};

use crate::{ProviderCallCountV1, ProviderCallFailureCodeV1, ProviderCallInputV1};

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
    /// A valid requested credential slot that differs from the binding, refused before any
    /// boundary.
    ///
    /// This case exists to close a blind spot measured on a real host adapter during the
    /// enterprise adoption (2026-08-18). Before it, both canonical cases were success paths
    /// expecting `resolver_calls: One, transport_calls: One`, so no cell in the table could
    /// separate correct wiring from a plausible shortcut. Two mutations survived the full suite:
    /// an adapter reporting the literal `(1, 1)` instead of reading its real atomic counters, and
    /// an adapter whose evidence never distinguished a call that was never made.
    ///
    /// This is the first case that expects a failure and zero calls at both boundaries, which is
    /// the combination the table was missing. `south-core` refuses the mismatched slot in its
    /// binding check before resolving any credential, so a correct adapter reports
    /// [`ProviderCallFailureCodeV1::CredentialBindingMismatch`] with `(Zero, Zero)`. An adapter
    /// hardcoding `(1, 1)` fails here with `ResolverCallCount` and `TransportCallCount`
    /// mismatches.
    CredentialSlotMismatch,
}

impl fmt::Debug for ProviderQuotaMetadataCaseIdV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::AllFields => "AllFields",
            Self::NoFields => "NoFields",
            Self::CredentialSlotMismatch => "CredentialSlotMismatch",
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

/// The raw metadata a fake upstream serves, or the fact that it is never asked for any.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderQuotaMetadataUpstreamV1 {
    /// Complete one exchange carrying this raw metadata.
    Metadata(ProviderQuotaMetadataRawV1),
    /// The transport boundary must not be reached.
    NotReached,
}

impl fmt::Debug for ProviderQuotaMetadataUpstreamV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(raw) => formatter.debug_tuple("Metadata").field(raw).finish(),
            Self::NotReached => formatter.write_str("NotReached"),
        }
    }
}

/// The exact expected terminal shape of one canonical metadata case.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProviderQuotaMetadataExpectedOutcomeV1 {
    /// Bounded metadata matched field by field, preserving the presence of every field.
    Metadata(ProviderQuotaMetadataRawV1),
    /// A known stable failure reached before any metadata could exist.
    Failure {
        /// Expected closed failure code.
        code: ProviderCallFailureCodeV1,
    },
}

impl fmt::Debug for ProviderQuotaMetadataExpectedOutcomeV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(raw) => formatter.debug_tuple("Metadata").field(raw).finish(),
            Self::Failure { code } => {
                formatter.debug_struct("Failure").field("code", code).finish()
            }
        }
    }
}

/// One immutable assembled-call fixture for the quota metadata extension.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ProviderQuotaMetadataFixtureV1 {
    case_id: ProviderQuotaMetadataCaseIdV1,
    input: ProviderCallInputV1,
    upstream: ProviderQuotaMetadataUpstreamV1,
    expected_outcome: ProviderQuotaMetadataExpectedOutcomeV1,
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

    /// Returns the canonical fake-upstream behavior.
    #[must_use]
    pub const fn upstream(&self) -> &ProviderQuotaMetadataUpstreamV1 {
        &self.upstream
    }

    /// Returns the exact terminal shape expected after the assembled host adapter.
    #[must_use]
    pub const fn expected_outcome(&self) -> &ProviderQuotaMetadataExpectedOutcomeV1 {
        &self.expected_outcome
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
            .field("upstream", &self.upstream)
            .field("expected_outcome", &self.expected_outcome)
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

/// The same canonical input with a valid requested slot the binding does not grant.
const MISMATCHED_SLOT_INPUT: ProviderCallInputV1 =
    ProviderCallInputV1 { requested_credential_slot: super::DIFFERENT_SLOT, ..INPUT };

const COMPLETED_EVIDENCE: ProviderQuotaMetadataExpectedEvidenceV1 =
    ProviderQuotaMetadataExpectedEvidenceV1 {
        resolver_calls: ProviderCallCountV1::One,
        transport_calls: ProviderCallCountV1::One,
    };

const UNREACHED_EVIDENCE: ProviderQuotaMetadataExpectedEvidenceV1 =
    ProviderQuotaMetadataExpectedEvidenceV1 {
        resolver_calls: ProviderCallCountV1::Zero,
        transport_calls: ProviderCallCountV1::Zero,
    };

const FIXTURES: &[ProviderQuotaMetadataFixtureV1] = &[
    ProviderQuotaMetadataFixtureV1 {
        case_id: ProviderQuotaMetadataCaseIdV1::AllFields,
        input: INPUT,
        upstream: ProviderQuotaMetadataUpstreamV1::Metadata(ALL_FIELDS),
        expected_outcome: ProviderQuotaMetadataExpectedOutcomeV1::Metadata(ALL_FIELDS),
        expected_evidence: COMPLETED_EVIDENCE,
    },
    ProviderQuotaMetadataFixtureV1 {
        case_id: ProviderQuotaMetadataCaseIdV1::NoFields,
        input: INPUT,
        upstream: ProviderQuotaMetadataUpstreamV1::Metadata(NO_FIELDS),
        expected_outcome: ProviderQuotaMetadataExpectedOutcomeV1::Metadata(NO_FIELDS),
        expected_evidence: COMPLETED_EVIDENCE,
    },
    ProviderQuotaMetadataFixtureV1 {
        case_id: ProviderQuotaMetadataCaseIdV1::CredentialSlotMismatch,
        input: MISMATCHED_SLOT_INPUT,
        upstream: ProviderQuotaMetadataUpstreamV1::NotReached,
        expected_outcome: ProviderQuotaMetadataExpectedOutcomeV1::Failure {
            code: ProviderCallFailureCodeV1::CredentialBindingMismatch,
        },
        expected_evidence: UNREACHED_EVIDENCE,
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
