//! The 0.73.0 provider-world declarations of the Messages pass-through record
//! (`docs/design/2026-10-17-messages-passthrough-and-count-tokens.md`): `messages` request
//! pass-through (M1, §3.1), `forwarded_client_headers` (M2, §4.1) and `count_tokens` (M3, §5.1).
//! Gate ① admits each on the provider world only, for declared families only, under the rules the
//! record lists, and `rewrite_count_tokens_path` is the one suffix replacement hosts and gate ②
//! share.

use std::path::Path;

use serde_json::json;
use south_provider_api::{
    ComponentManifestV1, CountTokensDeclarationV1, FORWARDING_REFUSED_HEADERS,
    MAX_COUNT_TOKENS_PATH_SUFFIX_BYTES, MAX_FORWARDED_CLIENT_HEADERS, MESSAGES_OUTPUT_CAP,
    ManifestErrorV1, ModelLocationV1, NorthProtocolV1, PathSuffixV1, RequestFactsV1,
    StreamLocationV1, rewrite_count_tokens_path,
};

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

fn counting(protocol: NorthProtocolV1, from: &str, to: &str) -> CountTokensDeclarationV1 {
    CountTokensDeclarationV1 {
        protocol,
        path_suffix: PathSuffixV1 { from: from.to_owned(), to: to.to_owned() },
    }
}

fn gemini_counting() -> CountTokensDeclarationV1 {
    counting(NorthProtocolV1::GenerateContent, ":generateContent", ":countTokens")
}

fn messages_counting() -> CountTokensDeclarationV1 {
    counting(NorthProtocolV1::Messages, "/v1/messages", "/v1/messages/count_tokens")
}

fn messages_facts() -> RequestFactsV1 {
    RequestFactsV1 {
        output_cap: vec![MESSAGES_OUTPUT_CAP.to_owned()],
        model: ModelLocationV1::Body("/model".to_owned()),
        stream: StreamLocationV1::Body("/stream".to_owned()),
    }
}

/// `provider-anthropic` without the 0.73.0 declarations, so each test adds what it needs.
fn anthropic_bare() -> ComponentManifestV1 {
    let mut manifest = shipped("provider-anthropic");
    manifest.request_facts.clear();
    manifest.north_passthrough.clear();
    manifest.north_request_passthrough.clear();
    manifest.forwarded_client_headers.clear();
    manifest.count_tokens.clear();
    manifest
}

/// `anthropic` passing Messages through on both sides, with the wire's request facts.
fn anthropic_passing_through() -> ComponentManifestV1 {
    let mut manifest = anthropic_bare();
    manifest.request_facts.insert("anthropic".to_owned(), messages_facts());
    manifest.north_passthrough.insert("anthropic".to_owned(), NorthProtocolV1::Messages);
    manifest.north_request_passthrough.insert("anthropic".to_owned(), NorthProtocolV1::Messages);
    manifest
}

fn detail_of(result: Result<(), ManifestErrorV1>) -> String {
    match result {
        Err(
            ManifestErrorV1::InvalidNorthRequestPassthrough { detail, .. }
            | ManifestErrorV1::InvalidForwardedClientHeaders { detail, .. }
            | ManifestErrorV1::InvalidCountTokens { detail, .. },
        ) => detail,
        other => panic!("expected a pass-through declaration refusal, got {other:?}"),
    }
}

// M1: `messages` request pass-through (§3.1).

#[test]
fn messages_request_passthrough_is_admitted_with_the_messages_wire_facts() {
    let manifest = anthropic_passing_through();
    assert_eq!(manifest.validate(), Ok(()));
    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(wire["north_request_passthrough"], json!({"anthropic": "messages"}));
    let back: ComponentManifestV1 = serde_json::from_value(wire).unwrap();
    assert_eq!(back, manifest);
}

