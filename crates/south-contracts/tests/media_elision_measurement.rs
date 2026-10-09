//! The timing S-I-1 records (image record §18.6, the E-Q1 pattern): `elide_v1` and the multipart
//! splitter on a 32 MiB base64 body. Ignored by default; run it in release mode and copy the
//! numbers into the release record:
//!
//! `cargo test --release -p south-contracts --test media_elision_measurement -- --ignored --nocapture`

use south_contracts::MultipartBoundaryV1;
use south_contracts::media::{
    MediaLimitsV1, PathPatternV1, elide_v1, encode_base64, parse_multipart_parts_v1,
};
use std::time::Instant;

const BODY_BYTES: usize = 32 * 1024 * 1024;

fn payload() -> String {
    // Deterministic pseudo-random bytes, so base64 has no long runs.
    let mut state = 0x2545_f491_u32;
    let raw: Vec<u8> = (0..BODY_BYTES / 4 * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect();
    encode_base64(&raw)
}

fn best_of<T>(runs: usize, mut work: impl FnMut() -> T) -> (std::time::Duration, T) {
    let mut best = None;
    let mut last = None;
    for _ in 0..runs {
        let started = Instant::now();
        let result = work();
        let elapsed = started.elapsed();
        best = Some(best.map_or(elapsed, |previous: std::time::Duration| previous.min(elapsed)));
        last = Some(result);
    }
    (best.expect("at least one run"), last.expect("at least one run"))
}

#[test]
#[ignore = "a measurement, not a check; run it by name in release mode"]
fn elision_and_splitting_of_a_32_mib_base64_body() {
    let encoded = payload();
    let document = format!(r#"{{"model":"gpt-image-1","prompt":"a cat","image":"{encoded}"}}"#);
    let declared = [PathPatternV1::parse("/image").expect("valid")];
    let (elide_time, (view, blobs)) = best_of(5, || {
        elide_v1(document.as_bytes(), &declared, &MediaLimitsV1::V1).expect("elides")
    });
    assert_eq!(blobs.len(), 1);
    println!(
        "elide_v1: {} byte document -> {} byte view, best of 5: {:?}",
        document.len(),
        view.len(),
        elide_time
    );

    let boundary = MultipartBoundaryV1::parse("measurementBoundary").expect("valid");
    let body = format!(
        "--measurementBoundary\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\na cat\r\n\
         --measurementBoundary\r\nContent-Disposition: form-data; name=\"image\"; filename=\"a.png\"\r\n\
         Content-Type: image/png\r\n\r\n{encoded}\r\n--measurementBoundary--\r\n"
    );
    let (split_time, (parts, blobs)) = best_of(5, || {
        parse_multipart_parts_v1(body.as_bytes(), &boundary, &MediaLimitsV1::V1).expect("splits")
    });
    assert_eq!(parts.parts.len(), 2);
    assert_eq!(blobs.len(), 1);
    println!("parse_multipart_parts_v1: {} byte body, best of 5: {:?}", body.len(), split_time);
}
