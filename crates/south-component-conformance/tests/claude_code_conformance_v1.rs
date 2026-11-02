//! The Claude Code subscription families of `provider-anthropic` 1.2.0: `claude-code` (the
//! login-token form, a minted OAuth access token) and `claude-code-setup-token` (a long-lived
//! setup token), their credential recipe, their fixture pack, and the values transcribed from the
//! host's native `claude_code` arm.
//!
//! Design record: `docs/design/2026-10-15-claude-code-family.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Value, json};
use south_component_conformance::credential_recipe::{
    FakeResponseV1, FixtureSignerV1, RecipeEffectsV1, RecipeOutcomeV1, RenderedRequestV1,
    run_recipe_v1,
};
use south_component_conformance::reference_anthropic::{
    AnthropicReferenceV1, claude_code, inject_claude_code_identity,
};
use south_component_conformance::{
    AdmittedAuthV1, DeclaredInstancesV1, FixturePackV1, ProviderComponentV1, ProviderFamilyV1,
    accepts_manifest, admit_descriptor_auth, run_provider_component_suite_v1_for_manifest,
};
use south_contracts::SecretHeaderV1;
use south_provider_api::{ComponentManifestV1, SlotV1};
use token_station_protocol::{
    Auth, ChatRequest, ErrorCode, ErrorEnvelope, HttpRequestDescriptor, ProviderConfig,
};

#[path = "support/anthropic_packs.rs"]
mod packs;

const FAMILIES: [&str; 3] = ["anthropic", claude_code::FAMILY, claude_code::SETUP_TOKEN_FAMILY];

fn manifest_value() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-anthropic/manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn shipped_manifest() -> ComponentManifestV1 {
    serde_json::from_value(manifest_value()).unwrap()
}

fn claude_code_pack() -> FixturePackV1 {
    packs::load(packs::CLAUDE_CODE_PACK)
}

fn config(family: &str) -> ProviderConfig {
    serde_json::from_value(json!({
        "provider": family,
        "base_url": "https://api.anthropic.com",
        "auth": "provider_api_key",
        "models": [{
            "model": "claude-x", "tool": true, "vision": true, "json_schema": true,
            "context_window": 200_000, "supported_parameters": []
        }]
    }))
    .unwrap()
}

fn chat(system: Option<Value>, stream: bool) -> ChatRequest {
    let mut messages = Vec::new();
    if let Some(system) = system {
        messages.push(json!({"role": "system", "content": system}));
    }
    messages.push(json!({"role": "user", "content": "hi"}));
    serde_json::from_value(json!({
        "model": "claude-x", "messages": messages,
        "sampling": {"max_output_tokens": 64}, "stream": stream
    }))
    .unwrap()
}

fn build(family: &str, request: &ChatRequest) -> Result<HttpRequestDescriptor, ErrorEnvelope> {
    AnthropicReferenceV1.build_http_request(request, &config(family))
}

fn headers(descriptor: &HttpRequestDescriptor) -> BTreeMap<String, String> {
    descriptor.headers.iter().map(|(name, value)| (name.clone(), value.clone())).collect()
}

fn body(descriptor: &HttpRequestDescriptor) -> Value {
    descriptor.body.clone().expect("a Messages request has a body")
}

// -- the pack ------------------------------------------------------------------------------------

/// Record §8: the rows the pack must carry. A pack can pass while having quietly lost a case.
#[test]
fn the_claude_code_pack_carries_every_decided_row() {
    let pack = claude_code_pack();
    let mut names: BTreeSet<String> = pack.cases().iter().map(|case| case.name.clone()).collect();
    names.extend(pack.credentials().cases().iter().map(|case| case.name.clone()));
    for row in [
        "provider.request.claude-code-login-chat",
        "provider.request.claude-code-login-without-system",
        "provider.request.claude-code-login-identity-already-first",
        "provider.request.claude-code-login-marked-system-blocks",
        "provider.request.claude-code-login-marked-identity-already-first",
        "provider.request.claude-code-login-end-user-hash",
        "provider.request.claude-code-setup-token-chat",
        "provider.request.claude-code-setup-token-system-untouched",
        "credential.rotation.claude-code-refresh",
        "credential.rotation.claude-code-rotation-without-new-token",
        "credential.clock.claude-code-expiry-missing-uses-the-default",
        "credential.clock.claude-code-expiry-clamped-to-a-day",
        "credential.clock.claude-code-no-refresh-token",
        "credential.on-status.claude-code-invalid-grant",
        "credential.on-status.claude-code-revoked",
        "credential.on-status.claude-code-forbidden-is-transient",
        "credential.on-status.claude-code-rate-limited-is-transient",
        "credential.on-status.claude-code-outage-is-transient",
    ] {
        assert!(names.contains(row), "the Claude Code pack lost `{row}`");
    }
}

