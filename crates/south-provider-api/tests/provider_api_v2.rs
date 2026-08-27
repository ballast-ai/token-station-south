//! Public contract tests for the v2 provider component ABI and its manifest.

use std::collections::BTreeSet;

use south_provider_api::{
    ADAPTER_WIT, COMPONENT_BEHAVIOR_SUITE, CompatibilityDeclarationV1, CompatibilityMismatchV1,
    ComponentManifestV1, ComponentPermissionsV1, ConformanceSpecV1, HostExpectationsV1,
    KNOWN_WORLDS, ManifestErrorV1, PROVIDER_WORLD, PROVIDER_WORLD_SCHEMA, WIT_PACKAGE,
    compatibility_matches, known_world,
};
use wit_parser::{Resolve, Type, TypeDefKind};

fn resolve() -> Resolve {
    let mut resolve = Resolve::new();
    resolve
        .push_str("provider-adapter.wit", ADAPTER_WIT)
        .expect("wit/provider-adapter.wit must parse");
    resolve
}

#[test]
fn wit_source_parses_and_names_the_frozen_package() {
    let resolve = resolve();
    let (_, package) = resolve.packages.iter().next().expect("one package");
    let name = &package.name;
    let rendered = format!(
        "{}:{}@{}",
        name.namespace,
        name.name,
        name.version.as_ref().expect("package is versioned")
    );
    assert_eq!(rendered, WIT_PACKAGE);
}

#[test]
fn the_provider_world_exists_and_is_the_only_world() {
    let resolve = resolve();
    let worlds: Vec<_> = resolve.worlds.iter().map(|(_, w)| w.name.clone()).collect();
    assert_eq!(worlds, vec![PROVIDER_WORLD.to_owned()]);
}

#[test]
fn the_world_reaches_neither_the_network_nor_the_file_system() {
    let resolve = resolve();
    for (_, world) in &resolve.worlds {
        for (key, _) in &world.imports {
            let name = resolve.name_world_key(key);
            assert!(
                !name.contains("wasi:sockets") && !name.contains("wasi:filesystem"),
                "world `{}` imports `{name}`; components have no network and no file system",
                world.name
            );
        }
    }
}

#[test]
fn the_world_imports_host_to_name_credentials_and_exports_no_way_to_read_one() {
    let resolve = resolve();
    let (_, world) = resolve
        .worlds
        .iter()
        .find(|(_, w)| w.name == PROVIDER_WORLD)
        .expect("provider world exists");
    let imports_host = world.imports.keys().any(|key| resolve.name_world_key(key).contains("host"));
    assert!(imports_host, "the provider world names credentials via `host`");
}

#[test]
fn the_chunk_entry_point_takes_raw_bytes_not_json() {
    // S0 ruling D2: eventstream dialects cross the boundary as bytes; a JSON
    // string chunk cannot carry them and base64 would tax every SSE chunk.
    let resolve = resolve();
    let (_, interface) = resolve
        .interfaces
        .iter()
        .find(|(_, i)| i.name.as_deref() == Some("provider-adapter"))
        .expect("provider-adapter interface exists");
    let function =
        interface.functions.get("parse-stream-chunk").expect("parse-stream-chunk exists");
    let chunk_type = &function.params.first().expect("chunk parameter exists").ty;
    let Type::Id(id) = chunk_type else {
        panic!("chunk parameter must be a named list type, got {chunk_type:?}");
    };
    let TypeDefKind::List(element) = &resolve.types[*id].kind else {
        panic!("chunk parameter must be a list");
    };
    assert_eq!(*element, Type::U8, "chunk element type must be u8");
}

