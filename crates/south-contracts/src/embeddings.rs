//! Embeddings contracts 1 and 2: the IR-independent request, estimate, usage and vector-locator
//! types, and the northbound parsing, vector extraction and rendering both hosts execute.
//!
//! Normative record: `docs/design/2026-09-30-embeddings-contract.md`. Contract 1 carries text and
//! token-id inputs only (§15); [`parse_embeddings_request_v1`] recognizes a media input and
//! refuses it. Contract 2 additionally carries a media input inline, bounded by the runtime
//! payload limit (§17); [`parse_embeddings_request_v2`] returns it as
//! [`EmbeddingInputV1::Media`]. Nothing else differs between the contracts. JSON is the wire for
//! every type here, and the serde shapes are pinned by golden tests because two hosts and the
//! guests must agree byte for byte.

use crate::MAX_BINARY_RESPONSE_BODY_BYTES;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::{borrow::Cow, fmt};
use thiserror::Error;

/// Embeddings contract 1, text and token-id inputs (record §12, §15).
///
/// Kept under the name it has always had, so a host that built its range from it keeps admitting
/// contract 1 packages; the newest contract is [`EMBEDDINGS_CONTRACT_VERSION_V2`].
pub const EMBEDDINGS_CONTRACT_VERSION: u16 = 1;
/// Embeddings contract 2: contract 1 plus inline media inputs (record §17).
pub const EMBEDDINGS_CONTRACT_VERSION_V2: u16 = 2;
/// Every embeddings contract this crate decodes, oldest first. A host's range lists these, and
/// `compatibility.json` records the last.
pub const EMBEDDINGS_CONTRACT_VERSIONS: [u16; 2] =
    [EMBEDDINGS_CONTRACT_VERSION, EMBEDDINGS_CONTRACT_VERSION_V2];
/// The longest serialized request view a host admits to a component (record §17.8), 15 MiB.
///
/// A request above it is the host's 413 before admission; the parsers, which see the northbound
/// body and not the view, do not enforce it. It sits 1 MiB under the runtime's per-call payload
/// limit (16 MiB, `RuntimeLimitsV1::max_payload_bytes`), which the runtime enforces on the frame the
/// guest returns as well as on the view it receives: the prepared request is longer than the view
/// by the dialect's wrapper, about 430 bytes for one input and about 100 bytes more per input
/// for Gemini, and the margin carries 2048 inputs with up to about 500 bytes of wrapper each.
pub const MAX_EMBEDDINGS_REQUEST_VIEW_BYTES: usize = 15 * 1024 * 1024;
/// The guest memory a host must give the embeddings runtime when it admits a package that
/// declares `media` (record §17.8), 192 MiB.
///
/// The default 64 MiB traps on a media input above about 7.9 MiB: the guest holds the input frame,
/// the parsed request, the body being built and the output frame, and its working memory was
/// measured at 8 to 12 times the payload. `south_provider_runtime::RuntimeLimitsV1::for_embeddings_media`
/// builds the limits with this value and every other limit unchanged.
pub const EMBEDDINGS_MEDIA_GUEST_MEMORY_BYTES: usize = 192 * 1024 * 1024;
/// At most this many inputs per request (record §3; the published `OpenAI` limit).
pub const MAX_EMBEDDING_INPUTS: usize = 2048;
/// The serialized parse context a component hands back through the host is at most this long.
pub const MAX_EMBEDDINGS_PARSE_CONTEXT_BYTES: usize = 8 * 1024;
/// At most this many unmodelled northbound fields are handed through (record §3, E-Q10).
pub const MAX_EMBEDDINGS_EXTRA_FIELDS: usize = 32;
/// The unmodelled northbound fields, serialized as one compact JSON object, are at most this long.
pub const MAX_EMBEDDINGS_EXTRA_BYTES: usize = 16 * 1024;
/// A vector-locator JSON Pointer is at most this many bytes.
pub const MAX_VECTOR_POINTER_BYTES: usize = 256;

/// The top-level northbound fields the request models; every other field goes to `extra`.
const MODELLED_FIELDS: [&str; 5] = ["model", "input", "dimensions", "encoding_format", "user"];
/// Extra keys under this prefix would collide with the shared reference vocabulary (record §3).
const RESERVED_EXTRA_PREFIX: &str = "$south.";
/// RFC 2045 `tspecials`: the printable ASCII bytes a token may not contain.
const TSPECIALS: &[u8] = b"()<>@,;:\\\"/[]?=";
/// The `OpenAI` embeddings response shape `NorthIdentical` names (record §5).
const NORTH_DATA_POINTER: &str = "/data";
const NORTH_VECTOR_POINTER: &str = "/embedding";
const NORTH_INDEX_POINTER: &str = "/index";
const NORTH_PROMPT_TOKENS_POINTER: &str = "/usage/prompt_tokens";

/// A refused northbound embeddings request. Every variant is a 400 before admission; the variants
/// stay distinct so the host can word each refusal itself. Diagnostics never echo the input.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum EmbeddingsRequestErrorV1 {
    /// The body is not a JSON object.
    #[error("embeddings request body must be a JSON object")]
    InvalidBody,
    /// The routed upstream model is empty.
    #[error("embeddings upstream model must be non-empty")]
    InvalidModel,
    /// `input` is missing or matches no row of the parsing table.
    #[error("invalid embeddings input")]
    InvalidInput,
    /// `input` carries more than [`MAX_EMBEDDING_INPUTS`] inputs.
    #[error("too many embeddings inputs")]
    TooManyInputs,
    /// An input is a base64 `data:` URI, or a [`EmbeddingInputV1::Media`], where the contract
    /// carries no media inputs: contract 1 (record §15), or a request built for contract 1.
    #[error("media embeddings inputs are not supported")]
    MediaInputNotSupported,
    /// `dimensions` is not an integer in `1..=u32::MAX`.
    #[error("invalid embeddings dimensions")]
    InvalidDimensions,
    /// `encoding_format` is neither `float` nor `base64`.
    #[error("invalid embeddings encoding format")]
    InvalidEncodingFormat,
    /// `user` is not a string.
    #[error("invalid embeddings user")]
    InvalidUser,
    /// The unmodelled fields exceed [`MAX_EMBEDDINGS_EXTRA_FIELDS`] or
    /// [`MAX_EMBEDDINGS_EXTRA_BYTES`].
    #[error("unmodelled embeddings fields exceed their bounds")]
    ExtraTooLarge,
    /// An unmodelled field starts with `$south.`, or a modelled name appears among them.
    #[error("unmodelled embeddings field uses a reserved name")]
    ReservedExtraField,
}