/// The two directories are judged as one pack, so every Claude Code row is `claude-code-*`, every
/// request row is for one of the two Claude Code families, and no case name repeats.
#[test]
fn the_claude_code_pack_holds_its_rows_only_and_no_name_repeats() {
    let pack = claude_code_pack();
    for case in pack.cases() {
        let case_part = case.name.splitn(3, '.').nth(2).unwrap();
        assert!(case_part.starts_with("claude-code-"), "{}", case.name);
        assert_eq!(case.family, ProviderFamilyV1::Request, "{}", case.name);
        let family = case.input["provider_config"]["provider"].as_str().unwrap();
        assert!(
            [claude_code::FAMILY, claude_code::SETUP_TOKEN_FAMILY].contains(&family),
            "{}",
            case.name
        );
    }
    for case in pack.credentials().cases() {
        assert!(
            case.name.splitn(3, '.').nth(2).unwrap().starts_with("claude-code-"),
            "{}",
            case.name
        );
    }
    let anthropic = packs::load(packs::ANTHROPIC_PACK);
    assert!(anthropic.cases().iter().all(|case| {
        case.family != ProviderFamilyV1::Request
            || case.input["provider_config"]["provider"] == "anthropic"
    }));
    assert!(
        anthropic.credentials().cases().is_empty(),
        "credential rows live in the Claude Code pack"
    );
    let union = packs::package_pack();
    let mut seen = BTreeSet::new();
    for name in union.cases().iter().map(|case| &case.name) {
        assert!(seen.insert(name.clone()), "`{name}` repeats across the two directories");
    }
    assert_eq!(union.cases().len(), anthropic.cases().len() + pack.cases().len());
}

/// Neither directory passes gate ② alone under the shipped manifest: the `anthropic` directory
/// lacks the credential rows the recipe owes, the Claude Code directory every response-side row.
#[test]
fn neither_directory_passes_gate_two_alone() {
    let manifest = shipped_manifest();
    for pack in [packs::ANTHROPIC_PACK, packs::CLAUDE_CODE_PACK] {
        let report = run_provider_component_suite_v1_for_manifest(
            &AnthropicReferenceV1,
            &packs::load(pack),
            &manifest,
        );
        assert!(!report.is_passing(), "{pack} alone passed: {report}");
    }
    let report = run_provider_component_suite_v1_for_manifest(
        &AnthropicReferenceV1,
        &packs::package_pack(),
        &manifest,
    );
    assert!(report.is_passing(), "{report}");
}

// -- the manifest --------------------------------------------------------------------------------

