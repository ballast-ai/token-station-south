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

// -- Embeddings contract 2: what a bounded inline media input costs (record §17.8, §17.10) -----

const MIB_BYTES: usize = 1024 * 1024;

mod inline_media {
    use super::*;
    use south_component_conformance::reference_gemini_embeddings::GeminiEmbeddingsReferenceV1;
    use south_contracts::{EmbeddingsRequestV1, parse_embeddings_request_v2};
    use token_station_protocol::ProviderConfig;

    pub const GEMINI: &str = "embeddings-gemini";
    const MODEL: &str = "gemini-embedding-2-preview";
    const MARKER: &str = "data:image/png;base64,";
    const CONFIG: &str = r#"{"provider":"gemini","base_url":"https://generativelanguage.googleapis.com","auth":"provider_api_key"}"#;

    /// A base64-looking payload of `chars` characters (a multiple of four, as base64 is).
    fn payload(chars: usize) -> String {
        "QUJD".repeat(chars / 4)
    }

    /// The northbound body: `inputs` payloads of `chars` characters each, one string when
    /// `single`, an array otherwise.
    fn northbound(inputs: usize, chars: usize, single: bool) -> serde_json::Value {
        let item = format!("{MARKER}{}", payload(chars));
        if single {
            serde_json::json!({ "input": item, "dimensions": 8 })
        } else {
            serde_json::json!({ "input": vec![item; inputs], "dimensions": 8 })
        }
    }

    /// What one size cost: the request view, the native steps and the sandboxed call.
    pub struct Cost {
        pub view_bytes: usize,
        pub frame_bytes: usize,
        pub parse: Duration,
        pub native_build: Duration,
        pub sandboxed: Result<Duration, CallErrorV1>,
    }

    pub fn cost(wasm: &Path, inputs: usize, chars: usize, single: bool) -> Cost {
        cost_in(wasm, RuntimeLimitsV1::default(), inputs, chars, single)
    }

    /// [`cost`] under other runtime limits.
    pub fn cost_in(
        wasm: &Path,
        limits: RuntimeLimitsV1,
        inputs: usize,
        chars: usize,
        single: bool,
    ) -> Cost {
        let body = northbound(inputs, chars, single);
        let (parse, parsed) = fastest(|| parse_embeddings_request_v2(&body, MODEL));
        let request: EmbeddingsRequestV1 = parsed.expect("the synthetic body parses");
        let view = serde_json::to_string(&request).expect("the view serializes");
        let config: ProviderConfig = serde_json::from_str(CONFIG).expect("the config parses");
        let (native_build, prepared) = fastest(|| {
            let prepared = GeminiEmbeddingsReferenceV1
                .build_embeddings_request(&config, &request)
                .expect("the native reference builds");
            wire::prepared_embeddings_json(&prepared).expect("the frame encodes").to_string()
        });
        let component = load_in(wasm, limits);
        let mut best = Duration::MAX;
        let mut sandboxed = Ok(Duration::MAX);
        for _ in 0..3 {
            let started = Instant::now();
            match component.call_build_embeddings_request(CONFIG, &view) {
                Ok(frame) => {
                    best = best.min(started.elapsed());
                    assert_eq!(
                        frame.len(),
                        prepared.len(),
                        "the guest agrees with the native build"
                    );
                    sandboxed = Ok(best);
                }
                Err(error) => {
                    sandboxed = Err(error);
                    break;
                }
            }
        }
        Cost { view_bytes: view.len(), frame_bytes: prepared.len(), parse, native_build, sandboxed }
    }

    /// The package loaded the way `embeddings_parity::load` does, under other runtime limits.
    fn load_in(wasm: &Path, limits: RuntimeLimitsV1) -> south_provider_runtime::LoadedComponentV1 {
        let runtime =
            south_provider_runtime::ComponentRuntimeV1::new(limits).expect("engine builds");
        south_provider_runtime::LoadedComponentV1::load_embedded(
            &runtime,
            &embeddings_parity::manifest_source(GEMINI),
            &std::fs::read(wasm).expect("the component reads"),
            &super::host_range::host_range(),
            south_provider_runtime::NoSecretsV1,
        )
        .expect("the shipped package passes every load gate")
    }

    pub fn describe(cost: &Cost) -> String {
        let outcome = match &cost.sandboxed {
            Ok(took) => format!("ok in {}", ms(*took)),
            Err(CallErrorV1::PayloadTooLarge { limit }) => {
                format!("refused: payload above the {} limit", mib(*limit))
            }
            Err(CallErrorV1::Trap(detail)) => {
                format!("trapped: {}", detail.lines().last().unwrap_or_default().trim())
            }
            Err(other) => format!("failed: {other}"),
        };
        format!(
            "view {} ({} B), prepared frame {} ({} B); parse_v2 {}, native build+codec {}; sandbox {outcome}",
            mib(cost.view_bytes),
            cost.view_bytes,
            mib(cost.frame_bytes),
            cost.frame_bytes,
            ms(cost.parse),
            ms(cost.native_build),
        )
    }

    /// The payload size (characters, a multiple of four) at which `inputs` media inputs make a
    /// request view of at most `bound` bytes, as close to it as the multiple allows.
    pub fn fit(inputs: usize, single: bool, bound: usize) -> usize {
        let view = |chars: usize| {
            let request = parse_embeddings_request_v2(&northbound(inputs, chars, single), MODEL)
                .expect("the synthetic body parses");
            serde_json::to_string(&request).expect("the view serializes").len()
        };
        let per_input = (bound - view(0)) / inputs / 4 * 4;
        assert!(view(per_input) <= bound && bound - view(per_input) < 5 * inputs);
        per_input
    }

