//! Request facts (B2, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §7.2 and §7.6): gate ①
//! refuses a malformed declaration, and gate ②'s `RequestFactsHonoured` turns red on a component
//! that writes the cap, the model or the stream flag anywhere but where its manifest says.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::reference_gemini::GeminiReferenceV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, FixturePackV1, ProviderComponentV1, ReportV1, StreamParserV1,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{
    ComponentManifestV1, ComponentMetadataV1, ManifestErrorV1, ModelLocationV1, RequestFactsV1,
    StreamLocationV1,
};
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ModelCapability, ProviderConfig,
};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = root().join(format!("../../components/{package}/manifest.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn pack(directory: &str) -> FixturePackV1 {
    FixturePackV1::load(&root().join(directory)).unwrap()
}

fn honoured_failures(report: &ReportV1) -> Vec<String> {
    report
        .failures()
        .filter(|outcome| outcome.check == CheckV1::RequestFactsHonoured)
        .map(|outcome| format!("{}: {}", outcome.case, outcome.detail()))
        .collect()
}

#[test]
fn gate_one_refuses_a_malformed_declaration() {
    let facts =
        |output_cap: &[&str], model: ModelLocationV1, stream: StreamLocationV1| RequestFactsV1 {
            output_cap: output_cap.iter().map(|pointer| (*pointer).to_owned()).collect(),
            model,
            stream,
        };
    let body = |pointer: &str| ModelLocationV1::Body(pointer.to_owned());
    let url = |template: &str| ModelLocationV1::Url(template.to_owned());
    let declared = |family: &str, facts: RequestFactsV1| {
        let mut manifest = shipped("provider-gemini");
        manifest.request_facts.clear();
        manifest.request_facts.insert(family.to_owned(), facts);
        manifest.validate()
    };
    let ok = declared(
        "gemini",
        facts(
            &["/generationConfig/maxOutputTokens"],
            url("/models/{model}:"),
            StreamLocationV1::Url,
        ),
    );
    assert_eq!(ok, Ok(()));
    for (family, bad) in [
        ("not-a-family", facts(&[], body("/model"), StreamLocationV1::None)),
        ("gemini", facts(&["/a", "/b", "/c", "/d", "/e"], body("/model"), StreamLocationV1::None)),
        ("gemini", facts(&["/a", "/a"], body("/model"), StreamLocationV1::None)),
        ("gemini", facts(&["max_tokens"], body("/model"), StreamLocationV1::None)),
        ("gemini", facts(&["/a//b"], body("/model"), StreamLocationV1::None)),
        ("gemini", facts(&[], body("/model~2"), StreamLocationV1::None)),
        ("gemini", facts(&[], url("/models/{model}/{model}"), StreamLocationV1::None)),
        ("gemini", facts(&[], url("/models/{model}?alt=sse"), StreamLocationV1::None)),
        ("gemini", facts(&[], url("models/{model}"), StreamLocationV1::None)),
        ("gemini", facts(&[], url("/models/{name}"), StreamLocationV1::None)),
        ("gemini", facts(&[], body("/model"), StreamLocationV1::Body("stream".to_owned()))),
    ] {
        assert!(
            matches!(
                declared(family, bad.clone()),
                Err(ManifestErrorV1::InvalidRequestFacts { .. })
            ),
            "{family}: {bad:?}"
        );
    }
}

#[test]
fn request_facts_round_trip_and_stay_off_the_wire_when_absent() {
    let gemini = shipped("provider-gemini");
    let wire = serde_json::to_value(&gemini).unwrap();
    assert_eq!(
        wire["request_facts"]["gemini"],
        json!({"output_cap": ["/generationConfig/maxOutputTokens"],
               "model": {"url": "/models/{model}:"}, "stream": "url"})
    );
    let openai = serde_json::to_value(shipped("provider-openai-compatible")).unwrap();
    assert!(openai.get("request_facts").is_none());
    assert_eq!(
        shipped("provider-openai-compatible").request_facts_for("openai-compatible"),
        RequestFactsV1::top_level()
    );
}

#[test]
fn a_declaration_the_requests_do_not_follow_fails_gate_two() {
    // Gemini without its declaration falls back to the top-level fields, which its requests do
    // not use.
    let mut undeclared = shipped("provider-gemini");
    undeclared.request_facts.clear();
    let report = run_provider_component_suite_v1_for_manifest(
        &GeminiReferenceV1,
        &pack("fixtures-gemini"),
        &undeclared,
    );
    assert!(!honoured_failures(&report).is_empty(), "{report}");

    // An `OpenAI` family declared as having no cap field, while the reference writes one.
    let mut capless = shipped("provider-openai-compatible");
    capless.request_facts.insert(
        "openai-compatible".to_owned(),
        RequestFactsV1 { output_cap: Vec::new(), ..RequestFactsV1::top_level() },
    );
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack("fixtures"),
        &capless,
    );
    assert!(
        honoured_failures(&report)
            .iter()
            .any(|failure| failure.contains("declares no cap location")),
        "{report}"
    );
}

