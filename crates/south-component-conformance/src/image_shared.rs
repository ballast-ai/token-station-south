//! Request-view and response-view pieces the synchronous JSON image references of step I3-1 share
//! (`image-minimax`, `image-qwen`; design record `docs/design/2026-10-12-image-i3-1-minimax-and-qwen.md`).
//!
//! The host hands `prepare` a request view in which a declared or oversized string is a
//! `{"$south.blob": {"id", "bytes", "head"}}` placeholder (image record §6.2). A component that
//! forwards such a value writes `{"$south.ref": {"blob", "transform": "as_is"}}` in its template so
//! the host puts the original string back; a placeholder may never reach a template itself.

use serde_json::{Map, Value, json};
use south_contracts::image::{ImageCallContextV1, MeteringFormV1};
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::ComponentResultV1;

const BLOB_KEY: &str = "$south.blob";
const REF_KEY: &str = "$south.ref";
/// The longest upstream message an envelope carries for the operator.
const PROVIDER_MESSAGE_CHARS: usize = 256;

pub fn invalid(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}

pub fn capability(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Capability, 400, message)
}

pub fn internal(message: impl Into<String>) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::Internal, 500, message)
}

/// The blob id of a placeholder, when `value` is one.
fn blob_id(value: &Value) -> Option<&str> {
    value.get(BLOB_KEY).and_then(|blob| blob.get("id")).and_then(Value::as_str)
}

/// Whether `value` is a host placeholder for an elided string.
pub fn is_blob(value: &Value) -> bool {
    value.get(BLOB_KEY).is_some()
}

/// `value` with every placeholder replaced by an `as_is` reference to its blob, for a value the
/// component forwards as the client sent it.
pub fn with_refs(value: &Value) -> Value {
    match value {
        Value::Object(map) => blob_id(value).map_or_else(
            || {
                Value::Object(
                    map.iter()
                        .map(|(key, item)| (key.clone(), with_refs(item)))
                        .collect::<Map<_, _>>(),
                )
            },
            |id| json!({ REF_KEY: { "blob": id, "transform": "as_is" } }),
        ),
        Value::Array(items) => Value::Array(items.iter().map(with_refs).collect()),
        other => other.clone(),
    }
}

/// One `image_url` entry as the template value: a string, or a host-elided string as a reference.
/// `None` for any other type.
pub fn url_entry(value: &Value) -> Option<Value> {
    match value {
        Value::String(url) => Some(json!(url)),
        _ if blob_id(value).is_some() => Some(with_refs(value)),
        _ => None,
    }
}

/// The response view's string at an elided position: present and non-empty, as a placeholder of at
/// least one byte or (from a host that did not elide it) a plain string.
pub fn non_empty_inline(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.is_empty(),
        _ => value
            .get(BLOB_KEY)
            .and_then(|blob| blob.get("bytes"))
            .and_then(Value::as_u64)
            .is_some_and(|bytes| bytes > 0),
    }
}

/// The family check every function starts with.
pub fn serves(config: &ProviderConfig, family: &str) -> ComponentResultV1<()> {
    if config.provider == family {
        Ok(())
    } else {
        Err(capability(format!("this package serves the {family} family")))
    }
}

/// The host's required metering forms, when this package can report them all: `images` only.
pub fn images_only(
    context: &ImageCallContextV1,
    upstream: &str,
) -> ComponentResultV1<Vec<MeteringFormV1>> {
    if context.metering_required.is_empty() {
        return Err(capability("the host requires no metering form"));
    }
    if let Some(form) =
        context.metering_required.iter().find(|form| **form != MeteringFormV1::Images)
    {
        return Err(capability(format!("{upstream} image models cannot report {form:?} metering")));
    }
    Ok(context.metering_required.clone())
}

/// A reported image count: a non-negative whole number, or a string of ASCII digits, that fits a
/// `u32`. Anything else is not a count, and the evidence fact is `null`.
pub fn reported_count(value: &Value) -> Option<u32> {
    let count = match value {
        Value::String(text) if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => {
            text.parse::<u64>().ok()
        }
        Value::Number(_) => crate::bailian_image::whole_count(value),
        _ => None,
    }?;
    u32::try_from(count).ok()
}

/// The error code a non-2xx status maps to when the body says nothing more specific.
pub const fn status_code(status: u16) -> ErrorCode {
    match status {
        401 | 403 => ErrorCode::Auth,
        402 => ErrorCode::PaymentRequired,
        408 => ErrorCode::Timeout,
        429 => ErrorCode::RateLimit,
        500..=599 => ErrorCode::UpstreamUnavailable,
        _ => ErrorCode::InvalidRequest,
    }
}

/// The operator-facing summary of a code, never upstream content.
pub const fn code_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::Auth => "the upstream rejected the credential",
        ErrorCode::PaymentRequired => {
            "the upstream requires payment or the account is out of funds"
        }
        ErrorCode::RateLimit => "the upstream rate limited this request",
        ErrorCode::ContentPolicy => "the upstream refused on content-policy grounds",
        ErrorCode::Timeout => "the upstream did not answer in time",
        ErrorCode::UpstreamUnavailable => "the upstream is unavailable",
        ErrorCode::ProviderProtocolError => "the upstream reported a failure in a 2xx answer",
        _ => "the upstream refused the request as malformed",
    }
}

/// An upstream message capped at 256 characters, for the envelope's `provider_message`.
pub fn capped(message: &str) -> Option<String> {
    let message = message.trim();
    (!message.is_empty()).then(|| message.chars().take(PROVIDER_MESSAGE_CHARS).collect())
}

/// The outcome of a non-2xx round: `rejected` for a 4xx other than 408 (the upstream refused
/// before producing anything), `unknown` otherwise (image-azure's rule, boundary record §6.4).
pub fn non_2xx(status: u16, error: ErrorEnvelope) -> crate::ImageOutcomeV1 {
    if (400..500).contains(&status) && status != 408 {
        crate::ImageOutcomeV1::Rejected { error }
    } else {
        crate::ImageOutcomeV1::Unknown {
            reason: format!("upstream status {status}"),
            error: Some(error),
        }
    }
}

/// The status a response view carries, `0` when absent.
pub fn view_status(response: &Value) -> u16 {
    response["status"].as_u64().and_then(|status| u16::try_from(status).ok()).unwrap_or(0)
}

/// `{"arm": "bearer", "slot": …}` on the configured slot, when there is one.
pub fn bearer(config: &ProviderConfig) -> Option<Value> {
    config.auth.as_ref().map(|slot| json!({ "arm": "bearer", "slot": slot.as_str() }))
}

/// A whole count from 1 to `max` from the request's `n`, `1` when absent; refusals name the
/// upstream as the native arms do.
pub fn requested(n: Option<&Value>, max: u64, upstream: &str) -> ComponentResultV1<u64> {
    let Some(raw) = n else { return Ok(1) };
    let n = crate::bailian_image::whole_count(raw)
        .ok_or_else(|| invalid(format!("'n' must be an integer (got {raw})")))?;
    if (1..=max).contains(&n) {
        Ok(n)
    } else {
        Err(invalid(format!("'n' must be in 1..={max} for {upstream} image models (got {n})")))
    }
}
