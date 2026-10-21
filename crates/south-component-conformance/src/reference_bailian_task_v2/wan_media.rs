//! Wan's `input.media[]` request shapes (`docs/design/2026-10-21-bailian-wan-media.md`):
//! wan3.0-video, wan2.7-i2v, wan2.7-r2v and wan2.7-videoedit.
//!
//! Each shape accepts only the northbound inputs its model documents, enforces the official
//! media combinations and counts before dispatch, writes the upstream's default `1080P` tier
//! when the caller names none, and reports as requested seconds the upper bound of the
//! `usage.duration` the upstream bills, which for the video-input shapes includes input seconds.
use super::invalid;
use crate::ComponentResultV1;
use serde_json::{Map, Value, json};
use token_station_protocol::ErrorEnvelope;

/// The upstream's resolution tier when a request names none (all four references: `1080P`).
const DEFAULT_RESOLUTION: &str = "1080P";

/// Every northbound media or option key some shape reads; a key outside a shape's own list is
/// refused for that shape instead of being dropped.
const GOVERNED_KEYS: [&str; 18] = [
    "image",
    "image_url",
    "first_frame_image",
    "last_frame",
    "last_frame_url",
    "reference_images",
    "reference_videos",
    "reference_audios",
    "video",
    "audio_url",
    "file",
    "link",
    "negative_prompt",
    "generate_audio",
    "audio_setting",
    "aspect_ratio",
    "ratio",
    "size",
];
const FIRST_FRAME_KEYS: [&str; 3] = ["image", "image_url", "first_frame_image"];
const LAST_FRAME_KEYS: [&str; 2] = ["last_frame", "last_frame_url"];

/// A model that takes all of its inputs in `input.media[]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MediaShape {
    /// wan3.0-video and wan3.0-video-prime.
    Wan3,
    /// wan2.7-i2v: first frame, first and last frame, or first-clip continuation.
    ImageToVideo,
    /// wan2.7-r2v: reference images and videos, optionally a first frame.
    ReferenceToVideo,
    /// wan2.7-videoedit: one video and up to four reference images.
    VideoEdit,
}

impl MediaShape {
    /// The media shape of an upstream model family (or model id), if it has one. Every other
    /// model, wan2.7-t2v and wan2.6 and older included, keeps its existing request shape.
    pub(super) fn of(shape: &str) -> Option<Self> {
        if shape.starts_with("wan3.") {
            Some(Self::Wan3)
        } else if shape.starts_with("wan2.7-i2v") {
            Some(Self::ImageToVideo)
        } else if shape.starts_with("wan2.7-r2v") {
            Some(Self::ReferenceToVideo)
        } else if shape.starts_with("wan2.7-videoedit") {
            Some(Self::VideoEdit)
        } else {
            None
        }
    }

    fn accepts(self, key: &str) -> bool {
        let frames = FIRST_FRAME_KEYS.contains(&key) || LAST_FRAME_KEYS.contains(&key);
        let first = FIRST_FRAME_KEYS.contains(&key);
        match self {
            Self::Wan3 => {
                frames
                    || matches!(
                        key,
                        "reference_images"
                            | "reference_videos"
                            | "reference_audios"
                            | "video"
                            | "generate_audio"
                            | "aspect_ratio"
                            | "ratio"
                    )
            }
            Self::ImageToVideo => {
                frames || matches!(key, "video" | "audio_url" | "negative_prompt")
            }
            Self::ReferenceToVideo => {
                first
                    || matches!(
                        key,
                        "reference_images"
                            | "reference_videos"
                            | "negative_prompt"
                            | "aspect_ratio"
                            | "ratio"
                    )
            }
            Self::VideoEdit => matches!(
                key,
                "video"
                    | "reference_images"
                    | "negative_prompt"
                    | "audio_setting"
                    | "aspect_ratio"
                    | "ratio"
            ),
        }
    }

