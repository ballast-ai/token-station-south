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

wasmtime::component::bindgen!({
    path: "../south-provider-api/wit",
    world: "provider-adapter-v2",
});
