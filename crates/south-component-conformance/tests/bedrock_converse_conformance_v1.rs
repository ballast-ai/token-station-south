//! Gates ① and ② for the official AWS Bedrock Converse component, run against
//! its native reference implementation over its own frozen fixture pack.
//!
//! A second component means a second pack: the suite is the same, the cases are
//! not. Sharing one pack between dialects would freeze whichever dialect
//! happened to be written first.
//!
//! This is the first provider component on the `host_signed` arm, so gate ①
//! here is also the first time that arm's manifest rules are exercised by a
//! shipped package: exactly one arm, a non-empty `emits`, and every emitted name
//! drawn from the signed-header vocabulary.

use std::path::Path;

use south_component_conformance::reference_bedrock_converse::{
    BedrockConverseBearerReferenceV1, BedrockConverseReferenceV1,
};
use south_component_conformance::{
    FixturePackV1, ProviderComponentV1, accepts_manifest, reported_identity_matches,
    run_provider_component_suite_v1_for_manifest,
};
use south_provider_api::{
    ComponentManifestV1, PROVIDER_WORLD, SIGNED_HEADER_NAMES, compatibility_admits,
};

#[path = "support/host_range.rs"]
mod host_range;

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).expect("repo root")
}

/// The manifest the component actually ships, read from disk rather than
/// hand-copied: a hand-copy resembles it, which is not the same as being it.
fn shipped_manifest() -> ComponentManifestV1 {
    let source = std::fs::read_to_string(
        repo_root().join("components/provider-bedrock-converse/manifest.json"),
    )
    .expect("the shipped component manifest reads");
    serde_json::from_str(&source).expect("the shipped component manifest parses")
}

fn shipped_pack() -> FixturePackV1 {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures-bedrock-converse");
    FixturePackV1::load(&directory).expect("the shipped fixture pack loads")
}

/// Gate ①: the package the component ships is admissible, and the identity it
/// reports at runtime is the identity its manifest claims.
#[test]
fn gate_one_admits_the_shipped_package_and_its_reported_identity() {
    let manifest = shipped_manifest();
    assert!(accepts_manifest(&manifest).is_ok(), "the shipped manifest must pass gate ①");
    assert!(
        compatibility_admits(&manifest, &host_range::host_range()).is_ok(),
        "the shipped manifest must fall inside this release's compatibility range"
    );
    assert!(
        reported_identity_matches(&BedrockConverseReferenceV1.metadata(), &manifest),
        "the identity the component reports must be the one its manifest claims"
    );
    assert_eq!(manifest.api_version, PROVIDER_WORLD);
}

/// The `host_signed` arm's own rules, spelled out rather than left to gate ①'s
/// pass/fail — this is the first shipped package to use the arm, so the facts it
/// depends on are worth naming.
#[test]
fn the_manifest_declares_the_host_signed_arm_and_nothing_beside_it() {
    let manifest = shipped_manifest();
    assert!(manifest.auth_arms.contains("host_signed"));
    assert_eq!(
        manifest.auth_arms.len(),
        1,
        "the schema refuses any second arm alongside host_signed; Bedrock API keys are the \
         Bearer sibling package's arm, not a second arm here"
    );
    // A component that never holds a credential must not claim a secret slot.
    assert!(
        manifest.permissions.secrets.is_empty(),
        "a host_signed component is signed for, so it references no secret of its own"
    );
    assert!(!manifest.emits.is_empty(), "the arm requires the emitted header set to be named");
    for header in &manifest.emits {
        assert!(
            SIGNED_HEADER_NAMES.contains(&header.as_str()),
            "`{header}` is not a signed-header name the host can emit"
        );
    }
    // The full set, not the three a credential without an STS session token
    // produces: `emits` is declared once and statically, while the host's actual
    // set is a function of the credential. Declaring the maximum is the only
    // choice that stays correct for the recommended (temporary-credential)
    // deployment shape. See the plan's A1 note for why neither choice is
    // strictly right.
    assert!(
        manifest.emits.iter().any(|header| header == "x-amz-security-token"),
        "the STS session-token header must be declared, or an STS deployment's signer would \
         emit a header the manifest never named"
    );
}

