use proptest::prelude::*;
use south_contracts::{
    HeaderPolicyError, MAX_PROVIDER_HEADER_COUNT, MAX_PROVIDER_HEADER_NAME_BYTES,
    MAX_PROVIDER_HEADER_TOTAL_BYTES, MAX_PROVIDER_HEADER_VALUE_BYTES, SafeHeaders,
};

#[test]
fn safe_headers_normalize_names() {
    let headers = SafeHeaders::try_from_iter([
        ("Content-Type", "application/json"),
        ("X-Request-ID", "request-123"),
    ])
    .unwrap();

    assert_eq!(headers.get("content-type"), Some("application/json"));
    assert_eq!(headers.get("X-REQUEST-ID"), Some("request-123"));
}

#[test]
fn safe_headers_reject_reserved_headers_without_exposing_values() {
    let secret = "must-not-appear";
    let error = SafeHeaders::try_from_iter([("Authorization", secret)]).unwrap_err();

    assert_eq!(error, HeaderPolicyError::ReservedHeader);
    assert!(!error.to_string().contains(secret));
    assert!(!error.to_string().contains("authorization"));
    assert!(!format!("{error:?}").contains(secret));
    assert!(!format!("{error:?}").contains("authorization"));
    assert_eq!(error.code(), "RESERVED_HEADER_FORBIDDEN");
}

#[test]
fn safe_headers_reject_the_complete_version_one_reserved_set() {
    let reserved = [
        "api-key",
        "authorization",
        "connection",
        "content-length",
        "cookie",
        "expect",
        "host",
        "keep-alive",
        "ocp-apim-subscription-key",
        "proxy-connection",
        "proxy-authorization",
        "set-cookie",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "user-agent",
        "x-api-key",
        "x-goog-api-key",
        "xi-api-key",
    ];

    for name in reserved {
        let error = SafeHeaders::try_from_iter([(name, "must-not-appear")]).unwrap_err();
        assert_eq!(error.code(), "RESERVED_HEADER_FORBIDDEN");
        assert!(!error.to_string().contains("must-not-appear"));
    }
}

#[test]
fn safe_headers_reject_duplicates_after_name_normalization() {
    let error =
        SafeHeaders::try_from_iter([("X-Test", "first"), ("x-test", "second")]).unwrap_err();

    assert_eq!(error, HeaderPolicyError::DuplicateHeader);
    assert!(!error.to_string().contains("x-test"));
    assert!(!error.to_string().contains("first"));
    assert!(!error.to_string().contains("second"));
    assert!(!format!("{error:?}").contains("x-test"));
}

#[test]
fn safe_headers_reject_invalid_values_without_exposing_them() {
    let invalid_value = "must-not-appear\r\ninjected: true";
    let error = SafeHeaders::try_from_iter([("x-test", invalid_value)]).unwrap_err();

    assert_eq!(error, HeaderPolicyError::InvalidValue);
    assert!(!error.to_string().contains("x-test"));
    assert!(!error.to_string().contains(invalid_value));
    assert!(!format!("{error:?}").contains("x-test"));
    assert!(!format!("{error:?}").contains(invalid_value));
}

#[test]
fn safe_headers_debug_output_contains_no_names_or_values() {
    let headers = SafeHeaders::try_from_iter([("x-private-marker", "must-not-appear")]).unwrap();
    let debug = format!("{headers:?}");

    assert!(!debug.contains("x-private-marker"));
    assert!(!debug.contains("must-not-appear"));
    assert!(debug.contains("count"));
}

#[test]
fn safe_headers_reject_excessive_header_count() {
    let headers: Vec<_> =
        (0..=MAX_PROVIDER_HEADER_COUNT).map(|index| (format!("x-test-{index}"), "value")).collect();

    let error = SafeHeaders::try_from_iter(headers).unwrap_err();
    assert_eq!(error, HeaderPolicyError::TooManyHeaders);
}

#[test]
fn safe_headers_reject_excessive_name_length() {
    let name = format!("x{}", "a".repeat(MAX_PROVIDER_HEADER_NAME_BYTES));

    let error = SafeHeaders::try_from_iter([(name, "value")]).unwrap_err();
    assert_eq!(error, HeaderPolicyError::NameTooLong);
}

#[test]
fn safe_headers_reject_excessive_value_length() {
    let value = "a".repeat(MAX_PROVIDER_HEADER_VALUE_BYTES + 1);

    let error = SafeHeaders::try_from_iter([("x-test", value)]).unwrap_err();
    assert_eq!(error, HeaderPolicyError::ValueTooLong);
}

#[test]
fn safe_headers_reject_excessive_total_size() {
    let value = "a".repeat(MAX_PROVIDER_HEADER_VALUE_BYTES);
    let header_count = (MAX_PROVIDER_HEADER_TOTAL_BYTES / MAX_PROVIDER_HEADER_VALUE_BYTES) + 1;
    let headers: Vec<_> = (0..header_count).map(|index| (format!("x-{index}"), &value)).collect();

    let error = SafeHeaders::try_from_iter(headers).unwrap_err();
    assert_eq!(error, HeaderPolicyError::TotalSizeExceeded);
}

proptest! {
    #[test]
    fn reserved_header_detection_is_ascii_case_insensitive(
        upper in proptest::collection::vec(any::<bool>(), "authorization".len()),
    ) {
        let name: String = "authorization"
            .chars()
            .zip(upper)
            .map(|(character, uppercase)| {
                if uppercase { character.to_ascii_uppercase() } else { character }
            })
            .collect();
        let secret = "property-secret";

        let error = SafeHeaders::try_from_iter([(name, secret)]).unwrap_err();

        prop_assert_eq!(error.code(), "RESERVED_HEADER_FORBIDDEN");
        prop_assert!(!error.to_string().contains(secret));
    }
}
