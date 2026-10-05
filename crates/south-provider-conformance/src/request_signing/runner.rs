//! The host harness and the runner of the request-signing suite.

use std::{collections::BTreeMap, fmt};

use south_contracts::SignedHeaderV1;
use south_provider_api::SigningV1;

use super::{
    REQUEST_SIGNING_CONFORMANCE_SUITE_ID, REQUEST_SIGNING_CONFORMANCE_SUITE_VERSION,
    RequestSigningCaseIdV1, RequestSigningExpectedV1, RequestSigningFixtureV1,
    RequestSigningRequestV1, request_signing_fixtures_v1,
    sigv4::{
        ScopeV1, amz_date, canonical_header_value, canonical_request, canonical_uri, sha256_hex,
        signature, split_url,
    },
};

/// The host refused to sign: a required credential input is absent, or the declaration is one it
/// does not admit. Nothing was emitted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RequestSigningRefusedV1;

/// What the host's finalizer is given for one case.
pub struct RequestSigningInputV1<'a> {
    signing: &'a SigningV1,
    emits: &'a [SignedHeaderV1],
    config_values: &'a BTreeMap<String, String>,
    credential_fields: &'a BTreeMap<String, String>,
    now_unix_seconds: i64,
    request: &'a RequestSigningRequestV1,
}

impl<'a> RequestSigningInputV1<'a> {
    /// The package's `signing` declaration. The host selects its finalizer by `scheme`.
    #[must_use]
    pub const fn signing(&self) -> &'a SigningV1 {
        self.signing
    }

    /// The package's `emits` declaration: every header the finalizer may emit.
    #[must_use]
    pub const fn emits(&self) -> &'a [SignedHeaderV1] {
        self.emits
    }

    /// The family's configuration values; the region is the one under
    /// `signing.region.template_param`.
    #[must_use]
    pub const fn config_values(&self) -> &'a BTreeMap<String, String> {
        self.config_values
    }

    /// The credential's field values, keyed by **field** name; `signing.credentials` maps each
    /// scheme input to its field.
    #[must_use]
    pub const fn credential_fields(&self) -> &'a BTreeMap<String, String> {
        self.credential_fields
    }

    /// The signing time in Unix seconds, which the host's clock is injected to return.
    #[must_use]
    pub const fn now_unix_seconds(&self) -> i64 {
        self.now_unix_seconds
    }

    /// The finished request.
    #[must_use]
    pub const fn request(&self) -> &'a RequestSigningRequestV1 {
        self.request
    }
}

impl fmt::Debug for RequestSigningInputV1<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestSigningInputV1")
            .field("scheme", &self.signing.scheme)
            .field("emits", &self.emits)
            .field("credential_field_names", &self.credential_fields.keys().collect::<Vec<_>>())
            .field("now_unix_seconds", &self.now_unix_seconds)
            .field("request", &self.request)
            .finish_non_exhaustive()
    }
}

/// What a host implements to run the suite: its generic signing finalizer, selected by the
/// declaration's scheme exactly as in production, with its clock injected.
pub trait RequestSigningHarnessV1 {
    /// Signs `input`'s request and returns every header the finalizer emitted, in the order it
    /// emitted them, or the refusal.
    fn sign(
        &self,
        input: &RequestSigningInputV1<'_>,
    ) -> Result<Vec<(SignedHeaderV1, Vec<u8>)>, RequestSigningRefusedV1>;
}

/// The closed reasons why a case can fail.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestSigningMismatchCategoryV1 {
    /// The host refused a request it should have signed.
    Refused,
    /// The host signed a request it should have refused.
    NotRefused,
    /// The emitted names differ from the expected set: one is missing, extra, outside `emits` or
    /// emitted twice.
    EmittedHeaders,
    /// `x-amz-date` is not the injected time.
    Date,
    /// `x-amz-content-sha256` is not the hex SHA-256 of the body.
    ContentSha256,
    /// `x-amz-security-token` is not the credential's session token.
    SecurityToken,
    /// `authorization` is not an `AWS4-HMAC-SHA256` value with a credential, a signed-header list
    /// and a 64-digit lowercase hex signature.
    AuthorizationMalformed,
    /// The credential is not `<access key id>/<yyyymmdd>/<region>/<service>/aws4_request` for the
    /// declared credential, time, region and service.
    CredentialScope,
    /// `SignedHeaders` is unsorted, repeats a name, leaves out `host`, `x-amz-date`,
    /// `x-amz-content-sha256` or an emitted `x-amz-security-token`, or names a header the request
    /// does not carry.
    SignedHeaders,
    /// The signature differs from the one recomputed from the request.
    Signature,
}

