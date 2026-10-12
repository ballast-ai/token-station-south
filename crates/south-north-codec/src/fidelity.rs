//! Inbound rules protocol 0.6.0 (canonical IR 4) added, shared by the client wires that carry
//! them: prompt-cache markers, and the IR keys a client must not write (KC1 record
//! `docs/design/2026-10-12-kc1-canonical-ir-4-south.md` §3).

use serde_json::Value;
use token_station_protocol::{CacheControl, CacheTtl};

use crate::{CodecError, describe};

/// The block's prompt-cache marker, if it carries one, checked against the closed kernel form.
///
/// The kernel refuses an invalid marker only when IR JSON is deserialized, and this codec builds
/// parts directly, so it applies the same rule here: `type` absent or `"ephemeral"`, `ttl` absent,
/// `"5m"` or `"1h"`, and no other key (owner ruling K6). Otherwise the request would pass the
/// codec and fail at the component as an internal error instead of a 400. `null` is absent.
pub fn cache_control(block: &Value, path: &str) -> Result<Option<CacheControl>, CodecError> {
    let Some(marker) = block.get("cache_control").filter(|marker| !marker.is_null()) else {
        return Ok(None);
    };
    let field = format!("{path}.cache_control");
    let Some(object) = marker.as_object() else {
        return Err(CodecError::unknown_value(
            field,
            describe(marker),
            "an object {\"type\": \"ephemeral\", \"ttl\"?: \"5m\" | \"1h\"}",
        ));
    };
    if let Some(key) = object.keys().find(|key| !matches!(key.as_str(), "type" | "ttl")) {
        return Err(CodecError::unknown_value(
            format!("{field}.{key}"),
            "an unknown key",
            "\"type\" or \"ttl\"",
        ));
    }
    if let Some(kind) = object.get("type")
        && kind.as_str() != Some("ephemeral")
    {
        return Err(CodecError::unknown_value(
            format!("{field}.type"),
            short(kind),
            "\"ephemeral\"",
        ));
    }
    let ttl = match object.get("ttl") {
        None | Some(Value::Null) => None,
        Some(Value::String(ttl)) if ttl == "5m" => Some(CacheTtl::FiveMinutes),
        Some(Value::String(ttl)) if ttl == "1h" => Some(CacheTtl::OneHour),
        Some(other) => {
            return Err(CodecError::unknown_value(
                format!("{field}.ttl"),
                short(other),
                "\"5m\" or \"1h\"",
            ));
        }
    };
    Ok(Some(CacheControl { ttl }))
}

/// Refuses a top-level key that is a canonical IR wire key this client wire does not define.
///
/// `ChatRequest.extensions` is flattened, so such a key copied there as data would be read back as
/// the typed IR field when a component deserializes the request, and a `host_values` key would
/// sit next to the map the host fills (KC-Q5). No client of these wires has a reason to send one.
pub fn refuse_reserved_keys(body: &Value, reserved: &[&str]) -> Result<(), CodecError> {
    let Some(object) = body.as_object() else { return Ok(()) };
    reserved.iter().find(|key| object.contains_key(**key)).map_or(Ok(()), |key| {
        Err(CodecError::unknown_value(*key, "a canonical IR key", "a field this protocol defines"))
    })
}

/// A short scalar is echoed so the caller sees what it sent; anything longer, or any container,
/// is described by kind only, as [`describe`] does, so error text never carries prompt content.
fn short(value: &Value) -> String {
    match value {
        Value::String(text) if text.len() <= 16 && text.chars().all(|c| c.is_ascii_graphic()) => {
            format!("{text:?}")
        }
        other => describe(other),
    }
}

/// An optional boolean field: absent or `null` is `None`; anything else must be a boolean.
pub fn optional_bool(value: Option<&Value>, field: &str) -> Result<Option<bool>, CodecError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(other) => Err(CodecError::unknown_value(field, describe(other), "true or false")),
    }
}