    /// [`cost`] for a request whose view is the host's inline bound, in the runtime a host builds
    /// for `media` packages.
    pub fn cost_at_the_bound(wasm: &Path, inputs: usize, single: bool) -> Cost {
        let chars = fit(inputs, single, south_contracts::MAX_EMBEDDINGS_REQUEST_VIEW_BYTES);
        cost_in(wasm, RuntimeLimitsV1::for_embeddings_media(), inputs, chars, single)
    }

    /// The largest payload (in characters, a multiple of four) of one single media input whose
    /// sandboxed build succeeds, by bisection between a size that works and the view limit.
    pub fn largest_single(wasm: &Path) -> usize {
        let works = |chars: usize| cost(wasm, 1, chars, true).sandboxed.is_ok();
        let (mut low, mut high) = (4 * 1024, RuntimeLimitsV1::default().max_payload_bytes / 4 * 4);
        assert!(works(low), "a 4 KiB media input must build");
        while high - low > 4 {
            let middle = (low + (high - low) / 2) / 4 * 4;
            if works(middle) {
                low = middle;
            } else {
                high = middle;
            }
        }
        low
    }
}

#[test]
#[ignore = "a release-mode measurement for the release record (embeddings record §17.10), not a short test"]
fn measure_inline_media() {
    use inline_media::cost_at_the_bound;
    use inline_media::{GEMINI, cost, describe, largest_single};
    use south_contracts::{EMBEDDINGS_MEDIA_GUEST_MEMORY_BYTES, MAX_EMBEDDINGS_REQUEST_VIEW_BYTES};

    let wasm = embeddings_parity::build(GEMINI, "build-embeddings-gemini-component.sh");
    let limits = RuntimeLimitsV1::default();
    println!(
        "runtime limits: payload {}, memory {}, call deadline {:?}; request view limit {}",
        mib(limits.max_payload_bytes),
        mib(limits.memory_bytes),
        limits.call_timeout,
        mib(MAX_EMBEDDINGS_REQUEST_VIEW_BYTES)
    );
    // The host-facing constants against the runtime they are about.
    assert_eq!(MAX_EMBEDDINGS_REQUEST_VIEW_BYTES + MIB_BYTES, limits.max_payload_bytes);
    assert_eq!(
        RuntimeLimitsV1::for_embeddings_media().memory_bytes,
        EMBEDDINGS_MEDIA_GUEST_MEMORY_BYTES
    );

    // The default limits (64 MiB guest memory): one input growing to the payload limit.
    let limit = limits.max_payload_bytes;
    for (label, chars) in [
        ("1 MiB", MIB_BYTES),
        ("4 MiB", 4 * MIB_BYTES),
        ("8 MiB", 8 * MIB_BYTES),
        ("12 MiB", 12 * MIB_BYTES),
        ("16 MiB view limit (payload fills the rest of the limit)", (limit - 256) / 4 * 4),
    ] {
        println!("== single input, payload {label}");
        println!("  {}", describe(&cost(&wasm, 1, chars, true)));
    }

    // The same total spread over a batch.
    println!("== 2 inputs, 7.9 MiB each");
    println!("  {}", describe(&cost(&wasm, 2, 8 * MIB_BYTES - 128 * 1024, false)));
    println!("== 2048 inputs, 7.9 KiB each");
    println!("  {}", describe(&cost(&wasm, 2048, 7936, false)));

    // The same near-limit request with more guest memory: what the wasm memory limit costs, apart
    // from the payload limit. The view is sized so that the prepared frame, which is a few hundred
    // bytes longer than the view, stays under the payload limit.
    let near_limit = (limit - 4096) / 4 * 4;
    for memory_mib in [64_usize, 96, 128, 192, 256] {
        let limits = RuntimeLimitsV1 {
            memory_bytes: memory_mib * MIB_BYTES,
            call_timeout: Duration::from_secs(20),
            ..RuntimeLimitsV1::default()
        };
        println!(
            "== single input, payload {near_limit} characters, guest memory {memory_mib} MiB, deadline 20 s"
        );
        println!("  {}", describe(&inline_media::cost_in(&wasm, limits, 1, near_limit, true)));
    }

    // The host's inline bound in the runtime a host builds for media packages: the view is the
    // bound itself, so the prepared frame shows the margin to the runtime's frame limit.
    for (label, inputs, single) in
        [("1 input", 1, true), ("2 inputs", 2, false), ("2048 inputs", 2048, false)]
    {
        let at_the_bound = cost_at_the_bound(&wasm, inputs, single);
        println!("== at the inline bound, {label}, guest memory 192 MiB, default deadline");
        println!("  {}", describe(&at_the_bound));
        println!(
            "  margin to the {} frame limit: {} bytes",
            mib(limits.max_payload_bytes),
            limits.max_payload_bytes - at_the_bound.frame_bytes
        );
        assert!(at_the_bound.sandboxed.is_ok(), "{label}: {}", describe(&at_the_bound));
    }

    let largest = largest_single(&wasm);
    let best = cost(&wasm, 1, largest, true);
    println!(
        "== largest single payload the sandbox builds: {largest} characters ({})",
        mib(largest)
    );
    println!("  {}", describe(&best));
    println!(
        "  headroom below the {} payload limit: {} bytes of view",
        mib(limit),
        limit - best.view_bytes
    );
}
