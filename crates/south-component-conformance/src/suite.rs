//! Gate ② — the component-behavior suite, and the order its checks run in.
//!
//! Every check reduces to invoking one component function on one fixture
//! input and looking at what came back, so the suite has one shape: turn a
//! case into a closure `input -> output`, then ask the same questions of it.
//! Only the questions that need a *typed* view of the input or the output —
//! endpoint confinement, auth-error retriability, stream incrementality —
//! reach past that closure.

use serde::Deserialize;
use serde_json::Value;
use token_station_protocol::{
    ChatRequest, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
    StreamEvent,
};

use crate::component::{ProviderComponentV1, StreamParserV1};
use crate::fixture::{CaseV1, FixturePackV1, ProviderFamilyV1};
use crate::report::{CheckV1, OutcomeV1, ReportV1};

/// The suite identifier, equal to the manifest's frozen
/// `conformance.required_suite`.
pub const PROVIDER_COMPONENT_SUITE_V1: &str = south_provider_api::COMPONENT_BEHAVIOR_SUITE;

/// The key injected to prove a component tolerates a newer peer's field.
const UNKNOWN_FIELD: &str = "__conformance_unknown_field";

/// What one component invocation produced, with a bad fixture told apart from
/// a bad component.
type Invoked = Result<Value, Failure>;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Failure {
    /// The component answered with an error.
    Component(String),
    /// The fixture did not deserialize into what the family feeds the
    /// component. Not the component's fault, and reported as its own reason.
    Fixture(String),
}

impl Failure {
    fn detail(&self) -> String {
        match self {
            Self::Component(detail) => format!("component returned an error: {detail}"),
            Self::Fixture(detail) => format!("fixture is not valid input: {detail}"),
        }
    }
}

fn component_error(error: &ErrorEnvelope) -> Failure {
    Failure::Component(format!("{:?}: {}", error.code, error.message))
}

fn parse<T: for<'de> Deserialize<'de>>(input: &Value) -> Result<T, Failure> {
    serde_json::from_value(input.clone()).map_err(|source| Failure::Fixture(source.to_string()))
}

fn encode<T: serde::Serialize>(value: &T) -> Invoked {
    serde_json::to_value(value).map_err(|source| Failure::Fixture(source.to_string()))
}

#[derive(Deserialize)]
struct RequestInput {
    provider_config: ProviderConfig,
    chat_request: ChatRequest,
}

/// A stream fixture's chunk list: UTF-8 text for SSE dialects, raw byte
/// arrays for binary dialects (eventstream). Either or both may appear; the
/// concatenation order is `chunks` then `chunks_bytes`.
#[derive(Deserialize)]
struct StreamInput {
    #[serde(default)]
    chunks: Vec<String>,
    #[serde(default)]
    chunks_bytes: Vec<Vec<u8>>,
}

impl StreamInput {
    fn body(&self) -> Vec<u8> {
        let mut body = Vec::new();
        for chunk in &self.chunks {
            body.extend_from_slice(chunk.as_bytes());
        }
        for chunk in &self.chunks_bytes {
            body.extend_from_slice(chunk);
        }
        body
    }
}

/// Runs `south.provider-component.v1` against a provider component.
///
/// Never panics on a bad component or a bad fixture: both become failures in
/// the report, because a host running this at admission time must not be
/// taken down by the package it is vetting.
#[must_use]
pub fn run_provider_component_suite_v1(
    component: &dyn ProviderComponentV1,
    pack: &FixturePackV1,
) -> ReportV1 {
    let mut outcomes = coverage(pack);
    let mut a_credential_was_rejected_somewhere = false;

    for case in pack.cases() {
        let invoke = |input: &Value| invoke_component(component, case.family, input);

        outcomes.extend(shared_checks(case, &invoke));

        match case.family {
            ProviderFamilyV1::Request => {
                outcomes.push(endpoint_confinement(case, &invoke(&case.input)));
            }
            ProviderFamilyV1::Error => {
                if let Some(outcome) = auth_errors_are_not_retriable(case, &invoke(&case.input)) {
                    a_credential_was_rejected_somewhere = true;
                    outcomes.push(outcome);
                }
            }
            ProviderFamilyV1::Stream => {
                outcomes.push(stream_incrementality(component, case));
            }
            ProviderFamilyV1::Capabilities | ProviderFamilyV1::Response => {}
        }
    }

    // A gate that never runs describes nothing. Without a fixture that rejects
    // a credential, a component passes `AuthErrorsAreNotRetriable` by never
    // being asked — so the missing fixture is itself the failure.
    if !a_credential_was_rejected_somewhere {
        outcomes.push(OutcomeV1::failed(
            CheckV1::AuthErrorsAreNotRetriable,
            "provider.error",
            "no fixture maps a 401 or 403, so the check that keeps a rejected credential from \
             being replayed across every configured upstream never ran",
        ));
    }

    ReportV1::new(PROVIDER_COMPONENT_SUITE_V1, outcomes)
}

