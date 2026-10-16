//! The `kimi-code` family of `provider-openai-compatible` (design record
//! `docs/design/2026-10-16-kimi-code-family.md`): the Kimi Code (Moonshot AI) coding subscription.
//!
//! Transcribed from the host's native `kimi_code` arm (record §2): the values pinned here are the
//! native constants and the native URL and auth tests' cases, not values derived from this
//! reference.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference::{
    OpenAiCompatibleReferenceV1, client_passthrough, glm_coding, kimi_code,
};
use south_component_conformance::{
    AdmittedAuthV1, CheckV1, FixturePackV1, ProviderComponentV1, admit_descriptor_auth,
    run_provider_component_suite_v1_for_manifest,
};
use token_station_protocol::{
    Auth, ChatRequest, HttpRequestDescriptor, ProviderConfig, SafeHeaders,
};

/// Native `ProviderType::KimiCode`'s preset endpoint (server `providers.rs`), written out.
const NATIVE_PRESET_ENDPOINT: &str = "https://api.moonshot.cn";
/// Native `kimi_code_url_uses_default_openai_compat_path` (server `upstream/tests.rs`): the Chat
/// URL on the preset endpoint, written out.
const NATIVE_CHAT_URL: &str = "https://api.moonshot.cn/v1/chat/completions";

