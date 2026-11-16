//! Quota metadata normalization: each slot family's value format, the family priority, and
//! malformed values (design record `docs/design/2026-11-16-provider-quota-normalization.md`).

use chrono::{DateTime, Duration, TimeZone, Utc};
use proptest::prelude::*;
use south_contracts::{ProviderQuotaMetadataFieldV1 as F, ProviderQuotaMetadataV1};
use south_core::{NormalizedQuotaV1, normalize_provider_quota_v1};

const ALL_FIELDS: [F; 9] = [
    F::XRateLimitLimitTokens,
    F::XRateLimitRemainingTokens,
    F::XRateLimitResetTokens,
    F::AnthropicRateLimitTokensLimit,
    F::AnthropicRateLimitTokensRemaining,
    F::AnthropicRateLimitTokensReset,
    F::AnthropicRateLimitUnifiedLimit,
    F::AnthropicRateLimitUnifiedRemaining,
    F::AnthropicRateLimitUnifiedReset,
];

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 9, 12, 0, 0).single().unwrap()
}

fn metadata(pairs: &[(F, &str)]) -> ProviderQuotaMetadataV1 {
    ProviderQuotaMetadataV1::try_from_iter(pairs.iter().map(|(f, v)| (*f, (*v).to_owned())))
        .unwrap()
}

fn normalize(pairs: &[(F, &str)]) -> NormalizedQuotaV1 {
    normalize_provider_quota_v1(&metadata(pairs), now())
}

fn duration_reset(raw: &str) -> Option<DateTime<Utc>> {
    normalize(&[(F::XRateLimitResetTokens, raw)]).reset_at
}

fn after(secs: i64) -> DateTime<Utc> {
    now() + Duration::seconds(secs)
}

#[test]
fn empty_metadata_normalizes_to_nothing() {
    assert_eq!(
        normalize_provider_quota_v1(&ProviderQuotaMetadataV1::default(), now()),
        NormalizedQuotaV1::default()
    );
}

#[test]
fn the_duration_family_reads_a_go_duration_relative_to_now() {
    let quota = normalize(&[
        (F::XRateLimitLimitTokens, "40000"),
        (F::XRateLimitRemainingTokens, "120"),
        (F::XRateLimitResetTokens, "6m0s"),
    ]);
    assert_eq!(quota.limit, Some(40_000));
    assert_eq!(quota.remaining, Some(120));
    assert_eq!(quota.reset_at, Some(after(360)));
}

#[test]
fn go_durations_accept_the_four_units_and_round_up_to_whole_seconds() {
    assert_eq!(duration_reset("1s"), Some(after(1)));
    assert_eq!(duration_reset("1h2m3s"), Some(after(3_723)));
    assert_eq!(duration_reset("1.5s"), Some(after(2)));
    assert_eq!(duration_reset("1500ms"), Some(after(2)));
    assert_eq!(duration_reset(" 2m "), Some(after(120)));
}

#[test]
fn milliseconds_are_matched_before_minutes() {
    // Read as minutes, 250ms would be a four-hour reset instead of a sub-second one.
    assert_eq!(duration_reset("250ms"), Some(after(1)));
}

#[test]
fn malformed_or_non_positive_durations_have_no_reset() {
    for raw in ["30", "", "soon", "1d", "-1s", "0s", "0.0001s", "s", "1.2.3s", "1s2", "1 s", "1S"] {
        assert_eq!(duration_reset(raw), None, "{raw:?}");
    }
}

#[test]
fn a_duration_beyond_the_representable_range_has_no_reset() {
    let huge = format!("{}h", "9".repeat(200));
    assert_eq!(duration_reset(&huge), None);
    assert_eq!(
        normalize_provider_quota_v1(
            &metadata(&[(F::XRateLimitResetTokens, "1s")]),
            DateTime::<Utc>::MAX_UTC
        )
        .reset_at,
        None
    );
}

#[test]
fn the_tokens_family_reads_an_rfc3339_instant() {
    let quota = normalize(&[
        (F::AnthropicRateLimitTokensLimit, "80000"),
        (F::AnthropicRateLimitTokensRemaining, "0"),
        (F::AnthropicRateLimitTokensReset, "2026-09-09T12:00:45Z"),
    ]);
    assert_eq!(quota.limit, Some(80_000));
    assert_eq!(quota.remaining, Some(0));
    assert_eq!(quota.reset_at, Some(after(45)));
}

#[test]
fn an_rfc3339_offset_is_converted_to_utc() {
    let quota = normalize(&[(F::AnthropicRateLimitTokensReset, " 2026-09-09T20:00:30+08:00 ")]);
    assert_eq!(quota.reset_at, Some(after(30)));
}

#[test]
fn a_past_instant_is_reported_as_is() {
    let quota = normalize(&[(F::AnthropicRateLimitUnifiedReset, "2026-09-09T11:59:00Z")]);
    assert_eq!(quota.reset_at, Some(after(-60)));
}