/// A refused embeddings value on the response side, or a refused locator or usage fact.
/// Diagnostics never echo the rejected input.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum EmbeddingsContractErrorV1 {
    /// A locator pointer is not an RFC 6901 JSON Pointer within its bound.
    #[error("invalid embeddings vector pointer")]
    InvalidPointer,
    /// Usage facts break their own rules (record §7.1, §7.4).
    #[error("invalid embeddings usage facts")]
    InvalidUsage,
    /// The response body exceeds the buffered binary bound.
    #[error("embeddings response body exceeds its bound")]
    BodyTooLarge,
    /// The response body is not one JSON value.
    #[error("embeddings response body is not JSON")]
    InvalidJson,
    /// The locator does not resolve, or the located array holds no vector.
    #[error("embeddings vectors not found")]
    VectorNotFound,
    /// A vector is neither a non-empty array of numbers nor non-empty base64 of little-endian
    /// f32, or holds a value that is not a finite f32.
    #[error("invalid embeddings vector")]
    InvalidVector,
    /// Vectors of one response differ in length.
    #[error("embeddings vectors differ in length")]
    VectorLengthMismatch,
    /// A declared index is missing, not an integer, or the indices are not exactly `0..n`; or
    /// only some `NorthIdentical` items carry an index.
    #[error("invalid embeddings vector index")]
    InvalidIndex,
    /// The component's count, the extracted vectors and the request's inputs disagree.
    #[error("embeddings vector count mismatch")]
    VectorCountMismatch,
    /// The vector length differs from the requested `dimensions`.
    #[error("embeddings dimensions mismatch")]
    DimensionsMismatch,
    /// A `NorthIdentical` body's `usage.prompt_tokens` is missing, invalid or differs from the
    /// reported count (record §7.4); the host sends this to manual review.
    #[error("embeddings northbound usage mismatch")]
    NorthUsageMismatch,
}

/// One northbound input (record §3; contract 1 has no media inputs, §15, and contract 2 carries
/// them inline, §17).
///
/// Wire: `{"text":"hello"}`, `{"token_ids":[1,2]}` or, in contract 2 only,
/// `{"media":{"media_type":"image/png","data":"iVBORw0KGgo="}}`; unknown fields are refused.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EmbeddingInputV1 {
    /// A non-empty text input that is not a media `data:` URI.
    Text(String),
    /// A non-empty token-id sequence.
    TokenIds(Vec<u32>),
    /// A media input, contract 2 only (record §17.2): the northbound string
    /// `data:<media_type>;base64,<data>` split at its first `;base64,`.
    Media {
        /// The text between `data:` and the first `;base64,`, verbatim; `type/subtype` with
        /// optional `;name=value` parameters, each part an RFC 2045 token.
        media_type: String,
        /// The text after that first `;base64,`, verbatim: the client's base64 string, neither
        /// decoded nor validated, and empty when the client sent nothing after the marker.
        data: String,
    },
}
impl EmbeddingInputV1 {
    /// Whether this input is a [`EmbeddingInputV1::Media`].
    #[must_use]
    pub const fn is_media(&self) -> bool {
        matches!(self, Self::Media { .. })
    }
    fn validate(&self, media: MediaPolicy) -> Result<(), EmbeddingsRequestErrorV1> {
        let invalid = EmbeddingsRequestErrorV1::InvalidInput;
        match (self, media) {
            (Self::Text(text), _) if text.is_empty() => Err(invalid),
            (Self::Text(text), MediaPolicy::Refuse) if media_type_of(text).is_some() => {
                Err(EmbeddingsRequestErrorV1::MediaInputNotSupported)
            }
            // The parser classifies such a string as `Media`; text holding one was built by hand.
            (Self::Text(text), MediaPolicy::Inline) if media_type_of(text).is_some() => {
                Err(invalid)
            }
            (Self::TokenIds(ids), _) if ids.is_empty() => Err(invalid),
            (Self::Media { .. }, MediaPolicy::Refuse) => {
                Err(EmbeddingsRequestErrorV1::MediaInputNotSupported)
            }
            (Self::Media { media_type, .. }, MediaPolicy::Inline) if !is_media_type(media_type) => {
                Err(invalid)
            }
            _ => Ok(()),
        }
    }
}
impl fmt::Debug for EmbeddingInputV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(text) => {
                formatter.debug_struct("Text").field("byte_count", &text.len()).finish()
            }
            Self::TokenIds(ids) => {
                formatter.debug_struct("TokenIds").field("count", &ids.len()).finish()
            }
            Self::Media { media_type, data } => formatter
                .debug_struct("Media")
                .field("media_type", media_type)
                .field("byte_count", &data.len())
                .finish(),
        }
    }
}

/// Whether a request may hold media inputs: the one difference between the contracts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaPolicy {
    /// Contract 1: a media input is refused.
    Refuse,
    /// Contract 2: a media input is carried inline.
    Inline,
}

/// Whether the northbound `input` was one input or an array (record §3, §8).
///
/// Wire: `"single"` or `"array"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputShapeV1 {
    /// A string or one token-id sequence; exactly one input.
    Single,
    /// An array of strings or of token-id sequences.
    Array,
}

/// A vector encoding: the one the northbound caller wants, or the one a vector arrived in.
///
/// Wire: `"float"` or `"base64"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncodingV1 {
    /// A JSON array of numbers.
    Float,
    /// Standard padded base64 of little-endian f32 values.
    Base64,
}

