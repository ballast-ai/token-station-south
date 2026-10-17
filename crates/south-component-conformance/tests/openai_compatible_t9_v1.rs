//! The T9 families of `provider-openai-compatible` (design record
//! `docs/design/2026-10-15-openai-compatible-t9.md`): `glm-coding`, the Z.ai GLM Coding Plan, and
//! `azure-openai-dated`, Azure `OpenAI`'s dated, deployment-addressed surface.
//!
//! Both are transcribed from the host's native arms (record §2): the values pinned here are the
//! native constants and the native URL tests' cases, not values derived from this reference.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference::{
    OpenAiCompatibleReferenceV1, azure_openai_cap_field, azure_openai_dated, client_passthrough,
    glm_coding,
};
use south_component_conformance::{
    AdmittedAuthV1, CheckV1, FixturePackV1, ProviderComponentV1, admit_descriptor_auth,
    run_provider_component_suite_v1_for_manifest,
};
use south_contracts::SecretHeaderV1;
use south_provider_api::{ComponentManifestV1, ValueSyntaxV1};
use token_station_protocol::{
    Auth, ChatRequest, ErrorCode, HttpRequestDescriptor, ProviderConfig, SafeHeaders,
};

/// Native `GLM_CODING_IMPERSONATION_UA` (server `upstream.rs`), written out.
const NATIVE_GLM_CODING_UA: &str = "opencode/1.15.6";
/// Native `AZURE_OPENAI_DEFAULT_API_VERSION` (server `upstream.rs`), written out.
const NATIVE_AZURE_DEFAULT_API_VERSION: &str = "2024-10-21";
/// The key native sends the cap under on an `azure_openai` row by default: the Chat canonical body
/// writes `max_completion_tokens` and `needs_max_tokens_rewrite` keeps it while the row's
/// `accepts_max_completion_tokens` is on (server `providers.rs`, column default 1 / `true`).
const NATIVE_AZURE_DEFAULT_CAP_FIELD: &str = "max_completion_tokens";

const GLM_FIXTURES: [&str; 2] =
    ["provider.request.glm-coding", "provider.request.glm-coding-stream"];