#[test]
fn a_malformed_instant_has_no_reset() {
    for raw in ["6m0s", "45", "2026-09-09", "tomorrow"] {
        assert_eq!(normalize(&[(F::AnthropicRateLimitTokensReset, raw)]).reset_at, None, "{raw:?}");
    }
}

#[test]
fn unified_wins_over_tokens() {
    let quota = normalize(&[
        (F::AnthropicRateLimitTokensLimit, "80000"),
        (F::AnthropicRateLimitTokensReset, "2026-09-09T12:09:00Z"),
        (F::AnthropicRateLimitUnifiedLimit, "5"),
        (F::AnthropicRateLimitUnifiedRemaining, "1"),
        (F::AnthropicRateLimitUnifiedReset, "2026-09-09T12:00:30Z"),
    ]);
    assert_eq!(quota.limit, Some(5));
    assert_eq!(quota.remaining, Some(1));
    assert_eq!(quota.reset_at, Some(after(30)));
}

#[test]
fn a_family_is_chosen_by_its_reset_or_limit_and_only_that_family_is_read() {
    // The unified limit alone selects unified, so the tokens reset is not read.
    let quota = normalize(&[
        (F::AnthropicRateLimitUnifiedLimit, "5"),
        (F::AnthropicRateLimitTokensReset, "2026-09-09T12:09:00Z"),
    ]);
    assert_eq!(quota, NormalizedQuotaV1 { limit: Some(5), ..NormalizedQuotaV1::default() });

    // A remaining value alone does not select a family.
    let quota = normalize(&[
        (F::AnthropicRateLimitUnifiedRemaining, "1"),
        (F::XRateLimitLimitTokens, "100"),
    ]);
    assert_eq!(quota, NormalizedQuotaV1 { limit: Some(100), ..NormalizedQuotaV1::default() });

    // The tokens reset alone selects tokens over the duration family.
    let quota = normalize(&[
        (F::AnthropicRateLimitTokensReset, "2026-09-09T12:00:10Z"),
        (F::XRateLimitLimitTokens, "100"),
        (F::XRateLimitResetTokens, "1s"),
    ]);
    assert_eq!(
        quota,
        NormalizedQuotaV1 { reset_at: Some(after(10)), ..NormalizedQuotaV1::default() }
    );
}

#[test]
fn malformed_integers_are_absent() {
    let quota = normalize(&[
        (F::XRateLimitLimitTokens, "1e3"),
        (F::XRateLimitRemainingTokens, "99999999999999999999"),
    ]);
    assert_eq!(quota.limit, None);
    assert_eq!(quota.remaining, None);
}

fn any_now() -> impl Strategy<Value = DateTime<Utc>> {
    (DateTime::<Utc>::MIN_UTC.timestamp()..=DateTime::<Utc>::MAX_UTC.timestamp())
        .prop_map(|secs| DateTime::from_timestamp(secs, 0).unwrap())
}

proptest! {
    #[test]
    fn normalization_never_panics(
        values in proptest::collection::vec(proptest::option::of("[ -~]{0,64}"), 9),
        now in any_now(),
    ) {
        let pairs = ALL_FIELDS
            .iter()
            .zip(values)
            .filter_map(|(field, value)| value.map(|value| (*field, value)));
        if let Ok(metadata) = ProviderQuotaMetadataV1::try_from_iter(pairs) {
            let quota = normalize_provider_quota_v1(&metadata, now);
            let duration_family = metadata.value(F::AnthropicRateLimitUnifiedReset).is_none()
                && metadata.value(F::AnthropicRateLimitUnifiedLimit).is_none()
                && metadata.value(F::AnthropicRateLimitTokensReset).is_none()
                && metadata.value(F::AnthropicRateLimitTokensLimit).is_none();
            if duration_family && let Some(reset_at) = quota.reset_at {
                prop_assert!(reset_at > now);
            }
        }
    }

    #[test]
    fn composed_durations_give_the_expected_instant(
        hours in 0_i64..1_000,
        minutes in 0_i64..1_000,
        seconds in 0_i64..1_000,
    ) {
        let total = hours * 3_600 + minutes * 60 + seconds;
        let expected = (total > 0).then(|| now() + Duration::seconds(total));
        prop_assert_eq!(duration_reset(&format!("{hours}h{minutes}m{seconds}s")), expected);
    }

    #[test]
    fn millisecond_durations_round_up(millis in 0_i64..10_000_000) {
        let secs = (millis + 999) / 1_000;
        let expected = (secs > 0).then(|| now() + Duration::seconds(secs));
        prop_assert_eq!(duration_reset(&format!("{millis}ms")), expected);
    }

    #[test]
    fn integers_round_trip_with_surrounding_whitespace(value: i64) {
        let raw = format!(" {value}\t");
        let quota = normalize(&[(F::XRateLimitLimitTokens, &raw)]);
        prop_assert_eq!(quota.limit, Some(value));
    }
}
