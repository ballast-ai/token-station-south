//! Gate ② for the speech world: `south.speech-component.v1` (speech record §11).
//!
//! Capabilities and prepare cases feed one component function each. A response case names the
//! prepare case it answers: the suite prepares that case, builds the response view a host would —
//! the declared body form and elision paths, through `build_media_response_view_v1`, or for the
//! `sse` form `build_media_sse_response_view_v1` over the body's events — and hands it to
//! `parse-response`, so a fixture pins what a host would see, not a view the fixture author wrote
//! by hand. An `sse` body is split into events with this crate's [`crate::sse_split`] splitter,
//! which `openai_responses_vocabulary_v1` holds equal to the host's `decode_sse_v1`; a host's own
//! decoding is the gate ③ obligation, with its vectors in `south-contracts`
//! (`tests/vectors/speech-v1.json`).
//!
//! The rows carried over from the image world (image record §12.1) are coverage, fixture match,
//! determinism, unknown-field tolerance, undeclared values ignored, descriptor auth within the
//! manifest, endpoint confinement, terminal only from the wire, auth errors not retriable,
//! pre-dispatch refusal, reference integrity and the two metering rows; the speech rows are
//! [`CheckV1::AbsentDurationIsNullOrUnknown`], [`CheckV1::ClientFormatRendering`],
//! [`CheckV1::UndeclaredOperationRefused`], [`CheckV1::SsmlEscaping`],
//! [`CheckV1::SecondHopArtifact`] and [`CheckV1::EmptyAudioIsUnknown`].
//!
//! The suite reads two northbound fields the host's product surface defines, never a provider's:
//! the speech request's `input` and `voice` (for SSML escaping), and the transcription request's
//! `response_format` part (for client format rendering; absent is `json`, as the `OpenAI` audio
//! API has it).
//!
//! The suite always runs with the package's manifest: its `auth_arms` are what
//! `DescriptorAuthWithinManifest` judges, its `query_parameters` the only names besides the
//! sanctioned ones a prepared call's query may carry (`EndpointConfinement`, `contracts.media` 3),
//! its declared keys what `UndeclaredValuesIgnored` stays clear of, its `secret_headers` what the
//! response view drops, and its `artifact_fetch` what `SecondHopArtifact` requires.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};
use south_contracts::media::{
    ArtifactUrlV1, MediaBodyV1, MediaLimitsV1, ResponseBodyFormV1, SseEventFieldsV1,
    UpstreamRoundV1, build_media_response_view_v1, build_media_sse_response_view_v1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechArtifactFormV1, SpeechCallContextV1, SpeechCharacterSourceV1,
    SpeechMeteringFormV1, SpeechMeteringV1, SpeechModelCapabilitiesV1, SpeechOperationV1,
    TranscriptDeliveryV1, check_required_metering_v1, parse_prepared_speech_call_with_declared_v1,
    upgrade_insecure_artifact_url_v1,
};
use south_provider_api::ComponentManifestV1;
use token_station_protocol::{ErrorCode, ErrorEnvelope, ProviderConfig};

use crate::speech_fixture::{SpeechCaseV1, SpeechFamilyV1, SpeechFixturePackV1};
use crate::speech_json as wire;
use crate::sse_split::SseSplitterV1;
use crate::{
    CheckV1, OutcomeV1, ReportV1, SpeechComponentV1, SpeechDeliveryV1, SpeechOutcomeV1,
    admit_media_descriptor_auth, credential_recipe_checks_v1,
};

/// The suite identifier, equal to the manifest's `conformance.required_suite`.
pub const SPEECH_COMPONENT_SUITE_V1: &str = south_provider_api::SPEECH_BEHAVIOR_SUITE;

/// The key injected to prove a component tolerates a newer host's field.
const UNKNOWN_FIELD: &str = "__conformance_unknown_field";
/// The placeholder namespace no output may carry outside a transcript reference.
const RESERVED_PREFIX: &str = "$south.";
/// The XML special characters: a northbound `voice` holding one must reach an attribute value
/// escaped (speech world batch V4 §4.4, SV-Q11); `ssml_escaping` requires the first four in both
/// the input and the voice of one case (record §5).
const XML_SPECIALS: [char; 5] = ['<', '&', '\'', '"', '>'];
/// The client formats `client_format_rendering` judges (§9).
const CLIENT_FORMATS: [&str; 5] = ["json", "text", "verbose_json", "srt", "vtt"];
/// The operations a model may declare (§4).
const OPERATIONS: [SpeechOperationV1; 3] =
    [SpeechOperationV1::Synthesize, SpeechOperationV1::Transcribe, SpeechOperationV1::Translate];

/// What one case produced as JSON; `Err` is a fixture or boundary failure, never the component's
/// own refusal, which is a value (`{"error": …}`).
type Invoked = Result<Value, String>;

fn parse<T: for<'de> Deserialize<'de>>(value: &Value) -> Result<T, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("fixture input has the wrong shape: {error}"))
}

fn refused(error: &ErrorEnvelope) -> Value {
    json!({ "error": error })
}

#[derive(Deserialize)]
struct CapabilitiesInput {
    provider_config: ProviderConfig,
}

#[derive(Deserialize)]
struct PrepareInput {
    provider_config: ProviderConfig,
    request: Value,
    context: SpeechCallContextV1,
}

#[derive(Deserialize)]
struct UpstreamInput {
    status: u16,
    #[serde(default)]
    headers: serde_json::Map<String, Value>,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    body_text: Option<String>,
    #[serde(default)]
    body_base64: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
}

#[derive(Deserialize)]
struct ResponseInput {
    prepare_case: String,
    response: Value,
}

