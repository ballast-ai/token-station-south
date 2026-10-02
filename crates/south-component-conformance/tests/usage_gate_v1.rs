//! Gate ②'s usage checks bite (B1, `docs/design/2026-09-30-host-zero-vendor-boundary.md`
//! §6.2 items 2–4 and 6).
//!
//! Each check is run against a component built to break exactly what it
//! guards, over the shipped `OpenAI`-compatible pack: a check that never turned
//! red on a wrong component would admit one.

use std::path::Path;

use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, FixtureErrorV1, FixturePackV1, ProviderComponentV1, StreamParserV1,
    run_provider_component_suite_v1, run_provider_component_suite_v1_with_usage_evidence,
};
use south_provider_api::{ComponentMetadataV1, UsageEvidenceV1};
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorEnvelope, Extensions, HttpRequestDescriptor, HttpResponseParts,
    ModelCapability, ProviderConfig, StreamEvent, Usage,
};

fn shipped_pack() -> FixturePackV1 {
    FixturePackV1::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"))
        .expect("the shipped fixture pack loads")
}

/// How a wrapper bends the reference's answers.
#[derive(Clone, Copy)]
enum Bend {
    /// The pre-B1 behaviour: a response whose usage cannot be read settles as
    /// zero instead of failing.
    DefaultMissingUsageToZero,
    /// An honest `absent` package: zero usage, never a usage event.
    NeverReportUsage,
    /// Reports more reasoning than output.
    ReasoningExceedsOutput,
}

struct Bent(Bend);

impl ProviderComponentV1 for Bent {
    fn metadata(&self) -> ComponentMetadataV1 {
        OpenAiCompatibleReferenceV1.metadata()
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ModelCapability>> {
        OpenAiCompatibleReferenceV1.model_capabilities(config)
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        OpenAiCompatibleReferenceV1.build_http_request(request, config)
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        let parsed = OpenAiCompatibleReferenceV1.parse_response(parts);
        match self.0 {
            Bend::DefaultMissingUsageToZero => parsed.or_else(|_| {
                Ok(ChatResponse {
                    id: String::new(),
                    model: String::new(),
                    choices: Vec::new(),
                    usage: Usage::default(),
                    extensions: Extensions::new(),
                })
            }),
            Bend::NeverReportUsage => parsed.map(|mut response| {
                response.usage = Usage::default();
                response
            }),
            Bend::ReasoningExceedsOutput => parsed.map(|mut response| {
                response.usage.reasoning_tokens = response.usage.output_tokens + 1;
                response
            }),
        }
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        OpenAiCompatibleReferenceV1.map_provider_error(parts)
    }

    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        Box::new(BentStream { inner: OpenAiCompatibleReferenceV1.stream_parser(), bend: self.0 })
    }
}

struct BentStream {
    inner: Box<dyn StreamParserV1>,
    bend: Bend,
}

impl StreamParserV1 for BentStream {
    fn parse_chunk(&mut self, chunk: &[u8]) -> ComponentResultV1<Vec<StreamEvent>> {
        let events = self.inner.parse_chunk(chunk)?;
        Ok(match self.bend {
            Bend::NeverReportUsage => events
                .into_iter()
                .filter(|event| !matches!(event, StreamEvent::Usage { .. }))
                .collect(),
            Bend::DefaultMissingUsageToZero | Bend::ReasoningExceedsOutput => events,
        })
    }
}

fn failed(report: &south_component_conformance::ReportV1, check: CheckV1) -> Vec<(String, String)> {
    report
        .failures()
        .filter(|outcome| outcome.check == check)
        .map(|outcome| (outcome.case.clone(), outcome.detail().to_owned()))
        .collect()
}

#[test]
fn a_component_that_defaults_missing_usage_to_zero_is_refused() {
    let report =
        run_provider_component_suite_v1(&Bent(Bend::DefaultMissingUsageToZero), &shipped_pack());
    let never_defaulted = failed(&report, CheckV1::UsageNeverDefaulted);
    assert!(never_defaulted.iter().any(|(case, _)| case == "provider.response.usage"), "{report}");
    let fixture_match = failed(&report, CheckV1::FixtureMatch);
    assert!(
        fixture_match.iter().any(|(case, _)| case == "provider.response.missing-usage"),
        "{report}"
    );
}

#[test]
fn the_reference_passes_every_usage_check() {
    let report = run_provider_component_suite_v1(&OpenAiCompatibleReferenceV1, &shipped_pack());
    for check in [
        CheckV1::UsageRows,
        CheckV1::UsageNeverDefaulted,
        CheckV1::UsagePartition,
        CheckV1::Coverage,
    ] {
        assert!(failed(&report, check).is_empty(), "{report}");
        assert!(
            report.outcomes().iter().any(|outcome| outcome.check == check),
            "`{check}` never ran"
        );
    }
}

