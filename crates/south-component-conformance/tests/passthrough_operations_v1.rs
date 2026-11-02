//! Gate ② for the Messages pass-through record
//! (`docs/design/2026-10-17-messages-passthrough-and-count-tokens.md` §6): `ForwardedHeadersAbsent`
//! and `CountTokensPath` run for the families that declare `forwarded_client_headers` and
//! `count_tokens`, pass for the shipped references, and turn red on a component that sets a
//! forwarded header or builds a URL the declared suffix does not rewrite. `provider-anthropic`'s
//! `messages` request pass-through also runs `DescriptorIndependentOfBody`.

use std::path::Path;

use south_component_conformance::reference_anthropic::AnthropicReferenceV1;
use south_component_conformance::reference_gemini::GeminiReferenceV1;
use south_component_conformance::reference_vertex::VertexReferenceV1;
use south_component_conformance::{
    CheckV1, ComponentResultV1, FixturePackV1, ProviderComponentV1, ReportV1, StreamParserV1,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{ComponentManifestV1, ComponentMetadataV1, NorthProtocolV1};
use token_station_protocol::{
    ChatRequest, ChatResponse, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts,
    ModelCapability, ProviderConfig, SafeHeaders,
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

/// How often `check` ran, and its failures.
fn outcomes_of(report: &ReportV1, check: CheckV1) -> (usize, Vec<String>) {
    let ran = report.outcomes().iter().filter(|outcome| outcome.check == check).count();
    let failed = report
        .failures()
        .filter(|outcome| outcome.check == check)
        .map(|outcome| format!("{}: {}", outcome.case, outcome.detail()))
        .collect();
    (ran, failed)
}

#[test]
fn the_shipped_packages_declare_the_record_s_families() {
    let anthropic = shipped("provider-anthropic");
    assert_eq!(anthropic.north_passthrough.get("anthropic"), Some(&NorthProtocolV1::Messages));
    assert_eq!(
        anthropic.north_request_passthrough.get("anthropic"),
        Some(&NorthProtocolV1::Messages)
    );
    assert_eq!(anthropic.forwarded_client_headers["anthropic"], ["anthropic-beta"]);
    assert_eq!(anthropic.count_tokens["anthropic"].protocol, NorthProtocolV1::Messages);
    for family in ["claude-code", "claude-code-setup-token"] {
        assert!(!anthropic.north_request_passthrough.contains_key(family), "{family}");
        assert!(!anthropic.forwarded_client_headers.contains_key(family), "{family}");
        assert!(!anthropic.count_tokens.contains_key(family), "{family}");
    }
    assert_eq!(anthropic.validate(), Ok(()));
    for (package, family) in [("provider-gemini", "gemini"), ("provider-vertex", "vertex-ai")] {
        let manifest = shipped(package);
        let counting = &manifest.count_tokens[family];
        assert_eq!(counting.protocol, NorthProtocolV1::GenerateContent, "{package}");
        assert_eq!(
            (counting.path_suffix.from.as_str(), counting.path_suffix.to.as_str()),
            (":generateContent", ":countTokens"),
            "{package}"
        );
        assert!(manifest.forwarded_client_headers.is_empty(), "{package}");
        assert_eq!(manifest.validate(), Ok(()), "{package}");
    }
    for package in ["provider-anthropic-bedrock-invoke", "provider-openai-compatible"] {
        assert!(shipped(package).count_tokens.is_empty(), "{package}");
    }
}

#[test]
fn the_shipped_references_pass_the_new_rows() {
    let report = run_provider_component_suite_v1_for_manifest(
        &AnthropicReferenceV1,
        &pack("fixtures-anthropic"),
        &shipped("provider-anthropic"),
    );
    for check in [
        CheckV1::DescriptorIndependentOfBody,
        CheckV1::ForwardedHeadersAbsent,
        CheckV1::CountTokensPath,
        CheckV1::RequestFactsHonoured,
    ] {
        let (ran, failed) = outcomes_of(&report, check);
        assert!(ran > 0, "{check:?} must run for provider-anthropic: {report}");
        assert!(failed.is_empty(), "{check:?}: {failed:#?}");
    }
    for (component, directory, package) in [
        (&GeminiReferenceV1 as &dyn ProviderComponentV1, "fixtures-gemini", "provider-gemini"),
        (&VertexReferenceV1, "fixtures-vertex", "provider-vertex"),
    ] {
        let report = run_provider_component_suite_v1_for_manifest(
            component,
            &pack(directory),
            &shipped(package),
        );
        let (ran, failed) = outcomes_of(&report, CheckV1::CountTokensPath);
        assert!(ran > 0, "{package}: {report}");
        assert!(failed.is_empty(), "{package}: {failed:#?}");
        assert_eq!(outcomes_of(&report, CheckV1::ForwardedHeadersAbsent).0, 0, "{package}");
    }
}

#[test]
fn a_family_that_declares_neither_never_runs_them() {
    let report = run_provider_component_suite_v1_for_manifest(
        &south_component_conformance::reference::OpenAiCompatibleReferenceV1,
        &pack("fixtures"),
        &shipped("provider-openai-compatible"),
    );
    assert_eq!(outcomes_of(&report, CheckV1::CountTokensPath).0, 0);
}

#[derive(Clone, Copy)]
enum Rogue {
    /// Sets `anthropic-beta`, which `anthropic` forwards from the client.
    SetsAForwardedHeader,
    /// Sends the non-stream request to `:generate` instead of `:generateContent`.
    OtherGenerationPath,
    /// Sets the policy header `bailian-openai-compatible` forwards from the client.
    SetsAPolicyHeader,
}

fn with_header(
    mut descriptor: HttpRequestDescriptor,
    name: &str,
    value: &str,
) -> HttpRequestDescriptor {
    let headers = descriptor
        .headers
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .chain([(name.to_owned(), value.to_owned())]);
    descriptor.headers = SafeHeaders::try_new(headers).unwrap();
    descriptor
}

struct Rogues(Rogue);

impl Rogues {
    fn inner(&self) -> &dyn ProviderComponentV1 {
        match self.0 {
            Rogue::SetsAForwardedHeader => &AnthropicReferenceV1,
            Rogue::OtherGenerationPath => &GeminiReferenceV1,
            Rogue::SetsAPolicyHeader => {
                &south_component_conformance::reference::OpenAiCompatibleReferenceV1
            }
        }
    }

    fn bend(&self, mut descriptor: HttpRequestDescriptor) -> HttpRequestDescriptor {
        match self.0 {
            Rogue::SetsAForwardedHeader => {
                descriptor = with_header(descriptor, "anthropic-beta", "a-beta-2026-01-01");
            }
            Rogue::SetsAPolicyHeader => {
                descriptor = with_header(descriptor, "x-dashscope-datainspection", "a-value");
            }
            Rogue::OtherGenerationPath => {
                descriptor.url = descriptor.url.replace(":generateContent", ":generate");
            }
        }
        descriptor
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
        self.inner().build_http_request(request, config).map(|built| self.bend(built))
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
fn a_rogue_component_fails_the_new_rows() {
    let report = run_provider_component_suite_v1_for_manifest(
        &Rogues(Rogue::SetsAForwardedHeader),
        &pack("fixtures-anthropic"),
        &shipped("provider-anthropic"),
    );
    let (_, failed) = outcomes_of(&report, CheckV1::ForwardedHeadersAbsent);
    assert!(failed.iter().any(|failure| failure.contains("sets `anthropic-beta`")), "{failed:#?}");

    let report = run_provider_component_suite_v1_for_manifest(
        &Rogues(Rogue::OtherGenerationPath),
        &pack("fixtures-gemini"),
        &shipped("provider-gemini"),
    );
    let (_, failed) = outcomes_of(&report, CheckV1::CountTokensPath);
    assert!(
        failed.iter().any(|failure| failure.contains("does not end with `:generateContent`")),
        "{failed:#?}"
    );
}

/// The client policy headers record §4: gate ② covers `forwarded_client_policy_headers` too.
#[test]
fn a_policy_header_a_component_sets_itself_fails_the_gate() {
    let honest = run_provider_component_suite_v1_for_manifest(
        &south_component_conformance::reference::OpenAiCompatibleReferenceV1,
        &pack("fixtures"),
        &shipped("provider-openai-compatible"),
    );
    let (ran, failed) = outcomes_of(&honest, CheckV1::ForwardedHeadersAbsent);
    assert!(ran > 0, "{honest}");
    assert!(failed.is_empty(), "{failed:#?}");

    let report = run_provider_component_suite_v1_for_manifest(
        &Rogues(Rogue::SetsAPolicyHeader),
        &pack("fixtures"),
        &shipped("provider-openai-compatible"),
    );
    let (_, failed) = outcomes_of(&report, CheckV1::ForwardedHeadersAbsent);
    assert!(
        failed.iter().any(|failure| failure.contains("sets `x-dashscope-datainspection`")),
        "{failed:#?}"
    );
}

/// `FORWARDING_REFUSED_HEADERS` mirrors `south_contracts`' reserved header set (record §4.1):
/// every mirrored name is one the contracts refuse on the ordinary header channel, under the policy
/// version the mirror was copied from. A new policy version fails here until the mirror is
/// re-copied.
#[test]
fn the_forwarding_refusal_list_mirrors_the_reserved_headers() {
    assert_eq!(south_contracts::RESERVED_HEADER_POLICY_VERSION, 2);
    for name in south_provider_api::FORWARDING_REFUSED_HEADERS {
        assert_eq!(
            south_contracts::SafeHeaders::try_from_iter([(*name, "v")]).err(),
            Some(south_contracts::HeaderPolicyError::ReservedHeader),
            "{name}"
        );
    }
    assert!(south_contracts::SafeHeaders::try_from_iter([("anthropic-beta", "v")]).is_ok());
}
