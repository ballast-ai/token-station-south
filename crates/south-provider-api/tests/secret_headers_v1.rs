//! Gate ① for `secret_headers` (B7a, `docs/design/2026-09-30-host-zero-vendor-boundary.md` §10):
//! a package may declare its own secret-bearing header names, and every rule that keeps them from
//! colliding with a name South already owns is refused by name.

use std::path::Path;

use south_provider_api::{
    ComponentManifestV1, MAX_SECRET_HEADER_NAME_BYTES, MAX_SECRET_HEADERS, ManifestErrorV1,
    UNDECLARABLE_SECRET_HEADER_NAMES, validate_secret_header_name,
};

fn shipped(package: &str) -> ComponentManifestV1 {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components")
        .join(package)
        .join("manifest.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// `provider-openai-compatible` declares `bearer`, `bearer_and_header_secret` and `header_secret`.
fn declaring(names: &[&str]) -> ComponentManifestV1 {
    let mut manifest = shipped("provider-openai-compatible");
    manifest.secret_headers = names.iter().map(|name| (*name).to_owned()).collect();
    manifest
}

#[test]
fn an_absent_declaration_is_todays_behavior_and_is_not_serialized() {
    let manifest = shipped("provider-openai-compatible");
    assert!(manifest.secret_headers.is_empty());
    assert_eq!(manifest.validate(), Ok(()));
    let encoded = serde_json::to_value(&manifest).unwrap();
    assert!(encoded.get("secret_headers").is_none());
}

#[test]
fn a_declared_list_validates_and_round_trips() {
    let manifest = declaring(&["x-acme-key", "x-acme-tenant-key"]);
    assert_eq!(manifest.validate(), Ok(()));
    let encoded = serde_json::to_string(&manifest).unwrap();
    assert!(encoded.contains(r#""secret_headers":["x-acme-key","x-acme-tenant-key"]"#));
    let decoded: ComponentManifestV1 = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, manifest);
}

#[test]
fn the_declaration_needs_the_header_secret_arm() {
    let mut bearer_only = declaring(&["x-acme-key"]);
    bearer_only.auth_arms.remove("header_secret");
    assert_eq!(
        bearer_only.validate(),
        Err(ManifestErrorV1::SecretHeadersRequireTheHeaderSecretArm("x-acme-key".to_owned()))
    );
}

#[test]
fn every_name_south_already_owns_is_refused() {
    for name in UNDECLARABLE_SECRET_HEADER_NAMES {
        assert_eq!(
            declaring(&[name]).validate(),
            Err(ManifestErrorV1::SecretHeaderIsReserved((*name).to_owned())),
            "{name} must stay undeclarable"
        );
    }
    // Spot checks of what the list exists for, so a list edit cannot drop them silently.
    for name in ["host", "content-length", "transfer-encoding", "connection", "authorization"] {
        assert!(UNDECLARABLE_SECRET_HEADER_NAMES.contains(&name));
    }
    for name in ["cookie", "set-cookie", "user-agent", "x-api-key", "x-amz-date"] {
        assert!(UNDECLARABLE_SECRET_HEADER_NAMES.contains(&name));
    }
}

#[test]
fn a_name_must_be_a_bounded_lowercase_token() {
    let too_long = "k".repeat(MAX_SECRET_HEADER_NAME_BYTES + 1);
    for name in ["", "X-Acme-Key", "x acme", "x:acme", "x/acme", "x-acme\n", too_long.as_str()] {
        assert_eq!(
            declaring(&[name]).validate(),
            Err(ManifestErrorV1::InvalidSecretHeaderName(name.to_owned())),
            "{name:?} must be refused"
        );
    }
    assert_eq!(validate_secret_header_name(&"k".repeat(MAX_SECRET_HEADER_NAME_BYTES)), Ok(()));
    assert_eq!(validate_secret_header_name("x_acme.key~1"), Ok(()));
}

#[test]
fn the_list_is_bounded_and_duplicate_free() {
    assert_eq!(
        declaring(&["x-acme-key", "x-acme-key"]).validate(),
        Err(ManifestErrorV1::SecretHeaderDeclaredTwice("x-acme-key".to_owned()))
    );
    let names: Vec<String> = (0..=MAX_SECRET_HEADERS).map(|index| format!("x-{index}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(
        declaring(&refs).validate(),
        Err(ManifestErrorV1::TooManySecretHeaders(MAX_SECRET_HEADERS + 1))
    );
    assert_eq!(declaring(&refs[..MAX_SECRET_HEADERS]).validate(), Ok(()));
}

#[test]
fn a_task_package_may_declare_them_alongside_header_secret() {
    let mut task = shipped("task-minimax-v2");
    task.secret_headers = vec!["x-acme-key".to_owned()];
    assert_eq!(
        task.validate(),
        Err(ManifestErrorV1::SecretHeadersRequireTheHeaderSecretArm("x-acme-key".to_owned()))
    );
    task.auth_arms.insert("header_secret".to_owned());
    assert_eq!(task.validate(), Ok(()));
}
