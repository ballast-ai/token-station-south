//! The Azure `OpenAI` families of `speech-openai-compatible` (design record
//! `docs/design/2026-10-15-speech-azure-openai.md`): `azure-openai-v1`, the GA v1 surface, and
//! `azure-openai-dated`, the dated, deployment-addressed surface.
//!
//! Both are transcribed from the host's native arms (record §2): the native catch-all speech arms
//! reached on an `azure_openai` or `azure_foundry` row, with the URL that native's
//! `build_upstream_url` builds and the `api-key` header that its `apply_provider_auth` sends. The
//! values pinned here are the native constants and the URLs native builds, written out, not values
//! derived from this reference.

use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::reference::azure_openai_dated;
use south_component_conformance::reference_openai_compatible_speech::{
    AZURE_OPENAI_DATED, AZURE_OPENAI_V1, AZURE_SECRET_HEADER, OPENAI_COMPATIBLE,
    OpenAiCompatibleSpeechReferenceV1,
};
use south_component_conformance::{CheckV1, SpeechComponentV1, run_speech_component_suite_v1};
use south_contracts::speech::SpeechCallContextV1;
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

#[path = "support/speech_v3_1.rs"]
mod support;

const PACKAGE: &str = "speech-openai-compatible";
const REFERENCE: OpenAiCompatibleSpeechReferenceV1 = OpenAiCompatibleSpeechReferenceV1;
/// Native `AZURE_OPENAI_DEFAULT_API_VERSION` (server `engine/upstream.rs:10`), written out.
const NATIVE_DEFAULT_API_VERSION: &str = "2024-10-21";
const CLASSIC: &str = "https://my-resource.openai.azure.com";
const FOUNDRY: &str = "https://my-resource.services.ai.azure.com";
/// The pack rows this change adds (record §7).
const AZURE_ROWS: [&str; 11] = [
    "speech-v1.capabilities.azure-dated-models",
    "speech-v1.prepare.azure-dated-chars",
    "speech-v1.prepare.azure-dated-default-version-tokens",
    "speech-v1.prepare.azure-v1-chars",
    "speech-v1.prepare.asr-azure-dated-transcribe",
    "speech-v1.prepare.asr-azure-dated-translate",
    "speech-v1.prepare.asr-azure-dated-text",
    "speech-v1.prepare.asr-azure-v1-transcribe",
    "speech-v1.response.azure-dated-audio",
    "speech-v1.response.azure-dated-status-401",
    "speech-v1.response.asr-azure-dated-transcribe",
];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

fn raw_manifest(package: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(
            repo_root().join("components").join(package).join("manifest.json"),
        )
        .expect("the manifest reads"),
    )
    .expect("the manifest parses")
}

fn config(provider: &str, base_url: &str, api_version: Option<&str>) -> ProviderConfig {
    let mut config = json!({
        "provider": provider,
        "base_url": base_url,
        "auth": "provider_api_key",
        "models": [
            { "model": "tts-prod" },
            { "model": "whisper-prod", "supported_parameters": ["speech.translate"] },
        ],
    });
    if let Some(version) = api_version {
        config["declared"] = json!({ "api_version": version });
    }
    serde_json::from_value(config).expect("the configuration parses")
}

fn context(upstream_model: &str, operation: &str) -> SpeechCallContextV1 {
    let forms = if operation == "synthesize" { ["characters"] } else { ["seconds"] };
    serde_json::from_value(json!({
        "upstream_model": upstream_model,
        "operation": operation,
        "metering_required": forms,
    }))
    .expect("the context parses")
}

fn request(operation: &str) -> Value {
    if operation == "synthesize" {
        json!({ "json": { "model": "media/tts", "input": "hello world", "voice": "alloy" } })
    } else {
        json!({ "multipart": { "parts": [
            { "kind": "text", "name": "model", "value": "media/asr" },
            { "kind": "file", "name": "file", "blob": "b0", "filename": "a.mp3",
              "media_type": "audio/mpeg", "bytes": 9 },
        ] } })
    }
}

fn prepare(
    config: &ProviderConfig,
    upstream_model: &str,
    operation: &str,
) -> Result<Value, ErrorEnvelope> {
    REFERENCE
        .prepare(config, &request(operation), &context(upstream_model, operation))
        .map(|prepared| serde_json::from_str(&prepared.to_json()).expect("the call is JSON"))
}