    const fn resolutions(self) -> &'static [&'static str] {
        match self {
            Self::Wan3 => &["480P", "720P", "1080P"],
            Self::ImageToVideo | Self::ReferenceToVideo | Self::VideoEdit => &["720P", "1080P"],
        }
    }
}

/// The media a request carries, read from the northbound keys.
struct Inputs<'a> {
    first: Option<&'a str>,
    last: Option<&'a str>,
    images: Vec<&'a str>,
    videos: Vec<&'a str>,
    audios: Vec<&'a str>,
    video: Option<&'a str>,
    audio: Option<&'a str>,
}

fn url(value: &Value) -> ComponentResultV1<&str> {
    value
        .as_str()
        .filter(|url| !url.is_empty())
        .ok_or_else(|| invalid("Bailian media URLs must be non-empty strings"))
}

fn present<'a>(body: &'a Value, key: &str) -> Option<&'a Value> {
    body.get(key).filter(|value| !value.is_null())
}

/// One media URL that the caller may name under several aliases; naming it twice is refused
/// rather than silently keeping one of the two.
fn one_url<'a>(body: &'a Value, keys: &[&str]) -> ComponentResultV1<Option<&'a str>> {
    let mut found = None;
    for key in keys {
        if let Some(value) = present(body, key) {
            if found.is_some() {
                return Err(invalid("Bailian media input is given under more than one name"));
            }
            found = Some(url(value)?);
        }
    }
    Ok(found)
}

fn url_list<'a>(body: &'a Value, key: &str) -> ComponentResultV1<Vec<&'a str>> {
    match present(body, key) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items.iter().map(url).collect(),
        Some(_) => Err(invalid("Bailian media lists must be arrays of URLs")),
    }
}

impl<'a> Inputs<'a> {
    fn read(body: &'a Value) -> ComponentResultV1<Self> {
        Ok(Self {
            first: one_url(body, &FIRST_FRAME_KEYS)?,
            last: one_url(body, &LAST_FRAME_KEYS)?,
            images: url_list(body, "reference_images")?,
            videos: url_list(body, "reference_videos")?,
            audios: url_list(body, "reference_audios")?,
            video: one_url(body, &["video"])?,
            audio: one_url(body, &["audio_url"])?,
        })
    }
}

fn item(kind: &str, url: &str) -> Value {
    json!({"type": kind, "url": url})
}

fn at_most(items: usize, max: usize, message: &str) -> ComponentResultV1<()> {
    if items > max { Err(invalid(message)) } else { Ok(()) }
}