const AZURE_FIXTURES: [&str; 7] = [
    "provider.request.azure-openai-dated",
    "provider.request.azure-openai-dated-default-version",
    "provider.request.azure-openai-dated-refuses-an-undated-version",
    "provider.request.azure-openai-dated-refuses-a-request-without-a-model",
    "provider.request.azure-openai-dated-max-tokens-field",
    "provider.request.azure-openai-v1-max-completion-tokens",
    "provider.request.azure-openai-dated-refuses-an-unknown-cap-field",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest_path() -> std::path::PathBuf {
    root().join("../../components/provider-openai-compatible/manifest.json")
}

fn shipped_manifest() -> ComponentManifestV1 {
    serde_json::from_str(&std::fs::read_to_string(manifest_path()).expect("the manifest reads"))
        .expect("the manifest parses")
}

fn fixture_input(case: &str) -> Value {
    let path = root().join("fixtures").join(format!("{case}.input.json"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
        .expect("the fixture parses")
}

fn parts(case: &str) -> (ProviderConfig, ChatRequest) {
    let input = fixture_input(case);
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

fn content_type_only() -> SafeHeaders {
    SafeHeaders::try_new([("content-type", "application/json")]).unwrap()
}

fn azure(
    base_url: &str,
    api_version: Option<&str>,
    deployment: &str,
) -> (ProviderConfig, ChatRequest) {
    let (_, mut request) = parts(AZURE_FIXTURES[0]);
    let mut input = fixture_input(AZURE_FIXTURES[0]);
    input["provider_config"]["base_url"] = json!(base_url);
    match api_version {
        Some(version) => input["provider_config"]["declared"] = json!({"api_version": version}),
        None => {
            input["provider_config"].as_object_mut().unwrap().remove("declared");
        }
    }
    deployment.clone_into(&mut request.model);
    (serde_json::from_value(input["provider_config"].clone()).unwrap(), request)
}

// -- glm-coding --------------------------------------------------------------------------------

/// Record §2.1: native posts Chat Completions to `{endpoint}/api/coding/paas/v4/chat/completions`
/// with the key as `Authorization: Bearer` and no other header but the user-agent, which the
/// package declares rather than sends.
#[test]
fn glm_coding_posts_to_the_coding_plan_path_with_a_bearer_key() {
    assert_eq!(glm_coding::CHAT_PATH, "/api/coding/paas/v4/chat/completions");
    let manifest = shipped_manifest();
    for case in GLM_FIXTURES {
        let (config, request) = parts(case);
        let descriptor = build(&config, &request);
        assert_eq!(
            descriptor.url, "https://api.z.ai/api/coding/paas/v4/chat/completions",
            "{case}"
        );
        assert_eq!(descriptor.headers, content_type_only(), "{case}");
        assert_eq!(descriptor.auth, Some(Auth::bearer(config.auth.clone().unwrap())), "{case}");
        assert_eq!(
            admit_descriptor_auth(&manifest, &config, &descriptor),
            Ok(AdmittedAuthV1::Bearer),
            "{case}"
        );
        assert_eq!(config.authorize(&descriptor), Ok(()), "{case}");
    }
}

/// Record §2.1 and §3.1: the user-agent is native's `OpenCode` impersonation, declared for this
/// family only, and the family is a subscription (native `is_coding_plan`).
#[test]
fn glm_coding_declares_the_native_user_agent_and_a_subscription_quota() {
    assert_eq!(glm_coding::USER_AGENT, NATIVE_GLM_CODING_UA);
    let manifest = shipped_manifest();
    assert_eq!(
        manifest.user_agent.get(glm_coding::FAMILY).map(String::as_str),
        Some(NATIVE_GLM_CODING_UA)
    );
    let instances = south_component_conformance::DeclaredInstancesV1::from_manifest(&manifest)
        .expect("the shipped manifest passes gate ①");
    assert_eq!(
        instances.user_agent(glm_coding::FAMILY).map(south_contracts::DeclaredUserAgentV1::as_str),
        Some(NATIVE_GLM_CODING_UA)
    );
    assert_eq!(instances.user_agent("openai-compatible"), None);
    // `kimi-code` joined later (`kimi_code_v1`, Kimi Code record §3).
    assert_eq!(
        manifest.subscription_quota_families,
        BTreeSet::from([
            "github-copilot".to_owned(),
            glm_coding::FAMILY.to_owned(),
            "kimi-code".to_owned()
        ])
    );
    assert_eq!(manifest.credentials_for(glm_coding::FAMILY), None);
    assert!(!manifest.endpoint.contains_key(glm_coding::FAMILY));
    assert!(!manifest.config_schema.contains_key(glm_coding::FAMILY));
}

/// Record §3.1: the family is `openai-compatible` with the Coding Plan root fixed. The same request
/// on `openai-compatible`, configured at that root, builds the same descriptor.
#[test]
fn glm_coding_shares_the_openai_compatible_translation() {
    for case in GLM_FIXTURES {
        let (config, request) = parts(case);
        let mut plain = config.clone();
        plain.provider = "openai-compatible".to_owned();
        plain.base_url =
            serde_json::from_value(json!("https://api.z.ai/api/coding/paas/v4")).unwrap();
        assert_eq!(build(&config, &request), build(&plain, &request), "{case}");
    }
}

// -- azure-openai-dated ------------------------------------------------------------------------

/// Record §2.2: native `build_upstream_url`'s dated arm, case by case from the native URL tests
/// (`azure_openai_url_*`): both hosts used verbatim, the operator's version, the default when
/// none is set, and a trailing slash on the endpoint.
#[test]
fn azure_openai_dated_builds_the_native_deployment_url() {
    for (base, version, deployment, url) in [
        (
            "https://my-resource.openai.azure.com",
            Some("2024-10-21"),
            "gpt-5-5-deployment",
            "https://my-resource.openai.azure.com/openai/deployments/gpt-5-5-deployment/chat/completions?api-version=2024-10-21",
        ),
        (
            "https://my-resource.services.ai.azure.com",
            Some("2024-10-21"),
            "my-deployment",
            "https://my-resource.services.ai.azure.com/openai/deployments/my-deployment/chat/completions?api-version=2024-10-21",
        ),
        (
            "https://x.openai.azure.com",
            Some("2025-04-01-preview"),
            "preview-model",
            "https://x.openai.azure.com/openai/deployments/preview-model/chat/completions?api-version=2025-04-01-preview",
        ),
        (
            "https://x.openai.azure.com",
            Some("2024-12-01-preview"),
            "d",
            "https://x.openai.azure.com/openai/deployments/d/chat/completions?api-version=2024-12-01-preview",
        ),
        (
            "https://x.openai.azure.com",
            None,
            "d",
            "https://x.openai.azure.com/openai/deployments/d/chat/completions?api-version=2024-10-21",
        ),
        (
            "https://x.openai.azure.com/",
            None,
            "d",
            "https://x.openai.azure.com/openai/deployments/d/chat/completions?api-version=2024-10-21",
        ),
    ] {
        let (config, request) = azure(base, version, deployment);
        let descriptor = build(&config, &request);
        assert_eq!(descriptor.url, url);
        assert_eq!(descriptor.body.as_ref().unwrap()["model"], json!(deployment), "{url}");
        assert_eq!(config.authorize(&descriptor), Ok(()), "{url}");
    }
}

/// Record §2.2: the key in `api-key`, as native's Azure arm sends it, and nothing else.
#[test]
fn azure_openai_dated_presents_the_key_in_api_key() {
    let manifest = shipped_manifest();
    for case in &AZURE_FIXTURES[..2] {
        let (config, request) = parts(case);
        let descriptor = build(&config, &request);
        let slot = config.auth.clone().unwrap();
        assert_eq!(descriptor.auth, Some(Auth::header("api-key", slot).unwrap()), "{case}");
        assert_eq!(descriptor.headers, content_type_only(), "{case}");
        assert_eq!(
            admit_descriptor_auth(&manifest, &config, &descriptor),
            Ok(AdmittedAuthV1::HeaderSecret(SecretHeaderV1::ApiKey)),
            "{case}"
        );
    }
    let (mut config, request) = parts(AZURE_FIXTURES[0]);
    config.auth = None;
    assert_eq!(build(&config, &request).auth, None);
}

/// Record §3.2: the family is `azure-openai-v1` with another route. The same request configured
/// on the v1 surface builds the same headers, body and auth.
#[test]
fn azure_openai_dated_differs_from_azure_openai_v1_only_in_the_url() {
    for case in &AZURE_FIXTURES[..2] {
        let (config, request) = parts(case);
        let dated = build(&config, &request);
        let mut v1 = config.clone();
        v1.provider = "azure-openai-v1".to_owned();
        v1.declared = token_station_protocol::ComponentValues::new();
        v1.base_url =
            serde_json::from_value(json!(format!("{}/openai/v1", config.base_url.as_str())))
                .unwrap();
        let v1 = build(&v1, &request);
        assert_eq!((&dated.headers, &dated.body, &dated.auth), (&v1.headers, &v1.body, &v1.auth));
        assert_ne!(dated.url, v1.url, "{case}");
    }
}

/// Record §3.2: the manifest's default is native's, and the key takes native's dated values only.
#[test]
fn azure_openai_dated_declares_the_native_default_version() {
    assert_eq!(azure_openai_dated::DEFAULT_API_VERSION, NATIVE_AZURE_DEFAULT_API_VERSION);
    assert_eq!(azure_openai_dated::API_VERSION_KEY, "api_version");
    let manifest = shipped_manifest();
    let key =
        &manifest.config_schema[azure_openai_dated::FAMILY][azure_openai_dated::API_VERSION_KEY];
    assert_eq!(key.syntax, ValueSyntaxV1::ApiVersionDate);
    assert!(!key.required);
    assert_eq!(key.default.as_deref(), Some(NATIVE_AZURE_DEFAULT_API_VERSION));
    // `api_version` and the cap field (§4 D6, `azure_families_send_the_cap_as_native_does`).
    assert_eq!(manifest.config_schema[azure_openai_dated::FAMILY].len(), 2);
    assert!(!manifest.endpoint.contains_key(azure_openai_dated::FAMILY));
    assert_eq!(manifest.credentials_for(azure_openai_dated::FAMILY), None);
    assert!(!manifest.subscription_quota_families.contains(azure_openai_dated::FAMILY));
    assert_eq!(manifest.user_agent.get(azure_openai_dated::FAMILY), None);
}

/// Record §4 D3: a value that is not a dated version never reaches the query, even from a host
/// that skipped the manifest's syntax check. `v1` (any case) is the other family's selector.
#[test]
fn azure_openai_dated_refuses_an_undated_version_as_a_configuration_error() {
    for version in
        ["v1", "V1", "latest", "preview", "2024-10-21&x=1", "2024-10-21-beta", "2024-1-21"]
    {
        let (config, request) = azure("https://x.openai.azure.com", Some(version), "d");
        let error = OpenAiCompatibleReferenceV1.build_http_request(&request, &config).unwrap_err();
        assert_eq!((error.code, error.http_status), (ErrorCode::Internal, 500), "{version}");
    }
    for version in ["2024-10-21", "2025-04-01-preview", "1999-99-99"] {
        let (config, request) = azure("https://x.openai.azure.com", Some(version), "d");
        assert!(build(&config, &request).url.ends_with(&format!("?api-version={version}")));
    }
}

/// Record §4 D4 and D5: the deployment is one path segment. An empty one is refused rather than
/// sent as `deployments//`; a `/` is escaped rather than splitting the path.
#[test]
fn azure_openai_dated_puts_the_deployment_in_one_segment() {
    let (config, request) = azure("https://x.openai.azure.com", None, "");
    let error = OpenAiCompatibleReferenceV1.build_http_request(&request, &config).unwrap_err();
    assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400));

    let (config, request) = azure("https://x.openai.azure.com", None, "team/gpt?x");
    let descriptor = build(&config, &request);
    assert_eq!(
        descriptor.url,
        "https://x.openai.azure.com/openai/deployments/team%2Fgpt%3Fx/chat/completions?api-version=2024-10-21"
    );
    assert_eq!(config.authorize(&descriptor), Ok(()));
}

// -- the cap field (record §4 D6) ---------------------------------------------------------------

/// The body key a family writes the cap under, `None` when the request carries no cap.
fn cap_key(descriptor: &HttpRequestDescriptor) -> Option<&'static str> {
    let body = descriptor.body.as_ref().expect("a chat request has a body");
    ["max_completion_tokens", "max_tokens"].into_iter().find(|key| body.get(key).is_some())
}

/// Record §4 D6: native sends the cap of an `azure_openai` row as `max_completion_tokens` unless
/// the row opts out, so both Azure families default to it and take the row's `max_tokens_field`;
/// every other family keeps `max_tokens`, as native does for `glm_coding` and the types served by
/// `openai-compatible`.
#[test]
fn azure_families_send_the_cap_as_native_does() {
    assert_eq!(azure_openai_cap_field::DEFAULT, NATIVE_AZURE_DEFAULT_CAP_FIELD);
    assert_eq!(azure_openai_cap_field::KEY, "max_tokens_field");
    assert_eq!(azure_openai_cap_field::FAMILIES, ["azure-openai-v1", azure_openai_dated::FAMILY]);
    let (dated, request) = parts(AZURE_FIXTURES[0]);
    assert_eq!(request.sampling.max_output_tokens, Some(321));
    let mut v1 = dated.clone();
    v1.provider = "azure-openai-v1".to_owned();
    v1.declared = token_station_protocol::ComponentValues::new();
    v1.base_url =
        serde_json::from_value(json!("https://my-resource.openai.azure.com/openai/v1")).unwrap();
    for config in [&dated, &v1] {
        let family = config.provider.clone();
        assert_eq!(cap_key(&build(config, &request)), Some("max_completion_tokens"), "{family}");
        for (value, key) in
            [("max_completion_tokens", "max_completion_tokens"), ("max_tokens", "max_tokens")]
        {
            let mut chosen = config.clone();
            chosen
                .declared
                .insert(azure_openai_cap_field::KEY, value)
                .expect("a well-formed value");
            let descriptor = build(&chosen, &request);
            assert_eq!(cap_key(&descriptor), Some(key), "{family} {value}");
            assert_eq!(descriptor.body.as_ref().unwrap()[key], json!(321), "{family} {value}");
        }
        let mut uncapped = request.clone();
        uncapped.sampling.max_output_tokens = None;
        assert_eq!(cap_key(&build(config, &uncapped)), None, "{family}");
    }
    let (glm, glm_request) = parts(GLM_FIXTURES[0]);
    assert!(glm_request.sampling.max_output_tokens.is_some());
    for family in ["openai-compatible", glm_coding::FAMILY, "gemini-openai-compatible", "kimi-code"]
    {
        let mut other = glm.clone();
        other.provider = family.to_owned();
        assert_eq!(cap_key(&build(&other, &glm_request)), Some("max_tokens"), "{family}");
    }
}

/// Record §4 D6: the key is declared on both Azure families only, an `enum` of the two wire names
/// with native's default, and a value outside it is refused before any request is built.
#[test]
fn the_cap_field_is_declared_with_the_native_default_and_refuses_other_values() {
    let manifest = shipped_manifest();
    for family in azure_openai_cap_field::FAMILIES {
        let key = &manifest.config_schema[family][azure_openai_cap_field::KEY];
        assert_eq!(
            key.syntax,
            ValueSyntaxV1::Enum(vec!["max_completion_tokens".to_owned(), "max_tokens".to_owned()]),
            "{family}"
        );
        assert!(!key.required, "{family}");
        assert_eq!(key.default.as_deref(), Some(NATIVE_AZURE_DEFAULT_CAP_FIELD), "{family}");
    }
    for family in [
        "openai-compatible",
        "github-copilot",
        "gemini-openai-compatible",
        "glm-coding",
        "kimi-code",
    ] {
        assert!(!manifest.declared_keys(family).contains(azure_openai_cap_field::KEY), "{family}");
    }
    let (config, request) = parts(AZURE_FIXTURES[0]);
    for value in ["none", "max_output_tokens", "MAX_TOKENS", "max_tokens "] {
        let mut chosen = config.clone();
        chosen.declared.insert(azure_openai_cap_field::KEY, value).expect("a well-formed value");
        let error = OpenAiCompatibleReferenceV1.build_http_request(&request, &chosen).unwrap_err();
        assert_eq!((error.code, error.http_status), (ErrorCode::Internal, 500), "{value}");
    }
}

// -- client thinking fields (record §10, T9-Q9 (a)) ---------------------------------------------

const PASSTHROUGH_FIXTURES: [&str; 4] = [
    "provider.request.glm-coding-client-thinking",
    "provider.request.openai-compatible-client-enable-thinking",
    "provider.request.azure-openai-dated-client-thinking",
    "provider.request.gemini-openai-compatible-drops-client-thinking",
];

/// Record §10: native sends the client's Chat body on every type these families serve, so a
/// client's `thinking` or `enable_thinking` reaches the upstream verbatim, `null` included, and is
/// absent when the client sent none. Native builds the Gemini body through the IR, so
/// `gemini-openai-compatible` forwards nothing. Forwarding adds the one key and changes nothing
/// else.
#[test]
fn client_thinking_fields_are_forwarded_as_native_does() {
    assert_eq!(client_passthrough::FIELDS, ["thinking", "enable_thinking"]);
    assert_eq!(
        client_passthrough::FAMILIES,
        [
            "openai-compatible",
            "azure-openai-v1",
            "github-copilot",
            "azure-openai-dated",
            "glm-coding",
            "kimi-code"
        ]
    );
    let (base, request) = parts(GLM_FIXTURES[0]);
    for family in client_passthrough::FAMILIES.into_iter().chain(["gemini-openai-compatible"]) {
        let mut config = base.clone();
        config.provider = family.to_owned();
        let plain = build(&config, &request);
        for field in client_passthrough::FIELDS {
            assert!(plain.body.as_ref().unwrap().get(field).is_none(), "{family} {field}");
            for value in [json!({"type": "disabled"}), json!(false), Value::Null] {
                let mut with = request.clone();
                with.extensions.insert(field.to_owned(), value.clone());
                let mut body = build(&config, &with).body.unwrap();
                let forwarded = body.as_object_mut().unwrap().remove(field);
                let expected = (family != "gemini-openai-compatible").then_some(value);
                assert_eq!(forwarded, expected, "{family} {field}");
                assert_eq!(Some(body), plain.body, "{family} {field}: nothing else changes");
            }
        }
    }
}

/// Record §10: no forwarded field shares a name with a key the package writes, so forwarding can
/// never overwrite the package's own body; an extension that names a written key (here `model`
/// and the cap) is not forwarded at all.
#[test]
fn forwarded_fields_never_collide_with_the_package_body() {
    let mut request: ChatRequest = serde_json::from_value(json!({
        "model": "glm-4.6",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"name": "f", "description": "d", "parameters": {"type": "object"}}],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
        "response_format": {"type": "json_object"},
        "sampling": {"temperature": 0.1, "top_p": 0.5, "max_output_tokens": 9, "stop": ["x"]},
        "reasoning_effort": "high",
        "stream": true
    }))
    .unwrap();
    let (config, _) = parts(GLM_FIXTURES[0]);
    let written = build(&config, &request).body.unwrap();
    for field in client_passthrough::FIELDS {
        assert!(written.get(field).is_none(), "{field}");
    }
    for key in ["model", "max_tokens", "stream"] {
        request.extensions.insert(key.to_owned(), json!("client"));
    }
    assert_eq!(build(&config, &request).body.unwrap(), written);
}

