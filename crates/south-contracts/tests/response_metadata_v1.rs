//! The two response-side metadata contracts added in South 0.20.0.
//!
//! They exist because a single mechanism could not serve two different readers. A host pacing
//! itself against rate limits needs values it may branch on, which demands a closed, reviewed set;
//! an operator asking what a provider actually answered needs breadth, which an allow-list can
//! never give — the header they want is the one nobody thought to approve. Splitting them keeps the
//! reviewed trust boundary intact on the programmable side while widening only what a human sees.

use south_contracts::{
    MAX_RESPONSE_DIAGNOSTIC_TOTAL_BYTES, MAX_RESPONSE_DIAGNOSTIC_VALUE_BYTES,
    MAX_RESPONSE_TRANSCRIPT_COUNT, MAX_RESPONSE_TRANSCRIPT_TOTAL_BYTES,
    MAX_RESPONSE_TRANSCRIPT_VALUE_BYTES, RESPONSE_DIAGNOSTIC_FIELD_COUNT,
    ResponseDiagnosticFieldV1, ResponseDiagnosticsV1, ResponseTranscriptV1, TransportErrorV1,
};
use static_assertions::assert_not_impl_any;
use std::fmt::Display;

// Neither type may be renderable into a user-facing string by accident; both carry upstream values.
assert_not_impl_any!(ResponseDiagnosticsV1: Display);
assert_not_impl_any!(ResponseTranscriptV1: Display);

const SENTINEL: &str = "response-metadata-sentinel";

#[test]
fn diagnostic_all_covers_every_variant_exactly_once() {
    assert_eq!(ResponseDiagnosticFieldV1::ALL.len(), RESPONSE_DIAGNOSTIC_FIELD_COUNT);

    let mut names: Vec<&str> = ResponseDiagnosticFieldV1::ALL
        .iter()
        .copied()
        .map(ResponseDiagnosticFieldV1::as_header_name)
        .collect();
    names.sort_unstable();
    let total = names.len();
    names.dedup();
    assert_eq!(names.len(), total, "ResponseDiagnosticFieldV1::ALL must not repeat a header name");
}

#[test]
fn diagnostic_header_names_are_frozen_and_lowercase() {
    let expected = [
        "x-request-id",
        "request-id",
        "anthropic-request-id",
        "openai-organization",
        "openai-processing-ms",
        "openai-version",
        "cf-ray",
        "server",
    ];

    for (field, name) in ResponseDiagnosticFieldV1::ALL.into_iter().zip(expected) {
        assert_eq!(field.as_header_name(), name);
        assert_eq!(name, name.to_ascii_lowercase());
    }
}

#[test]
fn diagnostic_round_trips_present_fields_and_leaves_absent_ones_absent() {
    let diagnostics = ResponseDiagnosticsV1::try_from_iter([
        (ResponseDiagnosticFieldV1::XRequestId, "req_abc".to_owned()),
        (ResponseDiagnosticFieldV1::CfRay, "8f-LHR".to_owned()),
    ])
    .expect("two approved fields should be accepted");

    assert_eq!(diagnostics.value(ResponseDiagnosticFieldV1::XRequestId), Some("req_abc"));
    assert_eq!(diagnostics.value(ResponseDiagnosticFieldV1::CfRay), Some("8f-LHR"));
    assert_eq!(diagnostics.value(ResponseDiagnosticFieldV1::Server), None);
    assert_eq!(diagnostics.present_field_count(), 2);

    // Absent fields are never synthesized into the iteration order.
    let seen: Vec<(&str, &str)> = diagnostics.iter().collect();
    assert_eq!(seen, [("x-request-id", "req_abc"), ("cf-ray", "8f-LHR")]);
}

#[test]
fn diagnostic_default_is_empty_and_allocates_nothing_observable() {
    let diagnostics = ResponseDiagnosticsV1::default();

    assert_eq!(diagnostics.present_field_count(), 0);
    assert_eq!(diagnostics.iter().count(), 0);
    for field in ResponseDiagnosticFieldV1::ALL {
        assert_eq!(diagnostics.value(field), None);
    }
}

#[test]
fn diagnostic_refuses_oversized_duplicate_and_illegal_values() {
    let oversized = "x".repeat(MAX_RESPONSE_DIAGNOSTIC_VALUE_BYTES + 1);
    let illegal = "invalid\rvalue".to_owned();

    for fields in [
        vec![(ResponseDiagnosticFieldV1::XRequestId, oversized)],
        vec![(ResponseDiagnosticFieldV1::XRequestId, illegal)],
        vec![
            (ResponseDiagnosticFieldV1::XRequestId, "first".to_owned()),
            (ResponseDiagnosticFieldV1::XRequestId, "second".to_owned()),
        ],
    ] {
        let error = ResponseDiagnosticsV1::try_from_iter(fields)
            .expect_err("invalid diagnostic metadata must be refused");
        assert_eq!(error, TransportErrorV1::ResponseMetadataInvalid);
    }
}

