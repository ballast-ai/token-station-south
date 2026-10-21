//! `immutable_body_paths` and `north_passthrough`, the two manifest fields the `OpenAI` Responses
//! upstream record proposes (`docs/design/2026-09-30-openai-responses-upstream-component.md` §3.4,
//! D7, D11): gate ① admits them on the provider world only, for declared families only, with the
//! task world's immutable-path grammar and a closed protocol set.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::json;
use south_provider_api::{
    ComponentManifestV1, MAX_IMMUTABLE_BODY_PATH_BYTES, MAX_IMMUTABLE_BODY_PATHS,
    MAX_IMMUTABLE_REQUEST_HEADER_NAME_BYTES, MAX_IMMUTABLE_REQUEST_HEADERS, ManifestErrorV1,
    NorthProtocolV1, are_immutable_body_paths, are_immutable_request_headers,
};

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn openai() -> ComponentManifestV1 {
    shipped("provider-openai-compatible")
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|path| (*path).to_owned()).collect()
}

#[test]
fn absent_declarations_are_omitted_and_change_nothing() {
    // `provider-openai-compatible` declares a path for `bailian-openai-compatible` since the Bailian
    // thinking cap record; `provider-kiro` declares neither field.
    let manifest = shipped("provider-kiro");
    assert!(manifest.immutable_body_paths.is_empty());
    assert!(manifest.north_passthrough.is_empty());
    let wire = serde_json::to_value(&manifest).unwrap();
    assert!(wire.get("immutable_body_paths").is_none());
    assert!(wire.get("north_passthrough").is_none());
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn a_declared_family_may_fix_paths_and_name_its_northbound_protocol() {
    let mut manifest = openai();
    manifest.immutable_body_paths =
        BTreeMap::from([("openai-compatible".to_owned(), paths(&["store", "text.format"]))]);
    manifest.north_passthrough =
        BTreeMap::from([("openai-compatible".to_owned(), NorthProtocolV1::ChatCompletions)]);
    assert_eq!(manifest.validate(), Ok(()));

    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        wire["immutable_body_paths"],
        json!({"openai-compatible": ["store", "text.format"]})
    );
    assert_eq!(wire["north_passthrough"], json!({"openai-compatible": "chat_completions"}));
    let back: ComponentManifestV1 = serde_json::from_value(wire).unwrap();
    assert_eq!(back, manifest);
}

#[test]
fn the_protocol_set_is_closed_and_spelled_apart_from_family_names() {
    for (word, protocol) in [
        ("chat_completions", NorthProtocolV1::ChatCompletions),
        ("responses", NorthProtocolV1::Responses),
        ("messages", NorthProtocolV1::Messages),
    ] {
        assert_eq!(serde_json::from_value::<NorthProtocolV1>(json!(word)).unwrap(), protocol);
    }
    for word in ["openai-responses", "Responses", "chat-completions", "anthropic", ""] {
        assert!(serde_json::from_value::<NorthProtocolV1>(json!(word)).is_err(), "{word}");
    }
    let mut wire = serde_json::to_value(openai()).unwrap();
    wire["north_passthrough"] = json!({"openai-compatible": "openai-responses"});
    assert!(serde_json::from_value::<ComponentManifestV1>(wire).is_err());
}

#[test]
fn an_undeclared_family_is_refused_by_name() {
    let mut fixed = openai();
    fixed.immutable_body_paths =
        BTreeMap::from([("openai-responses".to_owned(), paths(&["store"]))]);
    assert!(matches!(
        fixed.validate(),
        Err(ManifestErrorV1::InvalidImmutableBodyPaths { family, .. }) if family == "openai-responses"
    ));

    let mut passthrough = openai();
    passthrough.north_passthrough =
        BTreeMap::from([("openai-responses".to_owned(), NorthProtocolV1::Responses)]);
    assert!(matches!(
        passthrough.validate(),
        Err(ManifestErrorV1::InvalidNorthPassthrough { family, .. }) if family == "openai-responses"
    ));
}

#[test]
fn paths_follow_the_task_world_grammar() {
    for good in [&["store"][..], &["text.format", "reasoning-effort", "a_b.c-d.E9"], &[]] {
        assert!(are_immutable_body_paths(&paths(good)), "{good:?}");
    }
    let long = "a".repeat(MAX_IMMUTABLE_BODY_PATH_BYTES + 1);
    for bad in [
        &[""][..],
        &["store."],
        &[".store"],
        &["text..format"],
        &["input[0]"],
        &["/store"],
        &["st ore"],
        &["store", "store"],
        &[long.as_str()],
    ] {
        assert!(!are_immutable_body_paths(&paths(bad)), "{bad:?}");
        let mut manifest = openai();
        manifest.immutable_body_paths =
            BTreeMap::from([("openai-compatible".to_owned(), paths(bad))]);
        assert!(
            matches!(manifest.validate(), Err(ManifestErrorV1::InvalidImmutableBodyPaths { .. })),
            "{bad:?}"
        );
    }
    let at_limit = "a".repeat(MAX_IMMUTABLE_BODY_PATH_BYTES);
    assert!(are_immutable_body_paths(&paths(&[at_limit.as_str()])));
    let many: Vec<String> = (0..=MAX_IMMUTABLE_BODY_PATHS).map(|n| format!("p{n}")).collect();
    assert!(!are_immutable_body_paths(&many));
    assert!(are_immutable_body_paths(&many[..MAX_IMMUTABLE_BODY_PATHS]));
}

