//! Gate ② for the embeddings world: `south.embeddings-component.v1` (record §10). Contract 2
//! (§17) adds the `request.media` row for a package declaring `media`; there is no
//! `ReferenceIntegrity`, since media is carried inline.
//!
//! Request and error cases feed one component function each. A response case names the request
//! case it answers; the suite builds that request, extracts and erases the vectors of the
//! fixture's upstream body with the prepared locator (`extract_vectors_v1`), hands the skeleton
//! and the parse context to the component, and runs the host's consistency checks
//! (`check_embeddings_response_v1`) — the path a host takes, so a fixture pins what a host would
//! decide, not only what the component said.
//!
//! The suite always runs with the package's manifest: its capabilities decide whether a requested
//! `dimensions` binds the vector length and what the batch and dimensions rows must show, its
//! `auth_arms` are what `DescriptorAuthWithinManifest` judges against, and its declared keys are
//! what `UndeclaredValuesIgnored` stays clear of.

use serde::Deserialize;
use serde_json::{Value, json};
use south_contracts::{
    EmbeddingsContractErrorV1, EmbeddingsFailureOutcomeV1, EmbeddingsParsedV1, EmbeddingsRequestV1,
    InputShapeV1, UsageSourceV1, check_embeddings_response_v1, extract_vectors_v1,
};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, HttpResponseParts, ProviderConfig};

use crate::embeddings_fixture::{EmbeddingsCaseV1, EmbeddingsFamilyV1, EmbeddingsFixturePackV1};
use crate::embeddings_json as wire;
use crate::{
    CheckV1, ComponentResultV1, EmbeddingsComponentV1, OutcomeV1, PreparedEmbeddingsV1, ReportV1,
    admit_descriptor_auth, credential_recipe_checks_v1,
};

/// The suite identifier, equal to the manifest's `conformance.required_suite`.
pub const EMBEDDINGS_COMPONENT_SUITE_V1: &str = south_provider_api::EMBEDDINGS_BEHAVIOR_SUITE;

/// The rows every embeddings package ships by name (record §10). A missing row is a
/// [`CheckV1::Coverage`] failure. A package declaring `media` also ships
/// [`EMBEDDINGS_MEDIA_ROW_V2`].
pub const EMBEDDINGS_REQUIRED_ROWS_V1: [&str; 10] = [
    "embeddings.request.single-text",
    "embeddings.request.batch-text",
    "embeddings.request.dimensions",
    "embeddings.request.refused-capability",
    "embeddings.request.extra-fields",
    "embeddings.response.usage",
    "embeddings.response.missing-usage",
    "embeddings.response.count-mismatch",
    "embeddings.error.rejected-credential",
    "embeddings.error.server",
];

/// The row a package declaring the `media` capability ships by name (record §10, §17.6).
///
/// A request holding a media input that the component builds, the input's `data` appearing in the
/// body unchanged. A package that does not declare `media` is not asked for it.
pub const EMBEDDINGS_MEDIA_ROW_V2: &str = "embeddings.request.media";

/// Every row `manifest`'s package ships by name.
fn required_rows(manifest: &ComponentManifestV1) -> Vec<&'static str> {
    let mut rows = EMBEDDINGS_REQUIRED_ROWS_V1.to_vec();
    if manifest.capabilities.contains(south_provider_api::EMBEDDINGS_MEDIA_CAPABILITY) {
        rows.push(EMBEDDINGS_MEDIA_ROW_V2);
    }
    rows
}

/// The key injected to prove a component tolerates a newer peer's field.
const UNKNOWN_FIELD: &str = "__conformance_unknown_field";

/// The prefix of the key injected to prove a component ignores a `declared` key its package does
/// not declare; a numeric suffix keeps it clear of any key the package declares.
const UNDECLARED_VALUE: &str = "conformance_undeclared_";

/// What one case produced as JSON; `Err` is a fixture or boundary failure, never the component's
/// own refusal, which is a value (`{"error": ...}`).
type Invoked = Result<Value, String>;

fn parse<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("fixture input has the wrong shape: {error}"))
}

#[derive(Deserialize)]
struct RequestInput {
    provider_config: ProviderConfig,
    request: EmbeddingsRequestV1,
}

#[derive(Deserialize)]
struct ResponseInput {
    request_case: String,
    response: HttpResponseParts,
}