const KIMI_FIXTURES: [&str; 5] = [
    "provider.request.kimi-code",
    "provider.request.kimi-code-stream",
    "provider.request.kimi-code-endpoint-with-a-path",
    "provider.request.kimi-code-client-thinking",
    "provider.request.kimi-code-ignores-reasoning-mode",
];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn shipped_manifest() -> south_provider_api::ComponentManifestV1 {
    let path = root().join("../../components/provider-openai-compatible/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("the manifest reads"))
        .expect("the manifest parses")
}

fn parts(case: &str) -> (ProviderConfig, ChatRequest) {
    let path = root().join("fixtures").join(format!("{case}.input.json"));
    let input: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
            .expect("the fixture parses");
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

fn with_base(config: &ProviderConfig, base: &str) -> ProviderConfig {
    let mut config = config.clone();
    config.base_url = serde_json::from_value(json!(base)).unwrap();
    config
}

/// Record §2: native's default `build_upstream_url` arm appends the canonical path to the trimmed
/// endpoint, so the preset origin gives native's tested URL, a trailing slash changes nothing, and
/// an endpoint the operator edited to carry a path keeps it below `/v1/chat/completions`.
#[test]
fn kimi_code_posts_to_the_native_default_url() {
    assert_eq!(kimi_code::CHAT_PATH, "/v1/chat/completions");
    assert_eq!(kimi_code::NATIVE_PRESET_ENDPOINT, NATIVE_PRESET_ENDPOINT);
    let (config, request) = parts(KIMI_FIXTURES[0]);
    for (base, url) in [
        (NATIVE_PRESET_ENDPOINT, NATIVE_CHAT_URL),
        ("https://api.moonshot.cn/", NATIVE_CHAT_URL),
        ("https://api.kimi.com/coding", "https://api.kimi.com/coding/v1/chat/completions"),
        ("https://api.kimi.com/coding/", "https://api.kimi.com/coding/v1/chat/completions"),
    ] {
        let config = with_base(&config, base);
        let descriptor = build(&config, &request);
        assert_eq!(descriptor.url, url, "{base}");
        assert_eq!(config.authorize(&descriptor), Ok(()), "{base}");
    }
}

/// Record §3 and §4 D1: on an origin the family's URL is the one `openai-compatible` resolves from the
/// same base (the kernel's `/v1` default); on an endpoint with a path the two differ, and only the
/// family's matches native.
#[test]
fn kimi_code_agrees_with_openai_compatible_on_an_origin_only() {
    let (config, request) = parts(KIMI_FIXTURES[0]);
    let mut plain = config.clone();
    plain.provider = "openai-compatible".to_owned();
    assert_eq!(build(&config, &request), build(&plain, &request));

    let pathful = with_base(&config, "https://api.kimi.com/coding");
    let mut plain = pathful.clone();
    plain.provider = "openai-compatible".to_owned();
    assert_eq!(build(&plain, &request).url, "https://api.kimi.com/coding/chat/completions");
    assert_ne!(build(&pathful, &request).url, build(&plain, &request).url);
    // A row moved to `openai-compatible` would need the base `{endpoint}/v1` to reach native's URL.
    let plain = with_base(&plain, "https://api.kimi.com/coding/v1");
    assert_eq!(build(&plain, &request), build(&pathful, &request));
}

/// Record §2: native's `KimiCode` auth arm sends `Authorization: Bearer <api_key>` and nothing
/// else (`kimi_code_sets_bearer_auth_without_user_agent_impersonation`); the descriptor carries
/// `content-type` only, and the host admits the bearer from the static slot.
#[test]
fn kimi_code_presents_the_key_as_a_bearer_and_no_other_header() {
    let manifest = shipped_manifest();
    for case in KIMI_FIXTURES {
        let (config, request) = parts(case);
        let descriptor = build(&config, &request);
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
    let (mut config, request) = parts(KIMI_FIXTURES[0]);
    config.auth = None;
    assert_eq!(build(&config, &request).auth, None);
}

/// Record §2 and §3: the family is a subscription (native `is_coding_plan`), which is why it
/// exists; it declares no user-agent (native impersonates none, unlike `glm-coding`), no endpoint
/// template (the operator's endpoint is taken as is), no config keys and no credential recipe.
#[test]
fn kimi_code_declares_a_subscription_quota_and_nothing_else() {
    let manifest = shipped_manifest();
    assert!(manifest.providers.iter().any(|family| family == kimi_code::FAMILY));
    assert_eq!(
        manifest.subscription_quota_families,
        BTreeSet::from([
            "github-copilot".to_owned(),
            glm_coding::FAMILY.to_owned(),
            kimi_code::FAMILY.to_owned()
        ])
    );
    assert_eq!(manifest.user_agent.get(kimi_code::FAMILY), None);
    let instances = south_component_conformance::DeclaredInstancesV1::from_manifest(&manifest)
        .expect("the shipped manifest passes gate ①");
    assert_eq!(instances.user_agent(kimi_code::FAMILY), None);
    assert!(!manifest.endpoint.contains_key(kimi_code::FAMILY));
    assert!(!manifest.config_schema.contains_key(kimi_code::FAMILY));
    assert!(manifest.declared_keys(kimi_code::FAMILY).is_empty());
    assert_eq!(manifest.credentials_for(kimi_code::FAMILY), None);
    assert_eq!(
        manifest.request_facts_for(kimi_code::FAMILY),
        south_provider_api::RequestFactsV1::top_level()
    );
}

/// Record §2: native sends a `kimi_code` row's client Chat body itself, so a client's `thinking`
/// or `enable_thinking` reaches Moonshot verbatim, `null` included; forwarding adds the one key.
#[test]
fn kimi_code_forwards_the_client_thinking_fields() {
    assert!(client_passthrough::FAMILIES.contains(&kimi_code::FAMILY));
    let (config, request) = parts(KIMI_FIXTURES[0]);
    let plain = build(&config, &request);
    for field in client_passthrough::FIELDS {
        assert!(plain.body.as_ref().unwrap().get(field).is_none(), "{field}");
        for value in
            [json!({"type": "disabled"}), json!({"type": "enabled"}), json!(false), Value::Null]
        {
            let mut with = request.clone();
            with.extensions.insert(field.to_owned(), value.clone());
            let mut body = build(&config, &with).body.unwrap();
            assert_eq!(body.as_object_mut().unwrap().remove(field), Some(value), "{field}");
            assert_eq!(Some(body), plain.body, "{field}: nothing else changes");
        }
    }
}

/// Record §2 and D3: native has no `-thinking` mapping for `kimi_code` and renders a Messages
/// client's request through the `openai-compatible` translator, which writes no `thinking`; the
/// family does the same. A+ (`reasoning.mode` as `thinking`) stays `glm-coding`'s alone.
#[test]
fn kimi_code_writes_no_thinking_from_the_reasoning_mode() {
    let (config, plain) = parts(KIMI_FIXTURES[0]);
    let before = build(&config, &plain).body;
    for mode in ["enabled", "disabled", "adaptive"] {
        let mut request = plain.clone();
        request.reasoning.mode = Some(serde_json::from_value(json!(mode)).unwrap());
        request.reasoning.budget_tokens = Some(2048);
        assert_eq!(build(&config, &request).body, before, "{mode}");
    }
    let messages = south_north_codec::chat_request_from_anthropic_messages(&json!({
        "model": "kimi-k2-0905-preview",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Weather in Beijing?"}],
        "thinking": {"type": "enabled", "budget_tokens": 4096},
    }))
    .expect("the Messages body decodes");
    let body = build(&config, &messages).body.unwrap();
    assert!(body.get("thinking").is_none());
    assert_eq!(body["max_tokens"], json!(1024));
    assert_eq!(build(&config, &messages).url, NATIVE_CHAT_URL);
}

/// Record §2: native rewrites a `kimi_code` row's cap to `max_tokens` (`needs_max_tokens_rewrite`'s
/// `_ => true` arm); the family keeps the package's `max_tokens`.
#[test]
fn kimi_code_sends_the_cap_as_max_tokens() {
    let (config, request) = parts(KIMI_FIXTURES[0]);
    assert_eq!(request.sampling.max_output_tokens, Some(1024));
    let body = build(&config, &request).body.unwrap();
    assert_eq!(body["max_tokens"], json!(1024));
    assert!(body.get("max_completion_tokens").is_none());
}

/// A near miss of the family name (the native type's spelling among them) is refused rather than
/// served as another family.
#[test]
fn a_near_miss_of_the_family_is_refused() {
    let (config, request) = parts(KIMI_FIXTURES[0]);
    for family in ["kimi_code", "kimi", "kimi-code-v1", "moonshot", "Kimi-Code"] {
        let mut other = config.clone();
        other.provider = family.to_owned();
        let error = OpenAiCompatibleReferenceV1.build_http_request(&request, &other).unwrap_err();
        assert!(error.message.contains("unsupported provider dialect"), "{family}");
    }
}

/// Gate ② runs every `kimi-code` row natively; `sandbox_parity_v1` runs the same pack in the
/// sandbox.
#[test]
fn gate_two_runs_every_kimi_code_case() {
    let manifest = shipped_manifest();
    assert_eq!(manifest.validate(), Ok(()));
    let pack = FixturePackV1::load(&root().join("fixtures")).expect("the shipped pack loads");
    let report = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &manifest,
    );
    assert!(report.is_passing(), "{report}");
    for case in KIMI_FIXTURES {
        assert!(
            report
                .outcomes()
                .iter()
                .any(|outcome| outcome.check == CheckV1::FixtureMatch && outcome.case == case),
            "{case} never ran"
        );
    }
}