/// One response case run end to end.
struct Answered {
    input: PrepareInput,
    prepared: PreparedSpeechCallV1,
    /// The upstream response as the fixture wrote it (for grouping by upstream answer).
    upstream: Value,
    /// The upstream body as the upstream sent it, when it is JSON (for `url` pointers).
    raw_body: Value,
    /// The view handed to the component.
    view: Value,
    outcome: Result<SpeechOutcomeV1, ErrorEnvelope>,
}

/// The suite's view of one run.
struct Suite<'a> {
    component: &'a dyn SpeechComponentV1,
    pack: &'a SpeechFixturePackV1,
    manifest: &'a ComponentManifestV1,
    /// The manifest's declared query parameters, as a host builds them once at load.
    declared_query: Vec<south_contracts::DeclaredQueryParameterV1>,
}

fn host_unknown(reason: &str) -> SpeechOutcomeV1 {
    SpeechOutcomeV1::Unknown { reason: format!("host: {reason}"), error: None }
}

/// An `sse` body split into events as a host's `decode_sse_v1` would.
fn sse_events(body: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut splitter = SseSplitterV1::new();
    let mut frames = splitter.push(body).map_err(|error| format!("{error:?}"))?;
    frames.extend(splitter.finish().map_err(|error| format!("{error:?}"))?);
    Ok(frames.into_iter().map(|frame| (frame.event, frame.data)).collect())
}

impl Suite<'_> {
    fn prepare(&self, input: &PrepareInput) -> Result<PreparedSpeechCallV1, ErrorEnvelope> {
        self.component.prepare(&input.provider_config, &input.request, &input.context)
    }

    fn prepare_case(&self, name: &str) -> Result<(PrepareInput, PreparedSpeechCallV1), String> {
        let case =
            self.pack.case(name).ok_or_else(|| format!("names the missing case `{name}`"))?;
        if case.family != SpeechFamilyV1::Prepare {
            return Err(format!("names `{name}`, which is not a prepare case"));
        }
        let input: PrepareInput = parse(&case.input)?;
        let prepared = self
            .prepare(&input)
            .map_err(|error| format!("the prepare case `{name}` is refused: {}", error.message))?;
        Ok((input, prepared))
    }

    fn answer(&self, input: &Value) -> Result<Answered, String> {
        let response: ResponseInput = parse(input)?;
        let (input, prepared) = self.prepare_case(&response.prepare_case)?;
        let upstream: UpstreamInput = parse(&response.response)?;
        let (raw_body, body_bytes) =
            match (&upstream.body, &upstream.body_text, &upstream.body_base64) {
                (Some(body), None, None) => {
                    (body.clone(), serde_json::to_vec(body).map_err(|error| error.to_string())?)
                }
                (None, Some(text), None) => (Value::Null, text.as_bytes().to_vec()),
                (None, None, Some(encoded)) => (
                    Value::Null,
                    south_contracts::media::decode_base64(encoded)
                        .map_err(|error| error.to_string())?,
                ),
                (None, None, None) => (Value::Null, Vec::new()),
                _ => {
                    return Err(
                        "a response carries more than one of `body`, `body_text`, `body_base64`"
                            .into(),
                    );
                }
            };
        let headers: Vec<(String, String)> = upstream
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), value.as_str().unwrap_or_default().to_owned()))
            .collect();
        let secret_headers: Vec<&str> =
            self.manifest.secret_headers.iter().map(String::as_str).collect();
        let round = UpstreamRoundV1 {
            status: upstream.status,
            headers: &headers,
            body: &body_bytes,
            content_type: upstream.content_type.as_deref(),
        };
        let sse = prepared.response_body_form == ResponseBodyFormV1::Sse
            && (200..300).contains(&upstream.status);
        let built = if sse {
            match sse_events(&body_bytes) {
                Ok(events) => {
                    let fields: Vec<SseEventFieldsV1<'_>> = events
                        .iter()
                        .map(|(event, data)| SseEventFieldsV1 { event, data })
                        .collect();
                    build_media_sse_response_view_v1(
                        round,
                        &fields,
                        &secret_headers,
                        &prepared.response_elision_paths,
                        &MediaLimitsV1::V1,
                    )
                    .map_err(|error| error.to_string())
                }
                Err(error) => Err(format!("the sse body does not decode: {error}")),
            }
        } else {
            build_media_response_view_v1(
                round,
                &secret_headers,
                prepared.response_body_form,
                &prepared.response_elision_paths,
                &MediaLimitsV1::V1,
            )
            .map_err(|error| error.to_string())
        };
        let (view, outcome) = match built {
            Ok((view, _)) => {
                let view: Value =
                    serde_json::from_str(&view.to_json()).map_err(|error| error.to_string())?;
                let state: Value =
                    serde_json::from_str(&prepared.state).map_err(|error| error.to_string())?;
                let outcome = self.component.parse_response(&state, &view);
                (view, outcome)
            }
            // The host judges this round `unknown` without asking the component.
            Err(error) => (Value::Null, Ok(host_unknown(&error))),
        };
        Ok(Answered { input, prepared, upstream: response.response, raw_body, view, outcome })
    }

    fn invoke(&self, case: &SpeechCaseV1, input: &Value) -> Invoked {
        match case.family {
            SpeechFamilyV1::Capabilities => {
                let input: CapabilitiesInput = parse(input)?;
                Ok(match self.component.model_capabilities(&input.provider_config) {
                    Ok(models) => {
                        serde_json::to_value(models).map_err(|error| error.to_string())?
                    }
                    Err(error) => refused(&error),
                })
            }
            SpeechFamilyV1::Prepare => {
                let input: PrepareInput = parse(input)?;
                Ok(match self.prepare(&input) {
                    Ok(prepared) => serde_json::from_str(&prepared.to_json())
                        .map_err(|error| error.to_string())?,
                    Err(error) => refused(&error),
                })
            }
            SpeechFamilyV1::Response => {
                let answered = self.answer(input)?;
                Ok(match &answered.outcome {
                    Ok(outcome) => serde_json::from_str(&wire::speech_outcome_json(outcome)?)
                        .map_err(|error| error.to_string())?,
                    Err(error) => refused(error),
                })
            }
        }
    }
}

