//! The `bedrock-mantle` family of `provider-openai-responses`: AWS Bedrock's OpenAI-compatible
//! `bedrock-mantle` host, its endpoint template, its fixture pack, and the two `[DONE]` facts the
//! design record turns on.
//!
//! Design record: `docs/design/2026-10-15-bedrock-mantle.md` (accepted under lv's 2026-10-15
//! directive). The family's whole behaviour is the `openai-responses` family's; what is its own is
//! the endpoint template the host fills from the row's region (§3), and the rows transcribed from
//! the host's native Mantle tests (§6).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::reference_openai_responses::{
    FAMILY, MANTLE_FAMILY, OpenAiResponsesReferenceV1,
};
use south_component_conformance::{
    CheckV1, ComponentResultV1, FixturePackV1, ProviderComponentV1, ProviderFamilyV1, VerdictV1,
    accepts_manifest, run_provider_component_suite_v1_for_manifest,
};
use south_north_codec::responses::{ResponsesRequestOptions, chat_request_from_responses};
use south_provider_api::{ComponentManifestV1, EndpointValuesErrorV1, NorthProtocolV1};
use token_station_protocol::{ChatRequest, ErrorCode, ProviderConfig, StreamEvent};

#[path = "support/openai_responses_packs.rs"]
mod packs;

/// The id the host's native Mantle tests give the row's upstream model.
const HOST_MODEL: &str = "openai.gpt-oss-120b-1:0";

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn shipped_manifest() -> ComponentManifestV1 {
    let path = repo_root().join("components/provider-openai-responses/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn mantle_pack() -> FixturePackV1 {
    packs::load(packs::MANTLE_PACK)
}

fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

fn config_of(input: &Value) -> ProviderConfig {
    serde_json::from_value(input["provider_config"].clone()).unwrap()
}

fn request_of(input: &Value) -> ChatRequest {
    serde_json::from_value(input["chat_request"].clone()).unwrap()
}

fn built(
    component: &dyn ProviderComponentV1,
    request: &ChatRequest,
    config: &ProviderConfig,
) -> Result<Value, Value> {
    component
        .build_http_request(request, config)
        .map(|descriptor| serde_json::to_value(descriptor).unwrap())
        .map_err(|error| serde_json::to_value(error).unwrap())
}

/// Every event a parser gives for `chunks`, then for the clean end of the stream.
fn events(component: &dyn ProviderComponentV1, chunks: &[&str]) -> ComponentResultV1<Value> {
    let mut parser = component.stream_parser();
    let mut out: Vec<StreamEvent> = Vec::new();
    for chunk in chunks {
        out.extend(parser.parse_chunk(chunk.as_bytes())?);
    }
    out.extend(parser.finish()?);
    Ok(serde_json::to_value(out).unwrap())
}

fn chunks_of(input: &Value) -> Vec<&str> {
    input["chunks"].as_array().unwrap().iter().map(|chunk| chunk.as_str().unwrap()).collect()
}

// -- the pack ------------------------------------------------------------------

/// Record §6: the rows the pack must carry. A pack can pass while having quietly lost a case.
#[test]
fn the_mantle_pack_carries_every_decided_row() {
    let names: BTreeSet<String> =
        mantle_pack().cases().iter().map(|case| case.name.clone()).collect();
    for row in [
        "provider.request.mantle-host-pinned",
        "provider.request.mantle-host-pinned-stream",
        "provider.request.mantle-region-us-west-2",
        "provider.request.mantle-request-fields",
        "provider.request.mantle-tools-and-reasoning-effort",
        "provider.request.mantle-stateful-fields-never-sent",
        "provider.request.mantle-reasoning-replay-undeclared",
        "provider.response.mantle-host-pinned",
        "provider.response.mantle-reasoning-text-and-cache",
        "provider.stream.mantle-host-pinned",
        "provider.stream.mantle-trailing-done-refused",
        "provider.stream.mantle-reasoning-text",
    ] {
        assert!(names.contains(row), "the Mantle pack lost `{row}`");
    }
}

/// Every Mantle row is `mantle-*`; every request row is for `bedrock-mantle`, declares the region
/// the host passes, and has the base URL the host fills from it; the pack holds no credential row.
#[test]
fn the_mantle_pack_holds_mantle_rows_only_with_the_filled_endpoint() {
    let manifest = shipped_manifest();
    let pack = mantle_pack();
    let mut requests = 0;
    for case in pack.cases() {
        let case_part = case.name.splitn(3, '.').nth(2).unwrap();
        assert!(case_part.starts_with("mantle-"), "{}", case.name);
        if case.family != ProviderFamilyV1::Request {
            continue;
        }
        requests += 1;
        let config = &case.input["provider_config"];
        assert_eq!(config["provider"], MANTLE_FAMILY, "{}", case.name);
        let declared: BTreeMap<String, String> =
            serde_json::from_value(config["declared"].clone()).unwrap();
        assert_eq!(declared.keys().collect::<Vec<_>>(), ["region"], "{}", case.name);
        let filled = manifest.fill_endpoint(MANTLE_FAMILY, &declared).unwrap().unwrap();
        assert_eq!(config["base_url"], filled.as_str(), "{}", case.name);
    }
    assert!(requests >= 7, "too few request rows");
    assert!(pack.credentials().cases().is_empty(), "the family has no credentials section");
}

/// Gate ② over the whole package pack runs `UsageNeverDefaulted` on every Mantle row with a
/// sidecar, and passes it.
#[test]
fn every_mantle_row_with_a_usage_sidecar_runs_usage_never_defaulted() {
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiResponsesReferenceV1,
        &packs::package_pack(),
        &shipped_manifest(),
    );
    let mut judged = 0;
    for case in mantle_pack().cases() {
        if case.usage_chunk.is_none() && case.usage_pointer.is_none() {
            continue;
        }
        assert!(
            report.outcomes().iter().any(|outcome| outcome.check == CheckV1::UsageNeverDefaulted
                && outcome.case == case.name
                && outcome.verdict == VerdictV1::Passed),
            "{}: {report}",
            case.name
        );
        judged += 1;
    }
    assert_eq!(judged, 4, "two response rows and two stream rows carry a sidecar");
}