/// The four request rows run under gate ② natively and in the sandbox (`sandbox_parity_v1`).
#[test]
fn gate_two_runs_the_passthrough_cases() {
    let manifest = shipped_manifest();
    let pack = FixturePackV1::load(&root().join("fixtures")).expect("the shipped pack loads");
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &manifest,
    );
    assert!(report.is_passing(), "{report}");
    for case in PASSTHROUGH_FIXTURES {
        assert!(
            report
                .outcomes()
                .iter()
                .any(|outcome| outcome.check == CheckV1::FixtureMatch && outcome.case == case),
            "{case} never ran"
        );
    }
}

// -- reasoning.mode on glm-coding (GLM Messages record §5 O1, §6 Q-GM2 (a); T9 record §10 A+) ---

const REASONING_MODE_FIXTURES: [&str; 5] = [
    "provider.request.glm-coding-reasoning-enabled",
    "provider.request.glm-coding-reasoning-disabled",
    "provider.request.glm-coding-reasoning-adaptive",
    "provider.request.glm-coding-client-thinking-wins-over-reasoning-mode",
    "provider.request.openai-compatible-ignores-reasoning-mode",
];

/// The `thinking` the reference writes for `request` on `config`, `None` when it writes none.
fn thinking_of(config: &ProviderConfig, request: &ChatRequest) -> Option<Value> {
    build(config, request).body.unwrap().get("thinking").cloned()
}

