//! The `DashScope` multimodal-generation request and response pieces Bailian's image models share,
//! whichever world carries them: the asynchronous Wan 2.7 task package (`task-wan-image-v2`) and the
//! synchronous Qwen-Image package (`image-qwen`, image world step I3-1, design record
//! `docs/design/2026-10-12-image-i3-1-minimax-and-qwen.md` §3).
//!
//! Moved here from `reference_wan_image_task_v2.rs` rather than copied, so the two packages cannot
//! drift on the wire they share: the `image_url` parts placed before the prompt, the `WxH → W*H`
//! size translation, the parameters forwarded only when sent, and where the result's images sit.
//! The Wan package's behaviour is unchanged (its frozen pack pins it).

use serde_json::{Value, json};
use token_station_protocol::{ErrorCode, ErrorEnvelope};

/// The `parameters` fields both packages forward verbatim, and only when the client sent them.
pub const PASSTHROUGH: [&str; 5] =
    ["watermark", "bbox_list", "seed", "prompt_extend", "negative_prompt"];

fn invalid(message: &str) -> ErrorEnvelope {
    ErrorEnvelope::new(ErrorCode::InvalidRequest, 400, message)
}

/// A whole, non-negative image count; `2.0` counts as two (some JSON producers write it so).
pub fn whole_count(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|f| f.is_finite() && *f >= 0.0 && f.fract() == 0.0 && *f < 1e15)
            .and_then(|f| format!("{f:.0}").parse().ok())
    })
}

/// `OpenAI` `WxH` → `DashScope` `W*H`; named tiers and anything else pass through.
pub fn wan_size(size: &str) -> String {
    let trimmed = size.trim();
    let lower = trimmed.to_ascii_lowercase();
    if let Some((w, h)) = lower.split_once('x') {
        let (w, h) = (w.trim(), h.trim());
        if !w.is_empty()
            && !h.is_empty()
            && w.bytes().all(|b| b.is_ascii_digit())
            && h.bytes().all(|b| b.is_ascii_digit())
        {
            return format!("{w}*{h}");
        }
    }
    trimmed.to_owned()
}

/// The `{"image": …}` content parts of the request's `image_url` (absent, `null`, one entry or an
/// array of entries), in order. `entry` turns one entry into the part's value, or `None` when the
/// entry has the wrong type: the Wan package admits strings only; the Qwen package also admits a
/// host-elided string (image record §6.2).
pub fn image_url_parts(
    image_url: Option<&Value>,
    entry: impl Fn(&Value) -> Option<Value>,
) -> Result<Vec<Value>, ErrorEnvelope> {
    let mut content = Vec::new();
    match image_url {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            for item in items {
                let image = entry(item)
                    .ok_or_else(|| invalid("image_url array entries must be string URLs"))?;
                content.push(json!({"image": image}));
            }
        }
        Some(other) => {
            let image = entry(other).ok_or_else(|| {
                invalid(&format!(
                    "image_url must be a string URL or an array of string URLs (got {other})"
                ))
            })?;
            content.push(json!({"image": image}));
        }
    }
    Ok(content)
}

/// Every non-empty `output.choices[].message.content[].image`, flattened across choices, with the
/// JSON Pointer of each.
pub fn result_images(body: &Value) -> Vec<(String, &str)> {
    let Some(choices) = body.pointer("/output/choices").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut images = Vec::new();
    for (choice_index, choice) in choices.iter().enumerate() {
        let Some(parts) = choice.pointer("/message/content").and_then(Value::as_array) else {
            continue;
        };
        for (part_index, part) in parts.iter().enumerate() {
            if let Some(url) =
                part.get("image").and_then(Value::as_str).filter(|url| !url.is_empty())
            {
                images.push((
                    format!("/output/choices/{choice_index}/message/content/{part_index}/image"),
                    url,
                ));
            }
        }
    }
    images
}