#[test]
fn messages_request_passthrough_needs_the_messages_wire_facts() {
    for edit in [
        (|facts: &mut RequestFactsV1| facts.output_cap = vec!["/max_output_tokens".to_owned()])
            as fn(&mut RequestFactsV1),
        |facts| facts.output_cap.clear(),
        |facts| facts.model = ModelLocationV1::Url("/models/{model}".to_owned()),
        |facts| facts.model = ModelLocationV1::Body("/model_id".to_owned()),
        |facts| facts.stream = StreamLocationV1::Url,
        |facts| facts.stream = StreamLocationV1::None,
    ] {
        let mut manifest = anthropic_passing_through();
        edit(manifest.request_facts.get_mut("anthropic").unwrap());
        let detail = detail_of(manifest.validate());
        assert!(detail.contains("request_facts must put the cap at /max_tokens"), "{detail}");
    }
    let mut manifest = anthropic_passing_through();
    manifest.request_facts.clear();
    let detail = detail_of(manifest.validate());
    assert!(detail.contains("request_facts must put the cap at /max_tokens"), "{detail}");
}

#[test]
fn messages_request_passthrough_needs_the_response_side_too() {
    let mut manifest = anthropic_passing_through();
    manifest.north_passthrough.clear();
    assert!(detail_of(manifest.validate()).contains("response side must pass through"));
}

#[test]
fn chat_completions_and_responses_stay_refused_as_request_passthrough() {
    for protocol in [NorthProtocolV1::ChatCompletions, NorthProtocolV1::Responses] {
        let mut manifest = anthropic_passing_through();
        manifest.north_passthrough.insert("anthropic".to_owned(), protocol);
        manifest.north_request_passthrough.insert("anthropic".to_owned(), protocol);
        let detail = detail_of(manifest.validate());
        assert!(detail.contains("only generate_content and messages"), "{detail}");
    }
}

// M2: `forwarded_client_headers` (§4.1).

#[test]
fn absent_forwarding_and_counting_are_omitted_and_change_nothing() {
    let manifest = shipped("provider-openai-compatible");
    assert!(manifest.forwarded_client_headers.is_empty());
    assert!(manifest.count_tokens.is_empty());
    let wire = serde_json::to_value(&manifest).unwrap();
    assert!(wire.get("forwarded_client_headers").is_none());
    assert!(wire.get("count_tokens").is_none());
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn a_passthrough_family_may_forward_declared_client_headers() {
    let mut manifest = anthropic_passing_through();
    manifest.forwarded_client_headers.insert("anthropic".to_owned(), names(&["anthropic-beta"]));
    assert_eq!(manifest.validate(), Ok(()));
    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(wire["forwarded_client_headers"], json!({"anthropic": ["anthropic-beta"]}));
    let back: ComponentManifestV1 = serde_json::from_value(wire).unwrap();
    assert_eq!(back, manifest);
}

#[test]
fn forwarding_is_refused_unless_every_condition_holds() {
    let refused = |edit: &dyn Fn(&mut ComponentManifestV1), needle: &str| {
        let mut manifest = anthropic_passing_through();
        manifest
            .forwarded_client_headers
            .insert("anthropic".to_owned(), names(&["anthropic-beta"]));
        edit(&mut manifest);
        let detail = detail_of(manifest.validate());
        assert!(detail.contains(needle), "{needle}: {detail}");
    };
    refused(
        &|m| {
            m.forwarded_client_headers.insert("other".to_owned(), names(&["anthropic-beta"]));
        },
        "does not declare",
    );
    refused(
        &|m| {
            m.north_request_passthrough.clear();
        },
        "only a request pass-through family",
    );
    for bad in [
        names(&["Anthropic-Beta"]),
        names(&[""]),
        names(&["anthropic beta"]),
        names(&["anthropic-beta", "anthropic-beta"]),
        (0..=MAX_FORWARDED_CLIENT_HEADERS).map(|index| format!("x-forward-{index}")).collect(),
        vec!["a".repeat(65)],
    ] {
        refused(
            &move |m| {
                m.forwarded_client_headers.insert("anthropic".to_owned(), bad.clone());
            },
            "distinct lowercase RFC 9110 tchar",
        );
    }
    for reserved in FORWARDING_REFUSED_HEADERS {
        refused(
            &|m| {
                m.forwarded_client_headers.insert("anthropic".to_owned(), names(&[*reserved]));
            },
            "reserved",
        );
    }
    refused(
        &|m| {
            m.immutable_request_headers.insert("anthropic".to_owned(), names(&["anthropic-beta"]));
        },
        "immutable_request_headers",
    );
}

#[test]
fn a_forwarded_header_may_not_be_a_declared_secret_header() {
    let mut manifest = shipped("provider-openai-compatible");
    let family = manifest.providers[0].clone();
    manifest.auth_arms.insert("header_secret".to_owned());
    manifest.secret_headers = names(&["x-forwarded-secret"]);
    manifest.request_facts.insert(
        family.clone(),
        RequestFactsV1 {
            output_cap: vec![MESSAGES_OUTPUT_CAP.to_owned()],
            model: ModelLocationV1::Body("/model".to_owned()),
            stream: StreamLocationV1::Body("/stream".to_owned()),
        },
    );
    manifest.north_passthrough.insert(family.clone(), NorthProtocolV1::Messages);
    manifest.north_request_passthrough.insert(family.clone(), NorthProtocolV1::Messages);
    manifest.forwarded_client_headers.insert(family, names(&["x-forwarded-secret"]));
    assert!(detail_of(manifest.validate()).contains("secret_headers"));
}

#[test]
fn the_refused_list_names_only_lowercase_tchar_headers() {
    assert!(FORWARDING_REFUSED_HEADERS.contains(&"authorization"));
    assert!(FORWARDING_REFUSED_HEADERS.contains(&"x-api-key"));
    assert!(FORWARDING_REFUSED_HEADERS.contains(&"x-amz-security-token"));
    for name in FORWARDING_REFUSED_HEADERS {
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
            "{name}"
        );
    }
}