/// Record §3, §4, §9: the families, the user-agent, the subscription flag, the immutable system
/// prompt, the endpoints and the runtime, each against the native value.
#[test]
fn the_manifest_declares_the_two_families_with_the_native_values() {
    let manifest = shipped_manifest();
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert_eq!(manifest.providers, FAMILIES.map(str::to_owned).to_vec());
    for family in FAMILIES {
        assert_eq!(
            manifest.endpoint.get(family).map(String::as_str),
            Some("https://api.anthropic.com"),
            "{family}"
        );
    }
    // Native `upstream.rs` `CLAUDE_CODE_USER_AGENT`, sent by the login-token form only.
    assert_eq!(claude_code::USER_AGENT, "claude-cli/2.1.114 (external, cli)");
    let instances = DeclaredInstancesV1::from_manifest(&manifest).expect("gate ① passes");
    assert_eq!(
        instances.user_agent(claude_code::FAMILY).map(south_contracts::DeclaredUserAgentV1::as_str),
        Some(claude_code::USER_AGENT)
    );
    assert_eq!(instances.user_agent(claude_code::SETUP_TOKEN_FAMILY), None);
    assert_eq!(instances.user_agent("anthropic"), None);
    assert_eq!(
        manifest.subscription_quota_families,
        BTreeSet::from([
            claude_code::FAMILY.to_owned(),
            claude_code::SETUP_TOKEN_FAMILY.to_owned()
        ])
    );
    assert_eq!(
        manifest.immutable_body_paths,
        BTreeMap::from([(claude_code::FAMILY.to_owned(), vec!["system".to_owned()])])
    );
    // Published as 1.2.0 with 0.66.0; 1.2.1 carries both the 0.67.0 Q47 round and the
    // immutable request headers (record `2026-10-15-immutable-request-headers.md`); published as
    // 1.2.1 with 0.67.0, the T9 cap field's Q47 bump made it 1.2.2
    // (`shipped_packages_v1::t9_cap_field_retires_every_published_067_package_identity`);
    // published as 1.2.2 with 0.68.0, the `glm-coding` thinking switch's Q47 bump made it 1.2.3
    // (`shipped_packages_v1::glm_thinking_retires_every_published_068_package_identity`);
    // published as 1.2.3 with 0.69.0, the `kimi-code` family's Q47 bump made it 1.2.4
    // (`shipped_packages_v1::kimi_code_retires_every_published_069_package_identity`);
    // published as 1.2.4 with 0.70.0, the Vertex task credentials' Q47 bump made it 1.2.5
    // (`shipped_packages_v1::task_vertex_credentials_retires_every_published_070_package_identity`).
    // Published as 1.2.5 with 0.71.0; the explicit cache reads' Q47 bump made it 1.2.6
    // (`shipped_packages_v1::explicit_cache_reads_retires_every_published_071_package_identity`).
    // Published as 1.2.6 with 0.72.0; the Messages pass-through record's minor made it 1.3.0
    // (`messages_passthrough_retires_every_published_072_package_identity`).
    // Published as 1.3.0 with 0.73.1; the family model parameters record's minor made it 1.4.0
    // (`family_model_parameters_retires_every_published_0731_package_identity`).
    // Published as 1.4.0 with 0.74.0; the Kiro image input record's Q47 bump made it 1.4.1
    // (`kiro_images_retires_every_published_074_package_identity`).
    // Published as 1.4.1 with 0.75.0; the Bailian records' Q47 bump made it 1.4.2
    // (`bailian_retires_every_published_075_package_identity`).
    // Published as 1.4.2 with 0.76.0; the Gemini signatures re-pin made it 1.4.3
    // (`gemini_signatures_retires_every_published_076_package_identity`).
    // Published as 1.4.3 with 0.77.0; the Gemini placeholder signature round made it 1.4.4
    // (`gemini_placeholder_retires_every_published_077_package_identity`).
    // Published as 1.4.4 with 0.78.0; the 0.79.0 round made it 1.4.5
    // (`qwen_tier_and_responses_stop_retires_every_published_078_package_identity`).
    // Published as 1.4.5 with 0.79.0; the 0.80.0 round made it 1.4.6
    // (`veo_spelling_and_durationless_asr_retire_every_published_079_package_identity`).
    // Published as 1.4.6 with 0.80.0; the 0.81.0 round made it 1.4.7
    // (`served_models_retires_every_published_080_package_identity`).
    // Published as 1.4.7 with 0.81.0; the 0.82.0 round made it 1.4.8
    // (`sse_line_ends_retire_every_published_081_package_identity`).
    // Published as 1.4.8 with 0.82.0; the 0.83.0 round made it 1.4.9
    // (`client_policy_headers_retire_every_published_082_package_identity`).
    assert_eq!(manifest.version, "1.4.9");
    // `immutable_request_headers` needed 0.67.0 (`ComponentManifestV1` refuses unknown fields); from
    // 1.3.0 `north_request_passthrough` `messages` and `forwarded_client_headers` need 0.73.0
    // (record `2026-10-17-messages-passthrough-and-count-tokens.md` §8.1); from 1.4.0
    // `model_parameters` and `drops_unsigned_thinking` need 0.74.0 (record
    // `2026-10-18-family-model-parameters.md` §7), which shipped them.
    assert_eq!(manifest.compatibility.south_runtime, "0.77.0"); // release 0.77.0 raised every package (canonical_ir 5)
}