fn reference_manifest() -> ComponentManifestV1 {
    ComponentManifestV1 {
        name: "provider-openai-compatible".to_owned(),
        version: "1.0.0".to_owned(),
        api_version: PROVIDER_WORLD.to_owned(),
        providers: vec!["openai-compatible".to_owned()],
        capabilities: BTreeSet::from([
            "chat".to_owned(),
            "stream".to_owned(),
            "tool_call".to_owned(),
            "json_schema".to_owned(),
        ]),
        auth_arms: BTreeSet::from(["bearer".to_owned(), "header_secret".to_owned()]),
        emits: Vec::new(),
        permissions: ComponentPermissionsV1 {
            network: false,
            filesystem: false,
            secrets: vec!["provider_api_key".to_owned()],
        },
        conformance: ConformanceSpecV1 {
            required_suite: COMPONENT_BEHAVIOR_SUITE.to_owned(),
            fixtures: "fixtures/".to_owned(),
        },
        compatibility: CompatibilityDeclarationV1 {
            ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
            kernel_version: "0.2.0".to_owned(),
            kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
            wit_package: WIT_PACKAGE.to_owned(),
            south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
        },
    }
}

/// The S1 signing act: the official reference component's manifest, exactly as
/// it will ship, validates clean and reports the identity gate ① will compare
/// against the loaded component's `metadata()`.
#[test]
fn the_reference_component_manifest_signs_the_contract() {
    let manifest = reference_manifest();
    assert_eq!(manifest.validate(), Ok(()));

    let metadata = manifest.metadata();
    assert_eq!(metadata.name, "provider-openai-compatible");
    assert_eq!(metadata.api_version, PROVIDER_WORLD);

    let tuple = manifest.compatibility_tuple();
    assert_eq!(tuple.ir_schema_id, "token-station-protocol@0.3.0/v0.2.0");
    assert_eq!(tuple.kernel_version, "0.2.0");
    assert_eq!(tuple.wit_package, WIT_PACKAGE);
    assert_eq!(tuple.wit_world, PROVIDER_WORLD);
    assert_eq!(tuple.conformance_suite, COMPONENT_BEHAVIOR_SUITE);
    assert_eq!(tuple.component_version, "1.0.0");
}

#[test]
fn the_reference_manifest_round_trips_through_json() {
    let manifest = reference_manifest();
    let encoded = serde_json::to_string_pretty(&manifest).expect("serializable manifest");
    let decoded: ComponentManifestV1 = serde_json::from_str(&encoded).expect("valid manifest");
    assert_eq!(decoded, manifest);
    assert_eq!(decoded.validate(), Ok(()));
}

#[test]
fn unknown_manifest_fields_are_a_version_mismatch_not_a_shrug() {
    let mut value = serde_json::to_value(reference_manifest()).expect("serializable manifest");
    value["telemetry"] = serde_json::json!(["latency"]);
    let parsed: Result<ComponentManifestV1, _> = serde_json::from_value(value);
    assert!(parsed.is_err(), "unknown fields must fail loudly");
}

#[test]
fn rejects_network_and_filesystem_requests_with_named_reasons() {
    let mut networked = reference_manifest();
    networked.permissions.network = true;
    assert_eq!(networked.validate(), Err(ManifestErrorV1::NetworkPermissionDenied));

    let mut on_disk = reference_manifest();
    on_disk.permissions.filesystem = true;
    assert_eq!(on_disk.validate(), Err(ManifestErrorV1::FilesystemPermissionDenied));
}

#[test]
fn rejects_a_credential_pasted_into_the_secrets_list() {
    let mut manifest = reference_manifest();
    manifest.permissions.secrets = vec!["sk-live-abc123".to_owned()];
    assert_eq!(
        manifest.validate(),
        Err(ManifestErrorV1::SecretIsNotAReferenceName("sk-live-abc123".to_owned()))
    );
}

#[test]
fn the_provider_world_is_the_only_known_world_and_resolves_by_name() {
    assert_eq!(KNOWN_WORLDS, &[PROVIDER_WORLD_SCHEMA]);
    assert_eq!(known_world(PROVIDER_WORLD), Some(&PROVIDER_WORLD_SCHEMA));
    assert_eq!(known_world("task-adapter-v1"), None);
}