// -- the manifest ------------------------------------------------------------------

/// Record §3: the family's declarations are `openai-responses`'s plus an endpoint template on the
/// native host and one required `aws_region` key; no credentials section applies to it.
#[test]
fn the_family_declares_the_native_host_and_otherwise_what_openai_responses_declares() {
    let manifest = shipped_manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert!(manifest.providers.iter().any(|family| family == MANTLE_FAMILY));
    assert_eq!(
        manifest.endpoint.get(MANTLE_FAMILY).map(String::as_str),
        Some("https://bedrock-mantle.{region}.api.aws")
    );
    let keys = &manifest.config_schema[MANTLE_FAMILY];
    assert_eq!(keys.keys().collect::<Vec<_>>(), ["region"]);
    assert_eq!(
        serde_json::to_value(&keys["region"].syntax).unwrap(),
        json!("aws_region"),
        "the host's region syntax"
    );
    assert!(keys["region"].required && keys["region"].default.is_none());
    assert_eq!(manifest.declared_keys(MANTLE_FAMILY), BTreeSet::from(["region"]));
    assert!(manifest.credentials_for(MANTLE_FAMILY).is_none(), "a static Bedrock API key");
    assert_eq!(manifest.request_facts.get(MANTLE_FAMILY), manifest.request_facts.get(FAMILY));
    assert_eq!(
        manifest.immutable_body_paths.get(MANTLE_FAMILY),
        manifest.immutable_body_paths.get(FAMILY)
    );
    assert_eq!(manifest.north_passthrough.get(MANTLE_FAMILY), Some(&NorthProtocolV1::Responses));
}

/// Record §4: the template gives exactly the host's `bedrock_mantle_base(region)`
/// (server `engine/bedrock.rs:130-133`) for the regions the host's URL test pins, and the URL the
/// component then builds is the host's `build_bedrock_mantle_url(region, Responses)` (:164-170).
/// An operator-entered base URL is admitted only when it is that host for an AWS region.
#[test]
fn the_template_is_the_native_mantle_host_for_every_region() {
    let manifest = shipped_manifest();
    let input = packs::load(packs::MANTLE_PACK)
        .cases()
        .iter()
        .find(|case| case.name == "provider.request.mantle-host-pinned")
        .unwrap()
        .input
        .clone();
    for region in ["us-east-1", "us-west-2", "eu-west-1", "ap-northeast-1", "us-gov-west-1"] {
        let base = format!("https://bedrock-mantle.{region}.api.aws");
        let filled = manifest.fill_endpoint(MANTLE_FAMILY, &values(&[("region", region)]));
        assert_eq!(filled, Ok(Some(base.clone())), "{region}");
        assert_eq!(
            manifest.endpoint_values(MANTLE_FAMILY, &base),
            Ok(values(&[("region", region)])),
            "{region}"
        );
        let mut config = config_of(&input);
        config.base_url = serde_json::from_value(json!(base)).unwrap();
        let descriptor = built(&OpenAiResponsesReferenceV1, &request_of(&input), &config).unwrap();
        assert_eq!(descriptor["url"], format!("{base}/v1/responses"), "{region}");
    }
    // No region, or one outside the syntax, fills nothing.
    assert!(manifest.fill_endpoint(MANTLE_FAMILY, &values(&[])).is_err());
    assert!(manifest.fill_endpoint(MANTLE_FAMILY, &values(&[("region", "US East")])).is_err());
    // The sibling runtime host and other hosts are not this endpoint.
    for foreign in [
        "https://bedrock-runtime.us-east-1.amazonaws.com",
        "https://bedrock-mantle.us-east-1.amazonaws.com",
        "https://api.openai.com/v1",
        "http://bedrock-mantle.us-east-1.api.aws",
    ] {
        assert!(!manifest.endpoint_admits(MANTLE_FAMILY, foreign), "{foreign}");
        assert_eq!(
            manifest.endpoint_values(MANTLE_FAMILY, foreign),
            Err(EndpointValuesErrorV1::NotThisEndpoint),
            "{foreign}"
        );
    }
}