/// What a rogue component does to the descriptor the reference built.
#[derive(Clone, Copy)]
enum Rogue {
    /// Writes the cap a second time, where the manifest does not declare it.
    CapTwice,
    /// Names another model in the body.
    OtherModelInBody,
    /// Always asks the upstream to stream.
    AlwaysStream,
    /// Sends the request to another model's URL.
    OtherModelInUrl,
}

struct Rogues(Rogue);

impl Rogues {
    fn bend(
        &self,
        request: &ChatRequest,
        mut descriptor: HttpRequestDescriptor,
    ) -> HttpRequestDescriptor {
        let body = descriptor.body.get_or_insert_with(|| json!({}));
        match self.0 {
            Rogue::CapTwice => {
                if let Some(cap) = request.sampling.max_output_tokens {
                    body["max_output_tokens_mirror"] = json!(cap);
                }
            }
            Rogue::OtherModelInBody => body["model"] = json!("another-model"),
            Rogue::AlwaysStream => body["stream"] = Value::Bool(true),
            Rogue::OtherModelInUrl => {
                descriptor.url = descriptor.url.replace(&request.model, "another-model");
            }
        }
        descriptor
    }

    fn inner(&self) -> &dyn ProviderComponentV1 {
        match self.0 {
            Rogue::OtherModelInUrl => &GeminiReferenceV1,
            Rogue::CapTwice | Rogue::OtherModelInBody | Rogue::AlwaysStream => {
                &OpenAiCompatibleReferenceV1
            }
        }
    }
}

impl ProviderComponentV1 for Rogues {
    fn metadata(&self) -> ComponentMetadataV1 {
        self.inner().metadata()
    }

    fn model_capabilities(
        &self,
        config: &ProviderConfig,
    ) -> ComponentResultV1<Vec<ModelCapability>> {
        self.inner().model_capabilities(config)
    }

    fn build_http_request(
        &self,
        request: &ChatRequest,
        config: &ProviderConfig,
    ) -> ComponentResultV1<HttpRequestDescriptor> {
        self.inner().build_http_request(request, config).map(|built| self.bend(request, built))
    }

    fn parse_response(&self, parts: &HttpResponseParts) -> ComponentResultV1<ChatResponse> {
        self.inner().parse_response(parts)
    }

    fn map_provider_error(&self, parts: &HttpResponseParts) -> ComponentResultV1<ErrorEnvelope> {
        self.inner().map_provider_error(parts)
    }

    fn stream_parser(&self) -> Box<dyn StreamParserV1> {
        self.inner().stream_parser()
    }
}

#[test]
fn a_rogue_request_fails_gate_two() {
    for (rogue, package, directory, expected) in [
        (Rogue::CapTwice, "provider-openai-compatible", "fixtures", "outside the declared cap"),
        (Rogue::OtherModelInBody, "provider-openai-compatible", "fixtures", "IR model at `/model`"),
        (Rogue::AlwaysStream, "provider-openai-compatible", "fixtures", "stream flag"),
        (Rogue::OtherModelInUrl, "provider-gemini", "fixtures-gemini", "URL path"),
    ] {
        let report = run_provider_component_suite_v1_for_manifest(
            &Rogues(rogue),
            &pack(directory),
            &shipped(package),
        );
        let failures = honoured_failures(&report);
        assert!(
            failures.iter().any(|failure| failure.contains(expected)),
            "{expected}: {failures:#?}"
        );
    }
}

/// A Bedrock inference-profile ARN contains `/`; placed raw it would split the path and send the
/// request to another route than the model the host reserved for.
#[test]
fn a_model_with_a_slash_stays_one_url_segment() {
    use south_component_conformance::reference_bedrock_converse::BedrockConverseReferenceV1;
    let input: Value = serde_json::from_str(
        &std::fs::read_to_string(
            root().join("fixtures-bedrock-converse/provider.request.chat.input.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let config: ProviderConfig = serde_json::from_value(input["provider_config"].clone()).unwrap();
    let mut request: ChatRequest = serde_json::from_value(input["chat_request"].clone()).unwrap();
    request.model =
        "arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.claude-v1:0"
            .to_owned();
    let built = BedrockConverseReferenceV1.build_http_request(&request, &config).unwrap();
    assert!(
        built.url.ends_with(
            "/model/arn:aws:bedrock:us-east-1:123456789012:inference-profile%2Fus.anthropic.claude-v1:0/converse"
        ),
        "{}",
        built.url
    );
}