#[test]
fn a_pack_without_the_usage_rows_fails_coverage_by_name() {
    const ROWS: [&str; 5] = [
        "provider.response.usage",
        "provider.response.missing-usage",
        "provider.response.cached-usage",
        "provider.stream.usage-terminal",
        "provider.stream.no-usage",
    ];
    let pack = FixturePackV1::from_cases(
        shipped_pack()
            .cases()
            .iter()
            .filter(|case| !ROWS.contains(&case.name.as_str()))
            .cloned()
            .collect(),
    );
    let report = run_provider_component_suite_v1(&OpenAiCompatibleReferenceV1, &pack);
    let missing: Vec<String> =
        failed(&report, CheckV1::Coverage).into_iter().map(|(case, _)| case).collect();
    assert_eq!(missing, ROWS.map(str::to_owned), "{report}");

    // An `absent` package owes no named usage rows.
    let report = run_provider_component_suite_v1_with_usage_evidence(
        &Bent(Bend::NeverReportUsage),
        &pack,
        UsageEvidenceV1::Absent,
    );
    assert!(failed(&report, CheckV1::Coverage).is_empty(), "{report}");
}

#[test]
fn a_usage_row_that_does_not_show_its_name_fails() {
    let cases = shipped_pack()
        .cases()
        .iter()
        .cloned()
        .map(|mut case| {
            match case.name.as_str() {
                // The pointer is what lets `UsageNeverDefaulted` run at all.
                "provider.response.usage" => case.usage_pointer = None,
                // A stream that reports usage is not the no-usage row.
                "provider.stream.no-usage" => {
                    case.expected = serde_json::json!([
                        {"type": "usage", "usage": {"input_tokens": 1, "output_tokens": 1,
                         "cache_read_tokens": 0, "cache_write_tokens": 0,
                         "reasoning_tokens": 0}},
                        {"type": "done"}
                    ]);
                }
                _ => {}
            }
            case
        })
        .collect();
    let report = run_provider_component_suite_v1(
        &OpenAiCompatibleReferenceV1,
        &FixturePackV1::from_cases(cases),
    );
    let rows: Vec<String> =
        failed(&report, CheckV1::UsageRows).into_iter().map(|(case, _)| case).collect();
    assert_eq!(rows, ["provider.response.usage", "provider.stream.no-usage"], "{report}");
}

#[test]
fn an_absent_package_that_reports_usage_is_refused_and_an_honest_one_passes() {
    let liar = run_provider_component_suite_v1_with_usage_evidence(
        &OpenAiCompatibleReferenceV1,
        &shipped_pack(),
        UsageEvidenceV1::Absent,
    );
    let lied: Vec<String> = failed(&liar, CheckV1::AbsentFamilyEmitsNoUsage)
        .into_iter()
        .map(|(case, _)| case)
        .collect();
    assert!(lied.contains(&"provider.response.cached-usage".to_owned()), "{liar}");
    assert!(lied.contains(&"provider.stream.usage-terminal".to_owned()), "{liar}");

    let honest = run_provider_component_suite_v1_with_usage_evidence(
        &Bent(Bend::NeverReportUsage),
        &shipped_pack(),
        UsageEvidenceV1::Absent,
    );
    assert!(failed(&honest, CheckV1::AbsentFamilyEmitsNoUsage).is_empty(), "{honest}");
    assert!(
        honest.outcomes().iter().any(|outcome| outcome.check == CheckV1::AbsentFamilyEmitsNoUsage),
        "the check never ran"
    );
}

#[test]
fn reasoning_beyond_output_breaks_the_partition() {
    let report =
        run_provider_component_suite_v1(&Bent(Bend::ReasoningExceedsOutput), &shipped_pack());
    assert!(
        failed(&report, CheckV1::UsagePartition)
            .iter()
            .any(|(case, detail)| case == "provider.response.usage"
                && detail.contains("reasoning_tokens")),
        "{report}"
    );
}

#[test]
fn malformed_usage_metadata_is_a_package_error() {
    let directory =
        std::env::temp_dir().join(format!("south-usage-gate-test-{}", std::process::id()));
    let write = |name: &str, body: &str| {
        std::fs::write(directory.join(name), body).expect("fixture writes");
    };
    let load_error = || FixturePackV1::load(&directory).expect_err("the pack must be refused");
    for (case, meta) in [
        // A misspelt key must not silently disable the check it enables.
        ("provider.response.a", r#"{"usage_pointr": "/usage"}"#),
        ("provider.response.a", r#"{"usage_pointer": ""}"#),
        ("provider.stream.a", r#"{"usage_pointer": "/usage"}"#),
    ] {
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("scratch creates");
        write(&format!("{case}.input.json"), "{}");
        write(&format!("{case}.expected.json"), "{}");
        write(&format!("{case}.meta.json"), meta);
        assert!(matches!(load_error(), FixtureErrorV1::InvalidMeta { .. }), "{case}: {meta}");
    }
    std::fs::remove_dir_all(&directory).ok();
}
