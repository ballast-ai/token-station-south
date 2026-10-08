//! E-Q1 of the embeddings record (`docs/design/2026-09-30-embeddings-contract.md` §5, §14): what a
//! 2048 × 3072 batch costs on the erasure path, and whether the rejected alternative — the
//! component parsing the whole response — could carry it at all.
//!
//! A measurement, not a test of behavior: it is ignored, and it is not a short test. Run it once,
//! in release mode, and copy the printed table into the release record:
//!
//! ```text
//! cargo test --release -p south-component-conformance --features sandbox \
//!   --test embeddings_payload_measurement -- --ignored --nocapture
//! ```
//!
//! The bodies are synthetic `OpenAI` embeddings responses: values uniform in ±0.05, written as
//! the shortest decimal that round-trips the f32 (as upstreams print them), or as base64 of the
//! little-endian f32 bytes. Each timing is the fastest of three runs.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use south_component_conformance::reference_openai_compatible_embeddings::OpenAiCompatibleEmbeddingsReferenceV1;
use south_component_conformance::{EmbeddingsComponentV1, embeddings_json as wire};
use south_contracts::{
    EmbeddingsContractErrorV1, EncodingV1, MAX_BINARY_RESPONSE_BODY_BYTES, VectorLocatorV1,
    extract_vectors_v1, render_vectors_v1,
};
use south_provider_runtime::{CallErrorV1, RuntimeLimitsV1};
use token_station_protocol::HttpResponseParts;

#[path = "support/embeddings_parity.rs"]
mod embeddings_parity;
#[path = "support/host_range.rs"]
mod host_range;

const PACKAGE: &str = "embeddings-openai-compatible";
const DIMENSIONS: usize = 3072;
const BATCH: usize = 2048;
const MIB: f64 = 1024.0 * 1024.0;

