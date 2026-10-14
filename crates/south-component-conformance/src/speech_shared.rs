//! Pieces the text-to-speech references of speech world batches V3-1 and V3-2 share (design records
//! `docs/design/2026-10-14-speech-v3-1-tts.md` §1 and `docs/design/2026-10-14-speech-v3-2-tts.md`
//! §1): the northbound `input`, the operation and metering checks, the facts, the descriptor, the
//! delivered media type, the `binary` and `inline` successes and the non-2xx table.
//!
//! Every package reads the northbound speech request `{input, voice?, response_format?, …}` from the
//! request view's `json` member. A string above the 1 MiB fallback threshold arrives as a
//! `{"$south.blob": …}` placeholder; a member the component forwards is written back as an `as_is`
//! reference ([`with_refs`]), and `input` itself may not be elided (§1).

use serde_json::{Map, Value, json};
use south_contracts::media::{
    MediaLimitsV1, MediaRequestDescriptorV1, PathPatternV1, ResponseBodyFormV1,
};
use south_contracts::speech::{
    PreparedSpeechCallV1, SpeechArtifactFormV1, SpeechArtifactV1, SpeechCallContextV1,
    SpeechCharacterMeterV1, SpeechCharacterSourceV1, SpeechCharactersV1, SpeechEncodingV1,
    SpeechFactsV1, SpeechMeteringFormV1, SpeechMeteringV1, SpeechModelCapabilitiesV1,
    SpeechOperationV1, is_speech_media_type,
};
use south_contracts::{DeclaredQueryParameterV1, JsonPointerV1};
use token_station_protocol::{ErrorEnvelope, ProviderConfig};

pub use crate::image_shared::{bearer, capability, internal, invalid, is_blob, with_refs};
use crate::image_shared::{capped, code_message, status_code};
use crate::{ComponentResultV1, SpeechDeliveryV1, SpeechOutcomeV1};

/// The placeholder key a host writes for an elided string.
pub const BLOB_KEY: &str = "$south.blob";
/// The delivered media type when the upstream named none the contract admits (native
/// `tts.rs:899`, SV-Q2).
pub const FALLBACK_MEDIA_TYPE: &str = "audio/mpeg";

/// The family check every function starts with.
pub fn serves(config: &ProviderConfig, family: &str) -> ComponentResultV1<()> {
    crate::image_shared::serves(config, family)
}

/// The metering form as the contract spells it.
const fn form_word(form: SpeechMeteringFormV1) -> &'static str {
    match form {
        SpeechMeteringFormV1::Characters => "characters",
        SpeechMeteringFormV1::Seconds => "seconds",
        SpeechMeteringFormV1::Tokens => "tokens",
    }
}

/// The operation as the contract spells it.
const fn operation_word(operation: SpeechOperationV1) -> &'static str {
    match operation {
        SpeechOperationV1::Synthesize => "synthesize",
        SpeechOperationV1::Transcribe => "transcribe",
        SpeechOperationV1::Translate => "translate",
    }
}

/// `model-capabilities`: every configured model synthesizes and reports `forms` (§1; which rows
/// are routed here is the host's route and its `tts` modality check, not a list in the package).
pub fn capabilities(
    config: &ProviderConfig,
    family: &str,
    forms: &[SpeechMeteringFormV1],
) -> ComponentResultV1<Vec<SpeechModelCapabilitiesV1>> {
    serves(config, family)?;
    Ok(config
        .models
        .iter()
        .map(|model| SpeechModelCapabilitiesV1 {
            model: model.model.clone(),
            operations: vec![SpeechOperationV1::Synthesize],
            metering_forms: forms.to_vec(),
        })
        .collect())
}

/// The forms the host requires, when the package can report them all (record §8): `synthesize`
/// only, at least one form, each one of `reportable`.
pub fn required_forms(
    context: &SpeechCallContextV1,
    reportable: &[SpeechMeteringFormV1],
) -> ComponentResultV1<Vec<SpeechMeteringFormV1>> {
    if context.operation != SpeechOperationV1::Synthesize {
        return Err(capability(format!(
            "the model '{}' does not declare the operation {}",
            context.upstream_model,
            operation_word(context.operation)
        )));
    }
    if context.metering_required.is_empty() {
        return Err(capability("the host requires no metering form"));
    }
    if let Some(form) = context.metering_required.iter().find(|form| !reportable.contains(form)) {
        return Err(capability(format!("this model cannot report {}", form_word(*form))));
    }
    Ok(context.metering_required.clone())
}