/// GLM Messages record §5 O1 and §6 Q-GM2 (a), lv 2026-10-16: `glm-coding` renders `enabled` and
/// `disabled` as Z.ai's `thinking.type`; `adaptive` and an absent mode write nothing, so the
/// upstream default decides; `budget_tokens` has no GLM carrier and is never written. Rendering
/// adds the one key and changes nothing else.
#[test]
fn glm_coding_renders_the_reasoning_mode_as_thinking() {
    let (config, plain) = parts(GLM_FIXTURES[0]);
    assert!(plain.reasoning.is_unset());
    let written = build(&config, &plain).body.unwrap();
    assert!(written.get("thinking").is_none(), "an absent mode writes nothing");
    for (mode, budget, expected) in [
        ("enabled", None, Some(json!({"type": "enabled"}))),
        ("enabled", Some(2048), Some(json!({"type": "enabled"}))),
        ("disabled", None, Some(json!({"type": "disabled"}))),
        ("disabled", Some(2048), Some(json!({"type": "disabled"}))),
        ("adaptive", None, None),
        ("adaptive", Some(2048), None),
    ] {
        let mut request = plain.clone();
        request.reasoning.mode = Some(serde_json::from_value(json!(mode)).unwrap());
        request.reasoning.budget_tokens = budget;
        let mut body = build(&config, &request).body.unwrap();
        let thinking = body.as_object_mut().unwrap().remove("thinking");
        assert_eq!(thinking, expected, "{mode} {budget:?}");
        assert_eq!(body, written, "{mode} {budget:?}: nothing else changes");
    }
    let mut budget_only = plain;
    budget_only.reasoning.budget_tokens = Some(2048);
    assert_eq!(thinking_of(&config, &budget_only), None, "a budget alone has no GLM carrier");
}