fn invoke_component(
    component: &dyn ProviderComponentV1,
    family: ProviderFamilyV1,
    input: &Value,
) -> Invoked {
    match family {
        ProviderFamilyV1::Capabilities => {
            let config: ProviderConfig = parse(input)?;
            encode(&component.model_capabilities(&config).map_err(|e| component_error(&e))?)
        }
        ProviderFamilyV1::Request => {
            let RequestInput { provider_config, chat_request } = parse(input)?;
            encode(
                &component
                    .build_http_request(&chat_request, &provider_config)
                    .map_err(|e| component_error(&e))?,
            )
        }
        ProviderFamilyV1::Response => {
            let parts: HttpResponseParts = parse(input)?;
            encode(&component.parse_response(&parts).map_err(|e| component_error(&e))?)
        }
        ProviderFamilyV1::Error => {
            let parts: HttpResponseParts = parse(input)?;
            encode(&component.map_provider_error(&parts).map_err(|e| component_error(&e))?)
        }
        ProviderFamilyV1::Stream => {
            let input: StreamInput = parse(input)?;
            let body = input.body();
            let chunk_bounds: Vec<usize> = {
                let mut bounds = Vec::new();
                let mut offset = 0;
                for chunk in &input.chunks {
                    offset += chunk.len();
                    bounds.push(offset);
                }
                for chunk in &input.chunks_bytes {
                    offset += chunk.len();
                    bounds.push(offset);
                }
                bounds
            };
            encode(&feed(component.stream_parser().as_mut(), &body, &chunk_bounds)?)
        }
    }
}

/// Feeds `body` split at `bounds`, then flushes EOF via `finish`.
///
/// EOF is part of every stream's lifecycle, so the suite always drives it: a
/// dialect whose terminal accounting arrives only at EOF (or a body that ends
/// without its own terminal marker) is judged on what the flush emits too.
fn feed(
    parser: &mut dyn StreamParserV1,
    body: &[u8],
    bounds: &[usize],
) -> Result<Vec<StreamEvent>, Failure> {
    let mut events = Vec::new();
    let mut start = 0;
    for &end in bounds {
        events.extend(
            parser.parse_chunk(&body[start..end]).map_err(|error| component_error(&error))?,
        );
        start = end;
    }
    if start < body.len() {
        events.extend(parser.parse_chunk(&body[start..]).map_err(|e| component_error(&e))?);
    }
    events.extend(parser.finish().map_err(|error| component_error(&error))?);
    Ok(events)
}

/// The check that makes `ProviderConfig::authorize` a gate rather than a
/// suggestion.
///
/// Run against what the component *built*, not against the fixture's expected
/// descriptor. A component that fails `FixtureMatch` and confines its request
/// is broken; one that matches the fixture and does not is dangerous, and the
/// two must be distinguishable in the report.
fn endpoint_confinement(case: &CaseV1, built: &Invoked) -> OutcomeV1 {
    let check = CheckV1::EndpointConfinement;

    let input: RequestInput = match parse(&case.input) {
        Ok(input) => input,
        Err(failure) => return OutcomeV1::failed(check, &case.name, failure.detail()),
    };
    let built = match built {
        Ok(built) => built,
        Err(failure) => return OutcomeV1::failed(check, &case.name, failure.detail()),
    };
    let descriptor: HttpRequestDescriptor = match parse(built) {
        Ok(descriptor) => descriptor,
        Err(failure) => return OutcomeV1::failed(check, &case.name, failure.detail()),
    };

    match input.provider_config.authorize(&descriptor) {
        Ok(()) => OutcomeV1::passed(check, &case.name),
        Err(refusal) => OutcomeV1::failed(check, &case.name, refusal.to_string()),
    }
}

/// A rejected credential must never be retried on another upstream.
///
/// `None` when the case says nothing about credentials. Only `401` and `403`
/// unambiguously do: a `429` may legitimately map to `RateLimit` or
/// `Capacity`, both retriable, and the fixture already pins which.
fn auth_errors_are_not_retriable(case: &CaseV1, mapped: &Invoked) -> Option<OutcomeV1> {
    let check = CheckV1::AuthErrorsAreNotRetriable;

    let status = case.input.get("status").and_then(Value::as_u64)?;
    if !matches!(status, 401 | 403) {
        return None;
    }

    let Ok(mapped) = mapped else {
        return Some(OutcomeV1::failed(check, &case.name, "could not map the error to inspect"));
    };
    let envelope: ErrorEnvelope = match parse(mapped) {
        Ok(envelope) => envelope,
        Err(failure) => return Some(OutcomeV1::failed(check, &case.name, failure.detail())),
    };

    if envelope.code.is_retriable_elsewhere() {
        return Some(OutcomeV1::failed(
            check,
            &case.name,
            format!(
                "status {status} mapped to `{:?}`, which the router retries on another upstream; \
                 a rejected credential would be replayed across every configured provider",
                envelope.code
            ),
        ));
    }
    Some(OutcomeV1::passed(check, &case.name))
}

