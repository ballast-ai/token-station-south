//! Self-test of `south.request-signing.v1` (gate ③ of B2, design record §5.3 and §5.4).
//!
//! The suite verifies signatures with its own `SigV4` arithmetic, so this file carries a reference
//! finalizer written independently of it — selected by declaration, reading the region and the
//! credential through the declaration — and shows that it passes every case. It then breaks that
//! finalizer one way at a time and shows that each break fails exactly the cases that guard it.

use std::{
    collections::BTreeMap,
    fmt::{Display, Write as _},
    path::Path,
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use south_contracts::SignedHeaderV1;
use south_provider_api::{ComponentManifestV1, SIGNED_HEADER_NAMES, SigningSchemeV1};
use south_provider_conformance::{
    REQUEST_SIGNING_CONFORMANCE_SUITE_ID, REQUEST_SIGNING_CONFORMANCE_SUITE_VERSION,
    RequestSigningCaseIdV1, RequestSigningConformanceFailureV1, RequestSigningExpectedV1,
    RequestSigningFixtureV1, RequestSigningHarnessV1, RequestSigningInputV1,
    RequestSigningRefusedV1, request_signing_fixtures_v1, run_request_signing_conformance_v1,
};
use static_assertions::assert_not_impl_any;

assert_not_impl_any!(RequestSigningFixtureV1: Display);
assert_not_impl_any!(RequestSigningInputV1<'static>: Display);

use RequestSigningCaseIdV1 as Case;

#[test]
fn suite_identity_and_canonical_case_order_are_frozen() {
    assert_eq!(REQUEST_SIGNING_CONFORMANCE_SUITE_VERSION, 1);
    assert_eq!(REQUEST_SIGNING_CONFORMANCE_SUITE_ID, "south.request-signing.v1");
    let case_ids: Vec<_> =
        request_signing_fixtures_v1().iter().map(RequestSigningFixtureV1::case_id).collect();
    assert_eq!(
        case_ids,
        [
            Case::PostSignsTheExactBodyBytes,
            Case::SessionTokenIsSentAndSigned,
            Case::AbsentSessionTokenEmitsThreeHeaders,
            Case::ServiceAndRegionComeFromTheDeclaration,
            Case::CredentialFieldsComeFromTheMapping,
            Case::PathSegmentsAreEncodedTwice,
            Case::MissingSecretAccessKeyIsRefused,
        ]
    );
}

/// Every declaration is one gate ① admits, inside the shipped Converse manifest with its endpoint
/// template rewritten to the case's host and region parameter, so a host may not refuse it.
#[test]
fn every_declaration_passes_gate_one_inside_a_shipped_manifest() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../components/provider-bedrock-converse/manifest.json");
    let shipped: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for fixture in request_signing_fixtures_v1() {
        let signing = fixture.signing();
        let param = &signing.region.template_param;
        let region = &fixture.config_values()[param];
        let host = fixture.request().url().strip_prefix("https://").unwrap().split('/').next();
        let template =
            format!("https://{}", host.unwrap().replace(region, &format!("{{{param}}}")));
        let mut manifest = shipped.clone();
        manifest["signing"] = serde_json::to_value(&signing).unwrap();
        manifest["emits"] =
            fixture.emits().iter().map(|header| header.header_name()).collect::<Vec<_>>().into();
        manifest["endpoint"] = json!({ "bedrock": template });
        manifest["config_schema"] = json!({ "bedrock": { param.as_str(): {
            "syntax": "aws_region", "required": true, "description": "The region to sign for."
        } } });
        // Gate ① requires every signing input to name a declared secret field, required when the
        // scheme requires the input (§5.4, SF13), so the declaration's own fields are declared.
        let fields: serde_json::Map<String, Value> = signing
            .credentials
            .iter()
            .map(|(input, field)| {
                (
                    field.clone(),
                    json!({ "secret": true, "required": signing.scheme.requires(input) }),
                )
            })
            .collect();
        manifest["credentials"] =
            json!({ "schema": "south.credential-recipe.v1", "fields": fields });
        let manifest: ComponentManifestV1 = serde_json::from_value(manifest).unwrap();
        manifest
            .validate()
            .unwrap_or_else(|error| panic!("{:?} fails gate 1: {error}", fixture.case_id()));
        assert_eq!(signing.scheme, SigningSchemeV1::AwsSigv4);
    }
}

/// The expected header sets are what a correct finalizer emits: the three `SigV4` always emits,
/// the session token exactly when the credential carries one, and nothing outside `emits`.
#[test]
fn expected_header_sets_follow_the_credential() {
    for fixture in request_signing_fixtures_v1() {
        let RequestSigningExpectedV1::Signed(expected) = fixture.expected() else {
            continue;
        };
        let token = fixture
            .signing()
            .credentials
            .get("session_token")
            .is_some_and(|field| fixture.credential_fields().contains_key(field));
        let mut want = vec![
            SignedHeaderV1::Authorization,
            SignedHeaderV1::XAmzDate,
            SignedHeaderV1::XAmzContentSha256,
        ];
        if token {
            want.push(SignedHeaderV1::XAmzSecurityToken);
        }
        assert_eq!(expected, want, "{:?}", fixture.case_id());
        assert!(expected.iter().all(|header| fixture.emits().contains(header)));
    }
    let names: Vec<_> = SignedHeaderV1::ALL.iter().map(|header| header.header_name()).collect();
    assert_eq!(names, SIGNED_HEADER_NAMES);
}

/// Debug output never carries a credential value, so a host may log a failing case.
#[test]
fn debug_output_carries_no_credential_material() {
    let rendered = format!("{:?}", request_signing_fixtures_v1());
    for secret in ["AKID", "EXAMPLEKEY", "south-test-only"] {
        assert!(!rendered.contains(secret), "{rendered}");
    }
}

// ---------------------------------------------------------------------------------------------
// The reference finalizer and its broken variants.
// ---------------------------------------------------------------------------------------------

/// One mistake made on purpose.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    None,
    /// Hashes the body after re-serializing it as JSON, not the bytes the transport sends.
    HashesReserializedBody,
    /// Signs the session token but does not send it.
    OmitsSecurityToken,
    /// Sends the session token but leaves it out of `SignedHeaders`.
    SecurityTokenNotSigned,
    /// Emits `x-amz-security-token`, empty, whenever the declaration admits a session token.
    EmitsEmptySecurityToken,
    /// Signs for `bedrock` whatever the declaration says.
    HardCodesBedrockService,
    /// Signs for `us-east-1` whatever the configuration says.
    HardCodesRegion,
    /// Reads `access_key_id` and `secret_access_key` as field names, ignoring the mapping.
    ReadsCredentialFieldsByInputName,
    /// Encodes the path once, as S3 does.
    EncodesPathOnce,
    /// Signs with an empty secret when the credential has none.
    SignsWithEmptySecret,
    /// Signs at a time of its own instead of the injected one.
    IgnoresInjectedClock,
    /// Leaves `host` out of `SignedHeaders`.
    HostNotSigned,
}

