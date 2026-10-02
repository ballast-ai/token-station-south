//! Descriptor auth admission (B2, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §4.2 and
//! §4.4): every rule refuses what it guards, and gate ② turns red on a package whose descriptors
//! present an arm its manifest does not declare.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;
use south_component_conformance::reference::OpenAiCompatibleReferenceV1;
use south_component_conformance::{
    AdmittedAuthV1, CheckV1, DescriptorAuthErrorV1, FixturePackV1, ProviderComponentV1,
    admit_descriptor_auth, run_provider_component_suite_v1_for_manifest,
};
use south_contracts::SecretHeaderV1;
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{
    Auth, ChatRequest, DescriptorError, HttpRequestDescriptor, ProviderConfig, SecretRef,
};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest(arms: &[&str]) -> ComponentManifestV1 {
    let path = root().join("../../components/provider-openai-compatible/manifest.json");
    let mut manifest: ComponentManifestV1 =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    manifest.auth_arms = arms.iter().map(|arm| (*arm).to_owned()).collect::<BTreeSet<_>>();
    manifest
}

/// The shipped chat request fixture, and the descriptor the reference builds from it.
fn built() -> (ProviderConfig, HttpRequestDescriptor) {
    let input: Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("fixtures/provider.request.chat.input.json")).unwrap(),
    )
    .unwrap();
    let config: ProviderConfig = serde_json::from_value(input["provider_config"].clone()).unwrap();
    let request: ChatRequest = serde_json::from_value(input["chat_request"].clone()).unwrap();
    let descriptor = OpenAiCompatibleReferenceV1.build_http_request(&request, &config).unwrap();
    (config, descriptor)
}

fn slot(config: &ProviderConfig) -> SecretRef {
    config.auth.clone().expect("the chat fixture configures a slot")
}

#[test]
fn each_declared_arm_is_admitted_as_what_it_presents() {
    let (config, mut descriptor) = built();
    let both = manifest(&["bearer", "header_secret"]);
    assert_eq!(admit_descriptor_auth(&both, &config, &descriptor), Ok(AdmittedAuthV1::Bearer));

    descriptor.auth = Some(Auth::Header { name: "X-Api-Key".to_owned(), secret: slot(&config) });
    assert_eq!(
        admit_descriptor_auth(&both, &config, &descriptor),
        Ok(AdmittedAuthV1::HeaderSecret(SecretHeaderV1::XApiKey)),
        "header names compare case-insensitively"
    );

    let mut open = config;
    open.auth = None;
    descriptor.auth = None;
    assert_eq!(admit_descriptor_auth(&both, &open, &descriptor), Ok(AdmittedAuthV1::None));

    let signed = {
        let mut signed = manifest(&["host_signed"]);
        signed.emits = vec!["authorization".to_owned()];
        signed
    };
    assert_eq!(admit_descriptor_auth(&signed, &open, &descriptor), Ok(AdmittedAuthV1::HostSigned));
}

#[test]
fn an_arm_the_manifest_does_not_declare_is_refused() {
    let (config, mut descriptor) = built();
    assert_eq!(
        admit_descriptor_auth(&manifest(&["header_secret"]), &config, &descriptor),
        Err(DescriptorAuthErrorV1::BearerNotDeclared)
    );

    descriptor.auth = Some(Auth::Header { name: "x-api-key".to_owned(), secret: slot(&config) });
    assert_eq!(
        admit_descriptor_auth(&manifest(&["bearer"]), &config, &descriptor),
        Err(DescriptorAuthErrorV1::HeaderSecretNotDeclared)
    );

    descriptor.auth =
        Some(Auth::Header { name: "x-not-a-secret".to_owned(), secret: slot(&config) });
    assert_eq!(
        admit_descriptor_auth(&manifest(&["header_secret"]), &config, &descriptor),
        Err(DescriptorAuthErrorV1::HeaderNotSanctioned("x-not-a-secret".to_owned()))
    );

    descriptor.auth = Some(Auth::OAuth { secret: slot(&config), scopes: Vec::new() });
    assert_eq!(
        admit_descriptor_auth(&manifest(&["bearer"]), &config, &descriptor),
        Err(DescriptorAuthErrorV1::OAuthNotAdmitted)
    );
}

#[test]
fn a_host_signed_descriptor_names_no_credential() {
    let (config, descriptor) = built();
    let mut signed = manifest(&["host_signed"]);
    signed.emits = vec!["authorization".to_owned()];
    // The host passes no slot for a host-signed package, so a descriptor naming one fails the
    // kernel gate; with the slot passed anyway, the arm itself refuses it.
    assert_eq!(
        admit_descriptor_auth(&signed, &config, &descriptor),
        Err(DescriptorAuthErrorV1::HostSignedCarriesAuth)
    );
}

#[test]
fn the_kernel_gate_runs_first() {
    let (config, mut descriptor) = built();
    descriptor.url = "https://attacker.example/v1/chat/completions".to_owned();
    assert!(matches!(
        admit_descriptor_auth(&manifest(&["bearer"]), &config, &descriptor),
        Err(DescriptorAuthErrorV1::NotAuthorized(DescriptorError::UrlOutsideEndpoint { .. }))
    ));

    let (config, mut descriptor) = built();
    descriptor.auth = None;
    assert!(matches!(
        admit_descriptor_auth(&manifest(&["bearer"]), &config, &descriptor),
        Err(DescriptorAuthErrorV1::NotAuthorized(DescriptorError::MissingCredential))
    ));
}

#[test]
fn gate_two_refuses_a_package_whose_descriptors_present_an_undeclared_arm() {
    let pack = FixturePackV1::load(&root().join("fixtures")).unwrap();

    let shipped = run_provider_component_suite_v1_for_manifest(
        &OpenAiCompatibleReferenceV1,
        &pack,
        &manifest(&["bearer", "header_secret"]),
    );
    let ran: Vec<_> = shipped
        .outcomes()
        .iter()
        .filter(|outcome| outcome.check == CheckV1::DescriptorAuthWithinManifest)
        .collect();
    assert!(!ran.is_empty(), "the check never ran");
    assert!(ran.iter().all(|outcome| !outcome.is_failure()), "{shipped}");

    // The reference presents Bearer for `openai-compatible` and `api-key` for Azure; a manifest
    // that declares only one arm must see the other refused.
    for (arms, refused_case) in [
        (&["header_secret"][..], "provider.request.chat"),
        (&["bearer"][..], "provider.request.azure-header-auth"),
    ] {
        let report = run_provider_component_suite_v1_for_manifest(
            &OpenAiCompatibleReferenceV1,
            &pack,
            &manifest(arms),
        );
        assert!(
            report.failures().any(|outcome| outcome.check == CheckV1::DescriptorAuthWithinManifest
                && outcome.case == refused_case),
            "{arms:?}: {report}"
        );
    }
}