/// The northbound JSON body.
pub fn json_body(request: &Value) -> ComponentResultV1<&Map<String, Value>> {
    request
        .get("json")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("a speech request is a JSON body"))
}

/// `input`: a string, non-empty, not elided (§1).
pub fn input(body: &Map<String, Value>) -> ComponentResultV1<&str> {
    match body.get("input") {
        Some(value) if is_blob(value) => Err(invalid("'input' is longer than the gateway accepts")),
        Some(Value::String(input)) if !input.is_empty() => Ok(input),
        _ => Err(invalid("Missing 'input' field")),
    }
}

/// The Unicode scalar values of `input` (native `chars().count()`, `tts.rs:296`).
pub fn scalar_count(text: &str) -> u64 {
    u64::try_from(text.chars().count()).unwrap_or(u64::MAX)
}

/// The pre-dispatch facts: `synthesize`, `characters {count, upper: count}`, the required forms, no
/// reservation tightening, and the predicted media type.
pub fn facts(count: u64, forms: &[SpeechMeteringFormV1], media_type: &str) -> SpeechFactsV1 {
    SpeechFactsV1 {
        operation: SpeechOperationV1::Synthesize,
        characters: Some(SpeechCharactersV1 { count, upper: Some(count) }),
        metering_forms: forms.to_vec(),
        reservation: None,
        output_media_type: Some(media_type.to_owned()),
    }
}

/// A JSON `POST` descriptor: no `content-type` (the host's JSON executor sets it, I3-1 decision 7),
/// the auth arm when the configuration names a slot, the query when there is one, parsed with the
/// package's own declared query parameters.
pub fn descriptor(
    path: &str,
    query: Option<Value>,
    auth: Option<Value>,
    template: &Value,
    declared: &[DeclaredQueryParameterV1],
) -> ComponentResultV1<MediaRequestDescriptorV1> {
    let mut wire = Map::new();
    wire.insert("method".to_owned(), json!("POST"));
    wire.insert("path".to_owned(), json!(path));
    if let Some(query) = query {
        wire.insert("query".to_owned(), query);
    }
    if let Some(auth) = auth {
        wire.insert("auth".to_owned(), auth);
    }
    wire.insert("body".to_owned(), json!({ "json": { "template": template } }));
    MediaRequestDescriptorV1::parse_with_declared(
        &Value::Object(wire).to_string(),
        &MediaLimitsV1::V1,
        declared,
    )
    .map_err(|error| internal(format!("built an invalid descriptor: {error}")))
}

/// The prepared call of a JSON body with its immutable paths.
pub fn prepared(
    facts: SpeechFactsV1,
    descriptor: MediaRequestDescriptorV1,
    response_body_form: ResponseBodyFormV1,
    elision: &[&str],
    immutable: &[&str],
    state: &Value,
) -> ComponentResultV1<PreparedSpeechCallV1> {
    let response_elision_paths = elision
        .iter()
        .map(|path| PathPatternV1::parse(path).map_err(|_| internal("invalid elision path")))
        .collect::<ComponentResultV1<Vec<_>>>()?;
    Ok(PreparedSpeechCallV1 {
        facts,
        descriptor,
        response_body_form,
        response_elision_paths,
        immutable_body_paths: Some(immutable.iter().map(|path| (*path).to_owned()).collect()),
        immutable_form_fields: None,
        state: state.to_string(),
    })
}

/// `unknown` with a reason and no envelope.
pub fn unknown(reason: &str) -> SpeechOutcomeV1 {
    SpeechOutcomeV1::Unknown { reason: reason.to_owned(), error: None }
}

/// The status a response view carries, `0` when absent.
pub fn view_status(response: &Value) -> u16 {
    crate::image_shared::view_status(response)
}

/// The delivered media type (§1, SV-Q2, decision 3): the first `content-type` in the view's
/// headers when it is a speech media type, else its essence when that is one, else `audio/mpeg`.
pub fn delivered_media_type(response: &Value) -> String {
    let content_type = response["headers"].as_array().and_then(|headers| {
        headers.iter().find_map(|header| {
            let pair = header.as_array()?;
            if pair.first()?.as_str()? == "content-type" { pair.get(1)?.as_str() } else { None }
        })
    });
    let Some(content_type) = content_type else {
        return FALLBACK_MEDIA_TYPE.to_owned();
    };
    if is_speech_media_type(content_type) {
        return content_type.to_owned();
    }
    let essence = content_type.split(';').next().unwrap_or_default().trim();
    if is_speech_media_type(essence) { essence.to_owned() } else { FALLBACK_MEDIA_TYPE.to_owned() }
}