/// How a response case ended.
enum Answered {
    Parsed(EmbeddingsParsedV1),
    /// The component refused the skeleton.
    Refused(ErrorEnvelope),
    /// The host's extraction or consistency checks refused.
    HostRefusal(EmbeddingsContractErrorV1),
}

/// A response case's request: the parsed request and what the component prepared for it.
struct Paired {
    request: EmbeddingsRequestV1,
    prepared: PreparedEmbeddingsV1,
}

/// The suite's view of one run: the component, its pack and its manifest.
struct Suite<'a> {
    component: &'a dyn EmbeddingsComponentV1,
    pack: &'a EmbeddingsFixturePackV1,
    manifest: &'a ComponentManifestV1,
}

/// The stable word a fixture uses for a host refusal.
const fn refusal_word(error: EmbeddingsContractErrorV1) -> &'static str {
    match error {
        EmbeddingsContractErrorV1::InvalidPointer => "invalid_pointer",
        EmbeddingsContractErrorV1::InvalidUsage => "invalid_usage",
        EmbeddingsContractErrorV1::BodyTooLarge => "body_too_large",
        EmbeddingsContractErrorV1::InvalidJson => "invalid_json",
        EmbeddingsContractErrorV1::VectorNotFound => "vector_not_found",
        EmbeddingsContractErrorV1::InvalidVector => "invalid_vector",
        EmbeddingsContractErrorV1::VectorLengthMismatch => "vector_length_mismatch",
        EmbeddingsContractErrorV1::InvalidIndex => "invalid_index",
        EmbeddingsContractErrorV1::VectorCountMismatch => "vector_count_mismatch",
        EmbeddingsContractErrorV1::DimensionsMismatch => "dimensions_mismatch",
        EmbeddingsContractErrorV1::NorthUsageMismatch => "north_usage_mismatch",
    }
}

fn refused(error: &ErrorEnvelope) -> Value {
    json!({ "error": error })
}

/// The component's refusal, when `output` is exactly `{"error": <envelope>}`.
fn refusal_of(output: &Value) -> Option<&Value> {
    let map = output.as_object()?;
    if map.len() == 1 { map.get("error") } else { None }
}

impl Suite<'_> {
    fn build(&self, input: &RequestInput) -> ComponentResultV1<PreparedEmbeddingsV1> {
        self.component.build_embeddings_request(&input.provider_config, &input.request)
    }

    fn invoke(&self, case: &EmbeddingsCaseV1, input: &Value) -> Invoked {
        match case.family {
            EmbeddingsFamilyV1::Request => match self.build(&parse(input)?) {
                Ok(prepared) => wire::prepared_embeddings_json(&prepared),
                Err(error) => Ok(refused(&error)),
            },
            EmbeddingsFamilyV1::Response => {
                let input: ResponseInput = parse(input)?;
                let paired = self.pair(&input.request_case)?;
                Ok(match self.answer(&paired, &input.response, true) {
                    Answered::Parsed(parsed) => wire::embeddings_parsed_json(&parsed)?,
                    Answered::Refused(error) => refused(&error),
                    Answered::HostRefusal(error) => json!({ "host_refusal": refusal_word(error) }),
                })
            }
            EmbeddingsFamilyV1::Error => match self.component.map_provider_error(&parse(input)?) {
                Ok((outcome, error)) => wire::provider_error_json(outcome, &error),
                Err(error) => Ok(refused(&error)),
            },
        }
    }

    /// Builds the request case a response case names; the prepared value must cross the
    /// boundary, as it would from a guest.
    fn pair(&self, request_case: &str) -> Result<Paired, String> {
        let case = self
            .pack
            .case(request_case)
            .filter(|case| case.family == EmbeddingsFamilyV1::Request)
            .ok_or_else(|| format!("`{request_case}` names no request case in the pack"))?;
        let input: RequestInput = parse(&case.input)?;
        let prepared = self.build(&input).map_err(|error| {
            format!("the paired request `{request_case}` did not build: {}", error.message)
        })?;
        wire::prepared_embeddings_json(&prepared)?;
        Ok(Paired { request: input.request, prepared })
    }

    /// The host's path for one 2xx. `host_checks` off stops after the component's parse, so a
    /// check can judge the component's own answer.
    fn answer(&self, paired: &Paired, response: &HttpResponseParts, host_checks: bool) -> Answered {
        let extracted = match extract_vectors_v1(response.body.as_bytes(), &paired.prepared.vectors)
        {
            Ok(extracted) => extracted,
            Err(error) => return Answered::HostRefusal(error),
        };
        let mut skeleton = response.clone();
        skeleton.body = extracted.skeleton().to_string();
        let parsed = match self
            .component
            .parse_embeddings_response(&skeleton, &paired.prepared.parse_context)
        {
            Ok(parsed) => parsed,
            Err(error) => return Answered::Refused(error),
        };
        if host_checks {
            let dimensions_capability = self.manifest.capabilities.contains("dimensions");
            if let Err(error) = check_embeddings_response_v1(
                &paired.request,
                dimensions_capability,
                &extracted,
                &parsed,
            ) {
                return Answered::HostRefusal(error);
            }
        }
        Answered::Parsed(parsed)
    }
}