/// Protected request headers record (`docs/design/2026-10-15-immutable-request-headers.md`,
/// Q-CC3): both Claude Code families declare one list of immutable request headers, the
/// reference's. `anthropic` declared none until the family model parameters record
/// (`2026-10-18-family-model-parameters.md` §4) gave it `anthropic-version`, the one vendor name
/// a host held in its own managed-header list.
#[test]
fn the_claude_code_families_declare_their_immutable_request_headers() {
    let manifest = shipped_manifest();
    let declared: Vec<String> =
        claude_code::IMMUTABLE_REQUEST_HEADERS.iter().map(|name| (*name).to_owned()).collect();
    assert_eq!(
        manifest.immutable_request_headers,
        BTreeMap::from([
            ("anthropic".to_owned(), vec!["anthropic-version".to_owned()]),
            (claude_code::FAMILY.to_owned(), declared.clone()),
            (claude_code::SETUP_TOKEN_FAMILY.to_owned(), declared),
        ])
    );
    assert!(south_provider_api::are_immutable_request_headers(
        &manifest.immutable_request_headers[claude_code::FAMILY]
    ));
    let mut sorted = claude_code::IMMUTABLE_REQUEST_HEADERS.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, claude_code::IMMUTABLE_REQUEST_HEADERS);
}

/// The list covers what each family fixes: every header its requests carry in either mode, the
/// header its credential is presented in, and the user-agent the host fills (declared for
/// `claude-code`, the host's own for `claude-code-setup-token`). A header the component starts
/// sending without declaring it fails here.
#[test]
fn the_immutable_request_headers_cover_every_header_each_family_fixes() {
    let manifest = shipped_manifest();
    for family in [claude_code::FAMILY, claude_code::SETUP_TOKEN_FAMILY] {
        let declared = &manifest.immutable_request_headers[family];
        let protects = |name: &str| declared.iter().any(|declared| declared == name);
        assert!(protects("user-agent"), "{family}");
        for stream in [false, true] {
            for system in [None, Some(json!("Be terse."))] {
                let descriptor = build(family, &chat(system, stream)).unwrap();
                for name in headers(&descriptor).keys() {
                    assert!(protects(name.as_str()), "{family}: `{name}` is sent but not declared");
                }
                let credential = match &descriptor.auth {
                    Some(Auth::Header { name, .. }) => name.clone(),
                    Some(Auth::Bearer { .. }) => "authorization".to_owned(),
                    other => panic!("{family} presents {other:?}"),
                };
                assert!(protects(credential.as_str()), "{family}: `{credential}` is not declared");
            }
        }
        // Both presentations are protected on both forms, so an extra cannot add the other one.
        assert!(protects("authorization") && protects("x-api-key"), "{family}");
    }
    assert_eq!(manifest.immutable_request_headers["anthropic"], ["anthropic-version"]);
}