#[test]
fn rejects_the_v1_world_and_the_v1_suite() {
    let mut old_world = reference_manifest();
    old_world.api_version = "provider-adapter-v1".to_owned();
    assert_eq!(
        old_world.validate(),
        Err(ManifestErrorV1::ApiVersionIsNotAKnownWorld("provider-adapter-v1".to_owned()))
    );

    let mut old_suite = reference_manifest();
    old_suite.conformance.required_suite = "provider-protocol-v1".to_owned();
    assert_eq!(
        old_suite.validate(),
        Err(ManifestErrorV1::ConformanceSuiteIsNotTheWorldSuite {
            declared: "provider-protocol-v1".to_owned(),
            world: PROVIDER_WORLD.to_owned(),
            expected: COMPONENT_BEHAVIOR_SUITE.to_owned(),
        })
    );
}

#[test]
fn a_word_outside_the_declared_worlds_vocabulary_is_refused_by_name() {
    let mut embeddings = reference_manifest();
    embeddings.capabilities.insert("embeddings".to_owned());
    assert_eq!(
        embeddings.validate(),
        Err(ManifestErrorV1::CapabilityIsNotInTheWorldVocabulary {
            capability: "embeddings".to_owned(),
            world: PROVIDER_WORLD.to_owned(),
        })
    );

    let mut mutual_tls = reference_manifest();
    mutual_tls.auth_arms.insert("mtls".to_owned());
    assert_eq!(
        mutual_tls.validate(),
        Err(ManifestErrorV1::AuthArmIsNotInTheWorldVocabulary {
            auth_arm: "mtls".to_owned(),
            world: PROVIDER_WORLD.to_owned(),
        })
    );
}

fn host_signed_manifest() -> ComponentManifestV1 {
    let mut manifest = reference_manifest();
    manifest.auth_arms = BTreeSet::from(["host_signed".to_owned()]);
    manifest.emits = vec![
        "authorization".to_owned(),
        "x-amz-date".to_owned(),
        "x-amz-content-sha256".to_owned(),
        "x-amz-security-token".to_owned(),
    ];
    manifest
}

/// The component half of `HostSigned` (2026-08-27 manifest-schema record,
/// D2): the arm plus its `emits` allow-list validates clean and round-trips.
#[test]
fn a_host_signed_manifest_declares_the_arm_and_its_emits() {
    let manifest = host_signed_manifest();
    assert_eq!(manifest.validate(), Ok(()));

    let encoded = serde_json::to_string_pretty(&manifest).expect("serializable manifest");
    let decoded: ComponentManifestV1 = serde_json::from_str(&encoded).expect("valid manifest");
    assert_eq!(decoded, manifest);
    assert_eq!(decoded.validate(), Ok(()));
}

/// D2–D3's coherence rules, each refused by name: an empty allow-list makes
/// the finalizer diff vacuous, a mixed arm set makes a signed request
/// indistinguishable from an unauthenticated one, and `emits` without the arm
/// is a declaration about a finalizer that will never run.
#[test]
fn host_signed_coherence_is_refused_shape_by_shape() {
    let mut empty_emits = host_signed_manifest();
    empty_emits.emits.clear();
    assert_eq!(empty_emits.validate(), Err(ManifestErrorV1::HostSignedNamesNoHeader));

    let mut mixed = host_signed_manifest();
    mixed.auth_arms.insert("bearer".to_owned());
    assert_eq!(mixed.validate(), Err(ManifestErrorV1::HostSignedAdmitsNoOtherArm));

    let mut orphaned = reference_manifest();
    orphaned.emits = vec!["authorization".to_owned()];
    assert_eq!(
        orphaned.validate(),
        Err(ManifestErrorV1::EmitsRequireTheHostSignedArm("authorization".to_owned()))
    );

    let mut unknown_header = host_signed_manifest();
    unknown_header.emits = vec!["x-goog-signature".to_owned()];
    assert_eq!(
        unknown_header.validate(),
        Err(ManifestErrorV1::EmitIsNotASignedHeader("x-goog-signature".to_owned()))
    );

    let mut duplicated = host_signed_manifest();
    duplicated.emits = vec!["authorization".to_owned(), "authorization".to_owned()];
    assert_eq!(
        duplicated.validate(),
        Err(ManifestErrorV1::HostSignedNamesAHeaderTwice("authorization".to_owned()))
    );
}

