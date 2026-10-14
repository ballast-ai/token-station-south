//! The `sse` half of the speech world's host-obligation vectors, end to end (speech record §6,
//! §11; `south-contracts/tests/vectors/speech-v1.json`).
//!
//! `south-contracts` may not link `south-host-grammars` (boundary record §13.13), so its own test
//! checks only the step from events to view. Here, where that crate is a dev-dependency, each
//! vector's raw `body` is decoded with `decode_sse_v1` — the host's decoder — and must give the
//! vector's `events`; the view built from those events must be the vector's `view`; and the
//! splitter the suite decodes `sse` fixture bodies with must give the same events.

use serde_json::Value;
use south_component_conformance::sse_split::SseSplitterV1;
use south_contracts::media::{
    MediaLimitsV1, PathPatternV1, SseEventFieldsV1, UpstreamRoundV1,
    build_media_sse_response_view_v1,
};
use south_host_grammars::decode_sse_v1;

fn vectors() -> Value {
    serde_json::from_str(include_str!("../../south-contracts/tests/vectors/speech-v1.json"))
        .expect("the vector file is JSON")
}

#[test]
fn each_sse_body_decodes_to_its_events_and_its_view() {
    let vectors = vectors();
    let cases = vectors["sse_view"].as_array().expect("cases");
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let body = case["body"].as_str().expect("body").as_bytes();
        let expected: Vec<(String, String)> = case["events"]
            .as_array()
            .expect("events")
            .iter()
            .map(|event| {
                (
                    event["event"].as_str().expect("event").to_owned(),
                    event["data"].as_str().expect("data").to_owned(),
                )
            })
            .collect();

        let decoded: Vec<(String, String)> = decode_sse_v1(body)
            .unwrap_or_else(|error| panic!("{name}: {error}"))
            .into_iter()
            .map(|event| (event.event().to_owned(), event.data().to_owned()))
            .collect();
        assert_eq!(decoded, expected, "{name}: decode_sse_v1");

        let mut splitter = SseSplitterV1::new();
        let mut split = splitter.push(body).expect("splits");
        split.extend(splitter.finish().expect("finishes"));
        let split: Vec<(String, String)> =
            split.into_iter().map(|frame| (frame.event, frame.data)).collect();
        assert_eq!(split, expected, "{name}: the suite's splitter");

        let declared: Vec<PathPatternV1> = case["declared"]
            .as_array()
            .expect("declared")
            .iter()
            .map(|path| PathPatternV1::parse(path.as_str().expect("path")).expect("valid"))
            .collect();
        let fields: Vec<SseEventFieldsV1<'_>> =
            decoded.iter().map(|(event, data)| SseEventFieldsV1 { event, data }).collect();
        let (view, _) = build_media_sse_response_view_v1(
            UpstreamRoundV1 { status: 200, headers: &[], body, content_type: None },
            &fields,
            &[],
            &declared,
            &MediaLimitsV1::V1,
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(view.to_json(), case["view"].as_str().expect("view"), "{name}: view");
    }
}

/// `contracts.media` 3 host obligations 1 and 2 (speech world batch V3-1 record §6): a host builds
/// a package's declarations once, from its gate ①-validated manifest (`DeclaredInstancesV1`), and
/// parses that package's descriptors with them; the result matches the golden vectors, and the
/// same descriptors parsed with no declarations match the vectors' `without_declarations`.
#[test]
fn a_host_parse_with_and_without_the_package_declarations_matches_the_vectors() {
    use south_component_conformance::DeclaredInstancesV1;
    use south_contracts::media::{MediaDescriptorErrorV1, MediaRequestDescriptorV1};
    use south_contracts::{ContractErrorV1, QueryStringV1};

    let manifest: south_provider_api::ComponentManifestV1 =
        serde_json::from_str(include_str!("../../../components/speech-elevenlabs/manifest.json"))
            .expect("the manifest parses");
    let declared = DeclaredInstancesV1::from_manifest(&manifest).expect("gate ① admits it");
    let media: serde_json::Value =
        serde_json::from_str(include_str!("../../south-contracts/tests/vectors/media-v1.json"))
            .expect("the vector file is JSON");
    let cases = media["descriptor_declared_query"]["cases"].as_array().expect("cases");
    assert!(!cases.is_empty());
    for case in cases {
        let name = case["name"].as_str().expect("name");
        // Every vector declares exactly what the package's manifest declares.
        let names: Vec<&str> = case["declarations"]
            .as_array()
            .expect("declarations")
            .iter()
            .map(|entry| entry["name"].as_str().expect("name"))
            .collect();
        let manifest_names: Vec<&str> = declared
            .query_parameters()
            .iter()
            .map(south_contracts::DeclaredQueryParameterV1::name)
            .collect();
        assert_eq!(names, manifest_names, "{name}");
        let text = case["descriptor"].as_str().expect("descriptor");
        for (key, declarations) in
            [("with_declarations", declared.query_parameters()), ("without_declarations", &[][..])]
        {
            let expected = &case[key];
            let parsed = MediaRequestDescriptorV1::parse_with_declared(
                text,
                &MediaLimitsV1::V1,
                declarations,
            );
            if let Some(error) = expected["error"].as_str() {
                let error = match error {
                    "unsanctioned_query" => MediaDescriptorErrorV1::UnsanctionedQuery,
                    "invalid_query_value" => {
                        MediaDescriptorErrorV1::Contract(ContractErrorV1::InvalidQueryValue)
                    }
                    "duplicate_query_parameter" => {
                        MediaDescriptorErrorV1::Contract(ContractErrorV1::DuplicateQueryParameter)
                    }
                    other => panic!("{name}: an unknown error word {other}"),
                };
                assert_eq!(parsed.map(|_| ()), Err(error), "{name}: {key}");
            } else {
                let descriptor = parsed.unwrap_or_else(|error| panic!("{name}: {key}: {error}"));
                assert_eq!(
                    descriptor.query().map(QueryStringV1::as_str),
                    expected["query"].as_str(),
                    "{name}: {key}"
                );
                assert_eq!(
                    Some(u64::from(descriptor.minimum_contract_version())),
                    expected["minimum_contract_version"].as_u64(),
                    "{name}: {key}"
                );
            }
        }
    }
}