fn prepared(config: &ProviderConfig, upstream_model: &str, operation: &str) -> Value {
    prepare(config, upstream_model, operation)
        .unwrap_or_else(|error| panic!("the reference refused: {error:?}"))
}

/// The URL a host joins from the binding's base, the descriptor's path and its query.
fn url(base_url: &str, descriptor: &Value) -> String {
    let mut url =
        format!("{}/{}", base_url.trim_end_matches('/'), descriptor["path"].as_str().unwrap());
    if let Some(query) = descriptor["query"].as_array() {
        let pairs: Vec<String> = query
            .iter()
            .map(|pair| {
                format!("{}={}", pair["name"].as_str().unwrap(), pair["value"].as_str().unwrap())
            })
            .collect();
        url.push('?');
        url.push_str(&pairs.join("&"));
    }
    url
}

#[test]
fn the_families_and_the_version_key_are_the_provider_worlds() {
    assert_eq!(AZURE_OPENAI_V1, "azure-openai-v1");
    assert_eq!(AZURE_OPENAI_DATED, azure_openai_dated::FAMILY);
    assert_eq!(AZURE_OPENAI_DATED, "azure-openai-dated");
    assert_eq!(azure_openai_dated::DEFAULT_API_VERSION, NATIVE_DEFAULT_API_VERSION);

    let speech = raw_manifest(PACKAGE);
    let text = raw_manifest("provider-openai-compatible");
    assert_eq!(
        speech["providers"],
        json!([OPENAI_COMPATIBLE, AZURE_OPENAI_V1, AZURE_OPENAI_DATED])
    );
    assert_eq!(speech["auth_arms"], json!(["bearer", "header_secret"]));
    // One host row serves text and speech: the key, its syntax and its default are the provider
    // world's, byte for byte.
    assert_eq!(
        speech["config_schema"][AZURE_OPENAI_DATED],
        text["config_schema"][AZURE_OPENAI_DATED]
    );
    assert_eq!(
        speech["config_schema"][AZURE_OPENAI_DATED][azure_openai_dated::API_VERSION_KEY]["default"],
        NATIVE_DEFAULT_API_VERSION
    );
    let manifest: ComponentManifestV1 = serde_json::from_value(speech).unwrap();
    assert_eq!(manifest.validate(), Ok(()));
    assert!(manifest.declared_keys(OPENAI_COMPATIBLE).is_empty());
    assert!(manifest.declared_keys(AZURE_OPENAI_V1).is_empty());
    assert_eq!(
        manifest.declared_keys(AZURE_OPENAI_DATED).into_iter().collect::<Vec<_>>(),
        [azure_openai_dated::API_VERSION_KEY]
    );
}

/// Native `{endpoint}/openai/deployments/{upstream_model}{path}?api-version={version}`
/// (`upstream.rs:551-581`), `{path}` the canonical speech path with `/v1` stripped.
#[test]
fn the_dated_route_is_the_url_native_builds() {
    for (base, version, deployment, operation, native) in [
        (
            CLASSIC,
            Some("2025-03-01-preview"),
            "tts-prod",
            "synthesize",
            "https://my-resource.openai.azure.com/openai/deployments/tts-prod/audio/speech?api-version=2025-03-01-preview",
        ),
        (
            CLASSIC,
            None,
            "tts-prod",
            "synthesize",
            "https://my-resource.openai.azure.com/openai/deployments/tts-prod/audio/speech?api-version=2024-10-21",
        ),
        (
            FOUNDRY,
            Some("2024-06-01"),
            "whisper-prod",
            "transcribe",
            "https://my-resource.services.ai.azure.com/openai/deployments/whisper-prod/audio/transcriptions?api-version=2024-06-01",
        ),
        (
            CLASSIC,
            Some("2024-06-01"),
            "whisper-prod",
            "translate",
            "https://my-resource.openai.azure.com/openai/deployments/whisper-prod/audio/translations?api-version=2024-06-01",
        ),
        (
            FOUNDRY,
            None,
            "whisper-prod",
            "translate",
            "https://my-resource.services.ai.azure.com/openai/deployments/whisper-prod/audio/translations?api-version=2024-10-21",
        ),
    ] {
        let call = prepared(&config(AZURE_OPENAI_DATED, base, version), deployment, operation);
        assert_eq!(url(base, &call["descriptor"]), native, "{base} {version:?} {operation}");
    }
}

