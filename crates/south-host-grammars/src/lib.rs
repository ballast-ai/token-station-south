#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Pure, bounded wire grammars that only hosts call.
//!
//! South keeps a parsing grammar in `south-contracts` when components may link it, and here when
//! only hosts call it. The difference is not taste: every component links `south-contracts`, so a
//! change to it, even an unused function with the version bump Q47 requires, changes every
//! `component.wasm` and re-identifies every package (boundary record
//! `docs/design/2026-09-30-host-zero-vendor-boundary.md`, §13.12 and §13.13). A grammar only hosts
//! call can ship without touching a single package when it lives here.
//!
//! What the two homes share is the obligation: each grammar here is pure (no I/O, no clock, no
//! environment), bounded by explicit limits with typed errors, never panics on any input, and has
//! golden vectors, property tests and a scheduled fuzz target (`fuzz/fuzz_targets/
//! contract_parsers.rs`). Both hosts call the same function, so they split the same bytes the
//! same way.
//!
//! Two rules keep this crate host-only, and both are checked mechanically:
//!
//! - **No component links it.** `shipped_packages_v1` fails when a component lockfile names it,
//!   and `scripts/check-boundaries.sh` fails when one of the three component-linked crates
//!   (`south-contracts`, `south-provider-api`, `south-component-conformance`) depends on it.
//! - **It has no dependencies.** `scripts/check-boundaries.sh` refuses any normal or build
//!   dependency, so taking this crate adds nothing else to a host's graph.
//!
//! The crate carries the workspace version, like the other host-side crates.
//!
//! # Grammars
//!
//! - [`decode_sse_v1`] and [`SseDecoderV1`]: the server-sent events (`text/event-stream`)
//!   decoder, the SSE sibling of the eventstream deframer (§5.2). The media worlds use it to build
//!   a component's view of an SSE response body, and `north_passthrough` uses it to find the
//!   terminal frame of a stream the host forwards unchanged.

mod sse;

pub use sse::{
    DEFAULT_SSE_EVENT_TYPE, MAX_SSE_EVENT_BYTES, MAX_SSE_EVENTS, MAX_SSE_LINE_BYTES, SseDecoderV1,
    SseErrorV1, SseEventV1, decode_sse_v1,
};