/// However the body is split, the events must be the same.
///
/// The fixture's own chunking is one arbitrary split among many; the network
/// picks a different one every time. So the body is re-split at **every byte
/// boundary** — stricter than the v1 char-boundary donor, because the v2 ABI
/// carries raw bytes and a socket split can land inside a UTF-8 sequence —
/// each time through a fresh parser, and every run must reproduce the
/// expected events. Splitting at index `0` also feeds an empty chunk, which a
/// parser must tolerate rather than treat as end-of-stream.
fn stream_incrementality(component: &dyn ProviderComponentV1, case: &CaseV1) -> OutcomeV1 {
    let check = CheckV1::StreamIncrementality;

    let input: StreamInput = match parse(&case.input) {
        Ok(input) => input,
        Err(failure) => return OutcomeV1::failed(check, &case.name, failure.detail()),
    };
    let body = input.body();

    for split in 0..=body.len() {
        let events = match feed(component.stream_parser().as_mut(), &body, &[split]) {
            Ok(events) => events,
            Err(failure) => {
                return OutcomeV1::failed(
                    check,
                    &case.name,
                    format!("split at byte {split}: {}", failure.detail()),
                );
            }
        };

        let encoded = match encode(&events) {
            Ok(encoded) => encoded,
            Err(failure) => return OutcomeV1::failed(check, &case.name, failure.detail()),
        };
        if encoded != case.expected {
            return OutcomeV1::failed(
                check,
                &case.name,
                format!(
                    "split at byte {split} produced different events; a chunk off a socket is \
                     not a whole frame"
                ),
            );
        }
    }

    OutcomeV1::passed(check, &case.name)
}

fn coverage(pack: &FixturePackV1) -> Vec<OutcomeV1> {
    let missing = pack.missing_families();
    if missing.is_empty() {
        return vec![OutcomeV1::passed(CheckV1::Coverage, "provider")];
    }
    missing
        .into_iter()
        .map(|family| {
            OutcomeV1::failed(
                CheckV1::Coverage,
                format!("provider.{}", family.token()),
                "no fixture exercises this family",
            )
        })
        .collect()
}

fn shared_checks(case: &CaseV1, invoke: &dyn Fn(&Value) -> Invoked) -> Vec<OutcomeV1> {
    let first = invoke(&case.input);

    vec![
        fixture_match(case, &first),
        determinism(case, &first, &invoke(&case.input)),
        unknown_field_tolerance(case, invoke),
    ]
}

fn fixture_match(case: &CaseV1, actual: &Invoked) -> OutcomeV1 {
    match actual {
        Err(failure) => OutcomeV1::failed(CheckV1::FixtureMatch, &case.name, failure.detail()),
        Ok(actual) if *actual == case.expected => {
            OutcomeV1::passed(CheckV1::FixtureMatch, &case.name)
        }
        Ok(actual) => OutcomeV1::failed(
            CheckV1::FixtureMatch,
            &case.name,
            format!("expected {}, produced {}", truncate(&case.expected), truncate(actual)),
        ),
    }
}

fn determinism(case: &CaseV1, first: &Invoked, second: &Invoked) -> OutcomeV1 {
    if first == second {
        OutcomeV1::passed(CheckV1::Determinism, &case.name)
    } else {
        OutcomeV1::failed(
            CheckV1::Determinism,
            &case.name,
            "the same input produced different output twice; a suite that admitted this \
             component did not observe the component the host will run",
        )
    }
}

/// Injects a field this ABI version does not model, and requires the component
/// to carry on.
///
/// The field goes *inside* the IR object the family feeds the component, not
/// at the top of a wrapper the suite invented, which is why the pointer is
/// per family. A family with nowhere to put one passes without being asked.
fn unknown_field_tolerance(case: &CaseV1, invoke: &dyn Fn(&Value) -> Invoked) -> OutcomeV1 {
    let check = CheckV1::UnknownFieldTolerance;
    let Some(pointer) = case.family.unknown_field_pointer() else {
        return OutcomeV1::passed(check, &case.name);
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

    match invoke(&mutated) {
        Ok(_) => OutcomeV1::passed(check, &case.name),
        Err(failure) => OutcomeV1::failed(
            check,
            &case.name,
            format!(
                "a field this version does not model was refused: {}; a component must degrade \
                 in front of a newer peer, not fail",
                failure.detail()
            ),
        ),
    }
}

/// Keeps a failure readable when the payload is a whole chat response.
fn truncate(value: &Value) -> String {
    let rendered = value.to_string();
    if rendered.chars().count() <= 200 {
        return rendered;
    }
    let head: String = rendered.chars().take(200).collect();
    format!("{head}…")
}