/// Runs `south.speech-component.v1` for one package.
#[must_use]
pub fn run_speech_component_suite_v1(
    component: &dyn SpeechComponentV1,
    pack: &SpeechFixturePackV1,
    manifest: &ComponentManifestV1,
) -> ReportV1 {
    let declared_query = crate::DeclaredInstancesV1::from_manifest(manifest)
        .map(|instances| instances.query_parameters().to_vec())
        .unwrap_or_default();
    let suite = Suite { component, pack, manifest, declared_query };
    let mut outcomes = coverage(pack);
    let mut declared_models: Vec<SpeechModelCapabilitiesV1> = Vec::new();
    let mut tally = Tally::default();

    for case in pack.cases() {
        let first = suite.invoke(case, &case.input);
        outcomes.push(fixture_match(case, &first));
        outcomes.push(if first == suite.invoke(case, &case.input) {
            OutcomeV1::passed(CheckV1::Determinism, &case.name)
        } else {
            OutcomeV1::failed(
                CheckV1::Determinism,
                &case.name,
                "the same input produced different output twice",
            )
        });
        if let Ok(output) = &first {
            outcomes.push(no_reserved_keys(case, output));
        }
        match case.family {
            SpeechFamilyV1::Capabilities => {
                if let Ok(models) = first.as_ref().map(|value| {
                    serde_json::from_value::<Vec<SpeechModelCapabilitiesV1>>(value.clone())
                }) {
                    declared_models.extend(models.unwrap_or_default());
                }
            }
            SpeechFamilyV1::Prepare => {
                outcomes.push(unknown_field_tolerance(&suite, case, &first, "/context"));
                outcomes.push(unknown_field_tolerance(&suite, case, &first, "/request"));
                outcomes.push(undeclared_values_ignored(&suite, case, &first));
                outcomes.extend(prepare_checks(&suite, case, &first, &mut tally));
            }
            SpeechFamilyV1::Response => {
                outcomes.extend(tally_response(&suite, case, &mut tally));
            }
        }
    }

    outcomes.extend(suite_rows(&suite, &declared_models, &tally));
    if let Some(credentials) = &manifest.credentials {
        outcomes.extend(credential_recipe_checks_v1(credentials, pack.credentials()));
    }
    ReportV1::new(SPEECH_COMPONENT_SUITE_V1, outcomes)
}

fn coverage(pack: &SpeechFixturePackV1) -> Vec<OutcomeV1> {
    SpeechFamilyV1::ALL
        .iter()
        .map(|family| {
            let name = format!("speech-v1.{}", family.token());
            if pack.cases().iter().any(|case| case.family == *family) {
                OutcomeV1::passed(CheckV1::Coverage, name)
            } else {
                OutcomeV1::failed(
                    CheckV1::Coverage,
                    name,
                    "the pack carries no case of this family",
                )
            }
        })
        .collect()
}

fn fixture_match(case: &SpeechCaseV1, first: &Invoked) -> OutcomeV1 {
    match first {
        Ok(output) if *output == case.expected => {
            OutcomeV1::passed(CheckV1::FixtureMatch, &case.name)
        }
        Ok(output) => OutcomeV1::failed(
            CheckV1::FixtureMatch,
            &case.name,
            format!("expected {}, got {output}", case.expected),
        ),
        Err(detail) => OutcomeV1::failed(CheckV1::FixtureMatch, &case.name, detail.clone()),
    }
}

/// `ReferenceIntegrity` on any output: no `$south.` key other than a transcript's `$south.ref`
/// (judged against the view in [`response_checks`]), and no string above the fallback threshold.
fn no_reserved_keys(case: &SpeechCaseV1, output: &Value) -> OutcomeV1 {
    fn walk(value: &Value) -> Option<String> {
        match value {
            Value::String(text) if text.len() > MediaLimitsV1::V1.inline_string_bytes => {
                Some("an output string exceeds the fallback threshold".into())
            }
            Value::Array(items) => items.iter().find_map(walk),
            Value::Object(map) => map.iter().find_map(|(key, value)| {
                if key.starts_with(RESERVED_PREFIX) && key != "$south.ref" {
                    return Some(format!("an output carries the reserved key `{key}`"));
                }
                walk(value)
            }),
            _ => None,
        }
    }
    walk(output).map_or_else(
        || OutcomeV1::passed(CheckV1::ReferenceIntegrity, &case.name),
        |detail| OutcomeV1::failed(CheckV1::ReferenceIntegrity, &case.name, detail),
    )
}

fn unknown_field_tolerance(
    suite: &Suite<'_>,
    case: &SpeechCaseV1,
    first: &Invoked,
    at: &str,
) -> OutcomeV1 {
    let mut mutated = case.input.clone();
    let Some(Value::Object(map)) = mutated.pointer_mut(at) else {
        return OutcomeV1::failed(
            CheckV1::UnknownFieldTolerance,
            &case.name,
            format!("no object at `{at}`"),
        );
    };
    map.insert(UNKNOWN_FIELD.to_owned(), json!("tolerate me"));
    if suite.invoke(case, &mutated) == *first {
        OutcomeV1::passed(CheckV1::UnknownFieldTolerance, &case.name)
    } else {
        OutcomeV1::failed(
            CheckV1::UnknownFieldTolerance,
            &case.name,
            format!("a field this ABI does not model, added at `{at}`, changed the answer"),
        )
    }
}