/// The request a component builds from: the northbound request, parsed per record §3.
///
/// Wire: `{"model":"m","inputs":[{"text":"a"}],"input_shape":"single","dimensions":null,
/// "encoding_format":null,"user":null,"extra":{}}`; unknown fields are refused. The decoder is
/// contract 2's ([`EmbeddingsRequestV1::new_v2`]): the frame is the same for both contracts, and a
/// contract 1 guest must decode a media input to answer it with a capability error. The seam that
/// calls a component keeps media from a contract 1 package ([`Self::carries_media`]).
/// `encoding_format` keeps whether the caller sent it, so a component that forwards the client
/// body reproduces it exactly; [`EmbeddingsRequestV1::requested_encoding`] is the effective value.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "EmbeddingsRequestWire")]
pub struct EmbeddingsRequestV1 {
    model: String,
    inputs: Vec<EmbeddingInputV1>,
    input_shape: InputShapeV1,
    dimensions: Option<u32>,
    encoding_format: Option<EncodingV1>,
    user: Option<String>,
    extra: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmbeddingsRequestWire {
    model: String,
    inputs: Vec<EmbeddingInputV1>,
    input_shape: InputShapeV1,
    dimensions: Option<u32>,
    encoding_format: Option<EncodingV1>,
    user: Option<String>,
    extra: Map<String, Value>,
}
impl TryFrom<EmbeddingsRequestWire> for EmbeddingsRequestV1 {
    type Error = EmbeddingsRequestErrorV1;
    fn try_from(wire: EmbeddingsRequestWire) -> Result<Self, Self::Error> {
        Self::new_v2(
            wire.model,
            wire.inputs,
            wire.input_shape,
            wire.dimensions,
            wire.encoding_format,
            wire.user,
            wire.extra,
        )
    }
}

impl EmbeddingsRequestV1 {
    /// Validates every invariant the contract 1 northbound parser guarantees, so a request built
    /// by hand cannot carry what [`parse_embeddings_request_v1`] would refuse: a media input is
    /// [`EmbeddingsRequestErrorV1::MediaInputNotSupported`]. Use [`Self::new_v2`] for contract 2.
    pub fn new(
        model: String,
        inputs: Vec<EmbeddingInputV1>,
        input_shape: InputShapeV1,
        dimensions: Option<u32>,
        encoding_format: Option<EncodingV1>,
        user: Option<String>,
        extra: Map<String, Value>,
    ) -> Result<Self, EmbeddingsRequestErrorV1> {
        Self::build(
            MediaPolicy::Refuse,
            model,
            inputs,
            input_shape,
            dimensions,
            encoding_format,
            user,
            extra,
        )
    }
    /// Validates every invariant [`parse_embeddings_request_v2`] guarantees, which are those of
    /// [`Self::new`] except that a [`EmbeddingInputV1::Media`] is carried (record §17.3). Its
    /// media type must satisfy the media-type rule of [`media_type_of`]; a `Text` input must not
    /// match that rule, since the parser would have made it a `Media` input.
    pub fn new_v2(
        model: String,
        inputs: Vec<EmbeddingInputV1>,
        input_shape: InputShapeV1,
        dimensions: Option<u32>,
        encoding_format: Option<EncodingV1>,
        user: Option<String>,
        extra: Map<String, Value>,
    ) -> Result<Self, EmbeddingsRequestErrorV1> {
        Self::build(
            MediaPolicy::Inline,
            model,
            inputs,
            input_shape,
            dimensions,
            encoding_format,
            user,
            extra,
        )
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "the seven fields of the request plus the contract's media policy"
    )]
    fn build(
        media: MediaPolicy,
        model: String,
        inputs: Vec<EmbeddingInputV1>,
        input_shape: InputShapeV1,
        dimensions: Option<u32>,
        encoding_format: Option<EncodingV1>,
        user: Option<String>,
        extra: Map<String, Value>,
    ) -> Result<Self, EmbeddingsRequestErrorV1> {
        if model.is_empty() {
            return Err(EmbeddingsRequestErrorV1::InvalidModel);
        }
        if inputs.is_empty() || (input_shape == InputShapeV1::Single && inputs.len() != 1) {
            return Err(EmbeddingsRequestErrorV1::InvalidInput);
        }
        if inputs.len() > MAX_EMBEDDING_INPUTS {
            return Err(EmbeddingsRequestErrorV1::TooManyInputs);
        }
        inputs.iter().try_for_each(|input| input.validate(media))?;
        if dimensions == Some(0) {
            return Err(EmbeddingsRequestErrorV1::InvalidDimensions);
        }
        if extra.keys().any(|key| MODELLED_FIELDS.contains(&key.as_str())) {
            return Err(EmbeddingsRequestErrorV1::ReservedExtraField);
        }
        validate_extra(&extra)?;
        Ok(Self { model, inputs, input_shape, dimensions, encoding_format, user, extra })
    }
    /// The upstream model selected by routing, never the northbound `model` field.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }
    /// The inputs, in northbound order; `1..=MAX_EMBEDDING_INPUTS` of them.
    #[must_use]
    pub fn inputs(&self) -> &[EmbeddingInputV1] {
        &self.inputs
    }
    /// Whether the northbound input was a single input or an array.
    #[must_use]
    pub const fn input_shape(&self) -> InputShapeV1 {
        self.input_shape
    }
    /// Whether any input is a [`EmbeddingInputV1::Media`]. Only a package declaring the `media`
    /// capability may receive such a request (record §17.3).
    #[must_use]
    pub fn carries_media(&self) -> bool {
        self.inputs.iter().any(EmbeddingInputV1::is_media)
    }
    /// The lowest embeddings contract that can carry this request: contract 2 with any media
    /// input, else contract 1.
    #[must_use]
    pub fn minimum_contract_version(&self) -> u16 {
        if self.carries_media() {
            EMBEDDINGS_CONTRACT_VERSION_V2
        } else {
            EMBEDDINGS_CONTRACT_VERSION
        }
    }
    /// The requested vector length, above zero when present.
    #[must_use]
    pub const fn dimensions(&self) -> Option<u32> {
        self.dimensions
    }
    /// The `encoding_format` the northbound caller sent; `None` when absent or `null`.
    #[must_use]
    pub const fn encoding_format(&self) -> Option<EncodingV1> {
        self.encoding_format
    }
    /// The encoding the northbound caller wants: the one it sent, else [`EncodingV1::Float`].
    #[must_use]
    pub const fn requested_encoding(&self) -> EncodingV1 {
        match self.encoding_format {
            Some(encoding) => encoding,
            None => EncodingV1::Float,
        }
    }
    /// The northbound `user`, unchanged.
    #[must_use]
    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }
    /// Every other top-level northbound field, unchanged and bounded.
    #[must_use]
    pub const fn extra(&self) -> &Map<String, Value> {
        &self.extra
    }
}
impl fmt::Debug for EmbeddingsRequestV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmbeddingsRequestV1")
            .field("model", &self.model)
            .field("inputs", &self.inputs)
            .field("input_shape", &self.input_shape)
            .field("dimensions", &self.dimensions)
            .field("encoding_format", &self.encoding_format)
            .field("user_present", &self.user.is_some())
            .field("extra_field_count", &self.extra.len())
            .finish()
    }
}