#[test]
fn requires_chat_and_a_provider_family() {
    let mut chatless = reference_manifest();
    chatless.capabilities.remove("chat");
    assert_eq!(chatless.validate(), Err(ManifestErrorV1::ChatCapabilityRequired));

    let mut familyless = reference_manifest();
    familyless.providers.clear();
    assert_eq!(familyless.validate(), Err(ManifestErrorV1::ProviderFamilyRequired));

    let mut bad_family = reference_manifest();
    bad_family.providers = vec!["OpenAI Compatible".to_owned()];
    assert_eq!(
        bad_family.validate(),
        Err(ManifestErrorV1::InvalidProviderFamily("OpenAI Compatible".to_owned()))
    );
}

#[test]
fn identity_is_checked_before_role_coherence() {
    let mut manifest = reference_manifest();
    manifest.name = String::new();
    manifest.providers.clear();
    assert_eq!(manifest.validate(), Err(ManifestErrorV1::MissingName));
}

#[test]
fn component_names_must_be_one_bounded_kebab_component() {
    let overlong = "a".repeat(65);
    for unsafe_name in [
        "..",
        "../outside",
        "nested/component",
        r"nested\component",
        "/absolute",
        "Uppercase",
        "-leading",
        "trailing-",
        "contains space",
        "line\nbreak",
        overlong.as_str(),
    ] {
        let mut manifest = reference_manifest();
        manifest.name = unsafe_name.to_owned();
        assert!(
            manifest.validate().is_err(),
            "component name must be one bounded lowercase component: {unsafe_name:?}"
        );
    }
}

#[test]
fn fixtures_paths_reject_escape_and_ambiguity() {
    for unsafe_path in [
        "..",
        "../fixtures",
        "fixtures/../outside",
        "/absolute",
        r"fixtures\windows",
        "fixtures//nested",
        "./fixtures",
        "",
    ] {
        let mut manifest = reference_manifest();
        manifest.conformance.fixtures = unsafe_path.to_owned();
        assert!(
            manifest.validate().is_err(),
            "fixtures must be a normalized relative path: {unsafe_path:?}"
        );
    }
}

#[test]
fn the_compatibility_declaration_is_shape_checked_field_by_field() {
    let mut wrong_package = reference_manifest();
    wrong_package.compatibility.wit_package = "token-station:adapter@1.0.0".to_owned();
    assert_eq!(
        wrong_package.validate(),
        Err(ManifestErrorV1::WitPackageIsNotTheWorldPackage {
            declared: "token-station:adapter@1.0.0".to_owned(),
            world: PROVIDER_WORLD.to_owned(),
            expected: WIT_PACKAGE.to_owned(),
        })
    );

    for bad_schema_id in [
        "",
        "token-station-protocol@0.3.0",
        "token-station-protocol@0.3.0/0.2.0",
        "token-station-protocol@0.3/v0.2.0",
        "protocol@0.3.0/v0.2.0",
    ] {
        let mut manifest = reference_manifest();
        manifest.compatibility.ir_schema_id = bad_schema_id.to_owned();
        assert_eq!(
            manifest.validate(),
            Err(ManifestErrorV1::InvalidIrSchemaId(bad_schema_id.to_owned())),
            "ir_schema_id {bad_schema_id:?} must be rejected"
        );
    }

    let mut bad_kernel = reference_manifest();
    bad_kernel.compatibility.kernel_version = "v0.2.0".to_owned();
    assert!(matches!(bad_kernel.validate(), Err(ManifestErrorV1::InvalidKernelVersion(_))));

    for bad_revision in ["", "72458e3", &"7".repeat(41), &"Z".repeat(40)] {
        let mut manifest = reference_manifest();
        manifest.compatibility.kernel_revision = (*bad_revision).to_owned();
        assert!(
            matches!(manifest.validate(), Err(ManifestErrorV1::InvalidKernelRevision(_))),
            "kernel_revision {bad_revision:?} must be rejected"
        );
    }

    let mut bad_runtime = reference_manifest();
    bad_runtime.compatibility.south_runtime = "0.8".to_owned();
    assert!(matches!(bad_runtime.validate(), Err(ManifestErrorV1::InvalidSouthRuntimeVersion(_))));
}