/// `UndeclaredValuesIgnored` on a prepare case: the speech world's value channel (§17.3), judged
/// by the row the embeddings and image worlds share.
fn undeclared_values_ignored(suite: &Suite<'_>, case: &SpeechCaseV1, first: &Invoked) -> OutcomeV1 {
    crate::undeclared_values::undeclared_values_ignored(
        &case.name,
        &case.input,
        first,
        suite.manifest,
        &|mutated| suite.invoke(case, mutated),
    )
}

/// Every blob id a request view holds: `$south.blob` placeholders and multipart blobs.
fn view_blobs(view: &Value, ids: &mut BTreeSet<String>) {
    match view {
        Value::Object(map) => {
            if let Some(id) = map.get("$south.blob").and_then(|blob| blob["id"].as_str()) {
                ids.insert(id.to_owned());
            }
            if let Some(id) = map.get("blob").and_then(Value::as_str) {
                ids.insert(id.to_owned());
            }
            for value in map.values() {
                view_blobs(value, ids);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| view_blobs(item, ids)),
        _ => {}
    }
}

fn is_refusal_code(error: &Value) -> bool {
    serde_json::from_value::<ErrorEnvelope>(error.clone())
        .is_ok_and(|error| matches!(error.code, ErrorCode::InvalidRequest | ErrorCode::Capability))
}

/// The client format a recognition request view asks for: its first `response_format` text part
/// trimmed of ASCII whitespace, or `json` when it has none or it is empty after trimming (the
/// `OpenAI` audio API's default). A package that compares the value exactly refuses a padded one
/// before dispatch, so the trim only matters to one that trims as its native arm does (V3-3 record
/// §10 decision 3, `speech-azure`).
fn client_format(request: &Value) -> Option<String> {
    let parts = request.pointer("/multipart/parts")?.as_array()?;
    let format = parts
        .iter()
        .find(|part| part["kind"] == "text" && part["name"] == "response_format")
        .and_then(|part| part["value"].as_str())
        .map_or("", |value| value.trim_matches(|c: char| c.is_ascii_whitespace()));
    Some(if format.is_empty() { "json" } else { format }.to_owned())
}

/// Whether the model row the context routes to carries the dialect word `speech.verbose_json`.
fn declares_verbose_json(input: &PrepareInput) -> bool {
    input.provider_config.models.iter().any(|model| {
        model.model == input.context.upstream_model
            && model.supported_parameters.contains("speech.verbose_json")
    })
}

/// `PreDispatchRefusal`, `ReferenceIntegrity` (descriptor references), `EndpointConfinement`,
/// `DescriptorAuthWithinManifest`, `UndeclaredOperationRefused` and `SsmlEscaping` on a prepare
/// case.
fn prepare_checks(
    suite: &Suite<'_>,
    case: &SpeechCaseV1,
    first: &Invoked,
    tally: &mut Tally,
) -> Vec<OutcomeV1> {
    let Ok(output) = first else { return Vec::new() };
    let Ok(input) = parse::<PrepareInput>(&case.input) else { return Vec::new() };
    if let Some(error) = output.get("error") {
        tally.prepare_refusal = true;
        return vec![if is_refusal_code(error) {
            OutcomeV1::passed(CheckV1::PreDispatchRefusal, &case.name)
        } else {
            OutcomeV1::failed(
                CheckV1::PreDispatchRefusal,
                &case.name,
                "a prepare refusal must be `invalid_request` or `capability`",
            )
        }];
    }
    let Ok(prepared) = suite.prepare(&input) else { return Vec::new() };
    let mut outcomes = Vec::new();
    let mut ids = BTreeSet::new();
    view_blobs(&input.request, &mut ids);
    let dangling: Vec<String> = prepared
        .descriptor
        .referenced_blobs()
        .into_iter()
        .map(|id| id.as_str().to_owned())
        .filter(|id| !ids.contains(id))
        .collect();
    outcomes.push(if dangling.is_empty() {
        OutcomeV1::passed(CheckV1::ReferenceIntegrity, &case.name)
    } else {
        OutcomeV1::failed(
            CheckV1::ReferenceIntegrity,
            &case.name,
            format!("the descriptor references blobs the view does not hold: {dangling:?}"),
        )
    });
    // The prepared call as a host parses it: with the package's own declared query parameters
    // and no others (`contracts.media` 3, V3-1 record §6, host obligations 1 and 2). A descriptor
    // that names a query parameter its package does not declare is refused before admission.
    outcomes.push(
        match parse_prepared_speech_call_with_declared_v1(
            &prepared.to_json(),
            &MediaLimitsV1::V1,
            &suite.declared_query,
        ) {
            Ok(_) => OutcomeV1::passed(CheckV1::EndpointConfinement, &case.name),
            Err(error) => OutcomeV1::failed(
                CheckV1::EndpointConfinement,
                &case.name,
                format!(
                    "a host refuses the prepared call under the package's declarations: {error}"
                ),
            ),
        },
    );
    outcomes.push(
        match admit_media_descriptor_auth(
            suite.manifest,
            &input.provider_config,
            &prepared.descriptor,
        ) {
            Ok(_) => OutcomeV1::passed(CheckV1::DescriptorAuthWithinManifest, &case.name),
            Err(refusal) => OutcomeV1::failed(
                CheckV1::DescriptorAuthWithinManifest,
                &case.name,
                refusal.to_string(),
            ),
        },
    );
    let missing: Vec<&SpeechMeteringFormV1> = input
        .context
        .metering_required
        .iter()
        .filter(|form| !prepared.facts.metering_forms.contains(form))
        .collect();
    if !missing.is_empty() {
        outcomes.push(OutcomeV1::failed(
            CheckV1::MeteringSample,
            &case.name,
            format!("the facts do not echo the required metering forms {missing:?}"),
        ));
    }
    outcomes.extend(undeclared_operations(suite, case, &input, &prepared, tally));
    if let MediaBodyV1::Text(body) = prepared.descriptor.body() {
        outcomes.push(ssml_check(case, &input.request, &body.text, tally));
    }
    if prepared.facts.operation.is_recognition()
        && declares_verbose_json(&input)
        && let Some(format) = client_format(&input.request)
    {
        tally.accepted_formats.insert(format);
    }
    outcomes
}

/// `UndeclaredOperationRefused`: the prepared call names the operation it was asked for, and
/// asking the same case for each operation the model does not declare is refused before any
/// descriptor (§4, §11).
fn undeclared_operations(
    suite: &Suite<'_>,
    case: &SpeechCaseV1,
    input: &PrepareInput,
    prepared: &PreparedSpeechCallV1,
    tally: &mut Tally,
) -> Vec<OutcomeV1> {
    let check = CheckV1::UndeclaredOperationRefused;
    let mut outcomes = Vec::new();
    if prepared.facts.operation != input.context.operation {
        outcomes.push(OutcomeV1::failed(
            check,
            &case.name,
            format!(
                "asked to {:?}, the component prepared {:?}",
                input.context.operation, prepared.facts.operation
            ),
        ));
    }
    let declared =
        suite.component.model_capabilities(&input.provider_config).ok().and_then(|models| {
            models.into_iter().find(|model| model.model == input.context.upstream_model)
        });
    let Some(model) = declared else { return outcomes };
    if !model.operations.contains(&input.context.operation) {
        outcomes.push(OutcomeV1::failed(
            check,
            &case.name,
            "prepared an operation the model does not declare",
        ));
    }
    for operation in OPERATIONS.iter().filter(|operation| !model.operations.contains(operation)) {
        tally.undeclared_tried += 1;
        let mut mutated = case.input.clone();
        if let Some(slot) = mutated.pointer_mut("/context/operation") {
            *slot = json!(operation);
        }
        let row = format!("{}.{}", case.name, json!(operation).as_str().unwrap_or_default());
        outcomes.push(match suite.invoke(case, &mutated) {
            Ok(answer) if answer.get("error").is_some_and(is_refusal_code) => {
                OutcomeV1::passed(check, row)
            }
            Ok(_) => OutcomeV1::failed(
                check,
                row,
                format!(
                    "asked to {operation:?} on a model that does not declare it, the component \
                     did not refuse with `invalid_request` or `capability`"
                ),
            ),
            Err(detail) => OutcomeV1::failed(check, row, detail),
        });
    }
    outcomes
}

/// `SsmlEscaping` on a prepare case whose descriptor carries an SSML body, as speech world batch
/// V4 reads it (SV-Q11, ruled 2026-10-15): the document is well-formed XML 1.0 (no character the
/// `Char` production forbids, raw or as a reference); the northbound `input`, with those forbidden
/// characters removed, is some element's whole text; and a northbound `voice` that holds an XML
/// special character (`<`, `&`, `'`, `"`, `>`) appears, with the forbidden characters likewise
/// removed, as a substring of some attribute value. A voice without one is not checked: its
/// escaping is the identity, and mapping it to an upstream voice id is the component's business.
fn ssml_check(case: &SpeechCaseV1, request: &Value, ssml: &str, tally: &mut Tally) -> OutcomeV1 {
    let check = CheckV1::SsmlEscaping;
    tally.ssml = true;
    let Some(text) = request.pointer("/json/input").and_then(Value::as_str) else {
        return OutcomeV1::failed(check, &case.name, "the request has no `input` string");
    };
    let voice = request.pointer("/json/voice").and_then(Value::as_str);
    let document = match crate::ssml_check::parse_xml(ssml) {
        Ok(document) => document,
        Err(detail) => {
            return OutcomeV1::failed(
                check,
                &case.name,
                format!("the SSML is not well-formed: {detail}"),
            );
        }
    };
    let carried = crate::ssml_check::without_forbidden(text);
    if !document.texts.contains(&carried) {
        return OutcomeV1::failed(
            check,
            &case.name,
            "the SSML does not carry `input` (less the characters XML forbids) as an element's text",
        );
    }
    if let Some(voice) = voice.filter(|voice| XML_SPECIALS.iter().any(|c| voice.contains(*c))) {
        let carried = crate::ssml_check::without_forbidden(voice);
        if !document.attributes.iter().any(|value| value.contains(carried.as_str())) {
            return OutcomeV1::failed(
                check,
                &case.name,
                "the SSML does not carry `voice`, which holds an XML special character, inside an \
                 attribute value",
            );
        }
        if ['<', '&', '\'', '"'].iter().all(|c| text.contains(*c) && voice.contains(*c)) {
            tally.ssml_specials = true;
        }
    }
    OutcomeV1::passed(check, &case.name)
}

fn auth_not_retriable(case: &SpeechCaseV1, answered: &Answered) -> OutcomeV1 {
    let error = match &answered.outcome {
        Ok(SpeechOutcomeV1::Rejected { error } | SpeechOutcomeV1::ChargedFailure { error, .. }) => {
            Some(error)
        }
        Ok(SpeechOutcomeV1::Unknown { error, .. }) => error.as_ref(),
        Err(error) => Some(error),
        Ok(SpeechOutcomeV1::Succeeded { .. }) => None,
    };
    match error {
        Some(error) if !error.code.is_retriable_elsewhere() => {
            OutcomeV1::passed(CheckV1::AuthErrorsAreNotRetriable, &case.name)
        }
        _ => OutcomeV1::failed(
            CheckV1::AuthErrorsAreNotRetriable,
            &case.name,
            "a 401 or 403 mapped to a retriable error, or to none",
        ),
    }
}

/// Every `$south.blob` id a response view holds.
fn response_blobs(view: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    view_blobs(view, &mut ids);
    ids
}

/// The JSON tree an `inline` or `segments` pointer addresses: the body's `json` or `sse` member.
fn pointer_root(view: &Value) -> Option<&Value> {
    view.pointer("/body/json").or_else(|| view.pointer("/body/sse"))
}

/// `ReferenceIntegrity` and `SecondHopArtifact` on a succeeded audio delivery.
fn audio_checks(
    suite: &Suite<'_>,
    case: &SpeechCaseV1,
    answered: &Answered,
    form: &SpeechArtifactFormV1,
    fits: bool,
) -> Vec<OutcomeV1> {
    let mut outcomes = Vec::new();
    let integrity =
        |detail: &str| OutcomeV1::failed(CheckV1::ReferenceIntegrity, &case.name, detail);
    if !fits {
        outcomes.push(integrity("the artifact does not fit the declared response body form"));
    }
    let root = pointer_root(&answered.view);
    let at = |pointer: &str| root.and_then(|root| root.pointer(pointer));
    let elided = |value: &Value| value.get("$south.blob").is_some();
    match form {
        SpeechArtifactFormV1::Body => {
            if answered.view.pointer("/body/opaque").is_none() {
                outcomes.push(integrity("a `body` artifact on a view with no opaque body"));
            }
        }
        SpeechArtifactFormV1::Inline { pointer, .. } => {
            if !at(pointer.as_str()).is_some_and(|value| value.is_string() || elided(value)) {
                outcomes.push(integrity("an `inline` pointer lands on no string of the response"));
            }
        }
        SpeechArtifactFormV1::Segments { items } => {
            // §6: the fallback threshold does not catch small deltas, so every fragment must be one
            // the component declared for elision.
            if !items.iter().all(|item| at(item.pointer.as_str()).is_some_and(elided)) {
                outcomes
                    .push(integrity("a `segments` pointer lands on a string that is not elided"));
            }
        }
        SpeechArtifactFormV1::Url { pointer, upgrade_insecure } => {
            let check = CheckV1::SecondHopArtifact;
            let url = answered.raw_body.pointer(pointer.as_str()).and_then(Value::as_str);
            // SV-Q10 (speech world batch V4 §2.5): the host's one rewrite, then the unchanged
            // grammar.
            let url = url.map(|url| {
                if *upgrade_insecure {
                    upgrade_insecure_artifact_url_v1(url)
                } else {
                    std::borrow::Cow::Borrowed(url)
                }
            });
            outcomes.push(match url.as_deref().map(ArtifactUrlV1::parse) {
                None => OutcomeV1::failed(check, &case.name, "a `url` pointer lands on no string"),
                Some(Err(error)) => OutcomeV1::failed(
                    check,
                    &case.name,
                    format!("a `url` artifact fails the ArtifactUrlV1 grammar: {error}"),
                ),
                Some(Ok(_)) if !suite.manifest.capabilities.contains("artifact_fetch") => {
                    OutcomeV1::failed(
                        check,
                        &case.name,
                        "a `url` artifact from a package that does not declare `artifact_fetch`",
                    )
                }
                Some(Ok(_)) => OutcomeV1::passed(check, &case.name),
            });
        }
    }
    outcomes
}

/// The host's structural checks on a response case (§9, §10, §11).
fn response_checks(suite: &Suite<'_>, case: &SpeechCaseV1, answered: &Answered) -> Vec<OutcomeV1> {
    let mut outcomes = Vec::new();
    let unknown = matches!(answered.outcome, Ok(SpeechOutcomeV1::Unknown { .. }));
    match case.missing.as_deref() {
        Some(fact @ ("tokens" | "tokens.input" | "tokens.output")) => {
            outcomes.push(if unknown {
                OutcomeV1::passed(CheckV1::MissingMeterIsNotZero, &case.name)
            } else {
                OutcomeV1::failed(
                    CheckV1::MissingMeterIsNotZero,
                    &case.name,
                    format!("a 2xx missing `{fact}` did not end `unknown`"),
                )
            });
        }
        Some("seconds") => {
            let null = matches!(
                &answered.outcome,
                Ok(SpeechOutcomeV1::Succeeded {
                    metering: SpeechMeteringV1 { seconds: None, .. },
                    ..
                })
            );
            outcomes.push(if unknown || null {
                OutcomeV1::passed(CheckV1::AbsentDurationIsNullOrUnknown, &case.name)
            } else {
                OutcomeV1::failed(
                    CheckV1::AbsentDurationIsNullOrUnknown,
                    &case.name,
                    "a 2xx missing the duration did not end with `seconds: null` or `unknown`",
                )
            });
        }
        Some(_) => {
            outcomes.push(if unknown {
                OutcomeV1::passed(CheckV1::EmptyAudioIsUnknown, &case.name)
            } else {
                OutcomeV1::failed(
                    CheckV1::EmptyAudioIsUnknown,
                    &case.name,
                    "a 2xx without audio did not end `unknown`",
                )
            });
        }
        None => {}
    }
    let Ok(SpeechOutcomeV1::Succeeded { delivery, metering }) = &answered.outcome else {
        return outcomes;
    };
    // §9: every required fact of every declared form; a missing token count is never 0.
    if let Err(error) =
        check_required_metering_v1(&answered.prepared.facts.metering_forms, metering)
    {
        outcomes.push(OutcomeV1::failed(
            CheckV1::MissingMeterIsNotZero,
            &case.name,
            format!("`succeeded` without a required fact: {error}"),
        ));
    }
    match delivery {
        SpeechDeliveryV1::Audio(artifact) => {
            let fits = artifact.fits(answered.prepared.response_body_form);
            outcomes.extend(audio_checks(suite, case, answered, &artifact.form, fits));
        }
        SpeechDeliveryV1::Transcript(transcript) => {
            let held = response_blobs(&answered.view);
            let dangling: Vec<String> = transcript
                .referenced_blobs()
                .into_iter()
                .map(|id| id.as_str().to_owned())
                .filter(|id| !held.contains(id))
                .collect();
            if !dangling.is_empty() {
                outcomes.push(OutcomeV1::failed(
                    CheckV1::ReferenceIntegrity,
                    &case.name,
                    format!("the transcript references blobs the view does not hold: {dangling:?}"),
                ));
            }
            if let Some(format) = client_format(&answered.input.request)
                && !delivery_fits_format(transcript, &format)
            {
                outcomes.push(OutcomeV1::failed(
                    CheckV1::ClientFormatRendering,
                    &case.name,
                    format!(
                        "the client asked for `{format}` and received another kind of document"
                    ),
                ));
            }
        }
    }
    outcomes
}

/// Whether a transcript delivery is the kind of document a client format names: `json` and
/// `verbose_json` are JSON (a template, or the upstream's own JSON), `text`, `srt` and `vtt` are
/// text (rendered by the component, or the upstream's own text).
fn delivery_fits_format(transcript: &TranscriptDeliveryV1, format: &str) -> bool {
    match transcript {
        TranscriptDeliveryV1::Json { .. } => matches!(format, "json" | "verbose_json"),
        // The upstream's own document: JSON for the JSON formats, anything else for the others.
        TranscriptDeliveryV1::Passthrough { media_type } => {
            matches!(format, "json" | "verbose_json") == (media_type == "application/json")
        }
        TranscriptDeliveryV1::Text { .. } => matches!(format, "text" | "srt" | "vtt"),
    }
}

/// What the response cases showed across the pack.
#[derive(Default)]
#[allow(clippy::struct_excessive_bools, reason = "one flag per pack-wide row, read once")]
struct Tally {
    characters_sources: BTreeSet<&'static str>,
    sampled_seconds: bool,
    sampled_tokens: bool,
    missing_facts: BTreeSet<String>,
    /// The response body forms of successful syntheses, and of the empty-audio cases.
    audio_forms: BTreeSet<&'static str>,
    empty_audio_forms: BTreeSet<&'static str>,
    url_artifact: bool,
    non_2xx: bool,
    auth_refusal: bool,
    prepare_refusal: bool,
    undeclared_tried: usize,
    ssml: bool,
    ssml_specials: bool,
    /// The client formats a `speech.verbose_json` model's prepare cases accept.
    accepted_formats: BTreeSet<String>,
    /// `(upstream response, model)` -> client format -> the `seconds` value it settled with.
    renderings: BTreeMap<(String, String), BTreeMap<String, Option<String>>>,
}

const fn form_word(form: ResponseBodyFormV1) -> &'static str {
    match form {
        ResponseBodyFormV1::Json => "json",
        ResponseBodyFormV1::Binary => "binary",
        ResponseBodyFormV1::Text => "text",
        ResponseBodyFormV1::Sse => "sse",
    }
}