/// Parses a northbound `/v1/embeddings` body per the normative table of record §3, under the
/// contract 1 scope of §15: a media input is refused.
///
/// `upstream_model` is the model routing selected; the body's own `model` is neither read nor
/// handed through. Checks run in a fixed order and the first failure wins: the body is an object;
/// the model is non-empty; `input` has a table shape; the input count is within bound; no input
/// is a media `data:` URI; then `dimensions`, `encoding_format`, `user` and the unmodelled
/// fields. An explicit JSON `null` for `dimensions`, `encoding_format` or `user` means the field
/// is absent; `encoding_format` keeps whether it was sent, the default (`float`) is applied by
/// [`EmbeddingsRequestV1::requested_encoding`].
pub fn parse_embeddings_request_v1(
    body: &Value,
    upstream_model: &str,
) -> Result<EmbeddingsRequestV1, EmbeddingsRequestErrorV1> {
    parse_embeddings_request(MediaPolicy::Refuse, body, upstream_model)
}

/// Parses a northbound `/v1/embeddings` body for contract 2 (record §17.2).
///
/// Identical to [`parse_embeddings_request_v1`] in every check and in their order, except that a
/// string matching the media rule of [`media_type_of`] becomes an [`EmbeddingInputV1::Media`]
/// input, its `data` the text after the first `;base64,`, verbatim and undecoded, instead of the
/// [`EmbeddingsRequestErrorV1::MediaInputNotSupported`] refusal. `input_shape` keeps its meaning:
/// a media string alone is `Single`, an array of strings, text and media mixed, is `Array`. A
/// body with no media string parses to the request contract 1 would return.
///
/// The parser does not bound the request: the serialized view must not exceed
/// [`MAX_EMBEDDINGS_REQUEST_VIEW_BYTES`], which is the host's 413 before admission.
pub fn parse_embeddings_request_v2(
    body: &Value,
    upstream_model: &str,
) -> Result<EmbeddingsRequestV1, EmbeddingsRequestErrorV1> {
    parse_embeddings_request(MediaPolicy::Inline, body, upstream_model)
}

fn parse_embeddings_request(
    media: MediaPolicy,
    body: &Value,
    upstream_model: &str,
) -> Result<EmbeddingsRequestV1, EmbeddingsRequestErrorV1> {
    let body = body.as_object().ok_or(EmbeddingsRequestErrorV1::InvalidBody)?;
    if upstream_model.is_empty() {
        return Err(EmbeddingsRequestErrorV1::InvalidModel);
    }
    let (inputs, input_shape) = parse_inputs(media, body.get("input"))?;
    let present = |key: &str| body.get(key).filter(|value| !value.is_null());
    let dimensions = present("dimensions")
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0)
                .ok_or(EmbeddingsRequestErrorV1::InvalidDimensions)
        })
        .transpose()?;
    let encoding_format = match present("encoding_format") {
        None => None,
        Some(value) => match value.as_str() {
            Some("float") => Some(EncodingV1::Float),
            Some("base64") => Some(EncodingV1::Base64),
            _ => return Err(EmbeddingsRequestErrorV1::InvalidEncodingFormat),
        },
    };
    let user = present("user")
        .map(|value| value.as_str().map(str::to_owned).ok_or(EmbeddingsRequestErrorV1::InvalidUser))
        .transpose()?;
    let extra: Map<String, Value> = body
        .iter()
        .filter(|(key, _)| !MODELLED_FIELDS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    EmbeddingsRequestV1::build(
        media,
        upstream_model.to_owned(),
        inputs,
        input_shape,
        dimensions,
        encoding_format,
        user,
        extra,
    )
}

/// One northbound `input` item classified by shape only; media detection comes after the count.
enum InputItem<'a> {
    Text(&'a str),
    TokenIds(Vec<u32>),
}

fn parse_inputs(
    media: MediaPolicy,
    input: Option<&Value>,
) -> Result<(Vec<EmbeddingInputV1>, InputShapeV1), EmbeddingsRequestErrorV1> {
    let invalid = EmbeddingsRequestErrorV1::InvalidInput;
    let (items, shape) = match input {
        Some(Value::String(text)) => (vec![InputItem::Text(text)], InputShapeV1::Single),
        Some(Value::Array(items)) => match items.first() {
            Some(Value::String(_)) => (
                items
                    .iter()
                    .map(|item| item.as_str().map(InputItem::Text).ok_or(invalid))
                    .collect::<Result<_, _>>()?,
                InputShapeV1::Array,
            ),
            Some(Value::Number(_)) => {
                (vec![InputItem::TokenIds(token_ids(items)?)], InputShapeV1::Single)
            }
            Some(Value::Array(_)) => (
                items
                    .iter()
                    .map(|item| match item {
                        Value::Array(ids) => token_ids(ids).map(InputItem::TokenIds),
                        _ => Err(invalid),
                    })
                    .collect::<Result<_, _>>()?,
                InputShapeV1::Array,
            ),
            _ => return Err(invalid),
        },
        _ => return Err(invalid),
    };
    if items.iter().any(|item| matches!(item, InputItem::Text(""))) {
        return Err(invalid);
    }
    if items.len() > MAX_EMBEDDING_INPUTS {
        return Err(EmbeddingsRequestErrorV1::TooManyInputs);
    }
    let inputs = items
        .into_iter()
        .map(|item| match (item, media) {
            (InputItem::Text(text), MediaPolicy::Refuse) if media_type_of(text).is_some() => {
                Err(EmbeddingsRequestErrorV1::MediaInputNotSupported)
            }
            (InputItem::Text(text), MediaPolicy::Inline)
                if let Some((media_type, data)) = split_media(text) =>
            {
                Ok(EmbeddingInputV1::Media {
                    media_type: media_type.to_owned(),
                    data: data.to_owned(),
                })
            }
            (InputItem::Text(text), _) => Ok(EmbeddingInputV1::Text(text.to_owned())),
            (InputItem::TokenIds(ids), _) => Ok(EmbeddingInputV1::TokenIds(ids)),
        })
        .collect::<Result<_, _>>()?;
    Ok((inputs, shape))
}

/// A non-empty sequence of integers in `0..=u32::MAX`.
fn token_ids(items: &[Value]) -> Result<Vec<u32>, EmbeddingsRequestErrorV1> {
    if items.is_empty() {
        return Err(EmbeddingsRequestErrorV1::InvalidInput);
    }
    items
        .iter()
        .map(|item| {
            item.as_u64()
                .and_then(|id| u32::try_from(id).ok())
                .ok_or(EmbeddingsRequestErrorV1::InvalidInput)
        })
        .collect()
}