/// Native `{endpoint}/openai/v1{path}` for an `azure_openai` row whose version is `v1` and for
/// every `azure_foundry` row (`upstream.rs:537-550, 574-575`); the family's base is
/// `{endpoint}/openai/v1`, as for the embeddings family of the same name.
#[test]
fn the_v1_route_is_the_url_native_builds() {
    for (endpoint, operation, native) in [
        (CLASSIC, "synthesize", "https://my-resource.openai.azure.com/openai/v1/audio/speech"),
        (FOUNDRY, "synthesize", "https://my-resource.services.ai.azure.com/openai/v1/audio/speech"),
        (
            FOUNDRY,
            "transcribe",
            "https://my-resource.services.ai.azure.com/openai/v1/audio/transcriptions",
        ),
        (CLASSIC, "translate", "https://my-resource.openai.azure.com/openai/v1/audio/translations"),
    ] {
        let base = format!("{endpoint}/openai/v1");
        let call = prepared(&config(AZURE_OPENAI_V1, &base, None), "whisper-prod", operation);
        assert!(call["descriptor"].get("query").is_none(), "{operation}");
        assert_eq!(url(&base, &call["descriptor"]), native, "{endpoint} {operation}");
    }
}

/// Native `api-key: <key>` for `azure_openai` and `azure_foundry` (`upstream.rs:190-196`); Bearer
/// stays the `openai-compatible` family's. Without a configured slot there is no auth arm.
#[test]
fn the_azure_families_present_the_key_in_api_key() {
    assert_eq!(AZURE_SECRET_HEADER, "api-key");
    for operation in ["synthesize", "transcribe", "translate"] {
        for (provider, base, auth) in [
            (
                AZURE_OPENAI_DATED,
                CLASSIC.to_owned(),
                json!({ "arm": "header_secret", "header": "api-key", "slot": "provider_api_key" }),
            ),
            (
                AZURE_OPENAI_V1,
                format!("{CLASSIC}/openai/v1"),
                json!({ "arm": "header_secret", "header": "api-key", "slot": "provider_api_key" }),
            ),
            (
                OPENAI_COMPATIBLE,
                "https://api.openai.com".to_owned(),
                json!({ "arm": "bearer", "slot": "provider_api_key" }),
            ),
        ] {
            let mut with_slot = config(provider, &base, None);
            let call = prepared(&with_slot, "whisper-prod", operation);
            assert_eq!(call["descriptor"]["auth"], auth, "{provider} {operation}");
            with_slot.auth = None;
            let call = prepared(&with_slot, "whisper-prod", operation);
            assert!(call["descriptor"].get("auth").is_none(), "{provider} {operation}");
        }
    }
}

/// The three families run native's same two arms: apart from the route (path, query, auth) every
/// member of the prepared call is the `openai-compatible` family's.
#[test]
fn the_azure_families_differ_from_openai_compatible_only_in_the_route() {
    let strip = |mut call: Value| {
        let descriptor = call["descriptor"].as_object_mut().unwrap();
        for member in ["path", "query", "auth"] {
            descriptor.remove(member);
        }
        call
    };
    for operation in ["synthesize", "transcribe", "translate"] {
        let plain = strip(prepared(
            &config(OPENAI_COMPATIBLE, "https://api.openai.com", None),
            "whisper-prod",
            operation,
        ));
        let v1 = strip(prepared(
            &config(AZURE_OPENAI_V1, &format!("{CLASSIC}/openai/v1"), None),
            "whisper-prod",
            operation,
        ));
        let dated = strip(prepared(
            &config(AZURE_OPENAI_DATED, CLASSIC, Some("2024-06-01")),
            "whisper-prod",
            operation,
        ));
        assert_eq!(v1, plain, "{operation}");
        assert_eq!(dated, plain, "{operation}");
    }
}

