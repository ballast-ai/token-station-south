#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

//! Sandboxed execution of provider components — gate ④ of the conformance
//! layering, and the thing that makes the conformance crate's typed seam real
//! for a `.wasm` file.
//!
//! # The load path is the trust path
//!
//! [`LoadedComponentV1::load`] runs the gates in order, cheapest and most
//! decidable first: the manifest (gate ①'s schema half, before any code is
//! read), the import scan (`wasi:sockets` / `wasi:http` refused by name —
//! everything else WASI gets a locked-down implementation with no preopens,
//! no environment, no inherited stdio), then instantiation and the identity
//! gate (`metadata()` must equal the manifest's declaration). The
//! compatibility-tuple handshake runs in the admitting layer, which holds the
//! host's expected values.
//!
//! # The API face is JSON, deliberately
//!
//! Calls take and return JSON strings named by canonical type (raw bytes for
//! the stream chunk), and the component's own error channel is carried as an
//! **opaque** string: this crate never parses the Canonical IR. Typed
//! serialization lives in the conformance crate's sandbox seam and in host
//! adapter layers; the repository boundary gate enforces that this crate's
//! dependency tree stays IR-free. Failures of the runtime itself use the
//! stable, IR-free [`CallErrorV1`] vocabulary: component payload, deadline,
//! trap, payload bound, stream cap — and load-time ABI mismatch is
//! [`LoadErrorV1::NotAComponent`].
//!
//! # Resource bounds
//!
//! Every store carries a memory limit, every guest call a wall-clock deadline
//! (epoch interruption; a background ticker advances the engine epoch and a
//! hung guest traps), every boundary payload a byte ceiling, and live stream
//! instances a per-runtime cap. Streams get one instance each: the chunk
//! parser's tail is instance state, and sharing would interleave bodies.
//!
//! # Credentials
//!
//! The provider world imports `host.sign`. The runtime enforces the manifest
//! boundary *before* consulting any signer: a `secret-ref` the manifest did
//! not declare under `permissions.secrets` is refused here, so a
//! [`SecretSignerV1`] implementation never learns an undeclared name was
//! asked for.
//!
//! Design record: `docs/design/2026-08-21-provider-runtime.md`.

mod bindings;
mod component;
mod loader;
mod runtime;

pub use component::{ComponentStreamV1, LoadedComponentV1, NoSecretsV1, SecretSignerV1};
pub use loader::{CallErrorV1, LoadErrorV1, UnreadableReasonV1};
pub use runtime::{ComponentRuntimeV1, RuntimeLimitsV1};