fn validate_extra(extra: &Map<String, Value>) -> Result<(), EmbeddingsRequestErrorV1> {
    if extra.keys().any(|key| key.starts_with(RESERVED_EXTRA_PREFIX)) {
        return Err(EmbeddingsRequestErrorV1::ReservedExtraField);
    }
    let serialized = serde_json::to_vec(extra).map_or(usize::MAX, |bytes| bytes.len());
    if extra.len() > MAX_EMBEDDINGS_EXTRA_FIELDS || serialized > MAX_EMBEDDINGS_EXTRA_BYTES {
        return Err(EmbeddingsRequestErrorV1::ExtraTooLarge);
    }
    Ok(())
}

/// Returns the media type of a media input, or `None` for text (record §3).
///
/// A string is a media input when it is `data:<media type>;base64,<payload>`, where the media type
/// is the text between `data:` and the first `;base64,` and matches `type/subtype` followed by
/// zero or more `;name=value` parameters, each part an RFC 2045 token. The prefix and the
/// `;base64,` marker are matched case-sensitively, as the native arm does; the payload is never
/// decoded. Every other string, including a `data:` URI without `;base64,`, is text.
#[must_use]
pub fn media_type_of(text: &str) -> Option<&str> {
    split_media(text).map(|(media_type, _payload)| media_type)
}

/// The media type and the payload of a media input, split at the first `;base64,`.
fn split_media(text: &str) -> Option<(&str, &str)> {
    let (media_type, payload) = text.strip_prefix("data:")?.split_once(";base64,")?;
    is_media_type(media_type).then_some((media_type, payload))
}

/// The media-type grammar of [`media_type_of`]. A valid media type never contains `;base64,`
/// (a comma is a `tspecial`), so splitting a rebuilt URI at its first marker returns it.
fn is_media_type(media_type: &str) -> bool {
    let mut parts = media_type.split(';');
    let Some((kind, subtype)) = parts.next().and_then(|first| first.split_once('/')) else {
        return false;
    };
    is_token(kind)
        && is_token(subtype)
        && parts.all(|parameter| {
            parameter.split_once('=').is_some_and(|(name, value)| is_token(name) && is_token(value))
        })
}

/// An RFC 2045 token: printable ASCII without space or `tspecials`.
fn is_token(part: &str) -> bool {
    !part.is_empty()
        && part.bytes().all(|byte| byte.is_ascii_graphic() && !TSPECIALS.contains(&byte))
}

/// What the component expects from a request, fixed at build time (record §3, §7).
///
/// Wire: `{"fallback_input_tokens":12,"max_input_tokens":12}`; either may be `null`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingsEstimateV1 {
    fallback_input_tokens: Option<u64>,
    max_input_tokens: Option<u64>,
}
impl EmbeddingsEstimateV1 {
    /// Records the fallback estimate and the component's tightening of the host bound.
    #[must_use]
    pub const fn new(fallback_input_tokens: Option<u64>, max_input_tokens: Option<u64>) -> Self {
        Self { fallback_input_tokens, max_input_tokens }
    }
    /// Settles a `NotReported` call; `None` means such a call cannot be settled.
    #[must_use]
    pub const fn fallback_input_tokens(&self) -> Option<u64> {
        self.fallback_input_tokens
    }
    /// Tightens the host's reservation bound (§7.3); never raises it.
    #[must_use]
    pub const fn max_input_tokens(&self) -> Option<u64> {
        self.max_input_tokens
    }
}

/// An RFC 6901 JSON Pointer of at most [`MAX_VECTOR_POINTER_BYTES`]: empty (the whole value) or
/// `/`-separated reference tokens whose `~` is always `~0` or `~1`.
///
/// Wire: the pointer string, e.g. `"/data"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct JsonPointerV1(String);
impl JsonPointerV1 {
    /// Validates the pointer syntax and bound.
    pub fn parse(pointer: &str) -> Result<Self, EmbeddingsContractErrorV1> {
        let escapes_valid =
            pointer.split('~').skip(1).all(|rest| rest.starts_with('0') || rest.starts_with('1'));
        if pointer.len() > MAX_VECTOR_POINTER_BYTES
            || !(pointer.is_empty() || pointer.starts_with('/'))
            || !escapes_valid
        {
            return Err(EmbeddingsContractErrorV1::InvalidPointer);
        }
        Ok(Self(pointer.to_owned()))
    }
    /// Returns the pointer text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for JsonPointerV1 {
    type Error = EmbeddingsContractErrorV1;
    fn try_from(pointer: String) -> Result<Self, Self::Error> {
        Self::parse(&pointer)
    }
}
impl From<JsonPointerV1> for String {
    fn from(pointer: JsonPointerV1) -> Self {
        pointer.0
    }
}

/// Where the vectors are in a 2xx upstream body, declared per request (record §5).
///
/// Wire: `{"kind":"north_identical"}`,
/// `{"kind":"array","array":"/embeddings","vector":"/values","index":null}` or
/// `{"kind":"single","vector":"/embedding/values"}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VectorLocatorV1 {
    /// The body is an `OpenAI` embeddings response: `/data`, each vector at `/embedding`, each
    /// index at `/index`. The index is optional, as `OpenAI`-compatible servers differ: when
    /// every item carries it, it orders the vectors; when none does, body order holds.
    NorthIdentical,
    /// `array` points at the element array; `vector` (and `index`, when declared) point within
    /// one element. A declared index orders the vectors.
    Array { array: JsonPointerV1, vector: JsonPointerV1, index: Option<JsonPointerV1> },
    /// One vector.
    Single { vector: JsonPointerV1 },
}

/// Whether this response carried usage (record §3, §7.1).
///
/// Wire: `"reported"` or `"not_reported"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSourceV1 {
    /// The upstream reported the input token count.
    Reported,
    /// The upstream did not report usage this time.
    NotReported,
}