#[test]
fn an_unauthenticated_component_may_declare_no_arms_and_no_secrets() {
    let mut manifest = reference_manifest();
    manifest.auth_arms.clear();
    manifest.permissions.secrets.clear();
    assert_eq!(manifest.validate(), Ok(()));
}

// -- compatibility admission -------------------------------------------------
//
// These types moved here from `south-component-conformance` because they are
// production admission, not a test fixture: the loader now requires them, so a
// host cannot forget to wire the handshake. The four fields below are the ones
// only a live host knows — the manifest-side constants (`wit_package`, world
// name, suite name) are already exact-validated by `accepts_manifest`.

fn host_expectations() -> HostExpectationsV1 {
    HostExpectationsV1 {
        ir_schema_id: "token-station-protocol@0.3.0/v0.2.0".to_owned(),
        kernel_version: "0.2.0".to_owned(),
        kernel_revision: "72458e3a11fe157f9ac04818c44b62a3dd2cb09c".to_owned(),
        south_runtime: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

#[test]
fn a_matching_tuple_is_admitted() {
    let mut manifest = reference_manifest();
    manifest.compatibility.south_runtime = env!("CARGO_PKG_VERSION").to_owned();
    assert_eq!(compatibility_matches(&manifest, &host_expectations()), Ok(()));
}

/// Each of the four host-known fields, tampered one at a time. The handshake
/// refuses rather than degrading, and it names both sides so the operator can
/// see which half is stale.
#[test]
fn every_host_known_field_is_refused_when_it_disagrees() {
    let base = || {
        let mut manifest = reference_manifest();
        manifest.compatibility.south_runtime = env!("CARGO_PKG_VERSION").to_owned();
        manifest
    };

    let mut ir = base();
    ir.compatibility.ir_schema_id = "token-station-protocol@0.4.0/v0.2.0".to_owned();
    assert!(matches!(
        compatibility_matches(&ir, &host_expectations()),
        Err(CompatibilityMismatchV1::IrSchema { .. })
    ));

    let mut kernel = base();
    kernel.compatibility.kernel_version = "0.3.0".to_owned();
    assert!(matches!(
        compatibility_matches(&kernel, &host_expectations()),
        Err(CompatibilityMismatchV1::KernelVersion { .. })
    ));

    let mut revision = base();
    revision.compatibility.kernel_revision = "0".repeat(40);
    assert!(matches!(
        compatibility_matches(&revision, &host_expectations()),
        Err(CompatibilityMismatchV1::KernelRevision { .. })
    ));

    let mut runtime = base();
    runtime.compatibility.south_runtime = "0.15.0".to_owned();
    assert!(matches!(
        compatibility_matches(&runtime, &host_expectations()),
        Err(CompatibilityMismatchV1::SouthRuntime { .. })
    ));
}

/// A component built for the previous release is refused by name. This is the
/// case the tuple exists for, and until now nothing enforced it: the field was
/// declared in every manifest and compared nowhere outside these tests.
#[test]
fn a_component_from_the_previous_release_is_named_in_the_refusal() {
    // An explicit literal, not a derived value: this is the *previous* release,
    // and the whole point is that it no longer matches whatever this one is.
    let mut manifest = reference_manifest();
    manifest.compatibility.south_runtime = "0.15.0".to_owned();
    let Err(CompatibilityMismatchV1::SouthRuntime { declared, expected }) =
        compatibility_matches(&manifest, &host_expectations())
    else {
        panic!("a stale south_runtime must be refused");
    };
    assert_eq!(declared, "0.15.0");
    assert_eq!(expected, env!("CARGO_PKG_VERSION"));
}
