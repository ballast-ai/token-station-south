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
use south_contracts::{DeclaredSecretHeaderV1, SecretHeaderV1};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{
    Auth, ChatRequest, DescriptorError, HttpRequestDescriptor, ProviderConfig,
    SafeHeaders as KernelSafeHeaders, SecretRef,
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

/// A header the manifest declares in `secret_headers` is admitted as the declared arm (B7a,
/// host-zero-vendor-boundary §10). The descriptor is built in-process: see the next test for why a
/// component cannot produce it yet.
#[test]
fn a_declared_secret_header_is_admitted_as_the_declared_arm() {
    let (config, mut descriptor) = built();
    let mut declaring = manifest(&["header_secret"]);
    declaring.secret_headers = vec!["x-acme-key".to_owned()];
    assert_eq!(declaring.validate(), Ok(()));

    descriptor.auth = Some(Auth::Header { name: "X-Acme-Key".to_owned(), secret: slot(&config) });
    assert_eq!(
        admit_descriptor_auth(&declaring, &config, &descriptor),
        Ok(AdmittedAuthV1::DeclaredHeaderSecret(
            DeclaredSecretHeaderV1::parse("x-acme-key").unwrap()
        )),
        "declared names compare case-insensitively and admit in their declared form"
    );

    // The sanctioned names keep their closed arm whatever the declaration says.
    descriptor.auth = Some(Auth::Header { name: "x-api-key".to_owned(), secret: slot(&config) });
    assert_eq!(
        admit_descriptor_auth(&declaring, &config, &descriptor),
        Ok(AdmittedAuthV1::HeaderSecret(SecretHeaderV1::XApiKey))
    );

    descriptor.auth = Some(Auth::Header { name: "x-acme-other".to_owned(), secret: slot(&config) });
    assert_eq!(
        admit_descriptor_auth(&declaring, &config, &descriptor),
        Err(DescriptorAuthErrorV1::HeaderNotSanctioned("x-acme-other".to_owned()))
    );

    descriptor.auth = Some(Auth::Header { name: "x-acme-key".to_owned(), secret: slot(&config) });
    let mut undeclared_arm = declaring;
    undeclared_arm.auth_arms = BTreeSet::from(["bearer".to_owned()]);
    assert_eq!(
        admit_descriptor_auth(&undeclared_arm, &config, &descriptor),
        Err(DescriptorAuthErrorV1::HeaderSecretNotDeclared)
    );
}

/// Protocol 0.5.0 moved the name check out of the kernel: `Auth::header` admits any lowercase field
/// name outside the kernel's never-credential list, so a descriptor a component serializes can now
/// name a header outside `CREDENTIAL_HEADERS`, and this admission decides whether the package may
/// use it. (Under protocol 0.4.0 the same wire shape failed to deserialize, and the declared arm
/// was reachable only from descriptors built in-process.)
#[test]
fn a_deserialized_descriptor_reaches_the_declared_arm() {
    let (config, mut descriptor) = built();
    let mut declaring = manifest(&["header_secret"]);
    declaring.secret_headers = vec!["x-acme-key".to_owned()];
    assert_eq!(declaring.validate(), Ok(()));

    let wire = |name: &str| serde_json::json!({ "scheme": "header", "name": name, "secret": slot(&config).as_str() });
    descriptor.auth = Some(serde_json::from_value::<Auth>(wire("x-acme-key")).unwrap());
    assert_eq!(
        admit_descriptor_auth(&declaring, &config, &descriptor),
        Ok(AdmittedAuthV1::DeclaredHeaderSecret(
            DeclaredSecretHeaderV1::parse("x-acme-key").unwrap()
        ))
    );

    // The kernel now lets an undeclared name through; this admission still refuses it.
    descriptor.auth = Some(serde_json::from_value::<Auth>(wire("x-acme-other")).unwrap());
    assert_eq!(
        admit_descriptor_auth(&declaring, &config, &descriptor),
        Err(DescriptorAuthErrorV1::HeaderNotSanctioned("x-acme-other".to_owned()))
    );

    // What the kernel still refuses never reaches admission.
    for refused in ["user-agent", "content-type", "X-Acme-Key"] {
        assert!(serde_json::from_value::<Auth>(wire(refused)).is_err(), "{refused}");
    }

    for kernel_only in ["authorization", "proxy-authorization", "cookie", "set-cookie"] {
        assert!(Auth::header(kernel_only, slot(&config)).is_ok());
        assert!(
            south_provider_api::validate_secret_header_name(kernel_only).is_err(),
            "{kernel_only} is a kernel credential header but must stay undeclarable"
        );
    }
}

/// The kernel's combined arm (`Auth::BearerAndHeader`, protocol 0.5.0) is refused unless the
/// manifest declares `bearer_and_header_secret`: declaring `bearer` and `header_secret` separately
/// does not admit presenting one credential both ways (B7b, host-zero-vendor-boundary §4.3).
#[test]
fn a_bearer_and_header_descriptor_needs_the_combined_arm() {
    let (config, mut descriptor) = built();
    descriptor.auth = Some(Auth::bearer_and_header("x-goog-api-key", slot(&config)).unwrap());
    for arms in [&["bearer"][..], &["header_secret"], &["bearer", "header_secret"]] {
        assert_eq!(
            admit_descriptor_auth(&manifest(arms), &config, &descriptor),
            Err(DescriptorAuthErrorV1::BearerAndHeaderNotDeclared),
            "{arms:?}"
        );
    }
}

/// With `bearer_and_header_secret` declared, the combined arm is admitted as
/// [`AdmittedAuthV1::BearerAndHeaderSecret`], which a host maps onto
/// `RawAuthV1::BearerAndHeaderSecret`, from a descriptor built in-process or deserialized from
/// the kernel wire shape a component emits.
#[test]
fn the_combined_arm_is_admitted_when_declared() {
    let (config, mut descriptor) = built();
    let combined = manifest(&["bearer_and_header_secret"]);
    assert_eq!(combined.validate(), Ok(()));

    descriptor.auth = Some(Auth::bearer_and_header("x-goog-api-key", slot(&config)).unwrap());
    assert_eq!(
        admit_descriptor_auth(&combined, &config, &descriptor),
        Ok(AdmittedAuthV1::BearerAndHeaderSecret(SecretHeaderV1::XGoogApiKey))
    );

    let wire = serde_json::json!({
        "scheme": "bearer_and_header",
        "name": "x-goog-api-key",
        "secret": slot(&config).as_str(),
    });
    descriptor.auth = Some(serde_json::from_value::<Auth>(wire).unwrap());
    assert_eq!(
        admit_descriptor_auth(&combined, &config, &descriptor),
        Ok(AdmittedAuthV1::BearerAndHeaderSecret(SecretHeaderV1::XGoogApiKey))
    );

    // The combined arm admits neither half alone.
    descriptor.auth = Some(Auth::bearer(slot(&config)));
    assert_eq!(
        admit_descriptor_auth(&combined, &config, &descriptor),
        Err(DescriptorAuthErrorV1::BearerNotDeclared)
    );
    descriptor.auth = Some(Auth::header("x-goog-api-key", slot(&config)).unwrap());
    assert_eq!(
        admit_descriptor_auth(&combined, &config, &descriptor),
        Err(DescriptorAuthErrorV1::HeaderSecretNotDeclared)
    );
}

/// The combined arm names only a sanctioned secret header: the contract's
/// `ProviderAuthV1::BearerAndHeaderSecret` is closed over `SecretHeaderV1` (2026-09-08 combined-arm
/// record, D1), so a header the manifest declares in `secret_headers` is refused here, as is any
/// other name the kernel lets through.
#[test]
fn the_combined_arm_names_only_a_sanctioned_header() {
    let (config, mut descriptor) = built();
    let mut declaring = manifest(&["bearer_and_header_secret", "header_secret"]);
    declaring.secret_headers = vec!["x-acme-key".to_owned()];
    assert_eq!(declaring.validate(), Ok(()));

    for name in ["x-acme-key", "x-acme-other"] {
        descriptor.auth = Some(Auth::bearer_and_header(name, slot(&config)).unwrap());
        assert_eq!(
            admit_descriptor_auth(&declaring, &config, &descriptor),
            Err(DescriptorAuthErrorV1::HeaderNotSanctioned(name.to_owned())),
            "{name}"
        );
    }

    // Every sanctioned name is admitted, compared without case (the kernel keeps a catalog name
    // as written, in any case).
    for header in SecretHeaderV1::ALL {
        descriptor.auth = Some(
            Auth::bearer_and_header(header.header_name().to_ascii_uppercase(), slot(&config))
                .unwrap(),
        );
        assert_eq!(
            admit_descriptor_auth(&declaring, &config, &descriptor),
            Ok(AdmittedAuthV1::BearerAndHeaderSecret(header)),
            "{header:?}"
        );
    }
}

/// A `host_signed` package's descriptor still carries no auth, whatever the arm.
#[test]
fn a_host_signed_package_refuses_the_combined_arm() {
    let (config, mut descriptor) = built();
    descriptor.auth = Some(Auth::bearer_and_header("x-goog-api-key", slot(&config)).unwrap());
    assert_eq!(
        admit_descriptor_auth(&manifest(&["host_signed"]), &config, &descriptor),
        Err(DescriptorAuthErrorV1::HostSignedCarriesAuth)
    );
}

/// A declared name on the ordinary header channel is refused: nothing downstream redacts an
/// ordinary header, and the host's reserved check would refuse it later anyway.
#[test]
fn a_declared_secret_header_on_the_ordinary_channel_is_refused() {
    let (config, mut descriptor) = built();
    let mut declaring = manifest(&["bearer", "header_secret"]);
    declaring.secret_headers = vec!["x-acme-key".to_owned()];
    descriptor.headers = KernelSafeHeaders::try_new([("X-Acme-Key", "smuggled")]).unwrap();
    assert!(matches!(
        admit_descriptor_auth(&declaring, &config, &descriptor),
        Err(DescriptorAuthErrorV1::SecretHeaderOnOrdinaryChannel(_))
    ));
    // The same header is ordinary for a package that does not declare it.
    assert_eq!(
        admit_descriptor_auth(&manifest(&["bearer", "header_secret"]), &config, &descriptor),
        Ok(AdmittedAuthV1::Bearer)
    );
}

/// An OAuth descriptor is admitted, as Bearer, exactly when a credential recipe mints its slot
/// (B4, host-zero-vendor-boundary §3.3, §4.2).
#[test]
fn an_oauth_descriptor_is_admitted_only_on_a_minted_slot() {
    let (config, mut descriptor) = built();
    descriptor.auth = Some(Auth::OAuth { secret: slot(&config), scopes: Vec::new() });
    let mut minting = manifest(&["bearer"]);
    minting.credentials = Some(
        serde_json::from_value(serde_json::json!({
            "schema": "south.credential-recipe.v1",
            "fields": { "secret_key": { "secret": true, "required": true } },
            "slots": { slot(&config).as_str(): { "minted": "token" } },
            "recipes": { "token": {
                "steps": [{ "id": "jwt", "kind": "jwt_sign", "alg": "HS256",
                            "key": { "field": "secret_key" }, "claims": { "exp": { "now_plus": 60 } } }],
                "present": "jwt.jwt", "rotates_refresh_material": false,
                "default_seconds": 60 } }
        }))
        .unwrap(),
    );
    assert_eq!(minting.validate(), Ok(()));
    assert_eq!(admit_descriptor_auth(&minting, &config, &descriptor), Ok(AdmittedAuthV1::Bearer));

    // A section scoped to another family mints nothing for this one (§13.5 D2).
    let other = minting.providers.iter().find(|family| **family != config.provider).cloned();
    minting.credentials.as_mut().unwrap().families = Some(vec![other.unwrap()]);
    assert_eq!(minting.validate(), Ok(()));
    assert_eq!(
        admit_descriptor_auth(&minting, &config, &descriptor),
        Err(DescriptorAuthErrorV1::OAuthNotAdmitted)
    );
    minting.credentials.as_mut().unwrap().families = Some(vec![config.provider.clone()]);
    assert_eq!(admit_descriptor_auth(&minting, &config, &descriptor), Ok(AdmittedAuthV1::Bearer));

    minting.credentials.as_mut().unwrap().slots.clear();
    assert_eq!(
        admit_descriptor_auth(&minting, &config, &descriptor),
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
        &manifest(&["bearer", "header_secret", "bearer_and_header_secret"]),
    );
    let ran: Vec<_> = shipped
        .outcomes()
        .iter()
        .filter(|outcome| outcome.check == CheckV1::DescriptorAuthWithinManifest)
        .collect();
    assert!(!ran.is_empty(), "the check never ran");
    assert!(ran.iter().all(|outcome| !outcome.is_failure()), "{shipped}");

    // The reference presents Bearer for `openai-compatible`, `api-key` for Azure and both Bearer and
    // `x-goog-api-key` for `gemini-openai-compatible`; a manifest missing an arm must see the
    // case that presents it refused.
    for (arms, refused_case) in [
        (&["header_secret", "bearer_and_header_secret"][..], "provider.request.chat"),
        (&["bearer", "bearer_and_header_secret"], "provider.request.azure-header-auth"),
        (&["bearer", "header_secret"], "provider.request.gemini-openai-compatible"),
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