/// Usage facts a component parsed from a 2xx (record §7.1).
///
/// Wire: `{"source":"reported","input_tokens":7,"per_input_tokens":[3,4]}`; unknown fields are
/// refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UsageFactsWire")]
pub struct EmbeddingsUsageFactsV1 {
    source: UsageSourceV1,
    input_tokens: Option<u64>,
    per_input_tokens: Option<Vec<u64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UsageFactsWire {
    source: UsageSourceV1,
    input_tokens: Option<u64>,
    per_input_tokens: Option<Vec<u64>>,
}
impl TryFrom<UsageFactsWire> for EmbeddingsUsageFactsV1 {
    type Error = EmbeddingsContractErrorV1;
    fn try_from(wire: UsageFactsWire) -> Result<Self, Self::Error> {
        Self::new(wire.source, wire.input_tokens, wire.per_input_tokens)
    }
}

impl EmbeddingsUsageFactsV1 {
    /// Validates the rules that need no host data: `Reported` carries `input_tokens`;
    /// `NotReported` carries no count; `per_input_tokens`, when given, is non-empty, at most
    /// [`MAX_EMBEDDING_INPUTS`] long and sums exactly to `input_tokens`.
    pub fn new(
        source: UsageSourceV1,
        input_tokens: Option<u64>,
        per_input_tokens: Option<Vec<u64>>,
    ) -> Result<Self, EmbeddingsContractErrorV1> {
        let valid = match (source, input_tokens, per_input_tokens.as_deref()) {
            (UsageSourceV1::Reported, Some(_), None) | (UsageSourceV1::NotReported, None, None) => {
                true
            }
            (UsageSourceV1::Reported, Some(total), Some(each)) => {
                !each.is_empty()
                    && each.len() <= MAX_EMBEDDING_INPUTS
                    && each.iter().try_fold(0_u64, |sum, count| sum.checked_add(*count))
                        == Some(total)
            }
            _ => false,
        };
        if !valid {
            return Err(EmbeddingsContractErrorV1::InvalidUsage);
        }
        Ok(Self { source, input_tokens, per_input_tokens })
    }
    /// The upstream reported `input_tokens` and nothing per input.
    #[must_use]
    pub const fn reported(input_tokens: u64) -> Self {
        Self {
            source: UsageSourceV1::Reported,
            input_tokens: Some(input_tokens),
            per_input_tokens: None,
        }
    }
    /// The upstream reported nothing this time.
    #[must_use]
    pub const fn not_reported() -> Self {
        Self { source: UsageSourceV1::NotReported, input_tokens: None, per_input_tokens: None }
    }
    /// Whether the upstream reported usage.
    #[must_use]
    pub const fn source(&self) -> UsageSourceV1 {
        self.source
    }
    /// The reported input token count; present exactly when `Reported`.
    #[must_use]
    pub const fn input_tokens(&self) -> Option<u64> {
        self.input_tokens
    }
    /// The reported per-input counts, when the upstream reports per input.
    #[must_use]
    pub fn per_input_tokens(&self) -> Option<&[u64]> {
        self.per_input_tokens.as_deref()
    }
}

/// What `parse-embeddings-response` returns for an erased 2xx skeleton (record §3).
///
/// Wire: `{"usage":{...},"vector_count":2,"upstream_model":"m"}`; unknown fields are refused.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingsParsedV1 {
    usage: EmbeddingsUsageFactsV1,
    vector_count: u32,
    upstream_model: Option<String>,
}
impl EmbeddingsParsedV1 {
    /// Records the parsed facts; consistency with the request is [`check_embeddings_response_v1`].
    #[must_use]
    pub const fn new(
        usage: EmbeddingsUsageFactsV1,
        vector_count: u32,
        upstream_model: Option<String>,
    ) -> Self {
        Self { usage, vector_count, upstream_model }
    }
    /// The usage facts.
    #[must_use]
    pub const fn usage(&self) -> &EmbeddingsUsageFactsV1 {
        &self.usage
    }
    /// The number of vectors the component counted in the skeleton.
    #[must_use]
    pub const fn vector_count(&self) -> u32 {
        self.vector_count
    }
    /// The model the upstream echoed, for rendering only.
    #[must_use]
    pub fn upstream_model(&self) -> Option<&str> {
        self.upstream_model.as_deref()
    }
}

/// How `map-provider-error` classifies a non-2xx (record §4).
///
/// Wire: `"rejected"` or `"unknown"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingsFailureOutcomeV1 {
    /// The response proves the upstream produced nothing.
    Rejected,
    /// Anything else, including every 5xx.
    Unknown,
}

/// One extracted vector, kept in the representation it arrived in so pass-through and
/// same-encoding rendering never re-print a value (record §5, §8).
#[derive(Clone, PartialEq)]
pub struct EmbeddingVectorV1 {
    repr: VectorRepr,
}

#[derive(Clone, PartialEq)]
enum VectorRepr {
    /// The upstream JSON numbers, each a finite f32 once rounded.
    Float(Vec<Number>),
    /// The upstream base64 text and its decoded finite values.
    Base64 { text: String, values: Vec<f32> },
}

impl EmbeddingVectorV1 {
    /// Detects the encoding of one located vector (record §5).
    fn detect(value: Value) -> Result<Self, EmbeddingsContractErrorV1> {
        let invalid = EmbeddingsContractErrorV1::InvalidVector;
        let repr = match value {
            Value::Array(items) if !items.is_empty() => VectorRepr::Float(
                items
                    .into_iter()
                    .map(|item| match item {
                        Value::Number(number) if number_to_f32(&number).is_some() => Ok(number),
                        _ => Err(invalid),
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Value::String(text) => {
                let bytes = base64_decode(&text).ok_or(invalid)?;
                if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
                    return Err(invalid);
                }
                let values: Vec<f32> = bytes
                    .chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect();
                if !values.iter().all(|value| value.is_finite()) {
                    return Err(invalid);
                }
                VectorRepr::Base64 { text, values }
            }
            _ => return Err(invalid),
        };
        Ok(Self { repr })
    }
    /// The encoding the vector arrived in.
    #[must_use]
    pub const fn encoding(&self) -> EncodingV1 {
        match self.repr {
            VectorRepr::Float(_) => EncodingV1::Float,
            VectorRepr::Base64 { .. } => EncodingV1::Base64,
        }
    }
    /// The vector length; never zero.
    #[must_use]
    pub const fn len(&self) -> usize {
        match &self.repr {
            VectorRepr::Float(numbers) => numbers.len(),
            VectorRepr::Base64 { values, .. } => values.len(),
        }
    }
    /// Always false: an extracted vector is never empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The values as f32, each float number rounded to the nearest f32.
    #[must_use]
    pub fn to_f32(&self) -> Vec<f32> {
        match &self.repr {
            VectorRepr::Float(numbers) => numbers.iter().filter_map(number_to_f32).collect(),
            VectorRepr::Base64 { values, .. } => values.clone(),
        }
    }
}
impl fmt::Debug for EmbeddingVectorV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmbeddingVectorV1")
            .field("encoding", &self.encoding())
            .field("len", &self.len())
            .finish()
    }
}

/// Rounds a JSON number to the nearest f32, refusing one that is not finite there.
#[expect(
    clippy::cast_possible_truncation,
    reason = "rounding to the nearest f32 is the defined conversion (record §8); overflow is refused below"
)]
fn number_to_f32(number: &Number) -> Option<f32> {
    number.as_f64().map(|value| value as f32).filter(|value| value.is_finite())
}