/// One response case's per-case checks, recording what the pack-wide rows need.
fn tally_response(suite: &Suite<'_>, case: &SpeechCaseV1, tally: &mut Tally) -> Vec<OutcomeV1> {
    let Ok(answered) = suite.answer(&case.input) else {
        return Vec::new();
    };
    let mut outcomes = Vec::new();
    let status = case.input.pointer("/response/status").and_then(Value::as_u64).unwrap_or(0);
    if !(200..300).contains(&status) {
        tally.non_2xx = true;
        outcomes.push(match &answered.outcome {
            Ok(SpeechOutcomeV1::Succeeded { .. }) => OutcomeV1::failed(
                CheckV1::TerminalOnlyFromTheWire,
                &case.name,
                "a non-2xx produced `succeeded`",
            ),
            _ => OutcomeV1::passed(CheckV1::TerminalOnlyFromTheWire, &case.name),
        });
        if matches!(status, 401 | 403) {
            tally.auth_refusal = true;
            outcomes.push(auth_not_retriable(case, &answered));
        }
    }
    let form = form_word(answered.prepared.response_body_form);
    if let Some(fact) = &case.missing {
        tally.missing_facts.insert(fact.clone());
        if fact == "audio" {
            tally.empty_audio_forms.insert(form);
        }
    }
    if let Ok(SpeechOutcomeV1::Succeeded { delivery, metering }) = &answered.outcome {
        if let Some(characters) = metering.characters {
            tally.characters_sources.insert(match characters.source {
                SpeechCharacterSourceV1::Upstream => "upstream",
                SpeechCharacterSourceV1::Request => "request",
            });
        }
        tally.sampled_seconds |= metering.seconds.is_some();
        tally.sampled_tokens |=
            metering.tokens.is_some_and(|tokens| tokens.input.is_some() && tokens.output.is_some());
        match delivery {
            SpeechDeliveryV1::Audio(artifact) => {
                tally.audio_forms.insert(form);
                tally.url_artifact |= matches!(artifact.form, SpeechArtifactFormV1::Url { .. });
            }
            SpeechDeliveryV1::Transcript(_) => {
                if declares_verbose_json(&answered.input)
                    && let Some(format) = client_format(&answered.input.request)
                {
                    let key = (
                        answered.upstream.to_string(),
                        answered.input.context.upstream_model.clone(),
                    );
                    let seconds = metering.seconds.map(|seconds| seconds.value.to_string());
                    tally.renderings.entry(key).or_default().insert(format, seconds);
                }
            }
        }
    }
    outcomes.extend(response_checks(suite, case, &answered));
    outcomes
}