/// Record §4 D3: a version that is not `YYYY-MM-DD[-preview]` (native would send it; the literal
/// `v1` belongs to `azure-openai-v1`) is a 500 naming the key, ahead of any request refusal.
#[test]
fn an_undated_version_is_a_configuration_error_before_any_request_refusal() {
    for version in ["v1", "V1", "2024-10-21-beta", "2024-1-21", "latest"] {
        for operation in ["synthesize", "transcribe", "translate"] {
            let config = config(AZURE_OPENAI_DATED, CLASSIC, Some(version));
            let error = prepare(&config, "whisper-prod", operation).unwrap_err();
            assert_eq!(error.code, ErrorCode::Internal, "{version:?} {operation}");
            assert_eq!(error.http_status, 500);
            assert!(error.message.contains("`api_version`"), "{}", error.message);
            // A request the reference would also refuse: the configuration is named first.
            let broken = REFERENCE
                .prepare(&config, &json!({ "json": {} }), &context("whisper-prod", operation))
                .unwrap_err();
            assert_eq!(broken.code, ErrorCode::Internal, "{version:?} {operation}");
        }
    }
    // The v1 family reads no version, so the same value changes nothing there.
    let v1 = config(AZURE_OPENAI_V1, &format!("{CLASSIC}/openai/v1"), Some("v1"));
    assert!(prepare(&v1, "tts-prod", "synthesize").is_ok());
}

/// Record §4 D4: native would send `/openai/deployments//audio/speech`; the family refuses it.
#[test]
fn a_call_without_a_deployment_is_refused() {
    for operation in ["synthesize", "transcribe", "translate"] {
        let error = prepare(&config(AZURE_OPENAI_DATED, CLASSIC, None), "", operation).unwrap_err();
        assert_eq!(error.code, ErrorCode::Capability, "{operation}");
        assert_eq!(error.http_status, 400);
    }
}

/// Record §4 D5: the deployment is one escaped path segment (native sends it raw; valid Azure
/// deployment names are unchanged by the escape).
#[test]
fn the_deployment_is_one_escaped_segment() {
    let config = config(AZURE_OPENAI_DATED, CLASSIC, None);
    for (deployment, path) in [
        ("tts-prod_2.v1", "openai/deployments/tts-prod_2.v1/audio/speech"),
        ("tts prod", "openai/deployments/tts%20prod/audio/speech"),
        ("a/b", "openai/deployments/a%2Fb/audio/speech"),
    ] {
        let call = prepared(&config, deployment, "synthesize");
        assert_eq!(call["descriptor"]["path"], path, "{deployment}");
        // The body still carries the deployment unescaped, as native's does.
        assert_eq!(call["descriptor"]["body"]["json"]["template"]["model"], deployment);
    }
}

/// An undeclared family, near misses of the three included, is refused by both functions.
#[test]
fn an_undeclared_family_is_refused() {
    for provider in [
        "azure_openai",
        "azure_foundry",
        "azure-openai",
        "azure-foundry",
        "azure-speech",
        "AZURE-OPENAI-V1",
        "azure-openai-v1 ",
        "openai",
    ] {
        let config = config(provider, CLASSIC, None);
        let error = REFERENCE.model_capabilities(&config).unwrap_err();
        assert_eq!(error.code, ErrorCode::Capability, "{provider}");
        assert_eq!(
            error.message,
            "this package serves the openai-compatible, azure-openai-v1 and azure-openai-dated \
             families"
        );
        let error = prepare(&config, "tts-prod", "synthesize").unwrap_err();
        assert_eq!(error.code, ErrorCode::Capability, "{provider}");
    }
}

/// The capabilities of an Azure row are the `openai-compatible` family's for the same models.
#[test]
fn the_azure_families_declare_what_openai_compatible_declares() {
    let plain = REFERENCE
        .model_capabilities(&config(OPENAI_COMPATIBLE, "https://api.openai.com", None))
        .unwrap();
    for (provider, base) in [
        (AZURE_OPENAI_V1, format!("{CLASSIC}/openai/v1")),
        (AZURE_OPENAI_DATED, CLASSIC.to_owned()),
    ] {
        assert_eq!(
            REFERENCE.model_capabilities(&config(provider, &base, None)).unwrap(),
            plain,
            "{provider}"
        );
    }
}

/// The pack's Azure rows run and pass, both families included.
#[test]
fn the_pack_judges_the_azure_rows() {
    let manifest = support::manifest(PACKAGE);
    let report = run_speech_component_suite_v1(&REFERENCE, &support::pack(&manifest), &manifest);
    let failures = support::failures(&report);
    assert!(failures.is_empty(), "{failures:#?}");
    for row in AZURE_ROWS {
        assert!(
            report
                .outcomes()
                .iter()
                .any(|outcome| outcome.check == CheckV1::FixtureMatch && outcome.case == row),
            "{row} was not judged"
        );
    }
}