/// Gate ②: the reference implementation answers its own frozen pack exactly.
#[test]
fn gate_two_passes_over_the_shipped_pack() {
    let report = run_provider_component_suite_v1_for_manifest(
        &BedrockConverseReferenceV1,
        &shipped_pack(),
        &shipped_manifest(),
    );
    let failures: Vec<String> = report.failures().map(|outcome| format!("{outcome:?}")).collect();
    assert!(failures.is_empty(), "{} gate ② failures:\n{}", failures.len(), failures.join("\n"));
}

/// The pack must keep exercising the two shapes this dialect gets wrong most
/// easily. A pack can pass while having quietly lost a case, and these two are
/// the ones whose absence would not be obvious.
#[test]
fn the_shipped_pack_still_carries_the_decided_behaviours() {
    let names: Vec<String> = shipped_pack().cases().iter().map(|case| case.name.clone()).collect();
    for required in [
        // Withholding the whole toolConfig, not just its toolChoice.
        "tool-choice-none-withholds-the-whole-config",
        // Parallel results in one user message, which Converse requires.
        "parallel-tool-results-share-one-user-message",
        // Done waits for metadata, so a cut stream cannot settle as complete.
        "text-ends-in-two-phases",
        // A streaming request asks for the binary eventstream, as the host's native arm does
        // (host feedback SF14).
        "stream-asks-for-eventstream",
    ] {
        // `CaseV1::name` is the full `provider.<family>.<case>`, so the case
        // name is matched as the trailing segment.
        assert!(
            names.iter().any(|name| name.ends_with(required)),
            "the pack lost the `{required}` case; it is in the pack because the dialect is easy \
             to get wrong here, not because it was convenient to write"
        );
    }
}

// ── The Bearer sibling (host feedback SF16, host-zero-vendor-boundary §13.6) ─────────────────

fn read_manifest(package: &str) -> ComponentManifestV1 {
    let source =
        std::fs::read_to_string(repo_root().join("components").join(package).join("manifest.json"))
            .expect("the shipped component manifest reads");
    serde_json::from_str(&source).expect("the shipped component manifest parses")
}

/// Gates ① and ② for `provider-bedrock-converse-bearer`, over its own pack.
#[test]
fn the_bearer_sibling_passes_gates_one_and_two() {
    let manifest = read_manifest("provider-bedrock-converse-bearer");
    assert_eq!(accepts_manifest(&manifest), Ok(()));
    assert!(compatibility_admits(&manifest, &host_range::host_range()).is_ok());
    assert!(reported_identity_matches(&BedrockConverseBearerReferenceV1.metadata(), &manifest));
    assert_eq!(manifest.auth_arms.iter().collect::<Vec<_>>(), ["bearer"]);
    assert!(manifest.signing.is_none() && manifest.emits.is_empty());

    let pack = FixturePackV1::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures-bedrock-converse-bearer"),
    )
    .expect("the bearer fixture pack loads");
    let report = run_provider_component_suite_v1_for_manifest(
        &BedrockConverseBearerReferenceV1,
        &pack,
        &manifest,
    );
    let failures: Vec<String> = report.failures().map(|outcome| format!("{outcome:?}")).collect();
    assert!(failures.is_empty(), "{} gate ② failures:\n{}", failures.len(), failures.join("\n"));
}

/// "Otherwise the same declarations": the two manifests differ only in identity, family, auth arm,
/// the slot, `signing` / `emits` / `credentials` (which only the signed arm has) and the fixture
/// directory. A Converse change that forgot its sibling fails here.
#[test]
fn the_bearer_manifest_differs_from_converse_only_in_auth_and_family() {
    let converse = read_manifest("provider-bedrock-converse");
    let bearer = read_manifest("provider-bedrock-converse-bearer");
    assert_eq!(bearer.providers, ["bedrock-bearer"]);
    assert_eq!(bearer.capabilities, converse.capabilities);
    assert_eq!(bearer.stream_framing, converse.stream_framing);
    assert_eq!(bearer.usage_evidence, converse.usage_evidence);
    assert_eq!(bearer.compatibility, converse.compatibility);
    assert_eq!(bearer.endpoint.get("bedrock-bearer"), converse.endpoint.get("bedrock"));
    assert_eq!(bearer.config_schema.get("bedrock-bearer"), converse.config_schema.get("bedrock"));
    assert_eq!(bearer.request_facts.get("bedrock-bearer"), converse.request_facts.get("bedrock"));
    assert_eq!(bearer.permissions.secrets, ["provider_api_key"]);
}