fn row(check: CheckV1, name: impl Into<String>, ok: bool, detail: &str) -> OutcomeV1 {
    if ok { OutcomeV1::passed(check, name) } else { OutcomeV1::failed(check, name, detail) }
}

/// The rows the record requires across the pack (§11).
fn suite_rows(
    suite: &Suite<'_>,
    declared_models: &[SpeechModelCapabilitiesV1],
    tally: &Tally,
) -> Vec<OutcomeV1> {
    let mut outcomes = Vec::new();
    let forms: BTreeSet<SpeechMeteringFormV1> =
        declared_models.iter().flat_map(|model| model.metering_forms.iter().copied()).collect();
    let operations: BTreeSet<SpeechOperationV1> =
        declared_models.iter().flat_map(|model| model.operations.iter().copied()).collect();
    let sample = |name: &str, ok: bool| {
        row(
            CheckV1::MeteringSample,
            format!("speech-v1.metering.{name}"),
            ok,
            "no response case succeeds with an exact value of this form",
        )
    };
    if forms.contains(&SpeechMeteringFormV1::Characters) {
        outcomes.push(sample("characters", !tally.characters_sources.is_empty()));
        // §9: an upstream that reports characters may not; that answer is `source = request`.
        if tally.characters_sources.contains("upstream") {
            outcomes
                .push(sample("characters.request", tally.characters_sources.contains("request")));
        }
    }
    if forms.contains(&SpeechMeteringFormV1::Seconds) {
        outcomes.push(sample("seconds", tally.sampled_seconds));
        outcomes.push(row(
            CheckV1::AbsentDurationIsNullOrUnknown,
            "speech-v1.absent.seconds",
            tally.missing_facts.contains("seconds"),
            "no 2xx case is missing the duration",
        ));
    }
    if forms.contains(&SpeechMeteringFormV1::Tokens) {
        outcomes.push(sample("tokens", tally.sampled_tokens));
        for fact in ["tokens", "tokens.input", "tokens.output"] {
            outcomes.push(row(
                CheckV1::MissingMeterIsNotZero,
                format!("speech-v1.missing.{fact}"),
                tally.missing_facts.contains(fact),
                "no 2xx case is missing this required fact",
            ));
        }
    }
    if operations.contains(&SpeechOperationV1::Synthesize) {
        for form in &tally.audio_forms {
            outcomes.push(row(
                CheckV1::EmptyAudioIsUnknown,
                format!("speech-v1.empty-audio.{form}"),
                tally.empty_audio_forms.contains(form),
                "no 2xx case of this response body form carries no audio",
            ));
        }
        if tally.audio_forms.is_empty() {
            outcomes.push(OutcomeV1::failed(
                CheckV1::EmptyAudioIsUnknown,
                "speech-v1.empty-audio",
                "a synthesizing package has no successful synthesis case",
            ));
        }
    }
    if suite.manifest.capabilities.contains("artifact_fetch") {
        outcomes.push(row(
            CheckV1::SecondHopArtifact,
            "speech-v1.second-hop",
            tally.url_artifact,
            "the package declares `artifact_fetch` and no case succeeds with a `url` artifact",
        ));
    }
    if tally.ssml {
        outcomes.push(row(
            CheckV1::SsmlEscaping,
            "speech-v1.ssml.special-characters",
            tally.ssml_specials,
            "no SSML case has `<`, `&`, `'` and `\"` in both the input and the voice",
        ));
    }
    outcomes.push(row(
        CheckV1::UndeclaredOperationRefused,
        "speech-v1.undeclared-operation",
        tally.undeclared_tried > 0,
        "no prepare case names a declared model with an operation it does not declare",
    ));
    if !tally.accepted_formats.is_empty() {
        outcomes.push(client_format_row(tally));
    }
    for (seen, check, detail) in [
        (tally.non_2xx, CheckV1::TerminalOnlyFromTheWire, "no response case is a non-2xx"),
        (
            tally.auth_refusal,
            CheckV1::AuthErrorsAreNotRetriable,
            "no response case is a 401 or 403",
        ),
        (tally.prepare_refusal, CheckV1::PreDispatchRefusal, "no prepare case is refused"),
    ] {
        if !seen {
            outcomes.push(OutcomeV1::failed(check, "speech-v1", detail));
        }
    }
    outcomes
}

/// `client_format_rendering` across the pack: one upstream response rendered in every accepted
/// format, all with the same non-null `seconds`; and no upstream response rendered with
/// different `seconds` in two formats.
fn client_format_row(tally: &Tally) -> OutcomeV1 {
    let check = CheckV1::ClientFormatRendering;
    let name = "speech-v1.client-formats";
    let accepted: Vec<&String> = tally
        .accepted_formats
        .iter()
        .filter(|format| CLIENT_FORMATS.contains(&format.as_str()))
        .collect();
    for renderings in tally.renderings.values() {
        let values: BTreeSet<&Option<String>> = renderings.values().collect();
        if values.len() > 1 {
            return OutcomeV1::failed(
                check,
                name,
                "one upstream response settled with different `seconds` in different client formats",
            );
        }
    }
    let complete = tally.renderings.values().any(|renderings| {
        let values: BTreeSet<&Option<String>> = renderings.values().collect();
        accepted.iter().all(|format| renderings.contains_key(*format))
            && values.len() == 1
            && values.iter().all(|value| value.is_some())
    });
    row(
        check,
        name,
        complete,
        "no duration-bearing upstream response is rendered in every client format the package accepts",
    )
}
