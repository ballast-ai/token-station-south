//! Declared secret headers (auth contract version five, reserved-header policy version two;
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §10): a package-declared name is reserved
//! on the ordinary header channel and dropped from the response transcript, exactly as the
//! sanctioned names are.

use south_contracts::{
    AUTH_CONTRACT_VERSION, BearerAuthV1, ContractErrorV1, CredentialSlotV1, DeclaredSecretHeaderV1,
    DeclaredSecretHeadersV1, HeaderPolicyError, ProviderAuthV1, RESERVED_HEADER_POLICY_VERSION,
    ResponseTranscriptV1, SafeHeaders, SecretHeaderV1,
};

fn declared(names: &[&str]) -> DeclaredSecretHeadersV1 {
    DeclaredSecretHeadersV1::try_from_names(names.iter().copied()).unwrap()
}

#[test]
fn contract_numbers_name_the_declared_instances() {
    assert_eq!(AUTH_CONTRACT_VERSION, 5);
    assert_eq!(RESERVED_HEADER_POLICY_VERSION, 2);
}

#[test]
fn a_declared_name_is_reserved_on_the_ordinary_channel_for_its_package_only() {
    let package = declared(&["x-acme-key"]);
    for spelling in ["x-acme-key", "X-Acme-Key"] {
        assert_eq!(
            SafeHeaders::try_from_iter_with_secret_headers([(spelling, "smuggled")], &package),
            Err(HeaderPolicyError::ReservedHeader),
            "{spelling} must be refused for the declaring package"
        );
    }

    // Another package, or a call outside any package, may still send the name as an ordinary
    // header: the addition is per package, not a global widening of the list.
    let other = SafeHeaders::try_from_iter([("x-acme-key", "ordinary")]).unwrap();
    assert!(other.secret_headers().is_empty());

    // Every other name is judged exactly as policy version one judges it.
    let headers =
        SafeHeaders::try_from_iter_with_secret_headers([("x-acme-trace", "visible")], &package)
            .unwrap();
    assert_eq!(headers.get("x-acme-trace"), Some("visible"));
    assert_eq!(headers.secret_headers(), &package);
    assert_eq!(
        SafeHeaders::try_from_iter_with_secret_headers([("x-api-key", "v")], &package),
        Err(HeaderPolicyError::ReservedHeader)
    );
}

#[test]
fn a_declared_name_never_enters_the_response_transcript() {
    let package = declared(&["x-acme-key"]);
    let upstream = [
        ("X-Acme-Key", Some("echoed-secret")),
        ("x-acme-trace", Some("visible")),
        ("x-api-key", Some("echoed-sanctioned-secret")),
    ];

    let redacted = ResponseTranscriptV1::capture_redacting(upstream, &package);
    let names: Vec<&str> = redacted.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["x-acme-trace"]);
    assert!(!redacted.truncated(), "a redaction is the contract working, not a truncation");

    // Without the package's declaration the declared name is an ordinary response header, but a
    // sanctioned secret header is dropped under policy version two whatever the package.
    let plain = ResponseTranscriptV1::capture(upstream);
    let names: Vec<&str> = plain.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["x-acme-key", "x-acme-trace"]);
    for header in SecretHeaderV1::ALL {
        let transcript = ResponseTranscriptV1::capture([(header.header_name(), Some("echo"))]);
        assert!(transcript.is_empty(), "{} must never be transcribed", header.header_name());
    }
}

#[test]
fn the_declared_arm_carries_a_name_and_a_slot_but_no_value() {
    let header = DeclaredSecretHeaderV1::parse("x-acme-key").unwrap();
    let slot = CredentialSlotV1::parse("acme_api_key").unwrap();
    let auth = ProviderAuthV1::DeclaredHeaderSecret { header, slot: BearerAuthV1::new(slot) };
    assert_eq!(auth.credential_slot().as_str(), "acme_api_key");
    let debug = format!("{auth:?}");
    assert!(debug.contains("x-acme-key"), "the header name is not a secret: {debug}");
    assert!(!debug.contains("acme_api_key"), "the slot stays redacted as in every arm: {debug}");
}

#[test]
fn stable_codes_name_each_declaration_failure() {
    assert_eq!(
        DeclaredSecretHeaderV1::parse("Authorization").map_err(ContractErrorV1::code),
        Err("INVALID_SECRET_HEADER_NAME")
    );
    assert_eq!(
        DeclaredSecretHeadersV1::try_from_names(["x-a", "x-a"]).map_err(ContractErrorV1::code),
        Err("INVALID_SECRET_HEADER_SET")
    );
    assert_eq!(ContractErrorV1::UndeclaredSecretHeader.code(), "UNDECLARED_SECRET_HEADER");
}
