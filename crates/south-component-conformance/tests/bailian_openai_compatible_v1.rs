//! The `bailian-openai-compatible` family of `provider-openai-compatible` (design record
//! `docs/design/2026-10-21-bailian-thinking-cap.md`): Alibaba Bailian's OpenAI-compatible Chat
//! endpoint, whose thinking models cap the reasoning apart from the answer.
//!
//! The measured values pinned here come from the real upstream on 2026-10-21 (record §2), not
//! from this reference: `thinking_budget` caps the reasoning exactly, and the answer overruns
//! `max_tokens` by at most 3.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference::{
    OpenAiCompatibleReferenceV1, bailian_openai_compatible, client_passthrough,
};
use south_component_conformance::{
    AdmittedAuthV1, CheckV1, FixturePackV1, ProviderComponentV1, admit_descriptor_auth,
    run_provider_component_suite_v1_for_manifest,
};
use token_station_protocol::{
    Auth, ChatRequest, ErrorCode, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
    SafeHeaders,
};

const FAMILY: &str = "bailian-openai-compatible";
/// The measured overrun of the answer past `max_tokens` with thinking on (record §2).
const MEASURED_OVERHEAD: u32 = 3;
/// The Bailian API root an operator's row names (Beijing region).
const API_ROOT: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
const CHAT_URL: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";

/// Record §2: `(thinking_budget, max_tokens, reasoning, answer)` as the upstream answered.
const MEASURED: [(u32, u32, u32, u32); 5] = [
    (20, 50, 20, 53),    // qwen3.7-plus, cut by max_tokens
    (1000, 40, 748, 42), // qwen3.7-plus, thinking ended before the budget
    (30, 40, 30, 3),     // qwen3.8-flash, answer ended naturally
    (30, 40, 30, 43),    // qwen3.8-max, cut
    (30, 40, 30, 43),    // qwen3.8-omni-flash, cut
];

const BAILIAN_FIXTURES: [&str; 14] = [
    "provider.request.bailian-openai-compatible",
    "provider.request.bailian-openai-compatible-client-budget-stream",
    "provider.request.bailian-openai-compatible-ir-budget",
    "provider.request.bailian-openai-compatible-client-budget-wins",
    "provider.request.bailian-openai-compatible-clamps-the-budget",
    "provider.request.bailian-openai-compatible-clamps-a-zero-budget",
    "provider.request.bailian-openai-compatible-refuses-a-cap-too-small-to-split",
    "provider.request.bailian-openai-compatible-refuses-a-non-integer-budget",
    "provider.request.bailian-openai-compatible-client-thinking-off",
    "provider.request.bailian-openai-compatible-client-thinking-on",
    "provider.request.bailian-openai-compatible-ir-thinking-disabled",
    "provider.request.bailian-openai-compatible-undeclared-model",
    "provider.request.bailian-openai-compatible-uncapped",
    "provider.request.bailian-openai-compatible-trailing-slash",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn shipped_manifest() -> south_provider_api::ComponentManifestV1 {
    let path = root().join("../../components/provider-openai-compatible/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the manifest reads"))
        .expect("the manifest parses")
}