// M3: `count_tokens` (§5.1).

#[test]
fn a_passthrough_family_may_declare_counting_for_its_protocol() {
    let mut manifest = anthropic_passing_through();
    manifest.count_tokens.insert("anthropic".to_owned(), messages_counting());
    assert_eq!(manifest.validate(), Ok(()));
    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        wire["count_tokens"],
        json!({"anthropic": {
            "protocol": "messages",
            "path_suffix": {"from": "/v1/messages", "to": "/v1/messages/count_tokens"}
        }})
    );
    let back: ComponentManifestV1 = serde_json::from_value(wire).unwrap();
    assert_eq!(back, manifest);

    let mut gemini = shipped("provider-gemini");
    gemini.count_tokens.insert("gemini".to_owned(), gemini_counting());
    assert_eq!(gemini.validate(), Ok(()));
}

#[test]
fn a_counting_declaration_refuses_unknown_keys() {
    let mut wire = serde_json::to_value(anthropic_passing_through()).unwrap();
    wire["count_tokens"] = json!({"anthropic": {
        "protocol": "messages",
        "path_suffix": {"from": "/v1/messages", "to": "/v1/messages/count_tokens"},
        "billing": "none"
    }});
    assert!(serde_json::from_value::<ComponentManifestV1>(wire).is_err());
}