/// The ordered `media[]` items and whether an input video is billed, after the shape's
/// combination and count rules.
fn media(shape: MediaShape, inputs: &Inputs<'_>) -> ComponentResultV1<(Vec<Value>, bool)> {
    let mut items = Vec::new();
    let frames = |items: &mut Vec<Value>| {
        items.extend(inputs.first.map(|url| item("first_frame", url)));
        items.extend(inputs.last.map(|url| item("last_frame", url)));
    };
    let references = |items: &mut Vec<Value>, kind: &str, urls: &[&str]| {
        items.extend(urls.iter().map(|url| item(kind, url)));
    };
    match shape {
        MediaShape::Wan3 => {
            let videos: Vec<&str> = inputs.video.into_iter().chain(inputs.videos.clone()).collect();
            if inputs.last.is_some() && inputs.first.is_none() {
                return Err(invalid("Bailian last frame requires a first frame"));
            }
            let has_references =
                !inputs.images.is_empty() || !videos.is_empty() || !inputs.audios.is_empty();
            if inputs.first.is_some() && has_references {
                return Err(invalid(
                    "Bailian first and last frames cannot be combined with reference media",
                ));
            }
            at_most(inputs.images.len(), 10, "Bailian accepts at most 10 reference images")?;
            at_most(videos.len(), 5, "Bailian accepts at most 5 reference videos")?;
            at_most(inputs.audios.len(), 5, "Bailian accepts at most 5 reference audios")?;
            frames(&mut items);
            references(&mut items, "reference_image", &inputs.images);
            references(&mut items, "reference_video", &videos);
            references(&mut items, "reference_audio", &inputs.audios);
            Ok((items, !videos.is_empty()))
        }
        MediaShape::ImageToVideo => {
            let start = match (inputs.first, inputs.video) {
                (Some(_), Some(_)) => {
                    return Err(invalid(
                        "Bailian image-to-video takes a first frame or a first clip, not both",
                    ));
                }
                (None, None) => {
                    return Err(invalid(
                        "Bailian image-to-video requires a first frame or a first clip",
                    ));
                }
                (Some(first), None) => item("first_frame", first),
                (None, Some(clip)) => item("first_clip", clip),
            };
            if inputs.video.is_some() && inputs.audio.is_some() {
                return Err(invalid("Bailian driving audio requires a first frame"));
            }
            items.push(start);
            items.extend(inputs.last.map(|url| item("last_frame", url)));
            items.extend(inputs.audio.map(|url| item("driving_audio", url)));
            Ok((items, inputs.video.is_some()))
        }
        MediaShape::ReferenceToVideo => {
            let references_total = inputs.images.len() + inputs.videos.len();
            if !(1..=5).contains(&references_total) {
                return Err(invalid(
                    "Bailian reference-to-video takes 1 to 5 reference images and videos",
                ));
            }
            items.extend(inputs.first.map(|url| item("first_frame", url)));
            references(&mut items, "reference_image", &inputs.images);
            references(&mut items, "reference_video", &inputs.videos);
            Ok((items, !inputs.videos.is_empty()))
        }
        MediaShape::VideoEdit => {
            let video =
                inputs.video.ok_or_else(|| invalid("Bailian video editing requires a video"))?;
            at_most(
                inputs.images.len(),
                4,
                "Bailian video editing accepts at most 4 reference images",
            )?;
            items.push(item("video", video));
            references(&mut items, "reference_image", &inputs.images);
            Ok((items, true))
        }
    }
}

/// The requested `duration` as whole seconds; absent and `null` are `None`.
fn requested_duration(body: &Value) -> ComponentResultV1<Option<i32>> {
    let Some(value) = present(body, "duration") else {
        return Ok(None);
    };
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        .and_then(|seconds| i32::try_from(seconds).ok())
        .map(Some)
        .ok_or_else(|| invalid("Bailian duration must be a whole number of seconds"))
}

/// Checks the duration range and returns the upper bound of the billed `usage.duration`.
fn billed_seconds(
    shape: MediaShape,
    duration: Option<i32>,
    video_input: bool,
) -> ComponentResultV1<i32> {
    let out_of_range = || invalid("Bailian duration is out of range for this model");
    let within = |min: i32, max: i32, special: Option<i32>| match duration {
        Some(seconds) if Some(seconds) != special && !(min..=max).contains(&seconds) => {
            Err(out_of_range())
        }
        _ => Ok(()),
    };
    match shape {
        // Without input video `duration` is the billed output; -1 lets the model choose up to
        // 30 s. With input video the bill is input (at most 15 s) + output, at most 30 s.
        MediaShape::Wan3 => {
            within(2, 30, Some(-1))?;
            Ok(match (duration, video_input) {
                (Some(-1), _) | (None, true) => 30,
                (Some(seconds), true) => (15 + seconds).min(30),
                (Some(seconds), false) => seconds,
                (None, false) => 5,
            })
        }
        // A continuation bills the total, which is at least the 2-10 s clip.
        MediaShape::ImageToVideo => {
            within(2, 15, None)?;
            let seconds = duration.unwrap_or(5);
            Ok(if video_input { seconds.max(10) } else { seconds })
        }
        // At most 5 s of input video is billed on top of the output.
        MediaShape::ReferenceToVideo => {
            within(2, if video_input { 10 } else { 15 }, None)?;
            let seconds = duration.unwrap_or(5);
            Ok(if video_input { seconds + 5 } else { seconds })
        }
        // Input (2-10 s) + output, which is the input's length unless `duration` truncates it.
        MediaShape::VideoEdit => {
            within(2, 10, Some(0))?;
            Ok(10 + duration.filter(|seconds| *seconds > 0).unwrap_or(10))
        }
    }
}