/// The vectors of one 2xx body and its skeleton with every vector position replaced by `null`.
#[derive(Clone, PartialEq)]
pub struct ExtractedVectorsV1 {
    skeleton: Value,
    vectors: Vec<EmbeddingVectorV1>,
    north_identical: bool,
}
impl ExtractedVectorsV1 {
    /// The erased skeleton handed to `parse-embeddings-response`.
    #[must_use]
    pub const fn skeleton(&self) -> &Value {
        &self.skeleton
    }
    /// The vectors, in index order when the locator declares an index, else in body order.
    #[must_use]
    pub fn vectors(&self) -> &[EmbeddingVectorV1] {
        &self.vectors
    }
    /// The number of vectors; never zero.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.vectors.len()
    }
    /// Always false: extraction refuses a body without vectors.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }
    /// The common vector length.
    #[must_use]
    pub fn dimensions(&self) -> usize {
        self.vectors.first().map_or(0, EmbeddingVectorV1::len)
    }
    /// Whether every vector already has `encoding`.
    #[must_use]
    pub fn all_encoded_as(&self, encoding: EncodingV1) -> bool {
        self.vectors.iter().all(|vector| vector.encoding() == encoding)
    }
    /// Whether the host returns the upstream bytes unchanged: a `NorthIdentical` body whose
    /// vectors all have the requested encoding (record §8). Otherwise it renders with
    /// [`render_vectors_v1`].
    #[must_use]
    pub fn returns_upstream_bytes(&self, requested: EncodingV1) -> bool {
        self.north_identical && self.all_encoded_as(requested)
    }
    /// Splits into the skeleton and the vectors.
    #[must_use]
    pub fn into_parts(self) -> (Value, Vec<EmbeddingVectorV1>) {
        (self.skeleton, self.vectors)
    }
}
impl fmt::Debug for ExtractedVectorsV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExtractedVectorsV1")
            .field("len", &self.len())
            .field("dimensions", &self.dimensions())
            .field("north_identical", &self.north_identical)
            .finish_non_exhaustive()
    }
}

/// Extracts and erases the vectors of a 2xx body per the locator (record §5).
///
/// The encoding is detected per vector: an array of numbers is float, a string is padded standard
/// base64 of little-endian f32 whose decoded length is a non-zero multiple of four; every value
/// must be a finite f32. All vectors are non-empty and of one length. An `Array` locator's
/// declared index is required on every element: a non-negative integer, the vectors are ordered
/// by it, and the indices are exactly `0..n`, each once. `NorthIdentical`'s `/index` is optional:
/// when every item carries it, the same rule applies; when no item does, body order holds; a mix
/// is [`EmbeddingsContractErrorV1::InvalidIndex`]. The skeleton keeps every other field.
pub fn extract_vectors_v1(
    body: &[u8],
    locator: &VectorLocatorV1,
) -> Result<ExtractedVectorsV1, EmbeddingsContractErrorV1> {
    if body.len() > MAX_BINARY_RESPONSE_BODY_BYTES {
        return Err(EmbeddingsContractErrorV1::BodyTooLarge);
    }
    let mut skeleton: Value =
        serde_json::from_slice(body).map_err(|_| EmbeddingsContractErrorV1::InvalidJson)?;
    let vectors = match locator {
        VectorLocatorV1::NorthIdentical => extract_array(
            &mut skeleton,
            NORTH_DATA_POINTER,
            NORTH_VECTOR_POINTER,
            IndexRule::AllOrNone(NORTH_INDEX_POINTER),
        )?,
        VectorLocatorV1::Array { array, vector, index } => extract_array(
            &mut skeleton,
            array.as_str(),
            vector.as_str(),
            index.as_ref().map_or(IndexRule::Absent, |index| IndexRule::Required(index.as_str())),
        )?,
        VectorLocatorV1::Single { vector } => {
            vec![take_vector(skeleton.pointer_mut(vector.as_str()))?]
        }
    };
    let dimensions = vectors.first().map_or(0, EmbeddingVectorV1::len);
    if vectors.iter().any(|vector| vector.len() != dimensions) {
        return Err(EmbeddingsContractErrorV1::VectorLengthMismatch);
    }
    let north_identical = matches!(locator, VectorLocatorV1::NorthIdentical);
    Ok(ExtractedVectorsV1 { skeleton, vectors, north_identical })
}

fn take_vector(slot: Option<&mut Value>) -> Result<EmbeddingVectorV1, EmbeddingsContractErrorV1> {
    let slot = slot.ok_or(EmbeddingsContractErrorV1::VectorNotFound)?;
    EmbeddingVectorV1::detect(std::mem::replace(slot, Value::Null))
}