/// A Messages client of a `glm-coding` row: the Anthropic north codec consumes its `thinking` into
/// `reasoning.mode`, and the package writes it back as Z.ai's switch, the budget dropped. Native
/// sends this client's body to Z.ai's Anthropic endpoint (GLM Messages record §2); the wire switch
/// is the registered difference O1 accepts.
#[test]
fn a_messages_client_keeps_its_thinking_switch_on_glm_coding() {
    let (config, _) = parts(GLM_FIXTURES[0]);
    for (thinking, expected) in [
        (json!({"type": "enabled", "budget_tokens": 4096}), Some(json!({"type": "enabled"}))),
        (json!({"type": "disabled"}), Some(json!({"type": "disabled"}))),
        (json!({"type": "adaptive"}), None),
    ] {
        let request = south_north_codec::chat_request_from_anthropic_messages(&json!({
            "model": "glm-4.6",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Weather in Beijing?"}],
            "thinking": thinking,
        }))
        .expect("the Messages body decodes");
        assert!(!request.extensions.contains_key("thinking"), "the codec consumes the key");
        assert_eq!(thinking_of(&config, &request), expected, "{thinking}");
    }
    let request = south_north_codec::chat_request_from_anthropic_messages(&json!({
        "model": "glm-4.6",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Weather in Beijing?"}],
    }))
    .expect("the Messages body decodes");
    assert_eq!(thinking_of(&config, &request), None, "no thinking, no switch");
}