/// The bearer pack is the Converse pack with only the auth delta applied: request inputs name the
/// `bedrock-bearer` family and the `provider_api_key` slot, request expectations carry the bearer
/// auth, and every other file is byte-identical. A Converse fixture change that forgot its sibling
/// fails here, so the two packs cannot drift apart.
#[test]
fn the_bearer_pack_is_the_converse_pack_with_only_the_auth_delta() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let list = |dir: &str| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(root.join(dir))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let (converse, bearer) = ("fixtures-bedrock-converse", "fixtures-bedrock-converse-bearer");
    assert_eq!(list(converse), list(bearer), "the two packs hold the same cases");
    let read = |dir: &str, name: &str| std::fs::read(root.join(dir).join(name)).unwrap();
    let json = |bytes: &[u8]| -> serde_json::Value { serde_json::from_slice(bytes).unwrap() };
    for name in list(converse) {
        let (from, to) = (read(converse, &name), read(bearer, &name));
        if name.starts_with("provider.request.") && name.ends_with(".input.json") {
            let mut expected = json(&from);
            expected["provider_config"]["provider"] = "bedrock-bearer".into();
            expected["provider_config"]["auth"] = "provider_api_key".into();
            assert_eq!(json(&to), expected, "{name}");
        } else if name.starts_with("provider.request.") {
            let mut expected = json(&from);
            expected["auth"] =
                serde_json::json!({"scheme": "bearer", "secret": "provider_api_key"});
            assert_eq!(json(&to), expected, "{name}");
        } else if name.starts_with("provider.capabilities.") && name.ends_with(".input.json") {
            let mut expected = json(&from);
            expected["provider"] = "bedrock-bearer".into();
            assert_eq!(json(&to), expected, "{name}");
        } else {
            assert_eq!(from, to, "{name} is byte-identical in both packs");
        }
    }
}

/// The signed package's descriptor never carries auth, the bearer one always names its slot, and
/// both send the native arm's `accept` and `x-amzn-bedrock-accept` (host feedback SF14).
#[test]
fn both_packages_send_the_native_accept_headers() {
    let input: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures-bedrock-converse/provider.request.chat.input.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut request: token_station_protocol::ChatRequest =
        serde_json::from_value(input["chat_request"].clone()).unwrap();
    let mut config: token_station_protocol::ProviderConfig =
        serde_json::from_value(input["provider_config"].clone()).unwrap();
    for stream in [false, true] {
        request.stream = stream;
        let accept = if stream { "application/vnd.amazon.eventstream" } else { "application/json" };
        config.provider = "bedrock".to_owned();
        let signed = BedrockConverseReferenceV1.build_http_request(&request, &config).unwrap();
        config.provider = "bedrock-bearer".to_owned();
        let bearer =
            BedrockConverseBearerReferenceV1.build_http_request(&request, &config).unwrap();
        for descriptor in [&signed, &bearer] {
            assert_eq!(descriptor.headers.get("accept"), Some(accept));
            assert_eq!(descriptor.headers.get("x-amzn-bedrock-accept"), Some("application/json"));
        }
        assert!(signed.auth.is_none(), "the signed arm's descriptor carries no auth");
        assert_eq!(signed.url, bearer.url);
        assert_eq!(signed.body, bearer.body);
    }
    // Each package serves only its own family.
    config.provider = "bedrock".to_owned();
    assert!(BedrockConverseBearerReferenceV1.build_http_request(&request, &config).is_err());
    config.provider = "bedrock-bearer".to_owned();
    assert!(BedrockConverseReferenceV1.build_http_request(&request, &config).is_err());
}