/// Runs `south.embeddings-component.v1` against an embeddings component with its manifest.
///
/// Never panics on a bad component or a bad fixture: both become failures in the report. When
/// the manifest declares `credentials`, the pack's `credential.*` cases run through
/// `CredentialRecipeMatch` and the credential coverage rule. This is the entry point an admitting
/// host and a release's gate ② report use.
#[must_use]
pub fn run_embeddings_component_suite_v1(
    component: &dyn EmbeddingsComponentV1,
    pack: &EmbeddingsFixturePackV1,
    manifest: &ComponentManifestV1,
) -> ReportV1 {
    let suite = Suite { component, pack, manifest };
    let required = required_rows(manifest);
    let mut outcomes = coverage(pack, &required);
    let mut a_credential_was_rejected_somewhere = false;

    for case in pack.cases() {
        let first = suite.invoke(case, &case.input);
        outcomes.push(fixture_match(case, &first));
        outcomes.push(if first == suite.invoke(case, &case.input) {
            OutcomeV1::passed(CheckV1::Determinism, &case.name)
        } else {
            OutcomeV1::failed(
                CheckV1::Determinism,
                &case.name,
                "the same input produced different output twice (the fallback estimate \
                 included); a suite that admitted this component did not observe the one the \
                 host will run",
            )
        });
        outcomes.push(unknown_field_tolerance(&suite, case, &first));

        match case.family {
            EmbeddingsFamilyV1::Request => outcomes.extend(request_checks(&suite, case, &first)),
            EmbeddingsFamilyV1::Response => {
                outcomes.extend(response_checks(&suite, case));
            }
            EmbeddingsFamilyV1::Error => {
                if let Some(outcome) = auth_errors_are_not_retriable(case, &first) {
                    a_credential_was_rejected_somewhere = true;
                    outcomes.push(outcome);
                }
            }
        }
        if required.contains(&case.name.as_str()) {
            outcomes.push(named_row(&suite, case, &first));
        }
    }

    if !a_credential_was_rejected_somewhere {
        outcomes.push(OutcomeV1::failed(
            CheckV1::AuthErrorsAreNotRetriable,
            "embeddings.error",
            "no fixture maps a 401 or 403, so the check that keeps a rejected credential from \
             being replayed across every configured upstream never ran",
        ));
    }
    if let Some(credentials) = &manifest.credentials {
        outcomes.extend(credential_recipe_checks_v1(credentials, pack.credentials()));
    }
    ReportV1::new(EMBEDDINGS_COMPONENT_SUITE_V1, outcomes)
}