#[test]
fn diagnostic_refuses_a_set_past_the_combined_budget() {
    // Each value is individually legal; together they exceed the total.
    let value = "x".repeat(MAX_RESPONSE_DIAGNOSTIC_VALUE_BYTES);
    let fields: Vec<_> =
        ResponseDiagnosticFieldV1::ALL.into_iter().map(|f| (f, value.clone())).collect();
    let total: usize = fields.iter().map(|(_, v)| v.len()).sum();
    assert!(total <= MAX_RESPONSE_DIAGNOSTIC_TOTAL_BYTES, "the full set must sit at the budget");

    // One byte more anywhere must tip it over.
    let mut over = fields;
    over[0].1.push('x');
    let error = ResponseDiagnosticsV1::try_from_iter(over)
        .expect_err("a set past the combined budget must be refused");
    assert_eq!(error, TransportErrorV1::ResponseMetadataInvalid);
}

#[test]
fn diagnostic_debug_reveals_only_presence() {
    let diagnostics = ResponseDiagnosticsV1::try_from_iter([(
        ResponseDiagnosticFieldV1::XRequestId,
        SENTINEL.to_owned(),
    )])
    .expect("fixture should be valid");

    assert!(!format!("{diagnostics:?}").contains(SENTINEL));
}

#[test]
fn transcript_captures_broadly_and_preserves_upstream_order() {
    let transcript = ResponseTranscriptV1::capture([
        ("X-Request-Id", Some("req_abc")),
        ("openai-processing-ms", Some("412")),
        ("x-vendor-experimental", Some("on")),
    ]);

    // Header names are canonicalised to lowercase; the upstream's ordering is kept.
    assert_eq!(
        transcript.iter().collect::<Vec<_>>(),
        [
            ("x-request-id", "req_abc"),
            ("openai-processing-ms", "412"),
            ("x-vendor-experimental", "on"),
        ]
    );
    assert_eq!(transcript.len(), 3);
    assert!(!transcript.is_empty());
    assert!(!transcript.truncated());
}

#[test]
fn transcript_never_retains_a_credential_or_hop_by_hop_header() {
    let transcript = ResponseTranscriptV1::capture([
        ("set-cookie", Some("session=secret-value")),
        ("Set-Cookie2", Some("legacy=secret-value")),
        ("authorization", Some("Bearer secret-value")),
        ("proxy-authenticate", Some("Basic secret-value")),
        ("connection", Some("keep-alive")),
        ("transfer-encoding", Some("chunked")),
        ("upgrade", Some("h2c")),
        ("content-type", Some("application/json")),
    ]);

    assert_eq!(transcript.iter().collect::<Vec<_>>(), [("content-type", "application/json")]);
    assert!(!format!("{transcript:?}").contains("secret-value"));

    // A denied header is the contract working, not an incomplete capture: it must not be reported
    // as truncation, or a reader goes hunting for a header that is never coming.
    assert!(!transcript.truncated());
}

#[test]
fn transcript_is_case_insensitive_about_what_it_denies() {
    let transcript = ResponseTranscriptV1::capture([
        ("SET-COOKIE", Some("session=secret-value")),
        ("Authorization", Some("Bearer secret-value")),
    ]);

    assert!(transcript.is_empty());
    assert!(!format!("{transcript:?}").contains("secret-value"));
}

#[test]
fn transcript_bounds_count_and_marks_the_truncation() {
    let headers: Vec<(String, Option<String>)> = (0..=MAX_RESPONSE_TRANSCRIPT_COUNT)
        .map(|index| (format!("x-header-{index}"), Some("v".to_owned())))
        .collect();
    let transcript =
        ResponseTranscriptV1::capture(headers.iter().map(|(n, v)| (n.as_str(), v.as_deref())));

    assert_eq!(transcript.len(), MAX_RESPONSE_TRANSCRIPT_COUNT);
    assert!(transcript.truncated(), "dropping for want of room must be reported");
}

#[test]
fn transcript_bounds_total_bytes_and_marks_the_truncation() {
    let big = "x".repeat(MAX_RESPONSE_TRANSCRIPT_VALUE_BYTES);
    let headers: Vec<(String, Option<String>)> =
        (0..8).map(|index| (format!("x-big-{index}"), Some(big.clone()))).collect();
    let transcript =
        ResponseTranscriptV1::capture(headers.iter().map(|(n, v)| (n.as_str(), v.as_deref())));

    let retained: usize = transcript.iter().map(|(n, v)| n.len() + v.len()).sum();
    assert!(retained <= MAX_RESPONSE_TRANSCRIPT_TOTAL_BYTES);
    assert!(transcript.truncated());
}

#[test]
fn transcript_drops_malformed_values_rather_than_rendering_them_lossily() {
    let oversized = "x".repeat(MAX_RESPONSE_TRANSCRIPT_VALUE_BYTES + 1);
    let transcript = ResponseTranscriptV1::capture([
        ("x-non-utf8", None),
        ("x-oversized", Some(oversized.as_str())),
        ("x-fine", Some("kept")),
    ]);

    assert_eq!(transcript.iter().collect::<Vec<_>>(), [("x-fine", "kept")]);
    assert!(transcript.truncated());
}

#[test]
fn transcript_capture_is_total_for_an_empty_response() {
    let transcript = ResponseTranscriptV1::capture(std::iter::empty());

    assert!(transcript.is_empty());
    assert_eq!(transcript.len(), 0);
    assert!(!transcript.truncated());
}

#[test]
fn transcript_debug_reveals_only_shape() {
    let transcript = ResponseTranscriptV1::capture([("x-request-id", Some(SENTINEL))]);

    assert!(!format!("{transcript:?}").contains(SENTINEL));
}