/// Declaring the immutable request headers changes the manifest, so the identity published with
/// 0.66.0 retires: a patch, since no request the component builds changes. The reference's
/// `metadata()` moves with it.
#[test]
fn immutable_request_headers_retire_the_published_1_2_0_identity() {
    let manifest = shipped_manifest();
    assert_ne!(manifest.version, "1.2.0");
    // 1.2.1 was published with 0.67.0; the T9 cap field's Q47 bump made it 1.2.2.
    assert_ne!(manifest.version, "1.2.1");
    // 1.2.2 was published with 0.68.0; the `glm-coding` thinking switch's Q47 bump made it 1.2.3.
    assert_ne!(manifest.version, "1.2.2");
    // 1.2.3 was published with 0.69.0; the `kimi-code` family's Q47 bump made it 1.2.4.
    assert_ne!(manifest.version, "1.2.3");
    // 1.2.4 was published with 0.70.0; the Vertex task credentials' Q47 bump made it 1.2.5.
    assert_ne!(manifest.version, "1.2.4");
    // 1.2.5 was published with 0.71.0; the explicit cache reads' Q47 bump made it 1.2.6
    // (`shipped_packages_v1::explicit_cache_reads_retires_every_published_071_package_identity`).
    assert_ne!(manifest.version, "1.2.5");
    // Published as 1.2.6 with 0.72.0; the Messages pass-through record's minor made it 1.3.0
    // (`messages_passthrough_retires_every_published_072_package_identity`).
    // Published as 1.3.0 with 0.73.1; the family model parameters record's minor made it 1.4.0
    // (`family_model_parameters_retires_every_published_0731_package_identity`).
    // Published as 1.4.0 with 0.74.0; the Kiro image input record's Q47 bump made it 1.4.1
    // (`kiro_images_retires_every_published_074_package_identity`).
    // Published as 1.4.1 with 0.75.0; the Bailian records' Q47 bump made it 1.4.2
    // (`bailian_retires_every_published_075_package_identity`).
    // Published as 1.4.2 with 0.76.0; the Gemini signatures re-pin made it 1.4.3
    // (`gemini_signatures_retires_every_published_076_package_identity`).
    // Published as 1.4.3 with 0.77.0; the Gemini placeholder signature round made it 1.4.4
    // (`gemini_placeholder_retires_every_published_077_package_identity`).
    // Published as 1.4.4 with 0.78.0; the 0.79.0 round made it 1.4.5
    // (`qwen_tier_and_responses_stop_retires_every_published_078_package_identity`).
    // Published as 1.4.5 with 0.79.0; the 0.80.0 round made it 1.4.6
    // (`veo_spelling_and_durationless_asr_retire_every_published_079_package_identity`).
    // Published as 1.4.6 with 0.80.0; the 0.81.0 round made it 1.4.7
    // (`served_models_retires_every_published_080_package_identity`).
    // Published as 1.4.7 with 0.81.0; the 0.82.0 round made it 1.4.8
    // (`sse_line_ends_retire_every_published_081_package_identity`).
    // Published as 1.4.8 with 0.82.0; the 0.83.0 round made it 1.4.9
    // (`client_policy_headers_retire_every_published_082_package_identity`).
    assert_eq!(manifest.version, "1.4.9");
    assert_eq!(AnthropicReferenceV1.metadata().version, manifest.version);
}

/// Record §9: the families are new behaviour, so the package takes a minor version; the patch
/// identity the 0.66.0 cycle gave it (1.1.10, never published) and the published 1.1.9 retire.
#[test]
fn the_claude_code_families_retire_the_patch_identity() {
    let manifest = shipped_manifest();
    for retired in ["1.1.9", "1.1.10"] {
        assert_ne!(manifest.version, retired);
    }
    assert_eq!(AnthropicReferenceV1.metadata().version, manifest.version);
}

/// Record §4: the recipe applies to the login-token form only. The setup-token form and the
/// `anthropic` family keep the operator-entered static slot.
#[test]
fn the_recipe_applies_to_the_login_token_family_only() {
    let manifest = shipped_manifest();
    let credentials = manifest.credentials_for(claude_code::FAMILY).expect("a section applies");
    assert_eq!(
        credentials.slots.get("provider_api_key"),
        Some(&SlotV1::Minted("claude_code".to_owned()))
    );
    assert!(manifest.credentials_for(claude_code::SETUP_TOKEN_FAMILY).is_none());
    assert!(manifest.credentials_for("anthropic").is_none());
}

/// Record §4.1: the recipe, written out in full from the native `ClaudeCodeMint`
/// (`token_refresh.rs`): the token endpoint, the CLI's client id, the JSON refresh-token grant,
/// rotation with write-back, `expires_in` in seconds, the 400 / 401 latch and the import file.
#[test]
fn the_recipe_is_the_native_refresh() {
    let credentials = &manifest_value()["credentials"];
    assert_eq!(
        credentials,
        &json!({
            "schema": "south.credential-recipe.v1",
            "families": ["claude-code"],
            "fields": {
                "refresh_token": {
                    "secret": true,
                    "required": true,
                    "description": credentials["fields"]["refresh_token"]["description"]
                }
            },
            "import": {
                "refresh_token": {
                    "file": "claude-code-credentials-json",
                    "pointers": ["/claudeAiOauth/refreshToken"]
                }
            },
            "seed": {
                "file": "claude-code-credentials-json",
                "slot": "provider_api_key",
                "present": "/claudeAiOauth/accessToken",
                "expires_at": {"epoch_millis": "/claudeAiOauth/expiresAt"}
            },
            "slots": {"provider_api_key": {"minted": "claude_code"}},
            "recipes": {
                "claude_code": {
                    "steps": [{
                        "id": "refresh",
                        "kind": "oauth2_token",
                        "encoding": "json",
                        "endpoint": "https://console.anthropic.com/v1/oauth/token",
                        "requires": ["refresh_token"],
                        "params": {
                            "client_id": {"const": "9d1c250a-e61b-44d9-88ed-5944d1962f5e"},
                            "grant_type": {"const": "refresh_token"},
                            "refresh_token": {"field": "refresh_token"}
                        },
                        "on_status": {
                            "400": "reauth_required",
                            "401": "reauth_required",
                            "4xx": "transient"
                        },
                        "extract": {
                            "access_token": {"pointer": "/access_token", "secret": true},
                            "expires_at": {"relative_seconds": "/expires_in"},
                            "refresh_token": {
                                "pointer": "/refresh_token", "secret": true, "optional": true
                            }
                        }
                    }],
                    "present": "refresh.access_token",
                    "rotates_refresh_material": true,
                    "write_back": {"refresh_token": "refresh.refresh_token"},
                    "without_refresh_material": "fail",
                    "default_seconds": 60
                }
            }
        })
    );
}