fn coverage(pack: &EmbeddingsFixturePackV1, required: &[&'static str]) -> Vec<OutcomeV1> {
    let missing: Vec<OutcomeV1> = required
        .iter()
        .copied()
        .filter(|row| pack.case(row).is_none())
        .map(|row| {
            OutcomeV1::failed(
                CheckV1::Coverage,
                row,
                "every embeddings package ships this row by name (record §10); without it the \
                 property it pins was never shown",
            )
        })
        .collect();
    if missing.is_empty() {
        vec![OutcomeV1::passed(CheckV1::Coverage, EMBEDDINGS_COMPONENT_SUITE_V1)]
    } else {
        missing
    }
}

fn fixture_match(case: &EmbeddingsCaseV1, actual: &Invoked) -> OutcomeV1 {
    match actual {
        Ok(actual) if *actual == case.expected => {
            OutcomeV1::passed(CheckV1::FixtureMatch, &case.name)
        }
        Ok(actual) => OutcomeV1::failed(
            CheckV1::FixtureMatch,
            &case.name,
            format!("expected {}, produced {}", truncate(&case.expected), truncate(actual)),
        ),
        Err(detail) => OutcomeV1::failed(CheckV1::FixtureMatch, &case.name, detail.clone()),
    }
}

/// Injects a field this ABI version does not model into the object a newer host would extend —
/// the provider configuration or the response parts; the contract request type is strict by
/// design — and requires the very same output.
fn unknown_field_tolerance(
    suite: &Suite<'_>,
    case: &EmbeddingsCaseV1,
    first: &Invoked,
) -> OutcomeV1 {
    let check = CheckV1::UnknownFieldTolerance;
    let pointer = match case.family {
        EmbeddingsFamilyV1::Request => "/provider_config",
        EmbeddingsFamilyV1::Response => "/response",
        EmbeddingsFamilyV1::Error => "",
    };
    let mut mutated = case.input.clone();
    let Some(Value::Object(target)) = mutated.pointer_mut(pointer) else {
        return OutcomeV1::failed(
            check,
            &case.name,
            format!("fixture has no object at `{pointer}` to carry an unknown field"),
        );
    };
    target.insert(UNKNOWN_FIELD.to_owned(), Value::Bool(true));
    if suite.invoke(case, &mutated) == *first {
        OutcomeV1::passed(check, &case.name)
    } else {
        OutcomeV1::failed(
            check,
            &case.name,
            "a field this version does not model changed the component's answer; a component \
             must degrade in front of a newer peer, not fail or change course",
        )
    }
}

/// `UndeclaredValuesIgnored` on every request case, and `EndpointConfinement` and
/// `DescriptorAuthWithinManifest` on what the component built. A case the component refuses built
/// nothing to judge for the last two.
fn request_checks(suite: &Suite<'_>, case: &EmbeddingsCaseV1, first: &Invoked) -> Vec<OutcomeV1> {
    let mut outcomes = vec![undeclared_values_ignored(suite, case, first)];
    let Ok(input) = parse::<RequestInput>(&case.input) else {
        return outcomes;
    };
    if input.request.carries_media() {
        outcomes.push(media_follows_declaration(suite, case, first));
    }
    let Ok(prepared) = suite.build(&input) else {
        return outcomes;
    };
    let descriptor = &prepared.descriptor;
    outcomes.extend([
        match input.provider_config.authorize(descriptor) {
            Ok(()) => OutcomeV1::passed(CheckV1::EndpointConfinement, &case.name),
            Err(refusal) => {
                OutcomeV1::failed(CheckV1::EndpointConfinement, &case.name, refusal.to_string())
            }
        },
        match admit_descriptor_auth(suite.manifest, &input.provider_config, descriptor) {
            Ok(_) => OutcomeV1::passed(CheckV1::DescriptorAuthWithinManifest, &case.name),
            Err(refusal) => OutcomeV1::failed(
                CheckV1::DescriptorAuthWithinManifest,
                &case.name,
                refusal.to_string(),
            ),
        },
    ]);
    outcomes
}

/// `MediaInputsFollowTheDeclaration`: judged on what the component produced for a case whose
/// request holds a media input.
fn media_follows_declaration(
    suite: &Suite<'_>,
    case: &EmbeddingsCaseV1,
    first: &Invoked,
) -> OutcomeV1 {
    let check = CheckV1::MediaInputsFollowTheDeclaration;
    let declared =
        suite.manifest.capabilities.contains(south_provider_api::EMBEDDINGS_MEDIA_CAPABILITY);
    let answer = match first {
        Ok(output) => output,
        Err(detail) => return OutcomeV1::failed(check, &case.name, detail.clone()),
    };
    let refusal = refusal_of(answer);
    let capability_refusal = refusal.and_then(code_of) == Some("capability");
    match (declared, refusal.is_some(), capability_refusal) {
        (true, false, _) | (_, true, true) => OutcomeV1::passed(check, &case.name),
        (true, true, false) => OutcomeV1::failed(
            check,
            &case.name,
            "the package declares `media`, yet a media input was refused other than as a \
             `capability` error (a model that takes no media is a capability error)",
        ),
        (false, ..) => OutcomeV1::failed(
            check,
            &case.name,
            "the package does not declare `media`, so a media input must be refused with a \
             `capability` error before admission",
        ),
    }
}

/// Adds a key the manifest does not declare to `ProviderConfig.declared` and requires the same
/// answer, a refusal included (the provider suite's check, embeddings record §16). The key is valid
/// under the kernel's `ComponentValues` grammar, so only the declaration tells it apart from one
/// the component may read. The embeddings world has no `host_values`.
fn undeclared_values_ignored(
    suite: &Suite<'_>,
    case: &EmbeddingsCaseV1,
    first: &Invoked,
) -> OutcomeV1 {
    let check = CheckV1::UndeclaredValuesIgnored;
    let family = case.input.pointer("/provider_config/provider").and_then(Value::as_str);
    let declared = family.map(|family| suite.manifest.declared_keys(family)).unwrap_or_default();
    // One more candidate than there are declared keys, so one is always free.
    let key = (0..=declared.len())
        .map(|index| format!("{UNDECLARED_VALUE}{index}"))
        .find(|key| !declared.contains(key.as_str()))
        .unwrap_or_default();
    let mut mutated = case.input.clone();
    let Some(Value::Object(config)) = mutated.pointer_mut("/provider_config") else {
        return OutcomeV1::failed(
            check,
            &case.name,
            "fixture has no object at `/provider_config` to carry `declared`",
        );
    };
    let entry = config.entry("declared").or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Value::Object(values) = entry else {
        return OutcomeV1::failed(
            check,
            &case.name,
            "`/provider_config/declared` is not an object",
        );
    };
    values.insert(key.clone(), Value::String("undeclared".to_owned()));
    if suite.invoke(case, &mutated) == *first {
        OutcomeV1::passed(check, &case.name)
    } else {
        OutcomeV1::failed(
            check,
            &case.name,
            format!(
                "adding the undeclared key `declared.{key}` changed the answer; a component reads \
                 only the keys its package declares"
            ),
        )
    }
}

/// `LocatorResolves` on a response case the fixture calls valid, and `UsageNeverDefaulted` on a
/// case carrying a `usage_pointer`.
fn response_checks(suite: &Suite<'_>, case: &EmbeddingsCaseV1) -> Vec<OutcomeV1> {
    let mut outcomes = Vec::new();
    let paired = parse::<ResponseInput>(&case.input)
        .and_then(|input| suite.pair(&input.request_case).map(|paired| (input, paired)));
    let (input, paired) = match paired {
        Ok(found) => found,
        Err(detail) => {
            if case.usage_pointer.is_some() {
                outcomes.push(OutcomeV1::failed(CheckV1::UsageNeverDefaulted, &case.name, detail));
            }
            return outcomes;
        }
    };
    let valid = case
        .expected
        .as_object()
        .is_some_and(|map| !map.contains_key("error") && !map.contains_key("host_refusal"));
    if valid {
        let check = CheckV1::LocatorResolves;
        let inputs = paired.request.inputs().len();
        outcomes.push(
            match extract_vectors_v1(input.response.body.as_bytes(), &paired.prepared.vectors) {
                Ok(extracted) if extracted.len() == inputs => OutcomeV1::passed(check, &case.name),
                Ok(extracted) => OutcomeV1::failed(
                    check,
                    &case.name,
                    format!(
                        "the prepared locator resolves {} vectors in the paired response, but \
                         the request has {inputs} inputs",
                        extracted.len()
                    ),
                ),
                Err(error) => OutcomeV1::failed(
                    check,
                    &case.name,
                    format!("the prepared locator does not resolve the paired response: {error}"),
                ),
            },
        );
    }
    if let Some(pointer) = &case.usage_pointer {
        outcomes.push(usage_never_defaulted(suite, case, &input, &paired, pointer));
    }
    outcomes
}

/// Deletes what `pointer` names in the upstream body; the component must then refuse with a
/// provider protocol error or report `NotReported`, never `Reported`. Judged on the component's
/// own answer, before the host's checks, which would otherwise mask a defaulted zero.
fn usage_never_defaulted(
    suite: &Suite<'_>,
    case: &EmbeddingsCaseV1,
    input: &ResponseInput,
    paired: &Paired,
    pointer: &str,
) -> OutcomeV1 {
    let check = CheckV1::UsageNeverDefaulted;
    let failed = |detail: String| OutcomeV1::failed(check, &case.name, detail);
    let Ok(mut body) = serde_json::from_str::<Value>(&input.response.body) else {
        return failed("the fixture's body is not a JSON document".to_owned());
    };
    let (parent, key) = pointer.rsplit_once('/').unwrap_or(("", pointer));
    let key = key.replace("~1", "/").replace("~0", "~");
    let removed = match body.pointer_mut(parent) {
        Some(Value::Object(map)) => map.remove(&key).is_some(),
        _ => false,
    };
    if !removed {
        return failed(format!("usage_pointer `{pointer}` names nothing in the body"));
    }
    let mut response = input.response.clone();
    response.body = body.to_string();
    match suite.answer(paired, &response, false) {
        Answered::Refused(error) if error.code == ErrorCode::ProviderProtocolError => {
            OutcomeV1::passed(check, &case.name)
        }
        Answered::Parsed(parsed) if parsed.usage().source() == UsageSourceV1::NotReported => {
            OutcomeV1::passed(check, &case.name)
        }
        Answered::Parsed(_) => failed(format!(
            "with `{pointer}` deleted the component still reported usage; usage is funds \
             evidence and a missing report must be an error or `not_reported`, never a number"
        )),
        Answered::Refused(error) => failed(format!(
            "without its usage the response was refused, but as `{}`, not as a provider \
             protocol error",
            error.code.as_str()
        )),
        Answered::HostRefusal(error) => failed(format!(
            "deleting `{pointer}` broke vector extraction ({error}); the pointer must name the \
             usage only"
        )),
    }
}

/// A rejected credential must never be retried on another upstream: a 401 or 403 is `rejected`
/// with a code the router does not retry elsewhere. `None` for other statuses.
fn auth_errors_are_not_retriable(case: &EmbeddingsCaseV1, mapped: &Invoked) -> Option<OutcomeV1> {
    let check = CheckV1::AuthErrorsAreNotRetriable;
    let status = case.input.get("status").and_then(Value::as_u64)?;
    if !matches!(status, 401 | 403) {
        return None;
    }
    let mapped = match mapped
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|mapped| wire::parse_provider_error_json(&mapped.to_string()))
    {
        Ok(mapped) => mapped,
        Err(detail) => {
            return Some(OutcomeV1::failed(check, &case.name, format!("not mapped: {detail}")));
        }
    };
    Some(match mapped {
        (EmbeddingsFailureOutcomeV1::Rejected, envelope)
            if !envelope.code.is_retriable_elsewhere() =>
        {
            OutcomeV1::passed(check, &case.name)
        }
        (EmbeddingsFailureOutcomeV1::Rejected, envelope) => OutcomeV1::failed(
            check,
            &case.name,
            format!(
                "status {status} mapped to `{}`, which the router retries on another upstream; a \
                 rejected credential would be replayed across every configured provider",
                envelope.code.as_str()
            ),
        ),
        (EmbeddingsFailureOutcomeV1::Unknown, _) => OutcomeV1::failed(
            check,
            &case.name,
            format!("status {status} proves the upstream produced nothing, so it is `rejected`"),
        ),
    })
}