#[test]
fn other_worlds_refuse_both_declarations() {
    for package in ["embeddings-openai-compatible", "task-kling-v2", "image-azure"] {
        let manifest = shipped(package);
        let family = manifest.providers[0].clone();

        let mut fixed = manifest.clone();
        fixed.immutable_body_paths = BTreeMap::from([(family.clone(), paths(&["store"]))]);
        assert_eq!(
            fixed.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );

        let mut passthrough = manifest;
        passthrough.north_passthrough = BTreeMap::from([(family, NorthProtocolV1::Responses)]);
        assert_eq!(
            passthrough.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );
    }
}

/// `north_request_passthrough` (the `generateContent` request pass-through record §3.1): the shipped
/// `provider-gemini` declares it for `gemini` on both sides, and gate ① admits it.
#[test]
fn the_gemini_package_passes_generate_content_through_on_both_sides() {
    let manifest = shipped("provider-gemini");
    assert_eq!(
        manifest.north_request_passthrough.get("gemini"),
        Some(&NorthProtocolV1::GenerateContent)
    );
    assert_eq!(manifest.north_passthrough.get("gemini"), Some(&NorthProtocolV1::GenerateContent));
    assert_eq!(manifest.validate(), Ok(()));
    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(wire["north_request_passthrough"], json!({"gemini": "generate_content"}));
    let back: ComponentManifestV1 = serde_json::from_value(wire).unwrap();
    assert_eq!(back, manifest);
    assert!(!NorthProtocolV1::GenerateContent.has_north_codec());
    for protocol in
        [NorthProtocolV1::ChatCompletions, NorthProtocolV1::Responses, NorthProtocolV1::Messages]
    {
        assert!(protocol.has_north_codec(), "{protocol:?}");
    }
}

#[test]
fn request_passthrough_is_refused_unless_every_condition_holds() {
    let refused = |edit: &dyn Fn(&mut ComponentManifestV1), needle: &str| {
        let mut manifest = shipped("provider-gemini");
        edit(&mut manifest);
        match manifest.validate() {
            Err(ManifestErrorV1::InvalidNorthRequestPassthrough { detail, .. }) => {
                assert!(detail.contains(needle), "{detail}");
            }
            other => panic!("{needle}: {other:?}"),
        }
    };
    refused(
        &|m| {
            m.north_request_passthrough =
                BTreeMap::from([("other".to_owned(), NorthProtocolV1::GenerateContent)]);
        },
        "does not declare",
    );
    refused(
        &|m| {
            m.north_request_passthrough =
                BTreeMap::from([("gemini".to_owned(), NorthProtocolV1::ChatCompletions)]);
            m.north_passthrough =
                BTreeMap::from([("gemini".to_owned(), NorthProtocolV1::ChatCompletions)]);
        },
        "only generate_content and messages",
    );
    refused(&|m| m.north_passthrough.clear(), "response side must pass through");
    refused(
        &|m| {
            m.request_facts.get_mut("gemini").unwrap().output_cap = vec!["/max_tokens".to_owned()];
        },
        "request_facts must put the cap",
    );
    refused(
        &|m| {
            m.request_facts.get_mut("gemini").unwrap().stream =
                south_provider_api::StreamLocationV1::Body("/stream".to_owned());
        },
        "request_facts must put the cap",
    );
}

#[test]
fn other_worlds_refuse_request_passthrough() {
    for package in ["embeddings-openai-compatible", "task-kling-v2", "image-azure"] {
        let mut manifest = shipped(package);
        let family = manifest.providers[0].clone();
        manifest.north_request_passthrough =
            BTreeMap::from([(family, NorthProtocolV1::GenerateContent)]);
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );
    }
}