fn read(case: &str, suffix: &str) -> Value {
    let path = root().join("fixtures").join(format!("{case}.{suffix}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
        .expect("the fixture parses")
}

fn parts(case: &str) -> (ProviderConfig, ChatRequest) {
    let input = read(case, "input");
    (
        serde_json::from_value(input["provider_config"].clone()).unwrap(),
        serde_json::from_value(input["chat_request"].clone()).unwrap(),
    )
}

fn build(config: &ProviderConfig, request: &ChatRequest) -> HttpRequestDescriptor {
    OpenAiCompatibleReferenceV1
        .build_http_request(request, config)
        .unwrap_or_else(|error| panic!("the reference refused: {error:?}"))
}

fn body(config: &ProviderConfig, request: &ChatRequest) -> Value {
    build(config, request).body.expect("a body")
}

fn with_cap(request: &ChatRequest, cap: Option<u32>) -> ChatRequest {
    let mut request = request.clone();
    request.sampling.max_output_tokens = cap;
    request
}

fn with_client_budget(request: &ChatRequest, budget: Value) -> ChatRequest {
    let mut request = request.clone();
    request.extensions.insert("thinking_budget".to_owned(), budget);
    request
}

/// The two caps a split body carries, `(answer, reasoning)`.
fn caps(body: &Value) -> (Option<u64>, Option<u64>) {
    (
        body.get("max_tokens").and_then(Value::as_u64),
        body.get("thinking_budget").and_then(Value::as_u64),
    )
}

/// Record §2 and §5: the measured requests are exactly the ones the family builds when the client
/// asks for the measured budget under `N = budget + max_tokens + 3`, and every measured output
/// stays within that N.
#[test]
fn the_measured_requests_fit_the_authorised_cap() {
    assert_eq!(bailian_openai_compatible::CAP_OVERHEAD, MEASURED_OVERHEAD);
    let (config, request) = parts(BAILIAN_FIXTURES[0]);
    for (budget, max_tokens, reasoning, answer) in MEASURED {
        let cap = budget + max_tokens + MEASURED_OVERHEAD;
        let request = with_client_budget(&with_cap(&request, Some(cap)), json!(budget));
        let built = body(&config, &request);
        assert_eq!(caps(&built), (Some(u64::from(max_tokens)), Some(u64::from(budget))), "{cap}");
        assert!(reasoning <= budget, "the budget is a hard cap on the reasoning");
        assert!(answer <= max_tokens + MEASURED_OVERHEAD, "the measured overrun");
        assert!(reasoning + answer <= cap, "billed output {} > N {cap}", reasoning + answer);
    }
}

/// Record §4: for every cap from the smallest split up, and every budget source, the two caps and
/// the overhead add up to exactly N, each cap at least 1.
#[test]
fn every_split_adds_up_to_the_authorised_cap() {
    let (config, plain) = parts(BAILIAN_FIXTURES[0]);
    let overhead = u64::from(MEASURED_OVERHEAD);
    for cap in 5..=400_u32 {
        let base = with_cap(&plain, Some(cap));
        let mut ir_budget = base.clone();
        ir_budget.reasoning.budget_tokens = Some(cap / 3);
        let variants = [
            base.clone(),
            ir_budget,
            with_client_budget(&base, json!(0)),
            with_client_budget(&base, json!(1)),
            with_client_budget(&base, json!(cap / 2)),
            with_client_budget(&base, json!(cap)),
            with_client_budget(&base, json!(u64::MAX)),
        ];
        for request in variants {
            let (Some(answer), Some(reasoning)) = caps(&body(&config, &request)) else {
                panic!("{cap}: a split must carry both caps");
            };
            assert!(answer >= 1 && reasoning >= 1, "{cap}: {answer} + {reasoning}");
            assert_eq!(answer + reasoning + overhead, u64::from(cap), "{cap}");
        }
    }
    // Without a client or IR budget, the reasoning takes half of N - 3, rounded down.
    assert_eq!(caps(&body(&config, &with_cap(&plain, Some(1024)))), (Some(511), Some(510)));
}

/// Record §5: below 5 there is no room for an answer cap of 1, a reasoning cap of 1 and the
/// overhead; the request is refused before dispatch, as a client-shaped capability error.
#[test]
fn a_cap_too_small_to_split_is_refused_before_dispatch() {
    let (config, plain) = parts(BAILIAN_FIXTURES[0]);
    for cap in 0..5 {
        let error = OpenAiCompatibleReferenceV1
            .build_http_request(&with_cap(&plain, Some(cap)), &config)
            .unwrap_err();
        assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400), "{cap}");
    }
    // With thinking off the whole cap is the answer's, however small.
    let mut off = with_cap(&plain, Some(1));
    off.extensions.insert("enable_thinking".to_owned(), json!(false));
    assert_eq!(caps(&body(&config, &off)), (Some(1), None));
}

/// Record §5: a client budget that is not a non-negative integer is refused; `null` is absent.
#[test]
fn a_client_budget_must_be_a_non_negative_integer() {
    let (config, plain) = parts(BAILIAN_FIXTURES[0]);
    for bad in [json!("20"), json!(-1), json!(1.5), json!(true), json!([20]), json!({"n": 20})] {
        let error = OpenAiCompatibleReferenceV1
            .build_http_request(&with_client_budget(&plain, bad.clone()), &config)
            .unwrap_err();
        assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400), "{bad}");
    }
    assert_eq!(body(&config, &with_client_budget(&plain, Value::Null)), body(&config, &plain));
}