/// A row the suite requires by name shows what record §10 says it does. Judged on what the
/// component produced, which `FixtureMatch` ties to the fixture.
fn named_row(suite: &Suite<'_>, case: &EmbeddingsCaseV1, first: &Invoked) -> OutcomeV1 {
    let check = CheckV1::NamedRowAssertion;
    let problem = match first {
        Ok(output) => row_problem(suite, case, output),
        Err(detail) => Some(detail.clone()),
    };
    problem.map_or_else(
        || OutcomeV1::passed(check, &case.name),
        |problem| OutcomeV1::failed(check, &case.name, problem),
    )
}

fn code_of(refusal: &Value) -> Option<&str> {
    refusal.get("code").and_then(Value::as_str)
}

fn row_problem(suite: &Suite<'_>, case: &EmbeddingsCaseV1, output: &Value) -> Option<String> {
    let capability = |word: &str| suite.manifest.capabilities.contains(word);
    let refusal = refusal_of(output);
    let request = || parse::<RequestInput>(&case.input).map(|input| input.request);
    let problem = |text: &str| Some(text.to_owned());
    match case.name.trim_start_matches("embeddings.") {
        "request.single-text" => match request() {
            Ok(request)
                if request.input_shape() == InputShapeV1::Single
                    && matches!(request.inputs(), [south_contracts::EmbeddingInputV1::Text(_)]) =>
            {
                refusal.map(|_| "a single text input must build".to_owned())
            }
            Ok(_) => problem("must carry one text input of shape `single`"),
            Err(detail) => Some(detail),
        },
        "request.batch-text" => match request() {
            Ok(request)
                if request.input_shape() == InputShapeV1::Array
                    && request.inputs().len() > 1
                    && request.inputs().iter().all(|input| {
                        matches!(input, south_contracts::EmbeddingInputV1::Text(_))
                    }) =>
            {
                match (capability("batch"), refusal) {
                    (true, None) | (false, Some(_)) => None,
                    (true, Some(_)) => problem("the manifest declares `batch`, yet it was refused"),
                    (false, None) => problem("the manifest does not declare `batch`, yet it built"),
                }
            }
            Ok(_) => problem("must carry more than one text input of shape `array`"),
            Err(detail) => Some(detail),
        },
        "request.dimensions" => dimensions_row(suite, case, refusal.is_some()),
        "request.refused-capability" => match refusal.and_then(code_of) {
            Some("capability") => None,
            _ => problem("must be refused with a `capability` error before admission"),
        },
        "request.media" => media_row(case, output, refusal.is_some()),
        "request.extra-fields" => match request() {
            Ok(request) if request.extra().is_empty() => {
                problem("must carry unmodelled northbound fields")
            }
            Ok(_) => None,
            Err(detail) => Some(detail),
        },
        "response.usage" => {
            let usage = output.get("usage");
            match usage.and_then(|usage| usage.get("source")).and_then(Value::as_str) {
                Some("reported")
                    if usage
                        .and_then(|usage| usage.get("input_tokens"))
                        .and_then(Value::as_u64)
                        .is_none_or(|tokens| tokens == 0) =>
                {
                    problem("a usage sample must report a non-zero count")
                }
                Some("reported") if case.usage_pointer.is_none() => problem(
                    "carries no usage_pointer sidecar, so UsageNeverDefaulted has nothing to \
                     delete",
                ),
                Some(_) => None,
                None => problem("must be answered with parsed usage facts"),
            }
        }
        "response.missing-usage" => missing_usage_row(suite, case, output),
        "response.count-mismatch" => count_mismatch_row(suite, case, output),
        "error.rejected-credential" => match case.input.get("status").and_then(Value::as_u64) {
            Some(401 | 403) => match output.get("outcome").and_then(Value::as_str) {
                Some("rejected") => None,
                _ => problem("a rejected credential is `rejected`"),
            },
            _ => problem("must answer status 401 or 403"),
        },
        "error.server" => match case.input.get("status").and_then(Value::as_u64) {
            Some(500..=599) => match output.get("outcome").and_then(Value::as_str) {
                Some("unknown") => None,
                _ => problem("a 5xx does not prove nothing was produced, so it is `unknown`"),
            },
            _ => problem("must answer a 5xx status"),
        },
        _ => None,
    }
}

