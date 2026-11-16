//! Normalization of provider quota metadata (design record
//! `docs/design/2026-11-16-provider-quota-normalization.md`).
//!
//! The nine quota metadata slots are vendor-named, but each name stands for a value format: the
//! `x-ratelimit-*-tokens` reset is a Go-style duration, the `anthropic-ratelimit-tokens-*` and
//! `anthropic-ratelimit-unified-*` resets are RFC 3339 instants. This module owns that knowledge
//! so hosts read limit, remaining, and an absolute reset instant without naming a slot.

use chrono::{DateTime, Duration, Utc};
use south_contracts::{ProviderQuotaMetadataFieldV1 as Field, ProviderQuotaMetadataV1};

/// Quota headroom read from one provider response.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NormalizedQuotaV1 {
    /// The window's limit, when the chosen family reports an integer.
    pub limit: Option<i64>,
    /// What remains in the window, when the chosen family reports an integer.
    pub remaining: Option<i64>,
    /// The absolute instant the window resets. An instant the upstream reports in the past is
    /// returned as is; the caller decides whether it is later than `now`.
    pub reset_at: Option<DateTime<Utc>>,
}

/// How one family writes its reset value.
#[derive(Clone, Copy)]
enum ResetFormat {
    Rfc3339,
    GoDuration,
}

/// Reads limit, remaining, and the reset instant from the family the metadata carries.
///
/// The `unified` slots win when their reset or limit is present, then the `tokens` slots when
/// theirs is, and otherwise the `x-ratelimit-*-tokens` slots are read. Only the chosen family is
/// read. `now` anchors a duration-valued reset; the function never reads the clock.
#[must_use]
pub fn normalize_provider_quota_v1(
    metadata: &ProviderQuotaMetadataV1,
    now: DateTime<Utc>,
) -> NormalizedQuotaV1 {
    let present = |field| metadata.value(field).is_some();
    let (limit, remaining, reset, format) = if present(Field::AnthropicRateLimitUnifiedReset)
        || present(Field::AnthropicRateLimitUnifiedLimit)
    {
        (
            Field::AnthropicRateLimitUnifiedLimit,
            Field::AnthropicRateLimitUnifiedRemaining,
            Field::AnthropicRateLimitUnifiedReset,
            ResetFormat::Rfc3339,
        )
    } else if present(Field::AnthropicRateLimitTokensReset)
        || present(Field::AnthropicRateLimitTokensLimit)
    {
        (
            Field::AnthropicRateLimitTokensLimit,
            Field::AnthropicRateLimitTokensRemaining,
            Field::AnthropicRateLimitTokensReset,
            ResetFormat::Rfc3339,
        )
    } else {
        (
            Field::XRateLimitLimitTokens,
            Field::XRateLimitRemainingTokens,
            Field::XRateLimitResetTokens,
            ResetFormat::GoDuration,
        )
    };

    let integer = |field| metadata.value(field).and_then(|value| value.trim().parse::<i64>().ok());
    let reset_at = metadata.value(reset).and_then(|raw| match format {
        ResetFormat::Rfc3339 => {
            DateTime::parse_from_rfc3339(raw.trim()).ok().map(|instant| instant.with_timezone(&Utc))
        }
        ResetFormat::GoDuration => go_duration_ceil_secs(raw)
            .and_then(Duration::try_seconds)
            .and_then(|delay| now.checked_add_signed(delay)),
    });

    NormalizedQuotaV1 { limit: integer(limit), remaining: integer(remaining), reset_at }
}

/// Reads a Go-style duration (`6m0s`, `1s`, `250ms`, `1h2m3s`) as whole seconds, rounded up.
///
/// Units are `h`, `m`, `s` and `ms`; `ms` is matched before `m`. Each component is truncated to
/// whole milliseconds before the components are summed. A total that is not positive, a missing
/// or unknown unit, a malformed number, or an overflow is `None`.
fn go_duration_ceil_secs(raw: &str) -> Option<i64> {
    const MILLIS_PER_HOUR: f64 = 3_600_000.0;
    const MILLIS_PER_MINUTE: f64 = 60_000.0;
    const MILLIS_PER_SECOND: f64 = 1_000.0;

    let mut rest = raw.trim();
    if rest.is_empty() {
        return None;
    }
    let mut total_millis: i64 = 0;
    while !rest.is_empty() {
        let number_len =
            rest.bytes().take_while(|byte| byte.is_ascii_digit() || *byte == b'.').count();
        if number_len == 0 {
            return None;
        }
        let (number, after_number) = rest.split_at(number_len);
        let value: f64 = number.parse().ok()?;
        let (unit_millis, after_unit) = if let Some(after) = after_number.strip_prefix("ms") {
            (1.0, after)
        } else if let Some(after) = after_number.strip_prefix('h') {
            (MILLIS_PER_HOUR, after)
        } else if let Some(after) = after_number.strip_prefix('m') {
            (MILLIS_PER_MINUTE, after)
        } else if let Some(after) = after_number.strip_prefix('s') {
            (MILLIS_PER_SECOND, after)
        } else {
            return None;
        };
        total_millis = total_millis.checked_add(whole_millis(value * unit_millis)?)?;
        rest = after_unit;
    }
    if total_millis <= 0 {
        return None;
    }
    Some(total_millis.checked_add(999)? / 1_000)
}

/// Truncates a non-negative finite millisecond count to an integer, or `None` when it does not
/// fit in `i64`.
fn whole_millis(millis: f64) -> Option<i64> {
    // 2^63 is exactly representable; every finite value below it truncates into range.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if !millis.is_finite() || !(0.0..LIMIT).contains(&millis) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the range check above keeps the value in i64 and truncation to whole milliseconds is the rule"
    )]
    let whole = millis as i64;
    Some(whole)
}