fixed_debug!(RequestSigningMismatchCategoryV1 {
    Refused => "Refused",
    NotRefused => "NotRefused",
    EmittedHeaders => "EmittedHeaders",
    Date => "Date",
    ContentSha256 => "ContentSha256",
    SecurityToken => "SecurityToken",
    AuthorizationMalformed => "AuthorizationMalformed",
    CredentialScope => "CredentialScope",
    SignedHeaders => "SignedHeaders",
    Signature => "Signature",
});

/// One mismatch, without expected or observed values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RequestSigningMismatchV1 {
    case_id: RequestSigningCaseIdV1,
    category: RequestSigningMismatchCategoryV1,
}

impl RequestSigningMismatchV1 {
    /// The case that mismatched.
    #[must_use]
    pub const fn case_id(&self) -> RequestSigningCaseIdV1 {
        self.case_id
    }

    /// The closed mismatch category.
    #[must_use]
    pub const fn category(&self) -> RequestSigningMismatchCategoryV1 {
        self.category
    }
}

impl fmt::Debug for RequestSigningMismatchV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestSigningMismatchV1")
            .field("case_id", &self.case_id)
            .field("category", &self.category)
            .finish()
    }
}

/// A successful report for the complete suite.
pub struct RequestSigningConformanceReportV1 {
    passed_case_ids: Vec<RequestSigningCaseIdV1>,
}

impl RequestSigningConformanceReportV1 {
    /// The stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        REQUEST_SIGNING_CONFORMANCE_SUITE_ID
    }

    /// The suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        REQUEST_SIGNING_CONFORMANCE_SUITE_VERSION
    }

    /// Every passed case, in table order.
    #[must_use]
    pub fn passed_case_ids(&self) -> &[RequestSigningCaseIdV1] {
        &self.passed_case_ids
    }
}

impl fmt::Debug for RequestSigningConformanceReportV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestSigningConformanceReportV1")
            .field("suite_id", &REQUEST_SIGNING_CONFORMANCE_SUITE_ID)
            .field("suite_version", &REQUEST_SIGNING_CONFORMANCE_SUITE_VERSION)
            .field("passed_case_ids", &self.passed_case_ids)
            .finish()
    }
}

/// Every mismatch of an evaluated suite.
pub struct RequestSigningConformanceFailureV1 {
    evaluated_case_count: usize,
    mismatches: Vec<RequestSigningMismatchV1>,
}

impl RequestSigningConformanceFailureV1 {
    /// The stable suite identifier.
    #[must_use]
    pub const fn suite_id(&self) -> &'static str {
        REQUEST_SIGNING_CONFORMANCE_SUITE_ID
    }

    /// The suite version.
    #[must_use]
    pub const fn suite_version(&self) -> u32 {
        REQUEST_SIGNING_CONFORMANCE_SUITE_VERSION
    }

    /// How many cases were evaluated.
    #[must_use]
    pub const fn evaluated_case_count(&self) -> usize {
        self.evaluated_case_count
    }

    /// Every mismatch in evaluation order.
    #[must_use]
    pub fn mismatches(&self) -> &[RequestSigningMismatchV1] {
        &self.mismatches
    }

    /// The cases with at least one mismatch, in table order, without repeats.
    #[must_use]
    pub fn failed_case_ids(&self) -> Vec<RequestSigningCaseIdV1> {
        let mut failed: Vec<_> = self.mismatches.iter().map(|mismatch| mismatch.case_id).collect();
        failed.dedup();
        failed
    }
}

impl fmt::Debug for RequestSigningConformanceFailureV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestSigningConformanceFailureV1")
            .field("suite_id", &REQUEST_SIGNING_CONFORMANCE_SUITE_ID)
            .field("suite_version", &REQUEST_SIGNING_CONFORMANCE_SUITE_VERSION)
            .field("evaluated_case_count", &self.evaluated_case_count)
            .field("mismatches", &self.mismatches)
            .finish()
    }
}