// -- requests --------------------------------------------------------------------

/// Record §3: on the same base URL, `bedrock-mantle` builds exactly what `openai-responses` builds
/// — or refuses exactly what it refuses — for every request row of the package pack.
#[test]
fn the_family_builds_what_openai_responses_builds_for_every_request_row() {
    let mut judged = 0;
    for case in packs::package_pack().cases() {
        if case.family != ProviderFamilyV1::Request {
            continue;
        }
        let mut config = config_of(&case.input);
        if config.provider == "openai-codex" {
            continue;
        }
        let request = request_of(&case.input);
        config.provider = FAMILY.to_owned();
        let as_responses = built(&OpenAiResponsesReferenceV1, &request, &config);
        config.provider = MANTLE_FAMILY.to_owned();
        let as_mantle = built(&OpenAiResponsesReferenceV1, &request, &config);
        assert_eq!(as_mantle, as_responses, "{}", case.name);
        judged += 1;
    }
    assert!(judged >= 35, "too few request rows judged: {judged}");
}

/// The host's native Mantle tests send `responses_request(stream)`
/// (server `gateway/tests/responses_providers.rs:93-95`); with the northbound name replaced by the
/// row's upstream id, as the host does before building, the north codec's output is the two
/// host-pinned rows' IR.
#[test]
fn the_host_pinned_request_rows_are_north_codec_outputs() {
    for (case, stream) in [("mantle-host-pinned", false), ("mantle-host-pinned-stream", true)] {
        let body = json!({"model": HOST_MODEL, "input": "hi", "max_output_tokens": 512,
                          "stream": stream});
        let request =
            chat_request_from_responses(&body, &ResponsesRequestOptions::default()).unwrap();
        let input = mantle_pack()
            .cases()
            .iter()
            .find(|row| row.name == format!("provider.request.{case}"))
            .unwrap()
            .input
            .clone();
        assert_eq!(input["chat_request"], serde_json::to_value(&request).unwrap(), "{case}");
    }
}

// -- the two `[DONE]` facts ----------------------------------------------------------

/// Record §5, Responses surface: native's frame loop parses every `data:` as JSON and parks on a
/// trailing `data: [DONE]` (server `sender/responses.rs:2184-2192`); the component refuses it too,
/// after the terminal it has already seen.
#[test]
fn a_trailing_done_on_the_responses_stream_is_refused_as_native_refuses_it() {
    let pack = mantle_pack();
    let find = |name: &str| pack.cases().iter().find(|case| case.name == name).unwrap();
    let clean = find("provider.stream.mantle-host-pinned");
    let trailing = find("provider.stream.mantle-trailing-done-refused");
    let clean_chunks = chunks_of(&clean.input);
    let trailing_chunks = chunks_of(&trailing.input);
    assert_eq!(&trailing_chunks[..clean_chunks.len()], clean_chunks.as_slice());
    assert_eq!(trailing_chunks[clean_chunks.len()..], ["data: [DONE]\n\n"]);
    assert!(events(&OpenAiResponsesReferenceV1, &clean_chunks).is_ok());
    let refused = events(&OpenAiResponsesReferenceV1, &trailing_chunks).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ProviderProtocolError);
}

/// Record §5, chat surface: the host recorded that Mantle's chat stream closes without `[DONE]`
/// (server `gateway/KNOWLEDGE.md:855-869`), and native fails such a stream closed. The
/// `openai-compatible` family that serves chat on a Mantle row cannot tell Mantle apart (its stream
/// parser receives no configuration), and it ends the stream on the usage frame that follows the
/// finish: with or without the sentinel the events are the same. This pins difference D2, the fix
/// of a likely native defect (unverified), so a change to it is seen.
#[test]
fn the_chat_stream_ends_the_same_with_or_without_done() {
    let read = |suffix: &str| -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(format!("provider.stream.bedrock-mantle-chat-without-done.{suffix}.json"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    let input = read("input");
    let without = chunks_of(&input);
    let mut with = without.clone();
    with.push("data: [DONE]\n\n");
    let expected = read("expected");
    assert_eq!(events(&OpenAiCompatibleReferenceV1, &without).unwrap(), expected);
    assert_eq!(events(&OpenAiCompatibleReferenceV1, &with).unwrap(), expected);
    assert_eq!(expected.as_array().unwrap().last(), Some(&json!({"type": "done"})));
}