/// T9-Q9 (a) still holds: a Chat client's own `thinking` extension wins over the mode, whatever
/// either says, `null` included.
#[test]
fn a_client_thinking_extension_wins_over_the_reasoning_mode() {
    let (config, plain) = parts(GLM_FIXTURES[0]);
    for mode in ["enabled", "disabled", "adaptive"] {
        for client in [json!({"type": "disabled"}), json!({"type": "enabled"}), Value::Null] {
            let mut request = plain.clone();
            request.reasoning.mode = Some(serde_json::from_value(json!(mode)).unwrap());
            request.extensions.insert("thinking".to_owned(), client.clone());
            assert_eq!(thinking_of(&config, &request), Some(client.clone()), "{mode} {client}");
        }
    }
}

/// Only `glm-coding` renders the mode: every other family of the package writes no `thinking` for
/// it, as before.
#[test]
fn no_other_family_renders_the_reasoning_mode() {
    let (base, plain) = parts(GLM_FIXTURES[0]);
    let declared = shipped_manifest().providers;
    assert!(declared.iter().any(|family| family == glm_coding::FAMILY));
    for family in declared.iter().filter(|family| *family != glm_coding::FAMILY) {
        let mut config = base.clone();
        config.provider.clone_from(family);
        if family == azure_openai_dated::FAMILY || family == "azure-openai-v1" {
            config.base_url =
                serde_json::from_value(json!("https://x.openai.azure.com/openai/v1")).unwrap();
        }
        let before = build(&config, &plain).body;
        for mode in ["enabled", "disabled", "adaptive"] {
            let mut request = plain.clone();
            request.reasoning.mode = Some(serde_json::from_value(json!(mode)).unwrap());
            assert_eq!(build(&config, &request).body, before, "{family} {mode}");
        }
    }
}