/// `request.media` (record §17.6): the request holds a media input, the component built it, and
/// every media input's `data` appears in the descriptor body exactly as the client sent it, since
/// the host never decodes it.
fn media_row(case: &EmbeddingsCaseV1, output: &Value, was_refused: bool) -> Option<String> {
    let request = match parse::<RequestInput>(&case.input) {
        Ok(input) => input.request,
        Err(detail) => return Some(detail),
    };
    if !request.carries_media() {
        return Some("must carry a media input".to_owned());
    }
    if was_refused {
        return Some("the manifest declares `media`, yet the media request was refused".into());
    }
    let body = &output["descriptor"]["body"];
    for input in request.inputs() {
        if let south_contracts::EmbeddingInputV1::Media { data, .. } = input
            && !data.is_empty()
            && !contains_string(body, data)
        {
            return Some(
                "a media input's `data` does not appear in the body unchanged; the host never \
                 decodes it, so the component must pass the client's string through"
                    .to_owned(),
            );
        }
    }
    None
}

/// Whether `needle` is, as a whole, a string anywhere in `value`.
fn contains_string(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(text) => text == needle,
        Value::Array(items) => items.iter().any(|item| contains_string(item, needle)),
        Value::Object(map) => map.values().any(|item| contains_string(item, needle)),
        _ => false,
    }
}