/// Runs every case in table order without failing fast.
pub fn run_request_signing_conformance_v1(
    harness: &dyn RequestSigningHarnessV1,
) -> Result<RequestSigningConformanceReportV1, RequestSigningConformanceFailureV1> {
    let fixtures = request_signing_fixtures_v1();
    let mut passed_case_ids = Vec::with_capacity(fixtures.len());
    let mut mismatches = Vec::new();
    for fixture in fixtures {
        let before = mismatches.len();
        run_case(harness, fixture, &mut mismatches);
        if mismatches.len() == before {
            passed_case_ids.push(fixture.case_id());
        }
    }
    if mismatches.is_empty() {
        Ok(RequestSigningConformanceReportV1 { passed_case_ids })
    } else {
        Err(RequestSigningConformanceFailureV1 { evaluated_case_count: fixtures.len(), mismatches })
    }
}

struct Recorder<'a> {
    case_id: RequestSigningCaseIdV1,
    mismatches: &'a mut Vec<RequestSigningMismatchV1>,
}

impl Recorder<'_> {
    fn record_if(&mut self, condition: bool, category: RequestSigningMismatchCategoryV1) {
        if condition {
            self.mismatches.push(RequestSigningMismatchV1 { case_id: self.case_id, category });
        }
    }
}

fn run_case(
    harness: &dyn RequestSigningHarnessV1,
    fixture: &RequestSigningFixtureV1,
    mismatches: &mut Vec<RequestSigningMismatchV1>,
) {
    let mut recorder = Recorder { case_id: fixture.case_id(), mismatches };
    let signing = fixture.signing();
    let config_values = fixture.config_values();
    let credential_fields = fixture.credential_fields();
    let input = RequestSigningInputV1 {
        signing: &signing,
        emits: fixture.emits(),
        config_values: &config_values,
        credential_fields: &credential_fields,
        now_unix_seconds: fixture.now_unix_seconds(),
        request: fixture.request(),
    };
    let result = harness.sign(&input);
    match (fixture.expected(), result) {
        (RequestSigningExpectedV1::Refused, Ok(_)) => {
            recorder.record_if(true, RequestSigningMismatchCategoryV1::NotRefused);
        }
        (RequestSigningExpectedV1::Refused, Err(RequestSigningRefusedV1)) => {}
        (RequestSigningExpectedV1::Signed(_), Err(RequestSigningRefusedV1)) => {
            recorder.record_if(true, RequestSigningMismatchCategoryV1::Refused);
        }
        (RequestSigningExpectedV1::Signed(expected), Ok(emitted)) => {
            let input_value = |input: &str| {
                signing.credentials.get(input).and_then(|field| credential_fields.get(field))
            };
            let expected_values = Expected {
                access_key_id: input_value("access_key_id").map_or("", String::as_str),
                secret_access_key: input_value("secret_access_key").map_or("", String::as_str),
                session_token: input_value("session_token").map(String::as_str),
                region: config_values
                    .get(&signing.region.template_param)
                    .map_or("", String::as_str),
                service: &signing.service,
                amz_date: amz_date(fixture.now_unix_seconds()).unwrap_or_default(),
            };
            verify(fixture.request(), expected, &emitted, &expected_values, &mut recorder);
        }
    }
}

/// What the fixture says the signature must be bound to.
struct Expected<'a> {
    access_key_id: &'a str,
    secret_access_key: &'a str,
    session_token: Option<&'a str>,
    region: &'a str,
    service: &'a str,
    amz_date: String,
}