/// Record §4.1: the native arm latches a credential only on 400 and 401 from the token endpoint
/// (`note_reauth_required_if_terminal`); every other status fails the request and is retried on
/// the next one. Every status from 400 to 599 is run.
#[test]
fn only_400_and_401_latch_the_credential() {
    let manifest = shipped_manifest();
    let credentials = manifest.credentials_for(claude_code::FAMILY).unwrap();
    let fields = BTreeMap::from([("refresh_token".to_owned(), "fake-refresh-1".to_owned())]);
    for status in 400_u16..600 {
        let mut responder =
            |_: &RenderedRequestV1| Some(FakeResponseV1 { status, body: Value::Null });
        let mut effects = RecipeEffectsV1 {
            now: 1_767_225_600,
            signer: &FixtureSignerV1,
            responder: &mut responder,
        };
        let run = run_recipe_v1(credentials, "provider_api_key", &fields, &mut effects);
        match (status, &run.outcome) {
            (400 | 401, RecipeOutcomeV1::ReauthRequired { .. })
            | (402..=599, RecipeOutcomeV1::Transient { .. }) => {}
            (status, outcome) => panic!("status {status} ended as {outcome:?}"),
        }
    }
}

// -- the request ---------------------------------------------------------------------------------

/// Record §3: the header set of each family, exactly. `anthropic` is unchanged; the login-token
/// form carries the CLI's headers (native `build_sealed_request`); the setup-token form only the
/// OAuth beta flag. Both Claude Code families send `accept` for the request's mode.
#[test]
fn each_family_sends_exactly_its_native_headers() {
    for stream in [false, true] {
        let accept = if stream { "text/event-stream" } else { "application/json" };
        let request = chat(None, stream);
        let common = [("anthropic-version", "2023-06-01"), ("content-type", "application/json")];
        let expect = |extra: &[(&str, &str)]| -> BTreeMap<String, String> {
            common
                .iter()
                .chain(extra)
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect()
        };
        assert_eq!(headers(&build("anthropic", &request).unwrap()), expect(&[]));
        assert_eq!(
            headers(&build(claude_code::FAMILY, &request).unwrap()),
            expect(&[
                ("accept", accept),
                (
                    "anthropic-beta",
                    "claude-code-20250219,oauth-2025-04-20,interleaved-thinking-2025-05-14,prompt-caching-scope-2026-01-05"
                ),
                ("x-app", "cli"),
                ("anthropic-dangerous-direct-browser-access", "true"),
            ])
        );
        assert_eq!(
            headers(&build(claude_code::SETUP_TOKEN_FAMILY, &request).unwrap()),
            expect(&[("accept", accept), ("anthropic-beta", "oauth-2025-04-20")])
        );
    }
}