/// `request.dimensions`: the request carries `dimensions`; a component declaring the capability
/// builds it and places it in the body — changing the value changes the body — and one without it
/// refuses.
fn dimensions_row(suite: &Suite<'_>, case: &EmbeddingsCaseV1, was_refused: bool) -> Option<String> {
    let input = match parse::<RequestInput>(&case.input) {
        Ok(input) => input,
        Err(detail) => return Some(detail),
    };
    let request = &input.request;
    let Some(dimensions) = request.dimensions() else {
        return Some("must carry `dimensions`".to_owned());
    };
    match (suite.manifest.capabilities.contains("dimensions"), was_refused) {
        (true, false) => {}
        (false, true) => return None,
        (true, true) => {
            return Some("the manifest declares `dimensions`, yet it was refused".into());
        }
        (false, false) => {
            return Some("the manifest does not declare `dimensions`, yet it built".into());
        }
    }
    let other = if dimensions == u32::MAX { dimensions - 1 } else { dimensions + 1 };
    let changed = EmbeddingsRequestV1::new_v2(
        request.model().to_owned(),
        request.inputs().to_vec(),
        request.input_shape(),
        Some(other),
        request.encoding_format(),
        request.user().map(str::to_owned),
        request.extra().clone(),
    )
    .map_err(|error| error.to_string())
    .map(|changed| RequestInput {
        provider_config: input.provider_config.clone(),
        request: changed,
    });
    let bodies = changed.and_then(|changed| {
        let body = |input: &RequestInput| {
            suite
                .build(input)
                .map(|prepared| prepared.descriptor.body)
                .map_err(|error| error.message)
        };
        Ok((body(&input)?, body(&changed)?))
    });
    match bodies {
        Ok((before, after)) if before != after => None,
        Ok(_) => Some("the body does not change with `dimensions`, so it was not placed".into()),
        Err(detail) => Some(format!("rebuilding with other dimensions failed: {detail}")),
    }
}