/// Record §5: off (client `enable_thinking: false` or IR `disabled`), uncapped, or a model that does
/// not declare the word: no `thinking_budget`, and the cap, when set, is the answer's alone.
#[test]
fn no_split_unless_thinking_is_on_capped_and_declared() {
    let (config, plain) = parts(BAILIAN_FIXTURES[0]);
    let budget = with_client_budget(&plain, json!(300));

    let mut off = budget.clone();
    off.extensions.insert("enable_thinking".to_owned(), json!(false));
    let built = body(&config, &off);
    assert_eq!(caps(&built), (Some(1024), None));
    assert_eq!(built["enable_thinking"], json!(false));

    let mut disabled = budget.clone();
    disabled.reasoning.mode = Some(serde_json::from_value(json!("disabled")).unwrap());
    let built = body(&config, &disabled);
    assert_eq!(caps(&built), (Some(1024), None));
    assert_eq!(built["enable_thinking"], json!(false), "BT-Q1: the default is on upstream");

    assert_eq!(caps(&body(&config, &with_cap(&budget, None))), (None, None));

    let (undeclared, request) =
        parts("provider.request.bailian-openai-compatible-undeclared-model");
    let built = body(&undeclared, &with_client_budget(&request, json!(300)));
    assert_eq!(caps(&built), (Some(1024), None));
    assert!(built.get("enable_thinking").is_none(), "the gate is the declared word");

    // An `enable_thinking: true` or `null` from the client keeps thinking on.
    for on in [json!(true), Value::Null] {
        let mut request = plain.clone();
        request.extensions.insert("enable_thinking".to_owned(), on.clone());
        let built = body(&config, &request);
        assert_eq!(caps(&built), (Some(511), Some(510)), "{on}");
        assert_eq!(built["enable_thinking"], on);
    }
}

/// Record §5: a model that does not declare the word gets the request an `openai-compatible` row on
/// the same API root sends, so an existing Bailian row moves by changing its family alone.
#[test]
fn an_undeclared_model_matches_openai_compatible_on_the_same_root() {
    let (config, request) = parts("provider.request.bailian-openai-compatible-undeclared-model");
    let mut plain = config.clone();
    plain.provider = "openai-compatible".to_owned();
    let mut request = request;
    request.extensions.insert("enable_thinking".to_owned(), json!(true));
    assert_eq!(build(&config, &request), build(&plain, &request));
    assert_eq!(build(&config, &request).url, CHAT_URL);
}

/// Record §5: the endpoint is the API root; the component appends `/chat/completions` and presents
/// the key as a bearer, with `content-type` the only header.
#[test]
fn the_family_posts_below_the_api_root_with_a_bearer() {
    assert_eq!(bailian_openai_compatible::FAMILY, FAMILY);
    let manifest = shipped_manifest();
    for case in BAILIAN_FIXTURES {
        if read(case, "expected").get("error").is_some() {
            continue;
        }
        let (config, request) = parts(case);
        let descriptor = build(&config, &request);
        assert_eq!(descriptor.url, CHAT_URL, "{case}");
        assert_eq!(
            descriptor.headers,
            SafeHeaders::try_new([("content-type", "application/json")]).unwrap(),
            "{case}"
        );
        assert_eq!(descriptor.auth, Some(Auth::bearer(config.auth.clone().unwrap())), "{case}");
        assert_eq!(
            admit_descriptor_auth(&manifest, &config, &descriptor),
            Ok(AdmittedAuthV1::Bearer),
            "{case}"
        );
    }
    let (mut config, request) = parts(BAILIAN_FIXTURES[0]);
    assert_eq!(config.base_url.as_str(), API_ROOT);
    config.auth = None;
    assert_eq!(build(&config, &request).auth, None);
}

/// Record §5: the client's `enable_thinking` and `thinking` are forwarded as on `openai-compatible`;
/// its `thinking_budget` never is verbatim.
#[test]
fn the_client_fields_are_forwarded_and_the_budget_is_consumed() {
    assert!(client_passthrough::FAMILIES.contains(&FAMILY));
    let (config, request) = parts(BAILIAN_FIXTURES[0]);
    let mut with = request.clone();
    with.extensions.insert("thinking".to_owned(), json!({"type": "enabled"}));
    assert_eq!(body(&config, &with)["thinking"], json!({"type": "enabled"}));
    let built = body(&config, &with_client_budget(&with_cap(&request, Some(73)), json!(20)));
    assert_eq!(built["thinking_budget"], json!(20));
    assert_eq!(built["max_tokens"], json!(50));
}

