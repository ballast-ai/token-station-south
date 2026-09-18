//! Host-side bindings for the provider world.
//!
//! Generated from the same `wit/provider-adapter.wit` that
//! `south-provider-api` embeds and tests, so the world the runtime
//! instantiates is by construction the world the manifest schema names.
//! Guests are compiled against that file; this is the other half of the
//! contract.

#![allow(
    clippy::pedantic,
    clippy::all,
    reason = "generated code is held to wasmtime's style, not ours"
)]

// The path names the single file, not the `wit/` directory: since the task
// world arrived (2026-09-18) that directory resolves two packages, and a
// directory path would make the generated module layout depend on which
// packages happen to sit beside this one. The runtime instantiates the
// provider world, so it generates from the provider world's file.
wasmtime::component::bindgen!({
    path: "../south-provider-api/wit/provider-adapter.wit",
    world: "provider-adapter-v2",
});