fn string_option<'a>(body: &'a Value, key: &str) -> ComponentResultV1<Option<&'a str>> {
    present(body, key)
        .map(|value| value.as_str().ok_or_else(|| invalid("Bailian text options must be strings")))
        .transpose()
}

/// A prepared media request: the upstream body and the estimate facts.
pub(super) struct MediaRequest {
    pub(super) body: Value,
    pub(super) seconds: f64,
    pub(super) resolution: String,
    pub(super) image_count: u32,
}

/// Builds the upstream body of a media-shaped request.
pub(super) fn build(
    shape: MediaShape,
    model: &str,
    body: &Value,
) -> Result<MediaRequest, ErrorEnvelope> {
    for key in GOVERNED_KEYS {
        if present(body, key).is_some() && !shape.accepts(key) {
            return Err(invalid(match key {
                "file" | "link" => "Bailian file and link inputs are not supported yet",
                _ => "this Bailian model does not accept one of the given inputs",
            }));
        }
    }
    let inputs = Inputs::read(body)?;
    let (items, video_input) = media(shape, &inputs)?;
    let prompt = string_option(body, "prompt")?;
    let prompt_required = match shape {
        MediaShape::ReferenceToVideo => true,
        MediaShape::Wan3 => items.is_empty(),
        MediaShape::ImageToVideo | MediaShape::VideoEdit => false,
    };
    if prompt_required && prompt.is_none() {
        return Err(invalid("missing Bailian prompt"));
    }
    let duration = requested_duration(body)?;
    let seconds = billed_seconds(shape, duration, video_input)?;
    let resolution = match present(body, "resolution") {
        None => DEFAULT_RESOLUTION.to_owned(),
        Some(value) => value
            .as_str()
            .and_then(super::resolution_hint)
            .filter(|tier| shape.resolutions().contains(&tier.as_str()))
            .ok_or_else(|| invalid("Bailian resolution is not offered by this model"))?,
    };

    let mut input = Map::new();
    if let Some(prompt) = prompt {
        input.insert("prompt".into(), json!(prompt));
    }
    if let Some(negative) = string_option(body, "negative_prompt")? {
        input.insert("negative_prompt".into(), json!(negative));
    }
    if !items.is_empty() {
        input.insert("media".into(), Value::Array(items));
    }
    let mut parameters = Map::new();
    parameters.insert("resolution".into(), json!(resolution));
    let ratio = match string_option(body, "aspect_ratio")? {
        Some(ratio) => Some(ratio),
        None => string_option(body, "ratio")?,
    };
    if let Some(ratio) = ratio {
        parameters.insert("ratio".into(), json!(ratio));
    }
    if let Some(seconds) = duration {
        parameters.insert("duration".into(), json!(seconds));
    }
    if let Some(audio) = present(body, "generate_audio") {
        let audio =
            audio.as_bool().ok_or_else(|| invalid("Bailian generate_audio must be a boolean"))?;
        parameters.insert("audio".into(), json!(audio));
    }
    if let Some(setting) = string_option(body, "audio_setting")? {
        parameters.insert("audio_setting".into(), json!(setting));
    }
    for key in ["seed", "prompt_extend", "watermark"] {
        if let Some(value) = present(body, key) {
            parameters.insert(key.into(), value.clone());
        }
    }
    let images = usize::from(inputs.first.is_some())
        + usize::from(inputs.last.is_some())
        + inputs.images.len();
    Ok(MediaRequest {
        body: json!({"model": model, "input": input, "parameters": parameters}),
        seconds: f64::from(seconds),
        resolution,
        image_count: u32::try_from(images).map_err(|_| invalid("too many Bailian media inputs"))?,
    })
}
