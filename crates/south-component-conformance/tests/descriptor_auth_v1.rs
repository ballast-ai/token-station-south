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
/// host-zero-vendor-boundary §10). The descriptor is built in-process here; the next test builds
/// it the way a component's JSON reaches a host.
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

/// B7b: protocol 0.5.0 moved the credential header name check out of the kernel, so a component's
/// descriptor, deserialized from the JSON a component returns, can name a header the manifest
/// declares in `secret_headers`. The placeholder test that pinned the opposite
/// (`until_b7b_no_component_descriptor_can_name_a_declared_header`) is replaced by this one.
#[test]
fn a_deserialized_descriptor_naming_a_declared_header_is_admitted() {
    let (config, descriptor) = built();
    let mut declaring = manifest(&["bearer", "header_secret"]);
    declaring.secret_headers = vec!["x-acme-key".to_owned()];
    assert_eq!(declaring.validate(), Ok(()));

    let over_the_wire = |name: &str| -> HttpRequestDescriptor {
        let mut wire = serde_json::to_value(&descriptor).unwrap();
        wire["auth"] = serde_json::json!({
            "scheme": "header", "name": name, "secret": slot(&config).as_str()
        });
        serde_json::from_value(wire).expect("a lowercase token outside the never list decodes")
    };

    assert_eq!(
        admit_descriptor_auth(&declaring, &config, &over_the_wire("x-acme-key")),
        Ok(AdmittedAuthV1::DeclaredHeaderSecret(
            DeclaredSecretHeaderV1::parse("x-acme-key").unwrap()
        ))
    );
    // The kernel now admits the name; whether this package may use it is the manifest's call.
    assert_eq!(
        admit_descriptor_auth(
            &manifest(&["bearer", "header_secret"]),
            &config,
            &over_the_wire("x-acme-key")
        ),
        Err(DescriptorAuthErrorV1::HeaderNotSanctioned("x-acme-key".to_owned()))
    );
    assert_eq!(
        admit_descriptor_auth(&declaring, &config, &over_the_wire("x-acme-other")),
        Err(DescriptorAuthErrorV1::HeaderNotSanctioned("x-acme-other".to_owned()))
    );
    // A sanctioned name keeps its closed arm whatever the declaration says.
    assert_eq!(
        admit_descriptor_auth(&declaring, &config, &over_the_wire("x-goog-api-key")),
        Ok(AdmittedAuthV1::HeaderSecret(SecretHeaderV1::XGoogApiKey))
    );
}

/// The kernel still refuses, at deserialization, every name that cannot carry a credential, and
/// keeps admitting the 0.4.0 catalog names that South forbids to declare. For those the
/// manifest's rule, not the kernel's, is what stops them.
#[test]
fn the_kernel_refuses_unusable_names_and_south_forbids_the_catalog_names_it_keeps() {
    let (config, descriptor) = built();
    let wire = |name: &str| {
        let mut wire = serde_json::to_value(&descriptor).unwrap();
        wire["auth"] = serde_json::json!({
            "scheme": "header", "name": name, "secret": slot(&config).as_str()
        });
        serde_json::from_value::<HttpRequestDescriptor>(wire)
    };
    let too_long = "k".repeat(65);
    for name in
        ["user-agent", "content-type", "host", "X-Acme-Key", "x acme", "", too_long.as_str()]
    {
        assert!(wire(name).is_err(), "the kernel must refuse `{name}` while deserializing");
    }

    // Admitted by the kernel for 0.4.0 compatibility, never declarable here, so never admitted.
    let mut declaring = manifest(&["bearer", "header_secret"]);
    for name in ["authorization", "proxy-authorization", "cookie", "set-cookie"] {
        assert!(Auth::header(name, slot(&config)).is_ok(), "{name}: the kernel keeps accepting it");
        assert!(south_provider_api::validate_secret_header_name(name).is_err());
        // Even a manifest that skipped gate ① and listed the name declares nothing.
        declaring.secret_headers = vec![name.to_owned()];
        assert_eq!(
            admit_descriptor_auth(&declaring, &config, &wire(name).unwrap()),
            Err(DescriptorAuthErrorV1::HeaderNotSanctioned(name.to_owned())),
            "{name}"
        );
    }
}

/// The kernel's rule is looser than South's, and must stay so: every name the kernel refuses as
/// never able to carry a credential is also undeclarable here, and every name of the kernel's
/// default redaction set is either sanctioned or undeclarable. So the kernel's change cannot open
/// a name South forbids (§13.7 item 5, Q40). The other direction holds too: every sanctioned name
/// is in the kernel's redaction set, so a host that redacts by default redacts every closed arm.
#[test]
fn south_forbids_everything_the_kernel_never_lets_carry_a_credential() {
    for header in SecretHeaderV1::ALL {
        assert!(
            token_station_protocol::is_credential_header(header.header_name()),
            "`{}` is sanctioned but outside the kernel's default redaction set",
            header.header_name()
        );
    }
    for name in token_station_protocol::NEVER_CREDENTIAL_HEADERS {
        assert!(
            south_provider_api::UNDECLARABLE_SECRET_HEADER_NAMES.contains(name),
            "`{name}` is on the kernel's never-credential list but a package could declare it"
        );
        assert!(south_provider_api::validate_secret_header_name(name).is_err());
    }
    for name in token_station_protocol::CREDENTIAL_HEADERS {
        let sanctioned = SecretHeaderV1::ALL
            .iter()
            .any(|header| header.header_name().eq_ignore_ascii_case(name));
        assert!(
            sanctioned || south_provider_api::UNDECLARABLE_SECRET_HEADER_NAMES.contains(name),
            "`{name}` is in the kernel's redaction set and a package could declare it"
        );
    }
}

