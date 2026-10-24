#![no_main]
#[path = "../../patches/reqwest-0.13.5-local-write/parser.rs"]
mod parser;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let mut tracker = parser::WriteTracker::new();
    let width = usize::from(data.first().copied().unwrap_or(0)) + 1;
    for chunk in data.chunks(width) {
        let _ = tracker.accepted(chunk);
        let _ = tracker.flushed();
    }
    tracker.abort();
    assert!(tracker.flushed().is_none());
});