/// Record §3: the credential's presentation per family (native `build_sealed_request`): the
/// login-token form's minted access token in `x-api-key`, the setup token as Bearer, never both;
/// and the presentation passes descriptor auth admission against the shipped manifest.
#[test]
fn each_family_presents_its_credential_as_native_does() {
    let manifest = shipped_manifest();
    let request = chat(None, false);
    for (family, auth, admitted) in [
        ("anthropic", "header", AdmittedAuthV1::HeaderSecret(SecretHeaderV1::XApiKey)),
        (claude_code::FAMILY, "header", AdmittedAuthV1::HeaderSecret(SecretHeaderV1::XApiKey)),
        (claude_code::SETUP_TOKEN_FAMILY, "bearer", AdmittedAuthV1::Bearer),
    ] {
        let descriptor = build(family, &request).unwrap();
        match (&descriptor.auth, auth) {
            (Some(Auth::Header { name, .. }), "header") => {
                assert_eq!(name, "x-api-key", "{family}");
            }
            (Some(Auth::Bearer { .. }), "bearer") => {}
            (other, _) => panic!("{family} presents {other:?}"),
        }
        assert_eq!(admit_descriptor_auth(&manifest, &config(family), &descriptor), Ok(admitted));
    }
}

/// Record §3: only the login-token form injects the identity, and the bodies otherwise agree byte
/// for byte with the `anthropic` family's: the families share one body builder.
#[test]
fn only_the_login_token_form_injects_the_identity_and_the_bodies_otherwise_agree() {
    for system in [None, Some(json!("Be terse."))] {
        let request = chat(system.clone(), true);
        let anthropic = body(&build("anthropic", &request).unwrap());
        assert_eq!(body(&build(claude_code::SETUP_TOKEN_FAMILY, &request).unwrap()), anthropic);
        let mut expected = anthropic.clone();
        inject_claude_code_identity(&mut expected);
        let login = body(&build(claude_code::FAMILY, &request).unwrap());
        assert_eq!(login, expected);
        assert_eq!(login["system"][0]["text"], claude_code::SYSTEM_IDENTITY, "{system:?}");
        assert_ne!(login, anthropic);
    }
}

/// A family name the package does not declare is refused at build time, before anything is sent;
/// the native provider type's spelling (`claude_code`) is not a family.
#[test]
fn an_undeclared_family_is_refused() {
    for family in ["claude_code", "claude-code-login", "claude-code-setup"] {
        let error = build(family, &chat(None, false)).unwrap_err();
        assert_eq!(error.code, ErrorCode::Capability, "{family}");
    }
}

// -- the identity --------------------------------------------------------------------------------

/// Native `claude_code.rs` tests, transcribed: `inject_identity_adds_when_absent`,
/// `inject_identity_prepends_to_string_system`, `inject_identity_idempotent_for_prefixed_string`
/// and `inject_identity_idempotent_for_prefixed_array_block_0`.
#[test]
fn the_identity_is_injected_as_the_native_arm_injects_it() {
    let identity = json!({"type": "text", "text": claude_code::SYSTEM_IDENTITY});
    assert_eq!(
        claude_code::SYSTEM_IDENTITY,
        "You are Claude Code, Anthropic's official CLI for Claude."
    );

    let mut absent = json!({"model": "claude-opus-4-7", "messages": []});
    inject_claude_code_identity(&mut absent);
    assert_eq!(absent["system"], json!([identity]));

    let mut string = json!({"system": "Be brief.", "messages": []});
    inject_claude_code_identity(&mut string);
    assert_eq!(string["system"], json!([identity, {"type": "text", "text": "Be brief."}]));

    let combined = format!("{}\n\nBe brief.", claude_code::SYSTEM_IDENTITY);
    let mut prefixed = json!({"system": combined, "messages": []});
    inject_claude_code_identity(&mut prefixed);
    assert_eq!(prefixed["system"], json!(combined));

    let blocks =
        json!({"system": [identity, {"type": "text", "text": "Be brief."}], "messages": []});
    let mut unchanged = blocks.clone();
    inject_claude_code_identity(&mut unchanged);
    assert_eq!(unchanged, blocks);

    let mut array = json!({"system": [{"type": "text", "text": "Rules."}], "messages": []});
    inject_claude_code_identity(&mut array);
    assert_eq!(array["system"], json!([identity, {"type": "text", "text": "Rules."}]));

    // Anything that is not an object, or a `system` of another shape, is left alone.
    for mut other in [json!([]), json!({"system": 7})] {
        let before = other.clone();
        inject_claude_code_identity(&mut other);
        assert_eq!(other, before);
    }
}