/// The combined arm (B7b, §4.3): the one secret as `Authorization: Bearer` and in a sanctioned
/// header, admitted only for a manifest that declares `bearer_and_header_secret`.
#[test]
fn the_combined_arm_is_admitted_for_a_sanctioned_header_and_a_declaring_manifest() {
    let (config, mut descriptor) = built();
    let combined = manifest(&["bearer_and_header_secret"]);
    descriptor.auth =
        Some(Auth::bearer_and_header("x-goog-api-key", slot(&config)).expect("the Gemini pair"));
    assert_eq!(
        admit_descriptor_auth(&combined, &config, &descriptor),
        Ok(AdmittedAuthV1::BearerAndHeaderSecret(SecretHeaderV1::XGoogApiKey))
    );

    // Every sanctioned name is admissible, in any case the kernel keeps.
    for header in SecretHeaderV1::ALL {
        descriptor.auth = Some(Auth::BearerAndHeader {
            name: header.header_name().to_ascii_uppercase(),
            secret: slot(&config),
        });
        assert_eq!(
            admit_descriptor_auth(&combined, &config, &descriptor),
            Ok(AdmittedAuthV1::BearerAndHeaderSecret(header)),
            "{}",
            header.header_name()
        );
    }

    // Declaring the two arms it combines is not declaring it.
    descriptor.auth = Some(Auth::bearer_and_header("x-goog-api-key", slot(&config)).unwrap());
    for arms in [&["bearer"][..], &["header_secret"], &["bearer", "header_secret"], &["oauth"]] {
        assert_eq!(
            admit_descriptor_auth(&manifest(arms), &config, &descriptor),
            Err(DescriptorAuthErrorV1::BearerAndHeaderNotDeclared),
            "{arms:?}"
        );
    }
    // And declaring it admits neither of the single arms.
    let (_, bearer) = built();
    assert_eq!(
        admit_descriptor_auth(&combined, &config, &bearer),
        Err(DescriptorAuthErrorV1::BearerNotDeclared)
    );
    let mut header = bearer;
    header.auth = Some(Auth::header("x-goog-api-key", slot(&config)).unwrap());
    assert_eq!(
        admit_descriptor_auth(&combined, &config, &header),
        Err(DescriptorAuthErrorV1::HeaderSecretNotDeclared)
    );
}

/// The contract's combined arm is closed over the five sanctioned names (§16 Q35): a name the
/// manifest declares in `secret_headers` is refused on it, and so is one nobody declared.
#[test]
fn the_combined_arm_refuses_a_declared_or_unknown_header() {
    let (config, mut descriptor) = built();
    let mut declaring = manifest(&["bearer_and_header_secret", "header_secret"]);
    declaring.secret_headers = vec!["x-acme-key".to_owned()];
    assert_eq!(declaring.validate(), Ok(()));
    for name in ["x-acme-key", "x-not-declared"] {
        descriptor.auth = Some(Auth::bearer_and_header(name, slot(&config)).unwrap());
        assert_eq!(
            admit_descriptor_auth(&declaring, &config, &descriptor),
            Err(DescriptorAuthErrorV1::CombinedHeaderNotSanctioned(name.to_owned())),
            "{name}"
        );
    }
    // The same declared name on the single header arm is still admitted.
    descriptor.auth = Some(Auth::header("x-acme-key", slot(&config)).unwrap());
    assert!(matches!(
        admit_descriptor_auth(&declaring, &config, &descriptor),
        Ok(AdmittedAuthV1::DeclaredHeaderSecret(_))
    ));
}

/// The kernel itself refuses `authorization` as the second header of the combined arm, so a
/// descriptor cannot even name it; a declared secret header on the ordinary channel is refused
/// under the combined arm as under the others.
#[test]
fn the_combined_arm_keeps_the_ordinary_channel_rules() {
    let (config, mut descriptor) = built();
    assert!(Auth::bearer_and_header("authorization", slot(&config)).is_err());
    assert!(
        serde_json::from_str::<Auth>(
            r#"{"scheme":"bearer_and_header","name":"authorization","secret":"k"}"#
        )
        .is_err()
    );

    let mut declaring = manifest(&["bearer_and_header_secret", "header_secret"]);
    declaring.secret_headers = vec!["x-acme-key".to_owned()];
    descriptor.auth = Some(Auth::bearer_and_header("x-goog-api-key", slot(&config)).unwrap());
    descriptor.headers = KernelSafeHeaders::try_new([("X-Acme-Key", "smuggled")]).unwrap();
    assert!(matches!(
        admit_descriptor_auth(&declaring, &config, &descriptor),
        Err(DescriptorAuthErrorV1::SecretHeaderOnOrdinaryChannel(_))
    ));
}

/// A `host_signed` package carries no auth at all, the combined arm included.
#[test]
fn a_host_signed_package_refuses_the_combined_arm_like_any_other() {
    let (config, mut descriptor) = built();
    let mut signed = manifest(&["host_signed"]);
    signed.emits = vec!["authorization".to_owned()];
    descriptor.auth = Some(Auth::bearer_and_header("x-goog-api-key", slot(&config)).unwrap());
    assert_eq!(
        admit_descriptor_auth(&signed, &config, &descriptor),
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