/// How an array locator's per-element index orders the vectors.
#[derive(Clone, Copy)]
enum IndexRule<'a> {
    /// No index: body order.
    Absent,
    /// Every element carries the index.
    Required(&'a str),
    /// Every element carries the index, or none does (body order); a mix is refused.
    AllOrNone(&'a str),
}

fn extract_array(
    root: &mut Value,
    array: &str,
    vector: &str,
    index: IndexRule<'_>,
) -> Result<Vec<EmbeddingVectorV1>, EmbeddingsContractErrorV1> {
    let items = root
        .pointer_mut(array)
        .and_then(Value::as_array_mut)
        .filter(|items| !items.is_empty())
        .ok_or(EmbeddingsContractErrorV1::VectorNotFound)?;
    let index = match index {
        IndexRule::Absent => None,
        IndexRule::Required(pointer) => Some(pointer),
        IndexRule::AllOrNone(pointer) => {
            let carried = items.iter().filter(|item| item.pointer(pointer).is_some()).count();
            match carried {
                0 => None,
                all if all == items.len() => Some(pointer),
                _ => return Err(EmbeddingsContractErrorV1::InvalidIndex),
            }
        }
    };
    let mut located = Vec::with_capacity(items.len());
    for item in items.iter_mut() {
        let position = index
            .map(|pointer| {
                item.pointer(pointer)
                    .and_then(Value::as_u64)
                    .ok_or(EmbeddingsContractErrorV1::InvalidIndex)
            })
            .transpose()?;
        located.push((position, take_vector(item.pointer_mut(vector))?));
    }
    if index.is_some() {
        located.sort_by_key(|(position, _)| *position);
        let exact = located
            .iter()
            .enumerate()
            .all(|(expected, (position, _))| u64::try_from(expected).ok() == *position);
        if !exact {
            return Err(EmbeddingsContractErrorV1::InvalidIndex);
        }
    }
    Ok(located.into_iter().map(|(_, vector)| vector).collect())
}

/// Checks the facts of a parsed 2xx against the request and the extracted vectors: the §7.4
/// internal-consistency checks and the `NorthIdentical` usage equality. None needs host state.
///
/// `dimensions_capability` is whether the component declares the `dimensions` capability; only
/// then does a requested `dimensions` bind the vector length. The out-of-bound checks against the
/// host's reservation bound stay with the host.
pub fn check_embeddings_response_v1(
    request: &EmbeddingsRequestV1,
    dimensions_capability: bool,
    extracted: &ExtractedVectorsV1,
    parsed: &EmbeddingsParsedV1,
) -> Result<(), EmbeddingsContractErrorV1> {
    let inputs = request.inputs().len();
    if parsed.usage().per_input_tokens().is_some_and(|each| each.len() != inputs) {
        return Err(EmbeddingsContractErrorV1::InvalidUsage);
    }
    if usize::try_from(parsed.vector_count()).ok() != Some(inputs) || extracted.len() != inputs {
        return Err(EmbeddingsContractErrorV1::VectorCountMismatch);
    }
    if dimensions_capability
        && request.dimensions().is_some_and(|dimensions| {
            usize::try_from(dimensions).ok() != Some(extracted.dimensions())
        })
    {
        return Err(EmbeddingsContractErrorV1::DimensionsMismatch);
    }
    if extracted.north_identical {
        let echoed =
            extracted.skeleton().pointer(NORTH_PROMPT_TOKENS_POINTER).and_then(Value::as_u64);
        if echoed.is_none() || echoed != parsed.usage().input_tokens() {
            return Err(EmbeddingsContractErrorV1::NorthUsageMismatch);
        }
    }
    Ok(())
}

/// The generic `OpenAI` embeddings response (record §8).
#[derive(Serialize)]
struct RenderedResponse<'a> {
    object: &'static str,
    data: Vec<RenderedItem<'a>>,
    model: &'a str,
    usage: RenderedUsage,
}

#[derive(Serialize)]
struct RenderedItem<'a> {
    object: &'static str,
    index: usize,
    embedding: RenderedVector<'a>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum RenderedVector<'a> {
    /// Upstream numbers re-emitted as parsed.
    Numbers(&'a [Number]),
    /// Decoded f32 values, each emitted as its shortest round-trip decimal.
    Floats(&'a [f32]),
    /// Base64 text.
    Text(Cow<'a, str>),
}

#[derive(Serialize)]
struct RenderedUsage {
    prompt_tokens: u64,
    total_tokens: u64,
}

/// Renders a generic `OpenAI` embeddings response in the requested encoding (record §8).
///
/// The shape is `{"object":"list","data":[{"object":"embedding","index":0,"embedding":...}],"model":...,
/// "usage":{"prompt_tokens":n,"total_tokens":n}}`.
///
/// A vector already in the requested encoding is emitted as it arrived. Base64 → float emits each
/// f32 as its shortest round-trip decimal; float → base64 rounds each number to the nearest f32
/// and encodes it little-endian in padded standard base64. The vectors must be non-empty and of
/// one length, as extraction guarantees.
pub fn render_vectors_v1(
    vectors: &[EmbeddingVectorV1],
    encoding: EncodingV1,
    model: &str,
    prompt_tokens: u64,
) -> Result<Vec<u8>, EmbeddingsContractErrorV1> {
    let dimensions = vectors.first().map_or(0, EmbeddingVectorV1::len);
    if dimensions == 0 || vectors.iter().any(|vector| vector.len() != dimensions) {
        return Err(EmbeddingsContractErrorV1::VectorLengthMismatch);
    }
    let data = vectors
        .iter()
        .enumerate()
        .map(|(index, vector)| RenderedItem {
            object: "embedding",
            index,
            embedding: match (&vector.repr, encoding) {
                (VectorRepr::Float(numbers), EncodingV1::Float) => RenderedVector::Numbers(numbers),
                (VectorRepr::Base64 { values, .. }, EncodingV1::Float) => {
                    RenderedVector::Floats(values)
                }
                (VectorRepr::Base64 { text, .. }, EncodingV1::Base64) => {
                    RenderedVector::Text(Cow::Borrowed(text))
                }
                (VectorRepr::Float(numbers), EncodingV1::Base64) => {
                    let bytes: Vec<u8> = numbers
                        .iter()
                        .filter_map(number_to_f32)
                        .flat_map(f32::to_le_bytes)
                        .collect();
                    RenderedVector::Text(Cow::Owned(base64_encode(&bytes)))
                }
            },
        })
        .collect();
    let response = RenderedResponse {
        object: "list",
        data,
        model,
        usage: RenderedUsage { prompt_tokens, total_tokens: prompt_tokens },
    };
    serde_json::to_vec(&response).map_err(|_| EmbeddingsContractErrorV1::InvalidVector)
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Padded standard base64 (RFC 4648 §4).
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk.iter().enumerate().fold(0_u32, |group, (position, byte)| {
            group | (u32::from(*byte) << (16 - 8 * position))
        });
        for symbol in 0..4 {
            if symbol <= chunk.len() {
                let sextet = (group >> (18 - 6 * symbol)) & 0x3f;
                out.push(char::from(BASE64_ALPHABET[sextet as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Strict padded standard base64: the length is a multiple of four, padding only ends the text,
/// and the unused bits before padding are zero, so every byte string has exactly one accepted
/// spelling.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let groups = text.len() / 4;
    let mut out = Vec::with_capacity(groups * 3);
    for (position, quad) in text.chunks_exact(4).enumerate() {
        let padding = quad.iter().rev().take_while(|&&byte| byte == b'=').count();
        if padding > 2 || (padding > 0 && position + 1 != groups) {
            return None;
        }
        let mut group = 0_u32;
        for &byte in &quad[..4 - padding] {
            group = (group << 6) | sextet(byte)?;
        }
        group <<= 6 * padding;
        let [_, first, second, third] = group.to_be_bytes();
        let decoded = [first, second, third];
        let kept = 3 - padding;
        if decoded[kept..].iter().any(|byte| *byte != 0) {
            return None;
        }
        out.extend_from_slice(&decoded[..kept]);
    }
    Some(out)
}

const fn sextet(symbol: u8) -> Option<u32> {
    let value = match symbol {
        b'A'..=b'Z' => symbol - b'A',
        b'a'..=b'z' => symbol - b'a' + 26,
        b'0'..=b'9' => symbol - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };
    Some(value as u32)
}