/// The request's character count the state carries.
pub fn state_characters(state: &Value) -> ComponentResultV1<SpeechCharacterMeterV1> {
    let value =
        state["characters"].as_u64().ok_or_else(|| internal("the state has no character count"))?;
    Ok(SpeechCharacterMeterV1 { value, source: SpeechCharacterSourceV1::Request })
}

/// A 2xx of the `binary` form: no bytes is `unknown` (record §10, `empty_audio_is_unknown`);
/// otherwise the body, typed by the upstream's `content-type`, billed by the request's characters.
pub fn binary_outcome(state: &Value, response: &Value) -> ComponentResultV1<SpeechOutcomeV1> {
    if response["body"]["opaque"]["bytes"].as_u64().unwrap_or(0) == 0 {
        return Ok(unknown("a 2xx without audio"));
    }
    Ok(SpeechOutcomeV1::Succeeded {
        delivery: SpeechDeliveryV1::Audio(SpeechArtifactV1 {
            form: SpeechArtifactFormV1::Body,
            media_type: delivered_media_type(response),
            container: None,
        }),
        metering: SpeechMeteringV1 {
            characters: Some(state_characters(state)?),
            ..SpeechMeteringV1::default()
        },
    })
}

/// The JSON object a 2xx view of the `json` form carries, `None` for any other JSON value
/// (`image-minimax`'s rule: such a 2xx is `unknown`, V3-2 record §1).
pub fn json_object(response: &Value) -> Option<&Map<String, Value>> {
    response.pointer("/body/json").and_then(Value::as_object)
}

/// Whether the view's value at a declared `inline` audio path holds audio: a placeholder of at least
/// one byte or (from a host that did not elide it) a non-empty string. A missing value, another JSON
/// type and a 0-byte string do not (record §10, `empty_audio_is_unknown`).
pub fn holds_audio(value: Option<&Value>) -> bool {
    value.is_some_and(crate::image_shared::non_empty_inline)
}

/// A 2xx of the `json` form that delivered audio (V3-2 record §1): an `inline` artifact at
/// `pointer`, decoded by the host with `encoding` and delivered as `media_type` (the media type of
/// the format the package asked the upstream for), billed by `characters`.
pub fn inline_outcome(
    pointer: &str,
    encoding: SpeechEncodingV1,
    media_type: &str,
    characters: SpeechCharacterMeterV1,
) -> ComponentResultV1<SpeechOutcomeV1> {
    let pointer =
        JsonPointerV1::parse(pointer).map_err(|_| internal("invalid artifact pointer"))?;
    Ok(SpeechOutcomeV1::Succeeded {
        delivery: SpeechDeliveryV1::Audio(SpeechArtifactV1 {
            form: SpeechArtifactFormV1::Inline { pointer, encoding },
            media_type: media_type.to_owned(),
            container: None,
        }),
        metering: SpeechMeteringV1 { characters: Some(characters), ..SpeechMeteringV1::default() },
    })
}

/// The body of a non-2xx view as text, for a family whose error shape it is not: a text body as
/// is, a JSON body as compact JSON; an opaque body has none.
fn body_text(body: &Value) -> Option<String> {
    if let Some(text) = body["text"].as_str() {
        return Some(text.to_owned());
    }
    body.get("json").map(Value::to_string)
}

/// A non-2xx round (§1): the envelope by status, `provider_message` from the family's error body
/// (`message_of`, given the JSON body) or else the body text, capped at 256 characters; `rejected`
/// for a 4xx other than 408, `unknown` otherwise, a 3xx included (§10 P5, P6).
pub fn non_2xx(
    status: u16,
    response: &Value,
    message_of: fn(&Value) -> Option<String>,
) -> SpeechOutcomeV1 {
    let body = &response["body"];
    let code = status_code(status);
    let mut error = ErrorEnvelope::new(code, status, code_message(code));
    let message = body.get("json").and_then(message_of).or_else(|| body_text(body));
    error.provider_message = message.as_deref().and_then(capped);
    if (400..500).contains(&status) && status != 408 {
        SpeechOutcomeV1::Rejected { error }
    } else {
        SpeechOutcomeV1::Unknown { reason: format!("upstream status {status}"), error: Some(error) }
    }
}

/// The `OpenAI` error shape `{"error": {"message", …}}` (§2; Groq's, §4).
pub fn openai_error_message(body: &Value) -> Option<String> {
    body["error"]["message"].as_str().map(str::to_owned)
}