/// The five request rows run under gate ② natively and in the sandbox (`sandbox_parity_v1`).
#[test]
fn gate_two_runs_the_reasoning_mode_cases() {
    let manifest = shipped_manifest();
    let pack = FixturePackV1::load(&root().join("fixtures")).expect("the shipped pack loads");
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &manifest,
    );
    assert!(report.is_passing(), "{report}");
    for case in REASONING_MODE_FIXTURES {
        assert!(
            report
                .outcomes()
                .iter()
                .any(|outcome| outcome.check == CheckV1::FixtureMatch && outcome.case == case),
            "{case} never ran"
        );
    }
}

// -- the package -------------------------------------------------------------------------------

/// The family check: a family the package does not declare is refused, so a row naming the
/// native type (`glm_coding`, `azure_openai`) or a near miss never gets another family's route.
#[test]
fn an_undeclared_family_is_refused() {
    let (config, request) = parts(GLM_FIXTURES[0]);
    for family in ["glm_coding", "glm", "azure-openai", "azure_openai", "azure-openai-dated-v1"] {
        let mut other = config.clone();
        other.provider = family.to_owned();
        let error = OpenAiCompatibleReferenceV1.build_http_request(&request, &other).unwrap_err();
        assert_eq!((error.code, error.http_status), (ErrorCode::Capability, 400), "{family}");
        assert!(error.message.contains("unsupported provider dialect"), "{family}");
    }
}