/// A deterministic value stream (xorshift32), so every run measures the same bytes.
struct Values(u32);
impl Values {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a synthetic value; the precision is irrelevant"
    )]
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32 - 0.5) / 10.0
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |group, (at, byte)| group | (u32::from(*byte) << (16 - 8 * at)));
        for symbol in 0..4 {
            if symbol <= chunk.len() {
                out.push(char::from(ALPHABET[((group >> (18 - 6 * symbol)) & 0x3f) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// An `OpenAI` embeddings response of `rows` vectors in `encoding`.
fn body(rows: usize, encoding: EncodingV1) -> String {
    let mut values = Values(0x9E37_79B9);
    let mut out = String::from(r#"{"object":"list","data":["#);
    for row in 0..rows {
        if row > 0 {
            out.push(',');
        }
        write!(out, r#"{{"object":"embedding","index":{row},"embedding":"#).unwrap();
        match encoding {
            EncodingV1::Float => {
                out.push('[');
                for column in 0..DIMENSIONS {
                    if column > 0 {
                        out.push(',');
                    }
                    write!(out, "{}", values.next()).unwrap();
                }
                out.push(']');
            }
            EncodingV1::Base64 => {
                let bytes: Vec<u8> =
                    (0..DIMENSIONS).flat_map(|_| values.next().to_le_bytes()).collect();
                write!(out, "\"{}\"", base64(&bytes)).unwrap();
            }
        }
        out.push('}');
    }
    let tokens = rows * 8;
    write!(
        out,
        r#"],"model":"text-embedding-3-large","usage":{{"prompt_tokens":{tokens},"total_tokens":{tokens}}}}}"#
    )
    .unwrap();
    out
}

/// The fastest of three runs, and the last result.
fn fastest<T>(mut run: impl FnMut() -> T) -> (Duration, T) {
    let mut best = Duration::MAX;
    let mut last = None;
    for _ in 0..3 {
        let started = Instant::now();
        let result = run();
        best = best.min(started.elapsed());
        last = Some(result);
    }
    (best, last.expect("three runs"))
}

fn mib(bytes: usize) -> String {
    #[expect(clippy::cast_precision_loss, reason = "a printed size")]
    let value = bytes as f64 / MIB;
    format!("{value:.1} MiB")
}

fn ms(duration: Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1000.0)
}

/// The JSON `parse-embeddings-response` receives for `body`.
fn parts(body: &str) -> String {
    let parts: HttpResponseParts =
        serde_json::from_value(serde_json::json!({ "status": 200, "body": body }))
            .expect("the parts build");
    serde_json::to_string(&parts).expect("the parts serialize")
}

/// The sandboxed `parse-embeddings-response` over `parts` in a freshly loaded instance (a trap
/// poisons the instance it happened in): the fastest of three calls, or the first failure.
fn sandboxed_parse(wasm: &Path, parts: &str) -> Result<(Duration, u32), CallErrorV1> {
    let component = embeddings_parity::load(PACKAGE, wasm);
    let mut best = Duration::MAX;
    let mut vectors = 0;
    for _ in 0..3 {
        let started = Instant::now();
        let parsed = component.call_parse_embeddings_response(parts, "null")?;
        best = best.min(started.elapsed());
        vectors =
            wire::parse_embeddings_parsed_json(&parsed).expect("the facts parse").vector_count();
    }
    Ok((best, vectors))
}

fn describe(result: Result<(Duration, u32), CallErrorV1>) -> String {
    match result {
        Ok((took, vectors)) => format!("{} ({vectors} vectors)", ms(took)),
        Err(CallErrorV1::PayloadTooLarge { limit }) => {
            format!("refused before the call: payload above the {} limit", mib(limit))
        }
        // The last line of a trap names the instruction; the frames above it are unnamed.
        Err(CallErrorV1::Trap(detail)) => {
            format!("trapped: {}", detail.lines().last().unwrap_or_default().trim())
        }
        Err(other) => format!("failed: {other}"),
    }
}

fn measure(label: &str, rows: usize, encoding: EncodingV1, wasm: &Path) {
    let body = body(rows, encoding);
    let limit = RuntimeLimitsV1::default().max_payload_bytes;
    println!("== {label}: {rows} x {DIMENSIONS}, {encoding:?}");
    println!("  body: {} ({} bytes)", mib(body.len()), body.len());
    println!(
        "  under the 64 MiB response cap: {}; under the {} runtime payload limit: {}",
        body.len() <= MAX_BINARY_RESPONSE_BODY_BYTES,
        mib(limit),
        body.len() <= limit
    );

    let (took, extracted) =
        fastest(|| extract_vectors_v1(body.as_bytes(), &VectorLocatorV1::NorthIdentical));
    match extracted {
        Err(EmbeddingsContractErrorV1::BodyTooLarge) => {
            assert!(body.len() > MAX_BINARY_RESPONSE_BODY_BYTES);
            println!("  extract_vectors_v1: refused, body_too_large (in {})", ms(took));
        }
        Err(other) => panic!("the synthetic body must extract: {other}"),
        Ok(extracted) => {
            assert_eq!(extracted.len(), rows);
            println!("  extract_vectors_v1 (parse, extract, erase): {}", ms(took));
            let skeleton = extracted.skeleton().to_string();
            println!("  erased skeleton: {} bytes", skeleton.len());
            for target in [EncodingV1::Float, EncodingV1::Base64] {
                let (took, rendered) = fastest(|| {
                    render_vectors_v1(extracted.vectors(), target, "text-embedding-3-large", 1)
                        .expect("the vectors render")
                });
                println!(
                    "  render_vectors_v1 to {target:?}: {} ({})",
                    ms(took),
                    mib(rendered.len())
                );
            }
            println!(
                "  erasure path, sandboxed parse of the skeleton: {}",
                describe(sandboxed_parse(wasm, &parts(&skeleton)))
            );
        }
    }
    let whole = parts(&body);
    println!(
        "  alternative, sandboxed parse of the whole body ({}): {}",
        mib(whole.len()),
        describe(sandboxed_parse(wasm, &whole))
    );
}

#[test]
#[ignore = "a release-mode measurement for the release record (E-Q1), not a short test"]
fn measure_a_2048_by_3072_batch() {
    let wasm = embeddings_parity::build(PACKAGE, "build-embeddings-openai-compatible-component.sh");
    let component = embeddings_parity::load(PACKAGE, &wasm);
    assert_eq!(component.metadata(), OpenAiCompatibleEmbeddingsReferenceV1.metadata());
    let limits = RuntimeLimitsV1::default();
    println!(
        "runtime limits: payload {}, memory {}, call deadline {:?}",
        mib(limits.max_payload_bytes),
        mib(limits.memory_bytes),
        limits.call_timeout
    );

    measure("full batch, float", BATCH, EncodingV1::Float, &wasm);
    measure("full batch, base64", BATCH, EncodingV1::Base64, &wasm);
    measure("half batch, float", BATCH / 2, EncodingV1::Float, &wasm);

    // The largest float batch whose parts still cross the runtime payload limit: the most the
    // alternative could hand over in one call without raising this world's limits.
    let per_row = parts(&body(64, EncodingV1::Float)).len() / 64;
    let fits = (1..=BATCH)
        .rev()
        .step_by(16)
        .find(|rows| {
            rows * per_row < limits.max_payload_bytes
                && parts(&body(*rows, EncodingV1::Float)).len() <= limits.max_payload_bytes
        })
        .expect("some batch fits");
    measure("largest float batch under the payload limit", fits, EncodingV1::Float, &wasm);

    // The largest float batch the guest can also parse whole inside its memory limit.
    let parses = (1..=fits)
        .rev()
        .step_by(16)
        .find(|rows| sandboxed_parse(&wasm, &parts(&body(*rows, EncodingV1::Float))).is_ok())
        .expect("some batch parses");
    measure("largest float batch the guest parses whole", parses, EncodingV1::Float, &wasm);
}
