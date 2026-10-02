//! Gate ② — the component-behavior suite, and the order its checks run in.
//!
//! Every check reduces to invoking one component function on one fixture
//! input and looking at what came back, so the suite has one shape: turn a
//! case into a closure `input -> output`, then ask the same questions of it.
//! Only the questions that need a *typed* view of the input or the output —
//! endpoint confinement, auth-error retriability, stream incrementality —
//! reach past that closure.
//!
//! Usage is funds evidence, so a package whose manifest declares the default
//! `usage_evidence: reported` must ship five usage rows by name and pass
//! `UsageRows`, `UsageNeverDefaulted` and `UsagePartition`; a package declaring
//! `absent` is held to `AbsentFamilyEmitsNoUsage` instead (B1,
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md` §6.2).

use serde::Deserialize;
use serde_json::Value;
use south_provider_api::{
    ComponentManifestV1, ModelLocationV1, RequestFactsV1, StreamLocationV1, UsageEvidenceV1,
};
use token_station_protocol::{
    ChatRequest, ErrorEnvelope, HttpRequestDescriptor, HttpResponseParts, ProviderConfig,
    StreamEvent,
};

use crate::component::{ProviderComponentV1, StreamParserV1};
use crate::credential_fixture::credential_recipe_checks_v1;
use crate::descriptor_auth::admit_descriptor_auth;
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
    /// The component answered with an error: a readable detail, and the
    /// envelope as JSON for a case that expects this error.
    Component { detail: String, envelope: Value },
    /// The fixture did not deserialize into what the family feeds the
    /// component. Not the component's fault, and reported as its own reason.
    Fixture(String),
}

impl Failure {
    fn detail(&self) -> String {
        match self {
            Self::Component { detail, .. } => format!("component returned an error: {detail}"),
            Self::Fixture(detail) => format!("fixture is not valid input: {detail}"),
        }
    }
}

fn component_error(error: &ErrorEnvelope) -> Failure {
    Failure::Component {
        detail: format!("{:?}: {}", error.code, error.message),
        envelope: serde_json::to_value(error).unwrap_or(Value::Null),
    }
}

/// The usage rows a `reported` package ships by name (B1,
/// host-zero-vendor-boundary §6.2 item 2).
const USAGE_ROWS: [&str; 5] = [
    "provider.response.usage",
    "provider.response.missing-usage",
    "provider.response.cached-usage",
    "provider.stream.usage-terminal",
    "provider.stream.no-usage",
];

/// The error a response case expects, when its expected file is
/// `{"error": <envelope>}` rather than a chat response.
fn expected_error(case: &CaseV1) -> Option<&Value> {
    if case.family != ProviderFamilyV1::Response {
        return None;
    }
    let map = case.expected.as_object()?;
    if map.len() == 1 { map.get("error") } else { None }
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

/// Runs `south.provider-component.v1` against a provider component whose
/// manifest declares the default `usage_evidence: reported`.
///
/// Never panics on a bad component or a bad fixture: both become failures in
/// the report, because a host running this at admission time must not be
/// taken down by the package it is vetting.
#[must_use]
pub fn run_provider_component_suite_v1(
    component: &dyn ProviderComponentV1,
    pack: &FixturePackV1,
) -> ReportV1 {
    run_suite(component, pack, UsageEvidenceV1::Reported, None)
}

/// Runs `south.provider-component.v1` against a provider component with its manifest.
///
/// The manifest selects the usage checks (`usage_evidence`) and is what
/// `DescriptorAuthWithinManifest` judges descriptors against (`auth_arms`).
/// When it declares `credentials`, the pack's `credential.*` cases run
/// through `CredentialRecipeMatch` and the credential coverage rule.
/// This is the entry point an admitting host uses.
#[must_use]
pub fn run_provider_component_suite_v1_for_manifest(
    component: &dyn ProviderComponentV1,
    pack: &FixturePackV1,
    manifest: &ComponentManifestV1,
) -> ReportV1 {
    run_suite(component, pack, manifest.usage_evidence, Some(manifest))
}

fn run_suite(
    component: &dyn ProviderComponentV1,
    pack: &FixturePackV1,
    usage_evidence: UsageEvidenceV1,
    manifest: Option<&ComponentManifestV1>,
) -> ReportV1 {
    let mut outcomes = coverage(pack, usage_evidence);
    let mut a_credential_was_rejected_somewhere = false;

    for case in pack.cases() {
        let invoke = |input: &Value| invoke_component(component, case.family, input);

        outcomes.extend(shared_checks(case, &invoke));

        match case.family {
            ProviderFamilyV1::Request => {
                let built = invoke(&case.input);
                outcomes.push(endpoint_confinement(case, &built));
                if let Some(manifest) = manifest {
                    outcomes.push(descriptor_auth_within_manifest(case, &built, manifest));
                    outcomes.push(request_facts_honoured(case, &built, manifest, &invoke));
                }
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

        outcomes.extend(usage_checks(case, usage_evidence, &invoke));
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

    if let Some(credentials) = manifest.and_then(|manifest| manifest.credentials.as_ref()) {
        outcomes.extend(credential_recipe_checks_v1(credentials, pack.credentials()));
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

/// The descriptor presents its credential only through an arm the manifest
/// declares (B2, host-zero-vendor-boundary §4.4).
///
/// Run on what the component built. A host that presents strictly what the
/// descriptor says relies on this; the host-side admission is what binds a
/// third-party package, and this check is its early warning.
fn descriptor_auth_within_manifest(
    case: &CaseV1,
    built: &Invoked,
    manifest: &ComponentManifestV1,
) -> OutcomeV1 {
    let check = CheckV1::DescriptorAuthWithinManifest;
    let input: RequestInput = match parse(&case.input) {
        Ok(input) => input,
        Err(failure) => return OutcomeV1::failed(check, &case.name, failure.detail()),
    };
    let descriptor: HttpRequestDescriptor = match built {
        Ok(built) => match parse(built) {
            Ok(descriptor) => descriptor,
            Err(failure) => return OutcomeV1::failed(check, &case.name, failure.detail()),
        },
        Err(failure) => return OutcomeV1::failed(check, &case.name, failure.detail()),
    };
    match admit_descriptor_auth(manifest, &input.provider_config, &descriptor) {
        Ok(_) => OutcomeV1::passed(check, &case.name),
        Err(refusal) => OutcomeV1::failed(check, &case.name, refusal.to_string()),
    }
}

/// The built request carries the cap, the model and the stream flag where the
/// manifest's `request_facts` say (B2, host-zero-vendor-boundary §7.2, §7.6).
///
/// The cap is also a mutation check: the suite changes the IR cap, builds
/// again, and requires the two bodies to differ only at the declared cap
/// locations — for a family declaring none, not at all. A body that changes
/// elsewhere has written the cap somewhere the host's seal does not look.
fn request_facts_honoured(
    case: &CaseV1,
    built: &Invoked,
    manifest: &ComponentManifestV1,
    invoke: &dyn Fn(&Value) -> Invoked,
) -> OutcomeV1 {
    let check = CheckV1::RequestFactsHonoured;
    request_facts_problem(case, built, manifest, invoke).map_or_else(
        || OutcomeV1::passed(check, &case.name),
        |problem| OutcomeV1::failed(check, &case.name, problem),
    )
}

fn request_facts_problem(
    case: &CaseV1,
    built: &Invoked,
    manifest: &ComponentManifestV1,
    invoke: &dyn Fn(&Value) -> Invoked,
) -> Option<String> {
    let input: RequestInput = match parse(&case.input) {
        Ok(input) => input,
        Err(failure) => return Some(failure.detail()),
    };
    let descriptor: HttpRequestDescriptor = match built {
        Ok(built) => match parse(built) {
            Ok(descriptor) => descriptor,
            Err(failure) => return Some(failure.detail()),
        },
        Err(failure) => return Some(failure.detail()),
    };
    let facts = manifest.request_facts_for(&input.provider_config.provider);
    let body = descriptor.body.clone().unwrap_or(Value::Null);
    let request = &input.chat_request;

    if let Some(cap) = request.sampling.max_output_tokens
        && !facts.output_cap.is_empty()
    {
        let holding: Vec<&Value> =
            facts.output_cap.iter().filter_map(|pointer| body.pointer(pointer)).collect();
        if holding.len() != 1 || holding[0].as_u64() != Some(u64::from(cap)) {
            return Some(format!(
                "the IR cap {cap} must sit in exactly one declared location ({}), and the \
                 others must be absent",
                facts.output_cap.join(", ")
            ));
        }
    }
    if let Some(problem) = cap_moves_only_where_declared(&input, &facts, invoke) {
        return Some(problem);
    }

    match &facts.model {
        ModelLocationV1::Body(pointer) => {
            if body.pointer(pointer).and_then(Value::as_str) != Some(request.model.as_str()) {
                return Some(format!("the body must name the IR model at `{pointer}`"));
            }
        }
        ModelLocationV1::Url(template) => {
            let (prefix, suffix) = template.split_once("{model}").unwrap_or((template, ""));
            let expected =
                format!("{prefix}{}{suffix}", crate::url_segment::encode(&request.model));
            if !url_path(&descriptor.url).contains(&expected) {
                return Some(format!(
                    "the URL path must carry the IR model as one segment where `{template}` \
                     places it"
                ));
            }
        }
    }

    if let StreamLocationV1::Body(pointer) = &facts.stream {
        let flag = body.pointer(pointer);
        let consistent = if request.stream {
            flag == Some(&Value::Bool(true))
        } else {
            flag.is_none_or(|flag| *flag == Value::Bool(false))
        };
        if !consistent {
            return Some(format!("the stream flag at `{pointer}` must agree with the IR"));
        }
    }
    None
}

/// Builds the request twice through the same serialization, once with a
/// different IR cap, and compares the bodies with the declared cap locations
/// removed.
fn cap_moves_only_where_declared(
    input: &RequestInput,
    facts: &RequestFactsV1,
    invoke: &dyn Fn(&Value) -> Invoked,
) -> Option<String> {
    let other_cap = match input.chat_request.sampling.max_output_tokens {
        Some(cap) if cap > 1 => cap - 1,
        Some(_) => 2,
        None => 777,
    };
    let body_with = |cap: Option<u32>| -> Result<Value, String> {
        let mut request = input.chat_request.clone();
        request.sampling.max_output_tokens = cap;
        let wire = serde_json::json!({
            "provider_config": input.provider_config,
            "chat_request": request,
        });
        let built = invoke(&wire).map_err(|failure| failure.detail())?;
        let descriptor: HttpRequestDescriptor =
            parse(&built).map_err(|failure| failure.detail())?;
        let mut body = descriptor.body.unwrap_or(Value::Null);
        for pointer in &facts.output_cap {
            remove_pointer(&mut body, pointer);
        }
        Ok(body)
    };
    let (baseline, mutated) = match (
        body_with(input.chat_request.sampling.max_output_tokens),
        body_with(Some(other_cap)),
    ) {
        (Ok(baseline), Ok(mutated)) => (baseline, mutated),
        (Err(detail), _) | (_, Err(detail)) => {
            return Some(format!("rebuilding the request failed: {detail}"));
        }
    };
    if baseline == mutated {
        None
    } else if facts.output_cap.is_empty() {
        Some("the family declares no cap location, yet the body changed with the IR cap".to_owned())
    } else {
        Some("the body changed with the IR cap outside the declared cap locations".to_owned())
    }
}

/// Removes what `pointer` names, then every ancestor on its path that the
/// removal left empty: a cap is the only member of the object a dialect wraps
/// it in when nothing else is set (`generationConfig`), and that object is part
/// of the declared location.
fn remove_pointer(value: &mut Value, pointer: &str) {
    let mut path = pointer.to_owned();
    let mut removed_member = false;
    while let Some((parent, key)) = path.rsplit_once('/') {
        let key = key.replace("~1", "/").replace("~0", "~");
        let Some(Value::Object(map)) = value.pointer_mut(parent) else {
            return;
        };
        let removable = !removed_member
            || map
                .get(&key)
                .is_some_and(|child| child.as_object().is_some_and(serde_json::Map::is_empty));
        if !removable || map.remove(&key).is_none() {
            return;
        }
        removed_member = true;
        if !map.is_empty() || parent.is_empty() {
            return;
        }
        path = parent.to_owned();
    }
}

/// The path of a URL: after the authority, before any query or fragment.
fn url_path(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = after_scheme.find('/').map_or("", |start| &after_scheme[start..]);
    path.split(['?', '#']).next().unwrap_or(path)
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

fn coverage(pack: &FixturePackV1, usage_evidence: UsageEvidenceV1) -> Vec<OutcomeV1> {
    let mut outcomes: Vec<OutcomeV1> = pack
        .missing_families()
        .into_iter()
        .map(|family| {
            OutcomeV1::failed(
                CheckV1::Coverage,
                format!("provider.{}", family.token()),
                "no fixture exercises this family",
            )
        })
        .collect();
    // An `absent` package proves itself on every response and stream case
    // (`AbsentFamilyEmitsNoUsage`), so it owes no named rows.
    if usage_evidence.is_reported() {
        for row in USAGE_ROWS {
            if !pack.cases().iter().any(|case| case.name == row) {
                outcomes.push(OutcomeV1::failed(
                    CheckV1::Coverage,
                    row,
                    "a package whose upstream reports usage must ship this usage row; without it \
                     the check that keeps usage from defaulting to zero never ran",
                ));
            }
        }
    }
    if outcomes.is_empty() {
        outcomes.push(OutcomeV1::passed(CheckV1::Coverage, "provider"));
    }
    outcomes
}

/// The usage reports in a response or stream output, in order.
fn usage_reports(family: ProviderFamilyV1, output: &Value) -> Vec<&Value> {
    match family {
        ProviderFamilyV1::Response => output.get("usage").into_iter().collect(),
        ProviderFamilyV1::Stream => output
            .as_array()
            .map(|events| {
                events
                    .iter()
                    .filter(|event| event["type"] == "usage")
                    .map(|event| &event["usage"])
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn count(usage: &Value, field: &str) -> u64 {
    usage[field].as_u64().unwrap_or(0)
}

const USAGE_FIELDS: [&str; 7] = [
    "input_tokens",
    "output_tokens",
    "cache_read_tokens",
    "cache_write_tokens",
    "cache_write_5m_tokens",
    "cache_write_1h_tokens",
    "reasoning_tokens",
];

fn is_zero(usage: &Value) -> bool {
    USAGE_FIELDS.iter().all(|field| count(usage, field) == 0)
}

/// The partition the kernel's `Usage` promises, or why a report breaks it.
fn partition_violation(usage: &Value) -> Option<&'static str> {
    let [input, output, read, write, five_minute, one_hour, reasoning] =
        USAGE_FIELDS.map(|field| count(usage, field));
    if read.checked_add(write).is_none_or(|cached| cached > input) {
        return Some("cache read and cache write exceed input_tokens");
    }
    if five_minute.checked_add(one_hour).is_none_or(|tiers| tiers > write) {
        return Some("the cache-write tiers exceed cache_write_tokens");
    }
    if reasoning > output {
        return Some("reasoning_tokens exceeds output_tokens");
    }
    None
}

/// The usage checks for one case. Response and stream cases only.
fn usage_checks(
    case: &CaseV1,
    usage_evidence: UsageEvidenceV1,
    invoke: &dyn Fn(&Value) -> Invoked,
) -> Vec<OutcomeV1> {
    if !matches!(case.family, ProviderFamilyV1::Response | ProviderFamilyV1::Stream) {
        return Vec::new();
    }
    let produced = invoke(&case.input);
    let reports =
        produced.as_ref().map(|output| usage_reports(case.family, output)).unwrap_or_default();
    let mut outcomes = Vec::new();

    let partition = CheckV1::UsagePartition;
    outcomes.push(reports.iter().find_map(|usage| partition_violation(usage)).map_or_else(
        || OutcomeV1::passed(partition, &case.name),
        |violation| OutcomeV1::failed(partition, &case.name, violation),
    ));

    match usage_evidence {
        UsageEvidenceV1::Absent => {
            let check = CheckV1::AbsentFamilyEmitsNoUsage;
            let emitted = match case.family {
                ProviderFamilyV1::Stream => !reports.is_empty(),
                _ => reports.iter().any(|usage| !is_zero(usage)),
            };
            outcomes.push(if emitted {
                OutcomeV1::failed(
                    check,
                    &case.name,
                    "the package declares usage_evidence: absent, yet reported usage; the host \
                     never reads it and would bill an estimate beside a number it was given",
                )
            } else {
                OutcomeV1::passed(check, &case.name)
            });
        }
        UsageEvidenceV1::Reported => {
            if USAGE_ROWS.contains(&case.name.as_str()) {
                outcomes.push(usage_row(case));
            }
            if let Some(pointer) = &case.usage_pointer {
                outcomes.push(usage_never_defaulted(case, pointer, invoke));
            }
        }
    }
    outcomes
}

/// A named usage row shows what its name says. Judged on the fixture's
/// expected output, which `FixtureMatch` already ties to the component.
fn usage_row(case: &CaseV1) -> OutcomeV1 {
    let check = CheckV1::UsageRows;
    let reports = usage_reports(case.family, &case.expected);
    let problem = match case.name.as_str() {
        "provider.response.usage" if reports.iter().all(|usage| is_zero(usage)) => {
            Some("expects no non-zero usage")
        }
        "provider.response.usage" if case.usage_pointer.is_none() => {
            Some("carries no usage_pointer sidecar, so UsageNeverDefaulted has nothing to delete")
        }
        "provider.response.missing-usage" => match expected_error(case) {
            Some(error) if error["code"] == "provider_protocol_error" => None,
            _ => Some("must expect a provider_protocol_error, never a zero"),
        },
        "provider.response.cached-usage"
            if !reports.iter().any(|usage| {
                count(usage, "cache_read_tokens") > 0 || count(usage, "cache_write_tokens") > 0
            }) =>
        {
            Some("expects no cache bucket")
        }
        "provider.stream.usage-terminal" => {
            let events = case.expected.as_array().map(Vec::as_slice).unwrap_or_default();
            let last_usage = events
                .iter()
                .rposition(|event| event["type"] == "usage" && !is_zero(&event["usage"]));
            let done = events.iter().rposition(|event| event["type"] == "done");
            match (last_usage, done) {
                (Some(usage), Some(done)) if usage < done => None,
                _ => Some("must expect a non-zero usage event before the terminal done"),
            }
        }
        "provider.stream.no-usage" if !reports.is_empty() => {
            Some("must expect no usage event: an upstream that sent none is not a zero")
        }
        _ => None,
    };
    problem.map_or_else(
        || OutcomeV1::passed(check, &case.name),
        |problem| OutcomeV1::failed(check, &case.name, problem),
    )
}

/// Deletes what `pointer` names in the upstream body and requires a protocol
/// error.
fn usage_never_defaulted(
    case: &CaseV1,
    pointer: &str,
    invoke: &dyn Fn(&Value) -> Invoked,
) -> OutcomeV1 {
    let check = CheckV1::UsageNeverDefaulted;
    let failed = |detail: String| OutcomeV1::failed(check, &case.name, detail);

    let Some(mut body) =
        case.input["body"].as_str().and_then(|body| serde_json::from_str::<Value>(body).ok())
    else {
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
    let mut mutated = case.input.clone();
    mutated["body"] = Value::String(body.to_string());

    match invoke(&mutated) {
        Err(Failure::Component { envelope, .. })
            if envelope["code"] == "provider_protocol_error" =>
        {
            OutcomeV1::passed(check, &case.name)
        }
        Err(failure) => failed(format!(
            "without its usage the response was refused, but not as a provider protocol error: \
             {}",
            failure.detail()
        )),
        Ok(_) => failed(format!(
            "with `{pointer}` deleted the component still produced a response; usage is funds \
             evidence and a missing report must be an error, never a zero"
        )),
    }
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
    if let Some(expected) = expected_error(case) {
        return match actual {
            Err(Failure::Component { envelope, .. }) if envelope == expected => {
                OutcomeV1::passed(CheckV1::FixtureMatch, &case.name)
            }
            Err(Failure::Component { envelope, .. }) => OutcomeV1::failed(
                CheckV1::FixtureMatch,
                &case.name,
                format!("expected error {}, produced {}", truncate(expected), truncate(envelope)),
            ),
            Err(failure) => OutcomeV1::failed(CheckV1::FixtureMatch, &case.name, failure.detail()),
            Ok(actual) => OutcomeV1::failed(
                CheckV1::FixtureMatch,
                &case.name,
                format!("expected error {}, produced {}", truncate(expected), truncate(actual)),
            ),
        };
    }
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

    let mutated = invoke(&mutated);
    if let Some(expected) = expected_error(case) {
        // A case that expects a refusal must keep refusing for the same reason;
        // the unknown field may neither rescue it nor change why.
        return match mutated {
            Err(Failure::Component { envelope, .. }) if envelope == *expected => {
                OutcomeV1::passed(check, &case.name)
            }
            _ => OutcomeV1::failed(
                check,
                &case.name,
                "a field this version does not model changed how the component refused this input",
            ),
        };
    }
    match mutated {
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