/// The package declares both families, and gate ② runs every T9 case, the refusals included.
#[test]
fn the_package_declares_the_families_and_gate_two_runs_every_case() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.validate(), Ok(()));
    for family in [glm_coding::FAMILY, azure_openai_dated::FAMILY] {
        assert!(manifest.providers.iter().any(|declared| declared == family), "{family}");
    }
    let pack = FixturePackV1::load(&root().join("fixtures")).expect("the shipped pack loads");
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &manifest,
    );
    assert!(report.is_passing(), "{report}");
    for case in GLM_FIXTURES.iter().chain(&AZURE_FIXTURES) {
        assert!(
            report
                .outcomes()
                .iter()
                .any(|outcome| outcome.check == CheckV1::FixtureMatch && outcome.case == *case),
            "{case} never ran"
        );
    }
}

/// Record §8: a minor version in the open cycle, past the patch 2.6.10 it supersedes, reported by
/// the reference and recorded by the crate.
#[test]
fn the_t9_families_retire_the_patch_identity() {
    let manifest = shipped_manifest();
    for retired in ["2.6.9", "2.6.10"] {
        assert_ne!(manifest.version, retired);
    }
    // Published as 2.7.0 with 0.66.0; the speech Azure OpenAI families' Q47 bump made it 2.7.1,
    // published with 0.67.0. The cap field (§4 D6) changes what the two Azure families send, so
    // the package takes the minor 2.8.0
    // (`shipped_packages_v1::t9_cap_field_retires_every_published_067_package_identity`).
    assert_ne!(manifest.version, "2.7.1");
    // Published as 2.8.0 with 0.68.0; A+ on `glm-coding` (GLM Messages record §5 O1) changes what
    // the family sends, so the package takes the minor 2.9.0
    // (`shipped_packages_v1::glm_thinking_retires_every_published_068_package_identity`).
    assert_ne!(manifest.version, "2.8.0");
    // Published as 2.9.0 with 0.69.0; the `kimi-code` family (Kimi Code record §8) adds a family,
    // so the package takes the minor 2.10.0
    // (`shipped_packages_v1::kimi_code_retires_every_published_069_package_identity`).
    assert_ne!(manifest.version, "2.9.0");
    // Published as 2.10.0 with 0.70.0; the Vertex task credentials' Q47 bump made it 2.10.1
    // (`shipped_packages_v1::task_vertex_credentials_retires_every_published_070_package_identity`).
    assert_ne!(manifest.version, "2.10.0");
    // Published as 2.10.1 with 0.71.0; the explicit cache reads (a new reported usage bucket) make
    // it the minor 2.11.0
    // (`shipped_packages_v1::explicit_cache_reads_retires_every_published_071_package_identity`).
    assert_ne!(manifest.version, "2.10.1");
    // Published as 2.11.0 with 0.72.0; the Messages pass-through record's Q47 bump made it 2.11.1
    // (`messages_passthrough_retires_every_published_072_package_identity`).
    assert_eq!(manifest.version, "2.11.1");
    assert_eq!(OpenAiCompatibleReferenceV1.metadata().version, manifest.version);
    assert_eq!(manifest.compatibility.south_runtime, "0.60.0");
}