fn verify(
    request: &RequestSigningRequestV1,
    expected_names: &[SignedHeaderV1],
    emitted: &[(SignedHeaderV1, Vec<u8>)],
    expected: &Expected<'_>,
    recorder: &mut Recorder<'_>,
) {
    use RequestSigningMismatchCategoryV1 as Category;

    let mut values: BTreeMap<SignedHeaderV1, &str> = BTreeMap::new();
    let mut well_formed = emitted.len() == expected_names.len();
    for (header, value) in emitted {
        let text = std::str::from_utf8(value).unwrap_or_default();
        well_formed &= values.insert(*header, text).is_none() && expected_names.contains(header);
    }
    recorder.record_if(!well_formed, Category::EmittedHeaders);

    let payload_hash = sha256_hex(request.body());
    let value = |header| values.get(&header).copied();
    recorder.record_if(
        value(SignedHeaderV1::XAmzDate) != Some(expected.amz_date.as_str()),
        Category::Date,
    );
    recorder.record_if(
        value(SignedHeaderV1::XAmzContentSha256) != Some(payload_hash.as_str()),
        Category::ContentSha256,
    );
    if expected_names.contains(&SignedHeaderV1::XAmzSecurityToken) {
        recorder.record_if(
            value(SignedHeaderV1::XAmzSecurityToken) != expected.session_token,
            Category::SecurityToken,
        );
    }

    let Some(authorization) = value(SignedHeaderV1::Authorization).and_then(parse_authorization)
    else {
        recorder.record_if(true, Category::AuthorizationMalformed);
        return;
    };
    let date = expected.amz_date.get(..8).unwrap_or_default();
    let scope = format!(
        "{}/{date}/{}/{}/aws4_request",
        expected.access_key_id, expected.region, expected.service
    );
    recorder.record_if(authorization.credential != scope, Category::CredentialScope);

    let Some(canonical_headers) = canonical_headers(request, &values, &authorization) else {
        recorder.record_if(true, Category::SignedHeaders);
        return;
    };
    let Some((_, raw_path)) = split_url(request.url()) else {
        recorder.record_if(true, Category::SignedHeaders);
        return;
    };
    let canonical = canonical_request(
        request.method(),
        &canonical_uri(raw_path),
        &canonical_headers,
        &payload_hash,
    );
    let scope = ScopeV1 {
        amz_date: &expected.amz_date,
        region: expected.region,
        service: expected.service,
    };
    recorder.record_if(
        signature(expected.secret_access_key, &scope, &canonical) != authorization.signature,
        Category::Signature,
    );
}

struct AuthorizationV1<'a> {
    credential: &'a str,
    signed_headers: Vec<&'a str>,
    signature: &'a str,
}

/// Parses `AWS4-HMAC-SHA256 Credential=…, SignedHeaders=…, Signature=…`.
fn parse_authorization(value: &str) -> Option<AuthorizationV1<'_>> {
    let rest = value.strip_prefix("AWS4-HMAC-SHA256 ")?;
    let mut parts = rest.split(',').map(str::trim);
    let credential = parts.next()?.strip_prefix("Credential=")?;
    let signed_headers = parts.next()?.strip_prefix("SignedHeaders=")?.split(';').collect();
    let signature = parts.next()?.strip_prefix("Signature=")?;
    let hex_signature = signature.len() == 64
        && signature.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    (parts.next().is_none() && hex_signature).then_some(AuthorizationV1 {
        credential,
        signed_headers,
        signature,
    })
}

/// The signed headers with their canonical values, in `SignedHeaders` order; `None` when the list
/// is unsorted, repeats a name, misses a required name, or names a header the request does not
/// carry.
fn canonical_headers<'a>(
    request: &RequestSigningRequestV1,
    emitted: &BTreeMap<SignedHeaderV1, &str>,
    authorization: &AuthorizationV1<'a>,
) -> Option<Vec<(&'a str, String)>> {
    let names = &authorization.signed_headers;
    let sorted = names.windows(2).all(|pair| pair.first() < pair.last());
    let mut required = vec!["host", "x-amz-date", "x-amz-content-sha256"];
    if emitted.contains_key(&SignedHeaderV1::XAmzSecurityToken) {
        required.push("x-amz-security-token");
    }
    if !sorted || required.iter().any(|name| !names.contains(name)) {
        return None;
    }
    let (host, _) = split_url(request.url())?;
    names
        .iter()
        .map(|name| {
            let value = if *name == "host" {
                host.to_owned()
            } else if let Some(header) =
                SignedHeaderV1::ALL.into_iter().find(|header| header.header_name() == *name)
            {
                // `authorization` cannot sign itself.
                if header == SignedHeaderV1::Authorization {
                    return None;
                }
                canonical_header_value(emitted.get(&header)?)
            } else {
                let values: Vec<String> = request
                    .headers()
                    .iter()
                    .filter(|(header, _)| header.eq_ignore_ascii_case(name))
                    .map(|(_, value)| canonical_header_value(value))
                    .collect();
                if values.is_empty() {
                    return None;
                }
                values.join(",")
            };
            Some((*name, value))
        })
        .collect()
}