/// `response.missing-usage`: a dialect whose prepared request carries a fallback estimate reports
/// `not_reported`; one without a fallback refuses with a provider protocol error.
fn missing_usage_row(suite: &Suite<'_>, case: &EmbeddingsCaseV1, output: &Value) -> Option<String> {
    let paired = match parse::<ResponseInput>(&case.input)
        .and_then(|input| suite.pair(&input.request_case))
    {
        Ok(paired) => paired,
        Err(detail) => return Some(detail),
    };
    let source = output.get("usage").and_then(|usage| usage.get("source")).and_then(Value::as_str);
    let code = refusal_of(output).and_then(code_of);
    match (paired.prepared.estimate.fallback_input_tokens(), source, code) {
        (Some(_), Some("not_reported"), _) | (None, _, Some("provider_protocol_error")) => None,
        (Some(_), ..) => Some("a dialect with a fallback estimate reports `not_reported`".into()),
        (None, ..) => Some(
            "a dialect without a fallback estimate refuses a 2xx without usage as a provider \
             protocol error, never a zero"
                .into(),
        ),
    }
}

/// `response.count-mismatch`: the paired locator resolves a count other than the request's input
/// count, and the case ends in a refusal.
fn count_mismatch_row(
    suite: &Suite<'_>,
    case: &EmbeddingsCaseV1,
    output: &Value,
) -> Option<String> {
    let found = parse::<ResponseInput>(&case.input)
        .and_then(|input| suite.pair(&input.request_case).map(|paired| (input, paired)));
    let (input, paired) = match found {
        Ok(found) => found,
        Err(detail) => return Some(detail),
    };
    let resolved = extract_vectors_v1(input.response.body.as_bytes(), &paired.prepared.vectors)
        .map(|extracted| extracted.len());
    if resolved.is_ok_and(|count| count == paired.request.inputs().len()) {
        return Some("the response's vector count must differ from the request's inputs".into());
    }
    let refused = refusal_of(output).is_some()
        || output.get("host_refusal").and_then(Value::as_str).is_some();
    if refused { None } else { Some("a count mismatch must be refused, never parsed".into()) }
}

/// Keeps a failure readable when the payload is a whole prepared request.
fn truncate(value: &Value) -> String {
    let rendered = value.to_string();
    if rendered.chars().count() <= 200 {
        return rendered;
    }
    let head: String = rendered.chars().take(200).collect();
    format!("{head}…")
}