struct ReferenceFinalizer {
    fault: Fault,
}

impl RequestSigningHarnessV1 for ReferenceFinalizer {
    fn sign(
        &self,
        input: &RequestSigningInputV1<'_>,
    ) -> Result<Vec<(SignedHeaderV1, Vec<u8>)>, RequestSigningRefusedV1> {
        let fault = self.fault;
        let signing = input.signing();
        match signing.scheme {
            SigningSchemeV1::AwsSigv4 => {}
        }
        let fields = input.credential_fields();
        let field = |name: &str| {
            if fault == Fault::ReadsCredentialFieldsByInputName {
                fields.get(name)
            } else {
                signing.credentials.get(name).and_then(|field| fields.get(field))
            }
        };
        let access_key_id = field("access_key_id").ok_or(RequestSigningRefusedV1)?;
        let secret = match field("secret_access_key") {
            Some(secret) => secret.as_str(),
            None if fault == Fault::SignsWithEmptySecret => "",
            None => return Err(RequestSigningRefusedV1),
        };
        let token = field("session_token").filter(|token| !token.is_empty());
        let region = if fault == Fault::HardCodesRegion {
            "us-east-1"
        } else {
            input
                .config_values()
                .get(&signing.region.template_param)
                .ok_or(RequestSigningRefusedV1)?
        };
        let service =
            if fault == Fault::HardCodesBedrockService { "bedrock" } else { &signing.service };
        let now = if fault == Fault::IgnoresInjectedClock {
            input.now_unix_seconds() + 3600
        } else {
            input.now_unix_seconds()
        };
        let timestamp = timestamp(now);
        let date = &timestamp[..8];

        let request = input.request();
        let body = match serde_json::from_slice::<Value>(request.body()) {
            Ok(value) if fault == Fault::HashesReserializedBody => {
                serde_json::to_vec(&value).unwrap()
            }
            _ => request.body().to_vec(),
        };
        let payload_hash = hex(&Sha256::digest(&body));
        let (host, path) = request
            .url()
            .strip_prefix("https://")
            .unwrap()
            .split_at(request.url().strip_prefix("https://").unwrap().find('/').unwrap());
        let uri = if fault == Fault::EncodesPathOnce { path.to_owned() } else { encode(path) };

        let mut signed: BTreeMap<String, String> = request
            .headers()
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        if fault != Fault::HostNotSigned {
            signed.insert("host".to_owned(), host.to_owned());
        }
        signed.insert("x-amz-content-sha256".to_owned(), payload_hash.clone());
        signed.insert("x-amz-date".to_owned(), timestamp.clone());
        if let Some(token) = token
            && fault != Fault::SecurityTokenNotSigned
        {
            signed.insert("x-amz-security-token".to_owned(), token.clone());
        }
        let names = signed.keys().cloned().collect::<Vec<_>>().join(";");
        let canonical_headers = signed.iter().fold(String::new(), |mut text, (name, value)| {
            let _ = writeln!(text, "{name}:{value}");
            text
        });
        let canonical =
            format!("{}\n{uri}\n\n{canonical_headers}\n{names}\n{payload_hash}", request.method());
        let scope = format!("{date}/{region}/{service}/aws4_request");
        let signature = sign_string(secret, &timestamp, &scope, &canonical);

        let mut emitted = vec![
            (
                SignedHeaderV1::Authorization,
                format!(
                    "AWS4-HMAC-SHA256 Credential={access_key_id}/{scope}, \
                     SignedHeaders={names}, Signature={signature}"
                ),
            ),
            (SignedHeaderV1::XAmzDate, timestamp),
            (SignedHeaderV1::XAmzContentSha256, payload_hash),
        ];
        match token {
            Some(token) if fault != Fault::OmitsSecurityToken => {
                emitted.push((SignedHeaderV1::XAmzSecurityToken, token.clone()));
            }
            None if fault == Fault::EmitsEmptySecurityToken
                && signing.credentials.contains_key("session_token") =>
            {
                emitted.push((SignedHeaderV1::XAmzSecurityToken, String::new()));
            }
            _ => {}
        }
        Ok(emitted.into_iter().map(|(header, value)| (header, value.into_bytes())).collect())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// The signature over `canonical` for the credential `scope` (`<date>/<region>/<service>/…`).
fn sign_string(secret: &str, timestamp: &str, scope: &str, canonical: &str) -> String {
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let mut parts = scope.split('/');
    let date = parts.next().unwrap_or_default();
    let mut key = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    for part in parts {
        key = hmac(&key, part.as_bytes());
    }
    hex(&hmac(&key, string_to_sign.as_bytes()))
}

fn hmac(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut padded = if key.len() > 64 { Sha256::digest(key).to_vec() } else { key.to_vec() };
    padded.resize(64, 0);
    let inner: Vec<u8> = padded.iter().map(|byte| byte ^ 0x36).collect();
    let outer: Vec<u8> = padded.iter().map(|byte| byte ^ 0x5c).collect();
    let inner_hash = Sha256::new().chain_update(inner).chain_update(message).finalize();
    Sha256::new().chain_update(outer).chain_update(inner_hash).finalize().to_vec()
}

/// Encodes every byte outside the unreserved set and `/`: the second encoding `SigV4` applies to
/// a path that is already encoded on the wire.
fn encode(path: &str) -> String {
    path.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// `YYYYMMDDTHHMMSSZ` by walking the calendar, deliberately unlike the suite's arithmetic.
fn timestamp(unix_seconds: i64) -> String {
    let mut days = unix_seconds / 86_400;
    let seconds = unix_seconds % 86_400;
    let mut year = 1970;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let length = if leap { 366 } else { 365 };
        if days < length {
            let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            let mut month = 1;
            for length in months {
                if days < length {
                    break;
                }
                days -= length;
                month += 1;
            }
            return format!(
                "{year:04}{month:02}{:02}T{:02}{:02}{:02}Z",
                days + 1,
                seconds / 3600,
                seconds % 3600 / 60,
                seconds % 60
            );
        }
        days -= length;
        year += 1;
    }
}

fn run(fault: Fault) -> Result<Vec<Case>, RequestSigningConformanceFailureV1> {
    run_request_signing_conformance_v1(&ReferenceFinalizer { fault })
        .map(|report| report.passed_case_ids().to_vec())
}

#[test]
fn the_reference_finalizer_passes_every_case() {
    let passed = run(Fault::None).unwrap();
    assert_eq!(passed.len(), request_signing_fixtures_v1().len());
}

#[test]
fn each_broken_finalizer_fails_exactly_the_cases_that_guard_it() {
    let signed = [
        Case::PostSignsTheExactBodyBytes,
        Case::SessionTokenIsSentAndSigned,
        Case::AbsentSessionTokenEmitsThreeHeaders,
        Case::ServiceAndRegionComeFromTheDeclaration,
        Case::CredentialFieldsComeFromTheMapping,
        Case::PathSegmentsAreEncodedTwice,
    ];
    let expectations: &[(Fault, &[Case])] = &[
        (Fault::HashesReserializedBody, &[Case::PostSignsTheExactBodyBytes]),
        (Fault::OmitsSecurityToken, &[Case::SessionTokenIsSentAndSigned]),
        (Fault::SecurityTokenNotSigned, &[Case::SessionTokenIsSentAndSigned]),
        (Fault::EmitsEmptySecurityToken, &[Case::AbsentSessionTokenEmitsThreeHeaders]),
        (Fault::HardCodesBedrockService, &[Case::ServiceAndRegionComeFromTheDeclaration]),
        (Fault::HardCodesRegion, &[Case::ServiceAndRegionComeFromTheDeclaration]),
        (Fault::ReadsCredentialFieldsByInputName, &[Case::CredentialFieldsComeFromTheMapping]),
        (Fault::EncodesPathOnce, &[Case::PathSegmentsAreEncodedTwice]),
        (Fault::SignsWithEmptySecret, &[Case::MissingSecretAccessKeyIsRefused]),
        (Fault::IgnoresInjectedClock, &signed),
        (Fault::HostNotSigned, &signed),
    ];
    for (fault, expected) in expectations {
        let failure = run(*fault).expect_err(&format!("{fault:?} must fail the suite"));
        assert_eq!(failure.failed_case_ids(), *expected, "{fault:?}: {failure:?}");
        assert_eq!(failure.evaluated_case_count(), request_signing_fixtures_v1().len());
    }
}