/// Record §4 and §5: the manifest declares the family, its facts, its immutable reasoning cap and
/// its words; no endpoint template, user-agent, config keys, recipe or subscription quota.
#[test]
fn the_manifest_declares_the_family_and_its_reasoning_cap() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.validate(), Ok(()));
    assert_eq!(manifest.providers.last().map(String::as_str), Some(FAMILY));
    assert_eq!(
        serde_json::to_value(manifest.request_facts_for(FAMILY)).unwrap(),
        json!({"output_cap": ["/max_tokens"], "reasoning_cap": ["/thinking_budget"],
               "cap_overhead": MEASURED_OVERHEAD, "model": {"body": "/model"},
               "stream": {"body": "/stream"}})
    );
    assert_eq!(
        manifest.immutable_body_paths.get(FAMILY),
        Some(&vec!["thinking_budget".to_owned()])
    );
    assert_eq!(manifest.immutable_body_paths.len(), 1, "no other family declares paths");
    assert_eq!(
        manifest.model_parameters[FAMILY].words,
        south_component_conformance::declared_parameters::family::BAILIAN_OPENAI_COMPATIBLE
    );
    assert!(
        manifest.model_parameters[FAMILY]
            .words
            .contains(&bailian_openai_compatible::THINKING_BUDGET_WORD.to_owned())
    );
    assert!(!manifest.endpoint.contains_key(FAMILY));
    assert!(!manifest.config_schema.contains_key(FAMILY));
    assert!(!manifest.user_agent.contains_key(FAMILY));
    assert_eq!(manifest.credentials_for(FAMILY), None);
    assert!(!manifest.subscription_quota_families.contains(FAMILY));
    // Record §7: a new family is a minor; the reasoning cap fields need a runtime that reads them.
    // Published as 2.13.0 with 0.76.0; the Gemini signatures re-pin made it 2.13.1
    // (`gemini_signatures_retires_every_published_076_package_identity`).
    // Published as 2.13.1 with 0.77.0; the Gemini placeholder signature round made it 2.13.2
    // (`gemini_placeholder_retires_every_published_077_package_identity`).
    // Published as 2.13.2 with 0.78.0; the 0.79.0 round made it 2.13.3
    // (`qwen_tier_and_responses_stop_retires_every_published_078_package_identity`).
    // Published as 2.13.3 with 0.79.0; the 0.80.0 round made it 2.13.4
    // (`veo_spelling_and_durationless_asr_retire_every_published_079_package_identity`).
    // Published as 2.13.4 with 0.80.0; the 0.81.0 round made it 2.13.5
    // (`served_models_retires_every_published_080_package_identity`).
    // Published as 2.13.5 with 0.81.0; the 0.82.0 round made it 2.13.6
    // (`sse_line_ends_retire_every_published_081_package_identity`).
    // Published as 2.13.6 with 0.82.0; the 0.83.0 round made it 2.14.0
    // (`client_policy_headers_retire_every_published_082_package_identity`).
    assert_eq!(manifest.version, "2.14.0");
    assert_eq!(OpenAiCompatibleReferenceV1.metadata().version, manifest.version);
    // `forwarded_client_policy_headers` needs a runtime that reads it, which the release that ships
    // it (0.83.0) declares; until then the package keeps 0.77.0.
    assert_eq!(manifest.compatibility.south_runtime, "0.77.0"); // release 0.77.0 raised every package (canonical_ir 5)
}

/// Record §2: the reasoning is reported inside `completion_tokens`, which is what is billed; the
/// measured b20/mt50 response bills 73 = N.
#[test]
fn the_billed_output_includes_the_reasoning() {
    let input = read("provider.response.bailian-thinking-usage", "input");
    let parts: HttpResponseParts = serde_json::from_value(input).unwrap();
    let response = OpenAiCompatibleReferenceV1.parse_response(&parts).unwrap();
    assert_eq!(response.usage.output_tokens, 73);
    assert_eq!(response.usage.reasoning_tokens, 20);
    assert!(response.usage.output_tokens <= u64::from(20 + 50 + MEASURED_OVERHEAD));
}

/// A near miss of the family name is refused rather than served as another family.
#[test]
fn a_near_miss_of_the_family_is_refused() {
    let (config, request) = parts(BAILIAN_FIXTURES[0]);
    for family in ["bailian", "dashscope", "bailian_openai_compatible", "Bailian-OpenAI-Compatible"]
    {
        let mut other = config.clone();
        other.provider = family.to_owned();
        let error = OpenAiCompatibleReferenceV1.build_http_request(&request, &other).unwrap_err();
        assert!(error.message.contains("unsupported provider dialect"), "{family}");
    }
}

/// Gate ② runs every `bailian-openai-compatible` row natively, `RequestFactsHonoured` included
/// (the split form, record §4); `sandbox_parity_v1` runs the same pack in the sandbox.
#[test]
fn gate_two_runs_every_bailian_case() {
    let manifest = shipped_manifest();
    let pack = FixturePackV1::load(&root().join("fixtures")).expect("the shipped pack loads");
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &manifest,
    );
    assert!(report.is_passing(), "{report}");
    for case in BAILIAN_FIXTURES {
        assert!(
            report
                .outcomes()
                .iter()
                .any(|outcome| outcome.check == CheckV1::FixtureMatch && outcome.case == case),
            "{case} never ran"
        );
    }
    assert!(report.outcomes().iter().any(|outcome| outcome.check == CheckV1::RequestFactsHonoured
        && outcome.case == BAILIAN_FIXTURES[0]));
}