#[test]
fn counting_is_refused_unless_every_condition_holds() {
    let refused = |edit: &dyn Fn(&mut ComponentManifestV1), needle: &str| {
        let mut manifest = anthropic_passing_through();
        manifest.count_tokens.insert("anthropic".to_owned(), messages_counting());
        edit(&mut manifest);
        let detail = detail_of(manifest.validate());
        assert!(detail.contains(needle), "{needle}: {detail}");
    };
    refused(
        &|m| {
            m.count_tokens.insert("other".to_owned(), messages_counting());
        },
        "does not declare",
    );
    for protocol in [NorthProtocolV1::ChatCompletions, NorthProtocolV1::Responses] {
        refused(
            &move |m| {
                m.count_tokens.insert(
                    "anthropic".to_owned(),
                    counting(protocol, "/v1/messages", "/v1/messages/count_tokens"),
                );
            },
            "only generate_content and messages",
        );
    }
    refused(
        &|m| {
            m.count_tokens.insert("anthropic".to_owned(), gemini_counting());
        },
        "the same protocol in north_request_passthrough",
    );
    refused(
        &|m| {
            m.north_request_passthrough.clear();
            m.north_passthrough.clear();
        },
        "the same protocol in north_request_passthrough",
    );
    let long = "a".repeat(MAX_COUNT_TOKENS_PATH_SUFFIX_BYTES + 1);
    for (from, to) in [
        ("", "/v1/messages/count_tokens"),
        ("/v1/messages", ""),
        ("/v1/messages", "/v1/messages"),
        ("/v1/messages", "/v1/messages/count_tokens?beta=true"),
        ("/v1/messages#x", "/v1/messages/count_tokens"),
        ("/v1/messages", "/v1/messages/%63ount"),
        ("/v1/messages", "/v1/{model}/count"),
        ("/v1/messages", "/v1/messages/count tokens"),
        ("/v1/messages", long.as_str()),
    ] {
        let declaration = counting(NorthProtocolV1::Messages, from, to);
        refused(
            &move |m| {
                m.count_tokens.insert("anthropic".to_owned(), declaration.clone());
            },
            "path_suffix",
        );
    }
}

#[test]
fn other_worlds_refuse_forwarding_and_counting() {
    for package in ["embeddings-openai-compatible", "task-kling-v2", "image-azure"] {
        let mut manifest = shipped(package);
        let family = manifest.providers[0].clone();
        manifest.forwarded_client_headers.insert(family.clone(), names(&["anthropic-beta"]));
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );
        let mut manifest = shipped(package);
        manifest.count_tokens.insert(family, gemini_counting());
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );
    }
}

// The shared rewrite (§5.2 step 2).

#[test]
fn the_rewrite_replaces_the_declared_path_suffix_and_keeps_everything_else() {
    let gemini = gemini_counting();
    assert_eq!(
        rewrite_count_tokens_path(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent",
            &gemini.path_suffix,
        )
        .as_deref(),
        Some("https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:countTokens")
    );
    assert_eq!(
        rewrite_count_tokens_path(
            "https://us-central1-aiplatform.googleapis.com/v1/projects/p/locations/us-central1/\
             publishers/google/models/gemini-2.5-pro:generateContent?key=v",
            &gemini.path_suffix,
        )
        .as_deref(),
        Some(
            "https://us-central1-aiplatform.googleapis.com/v1/projects/p/locations/us-central1/\
             publishers/google/models/gemini-2.5-pro:countTokens?key=v"
        )
    );
    let messages = messages_counting();
    assert_eq!(
        rewrite_count_tokens_path("http://127.0.0.1:8080/v1/messages", &messages.path_suffix)
            .as_deref(),
        Some("http://127.0.0.1:8080/v1/messages/count_tokens")
    );
}

#[test]
fn the_rewrite_refuses_a_path_without_the_suffix() {
    let gemini = gemini_counting();
    for url in [
        "https://generativelanguage.googleapis.com/v1beta/models/m:streamGenerateContent?alt=sse",
        "https://generativelanguage.googleapis.com/v1beta/models/m:generateContent/extra",
        // The suffix in the query, not the path.
        "https://example.com/v1/models/m?next=:generateContent",
        // The suffix as the host name, with no path at all.
        "https://example.com:generateContent",
        "not a url",
        "",
    ] {
        assert_eq!(rewrite_count_tokens_path(url, &gemini.path_suffix), None, "{url}");
    }
}
