//! The `MiniMax` `base_resp` reading the two synchronous `MiniMax` references share, whichever world
//! carries them: `image-minimax` (image world step I3-1, design record
//! `docs/design/2026-10-12-image-i3-1-minimax-and-qwen.md` §2) and `speech-minimax` (speech world
//! batch V3-2, design record `docs/design/2026-10-14-speech-v3-2-tts.md` §3).
//!
//! `MiniMax` answers HTTP 200 for logical failures and puts the real status in
//! `base_resp.status_code` (speech record §7). Moved here from `reference_minimax_image.rs` rather
//! than copied, as `bailian_image.rs` holds the pieces of the two Bailian packages, so the two
//! families cannot drift on the table they share: the host's status for each code
//! (`minimax_status.rs`), and which codes mean the upstream refused before producing anything (the
//! rest, 1026 included, are `unknown`: IM-Q4). The image package's behaviour is unchanged (its frozen
//! pack pins it).

use serde_json::Value;
use token_station_protocol::{ErrorCode, ErrorEnvelope};

use crate::image_shared::{capped, code_message};

/// The `base_resp` codes that mean the upstream refused before producing anything.
const REFUSED_BEFORE_OUTPUT: [i64; 6] = [1004, 2049, 2013, 1002, 1008, 2153];

/// The reason an `unknown` outcome carries when `base_resp.status_code` is not an integer.
pub const MALFORMED_STATUS_CODE: &str = "a malformed base_resp.status_code";

/// What a 2xx body's `base_resp` says.
#[derive(Debug)]
pub enum BaseRespV1 {
    /// `status_code` absent, `null` or `0`.
    Success,
    /// `status_code` present and not an integer (I3-1 decision 3).
    Malformed,
    /// A non-zero code that means the upstream refused before producing anything: `rejected`.
    Refused(ErrorEnvelope),
    /// Any other non-zero code, 1026 included (possibly charged, IM-Q4): `unknown`, with the
    /// reason [`failure_reason`] gives.
    Failed {
        /// The code.
        code: i64,
        /// The envelope with the host's status for the code.
        error: ErrorEnvelope,
    },
}

/// The reason an `unknown` outcome carries for a failed code.
pub fn failure_reason(code: i64) -> String {
    format!("MiniMax base_resp status code {code}")
}

/// The host's status for a `base_resp` code (`minimax_status.rs`), as an envelope whose provider
/// message is `base_resp.status_msg`, capped.
fn body_failure(code: i64, base_resp: &Value) -> ErrorEnvelope {
    let (error_code, status) = match code {
        1004 | 2049 => (ErrorCode::Auth, 401),
        1026 => (ErrorCode::ContentPolicy, 400),
        2013 => (ErrorCode::InvalidRequest, 400),
        1002 => (ErrorCode::RateLimit, 429),
        1008 | 2153 => (ErrorCode::UpstreamUnavailable, 503),
        _ => (ErrorCode::ProviderProtocolError, 502),
    };
    let mut envelope = ErrorEnvelope::new(error_code, status, code_message(error_code));
    envelope.provider_message = base_resp["status_msg"].as_str().and_then(capped);
    envelope
}

/// Reads the `base_resp` of a 2xx JSON body.
pub fn base_resp(body: &Value) -> BaseRespV1 {
    let base_resp = &body["base_resp"];
    let code = match &base_resp["status_code"] {
        Value::Null => 0,
        value => match value.as_i64() {
            Some(code) => code,
            None => return BaseRespV1::Malformed,
        },
    };
    if code == 0 {
        return BaseRespV1::Success;
    }
    let error = body_failure(code, base_resp);
    if REFUSED_BEFORE_OUTPUT.contains(&code) {
        BaseRespV1::Refused(error)
    } else {
        BaseRespV1::Failed { code, error }
    }
}

/// The provider message of a non-2xx `MiniMax` body: `base_resp.status_msg`.
pub fn status_msg(body: &Value) -> Option<String> {
    body["base_resp"]["status_msg"].as_str().map(str::to_owned)
}