/// `subscription_quota_families` (the host's subscription-quota alert, lv 2026-10-14): the shipped
/// packages declare their subscription families; gate ① refuses an undeclared family and any world
/// but the provider world.
#[test]
fn subscription_quota_families_name_declared_provider_families() {
    let copilot = shipped("provider-openai-compatible");
    assert!(copilot.subscription_quota_families.contains("github-copilot"));
    assert!(!copilot.subscription_quota_families.contains("openai-compatible"));
    assert_eq!(copilot.validate(), Ok(()));
    let codex = shipped("provider-openai-responses");
    assert!(codex.subscription_quota_families.contains("openai-codex"));
    assert_eq!(codex.validate(), Ok(()));
    assert!(shipped("provider-gemini").subscription_quota_families.is_empty());

    let mut undeclared = shipped("provider-openai-compatible");
    undeclared.subscription_quota_families.insert("nowhere".to_owned());
    assert_eq!(
        undeclared.validate(),
        Err(ManifestErrorV1::InvalidSubscriptionQuotaFamily("nowhere".to_owned()))
    );

    for package in ["embeddings-openai-compatible", "task-kling-v2", "image-azure"] {
        let mut manifest = shipped(package);
        let family = manifest.providers[0].clone();
        manifest.subscription_quota_families.insert(family);
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );
    }
}

/// `immutable_request_headers` (the protected request headers record,
/// `docs/design/2026-10-15-immutable-request-headers.md`; Q-CC3): absent on a package that declares
/// none, omitted from the wire, and nothing changes.
#[test]
fn absent_immutable_request_headers_are_omitted_and_change_nothing() {
    let manifest = openai();
    assert!(manifest.immutable_request_headers.is_empty());
    let wire = serde_json::to_value(&manifest).unwrap();
    assert!(wire.get("immutable_request_headers").is_none());
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn a_declared_family_may_fix_request_headers() {
    let mut manifest = openai();
    manifest.immutable_request_headers = BTreeMap::from([(
        "openai-compatible".to_owned(),
        paths(&["authorization", "user-agent", "x-example-version"]),
    )]);
    assert_eq!(manifest.validate(), Ok(()));

    let wire = serde_json::to_value(&manifest).unwrap();
    assert_eq!(
        wire["immutable_request_headers"],
        json!({"openai-compatible": ["authorization", "user-agent", "x-example-version"]})
    );
    let back: ComponentManifestV1 = serde_json::from_value(wire).unwrap();
    assert_eq!(back, manifest);

    // An empty list says nothing, as for body paths.
    manifest.immutable_request_headers = BTreeMap::from([("openai-compatible".to_owned(), vec![])]);
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn immutable_request_headers_for_an_undeclared_family_are_refused_by_name() {
    let mut manifest = openai();
    manifest.immutable_request_headers =
        BTreeMap::from([("nowhere".to_owned(), paths(&["user-agent"]))]);
    assert!(matches!(
        manifest.validate(),
        Err(ManifestErrorV1::InvalidImmutableRequestHeaders { family, detail })
            if family == "nowhere" && detail.contains("does not declare")
    ));
}

#[test]
fn immutable_request_headers_are_distinct_lowercase_tchar_names() {
    let long = "a".repeat(MAX_IMMUTABLE_REQUEST_HEADER_NAME_BYTES + 1);
    let at_limit = "a".repeat(MAX_IMMUTABLE_REQUEST_HEADER_NAME_BYTES);
    for good in [
        &["x-api-key"][..],
        &["authorization", "user-agent", "content-type", "accept"],
        &["x_custom.v2", "a!#$%&'*+-.^_`|~9"],
        &[at_limit.as_str()],
        &[],
    ] {
        assert!(are_immutable_request_headers(&paths(good)), "{good:?}");
    }
    for bad in [
        &[""][..],
        &["User-Agent"],
        &["X-API-KEY"],
        &["x api key"],
        &["x-api-key:"],
        &["anthropic-beta\r\n"],
        &["(comment)"],
        &["x/y"],
        &["x-api-key", "x-api-key"],
        &[long.as_str()],
    ] {
        assert!(!are_immutable_request_headers(&paths(bad)), "{bad:?}");
        let mut manifest = openai();
        manifest.immutable_request_headers =
            BTreeMap::from([("openai-compatible".to_owned(), paths(bad))]);
        assert!(
            matches!(
                manifest.validate(),
                Err(ManifestErrorV1::InvalidImmutableRequestHeaders { .. })
            ),
            "{bad:?}"
        );
    }
    let many: Vec<String> = (0..=MAX_IMMUTABLE_REQUEST_HEADERS).map(|n| format!("h{n}")).collect();
    assert!(!are_immutable_request_headers(&many));
    assert!(are_immutable_request_headers(&many[..MAX_IMMUTABLE_REQUEST_HEADERS]));
}

#[test]
fn other_worlds_refuse_immutable_request_headers() {
    for package in ["embeddings-openai-compatible", "task-kling-v2", "image-azure"] {
        let mut manifest = shipped(package);
        let family = manifest.providers[0].clone();
        manifest.immutable_request_headers = BTreeMap::from([(family, paths(&["user-agent"]))]);
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::DeliveryIsAProviderWorldDeclaration),
            "{package}"
        );
    }
}
