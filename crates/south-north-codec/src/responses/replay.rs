use crate::CodecError;
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Map, Value, json};

const PREFIX: &str = "tsr.c1.";
const NAMESPACE: &str = "token-station.reasoning-replay";
const FAMILY: &str = "claude-signed-thinking";
const MAX_BLOCKS: usize = 128;
const MAX_OPAQUE_BLOCK_BYTES: usize = 1024 * 1024;
const MAX_OPAQUE_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;

pub const CLAUDE_REASONING_REPLAY_CAPABILITY: &str = "reasoning_replay.claude.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReasoningReplayBlock {
    Thinking { thinking: String, signature: String },
    RedactedThinking { data: String },
    TextRef { ordinal: u32 },
    ToolCallRef { call_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReasoningReplayCarrier {
    blocks: Vec<ReasoningReplayBlock>,
}

impl ReasoningReplayCarrier {
    #[must_use]
    pub fn claude(blocks: Vec<ReasoningReplayBlock>) -> Self {
        Self { blocks }
    }

    #[must_use]
    pub fn blocks(&self) -> &[ReasoningReplayBlock] {
        &self.blocks
    }
}

fn invalid() -> CodecError {
    CodecError::ReasoningReplayInvalid { field: "encrypted_content".to_owned() }
}

fn exact_keys(object: &Map<String, Value>, keys: &[&str]) -> bool {
    object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
}

fn validate(carrier: &ReasoningReplayCarrier) -> Result<(), CodecError> {
    if carrier.blocks.len() > MAX_BLOCKS {
        return Err(invalid());
    }
    let mut total = 0usize;
    for block in &carrier.blocks {
        let fields: &[&str] = match block {
            ReasoningReplayBlock::Thinking { thinking, signature } => {
                &[thinking.as_str(), signature.as_str()]
            }
            ReasoningReplayBlock::RedactedThinking { data } => &[data.as_str()],
            ReasoningReplayBlock::TextRef { .. } => &[],
            ReasoningReplayBlock::ToolCallRef { call_id } => {
                if call_id.is_empty() {
                    return Err(invalid());
                }
                &[]
            }
        };
        let block_bytes = fields
            .iter()
            .try_fold(0usize, |sum, field| sum.checked_add(field.len()).ok_or_else(invalid))?;
        if block_bytes > MAX_OPAQUE_BLOCK_BYTES {
            return Err(invalid());
        }
        total = total.checked_add(block_bytes).ok_or_else(invalid)?;
        if total > MAX_OPAQUE_TOTAL_BYTES {
            return Err(invalid());
        }
    }
    Ok(())
}

pub fn encode_reasoning_replay_carrier(
    carrier: &ReasoningReplayCarrier,
) -> Result<String, CodecError> {
    validate(carrier)?;
    let blocks = carrier
        .blocks
        .iter()
        .map(|block| match block {
            ReasoningReplayBlock::Thinking { thinking, signature } => {
                json!({"kind":"thinking","thinking":thinking,"signature":signature})
            }
            ReasoningReplayBlock::RedactedThinking { data } => {
                json!({"kind":"redacted_thinking","data":data})
            }
            ReasoningReplayBlock::TextRef { ordinal } => {
                json!({"kind":"text_ref","ordinal":ordinal})
            }
            ReasoningReplayBlock::ToolCallRef { call_id } => {
                json!({"kind":"tool_call_ref","call_id":call_id})
            }
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&json!({
        "namespace": NAMESPACE,
        "version": 1,
        "protocol_family": FAMILY,
        "blocks": blocks,
    }))
    .map_err(|_| invalid())?;
    Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

pub fn decode_reasoning_replay_carrier(value: &str) -> Result<ReasoningReplayCarrier, CodecError> {
    let encoded = value.strip_prefix(PREFIX).ok_or_else(invalid)?;
    if encoded.len() > MAX_ENCODED_BYTES || encoded.contains('=') {
        return Err(invalid());
    }
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| invalid())?;
    if bytes.len() > MAX_OPAQUE_TOTAL_BYTES + (MAX_BLOCKS * 256) {
        return Err(invalid());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let object = value.as_object().ok_or_else(invalid)?;
    if !exact_keys(object, &["namespace", "version", "protocol_family", "blocks"])
        || object["namespace"] != NAMESPACE
        || object["version"] != 1
        || object["protocol_family"] != FAMILY
    {
        return Err(invalid());
    }
    let blocks = object["blocks"].as_array().ok_or_else(invalid)?;
    if blocks.len() > MAX_BLOCKS {
        return Err(invalid());
    }
    let mut parsed = Vec::with_capacity(blocks.len());
    for block in blocks {
        let object = block.as_object().ok_or_else(invalid)?;
        let kind = object.get("kind").and_then(Value::as_str).ok_or_else(invalid)?;
        parsed.push(match kind {
            "thinking" if exact_keys(object, &["kind", "thinking", "signature"]) => {
                ReasoningReplayBlock::Thinking {
                    thinking: object["thinking"].as_str().ok_or_else(invalid)?.to_owned(),
                    signature: object["signature"].as_str().ok_or_else(invalid)?.to_owned(),
                }
            }
            "redacted_thinking" if exact_keys(object, &["kind", "data"]) => {
                ReasoningReplayBlock::RedactedThinking {
                    data: object["data"].as_str().ok_or_else(invalid)?.to_owned(),
                }
            }
            "text_ref" if exact_keys(object, &["kind", "ordinal"]) => {
                ReasoningReplayBlock::TextRef {
                    ordinal: object["ordinal"]
                        .as_u64()
                        .and_then(|value| u32::try_from(value).ok())
                        .ok_or_else(invalid)?,
                }
            }
            "tool_call_ref" if exact_keys(object, &["kind", "call_id"]) => {
                ReasoningReplayBlock::ToolCallRef {
                    call_id: object["call_id"].as_str().ok_or_else(invalid)?.to_owned(),
                }
            }
            _ => return Err(invalid()),
        });
    }
    let carrier = ReasoningReplayCarrier::claude(parsed);
    validate(&carrier)?;
    Ok(carrier)
}
