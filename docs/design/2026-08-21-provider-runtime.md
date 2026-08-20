# Provider Component Runtime (S3)

Status: D1–D5 ruled 2026-08-21 (all follow the S2 layering and the donor's
sandbox posture; the seam re-cut is the plan's own instruction); shipped as
0.10.0.

Date: 2026-08-21

Predecessors: `2026-08-21-component-conformance.md` (S2 — the typed seam this
runtime makes real for a `.wasm` file, and the frozen gate ② run it must
reproduce), `2026-08-21-provider-api-promotion.md` (S1 — the world it
instantiates and the manifest it gates on), and the community host's
`crates/plugin-runtime` (donor: engine/ticker/gates/instance-per-stream,
1,352 lines, wasmtime 46).

## 1. Problem, and the seam re-cut

The donor runtime implements the community conformance crate's typed
`ProviderAdapter` directly: its `Cargo.toml` depends on the protocol and
conformance crates, and every call site does `to_json`/`from_json` inside the
runtime. The plan's S3 instruction is to cut both couplings: the runtime owns
JSON/WASM mechanics only, typed judgement stays where the sanctioned IR edge
already lives. Concretely:

- **`south-provider-runtime`** exposes `json in → json out` (raw bytes in for
  the stream chunk): `LoadedComponentV1::call_*` take and return JSON strings
  named by canonical type, and the component's own error channel is carried
  **opaquely** (`CallErrorV1::Component(String)`) — this crate never parses
  the IR. The repository boundary gate enforces the obligation structurally:
  the kernel-IR carve-out admits only the conformance crate, so a runtime
  dependency on `token-station-protocol`, the kernel, or the conformance
  crate fails the gate.
- **The typed seam over the sandbox** is the conformance crate's new
  `sandbox` module (cargo feature `sandbox`): `SandboxedComponentV1`
  implements `ProviderComponentV1` over a `LoadedComponentV1`, doing the
  typed↔JSON conversion in the one crate sanctioned to know the IR. This is
  the S2 statement "the S3 runtime becomes an implementation of the seam",
  delivered as a wrapper in the seam's own crate.
- **The guest-side halves** of the same boundary live in the conformance
  crate's new `abi` module: JSON shims over any `ProviderComponentV1`
  (envelope encoding, serialization-failure handling, a per-stream
  `StreamAbiV1`), written once and reused by every official component.

Runtime failures use the stable, IR-free vocabulary the plan names:
`Component` (opaque payload), `Deadline` (epoch interrupt, the one trap a
caller can act on differently), `Trap` (panic/OOM/other — a component that
did not answer), `PayloadTooLarge` (both directions, both channels),
`StreamLimit`; load-time ABI mismatch is `LoadErrorV1::NotAComponent`.

## 2. What ships

- **`ComponentRuntimeV1`** — shared engine with epoch interruption, on-disk
  compilation cache, a single ticker thread (10 ms resolution), and
  `RuntimeLimitsV1` (memory ceiling per store, wall-clock deadline per call,
  boundary payload ceiling, 64-live-stream cap per runtime). Donor semantics
  preserved wholesale.
- **`LoadedComponentV1`** — the load path is the trust path, in gate order:
  manifest (`ComponentManifestV1::validate`, before any wasm is read; the
  compatibility-tuple handshake stays in the admitting layer, which holds the
  host's expected values), the forbidden-import scan (`wasi:sockets/`,
  `wasi:http/` refused **by name** — the rest of WASI gets a locked-down
  implementation with no preopens, no environment, no inherited stdio,
  because a std-compiled guest imports it whether its author wanted to or
  not), then instantiation and the identity gate (`metadata()` must equal the
  manifest's declaration). `load` (directory: `manifest.json` +
  `component.wasm`) and `load_embedded` (bytes) run identical gates.
- **`host.sign`** behind the manifest fence: an undeclared `secret-ref` is
  refused before any `SecretSignerV1` learns it was asked for
  (`NoSecretsV1` for hosts with nothing to sign).
- **Streams**: one instance per stream (`open_stream`), permit-capped;
  `ComponentStreamV1::parse_chunk(&[u8])` with EOF as the empty chunk, per
  the seam's convention.
- **The first official component**: `components/provider-openai-compatible`
  (standalone `wasm32-wasip2` crate, built by
  `scripts/build-reference-component.sh`) — a wit-bindgen shell over
  `reference::OpenAiCompatibleReferenceV1` through the `abi` shims, shipped
  with its gate-①-valid `manifest.json`. The component and the native
  reference are the same code, so parity is a property of construction that
  the acceptance test then proves end to end.
- **Tests**: the runtime's own suite drives a hostile test guest
  (`tests/guests/test-provider`, JSON-only like the runtime it tests) through
  every gate and every bound — manifest-first ordering, identity lie,
  hand-written WAT importing `wasi:sockets` refused by name, hang cut at the
  deadline (classified `Deadline`), 256 MiB growth against a 64 MiB store
  trapped, guest panic contained, oversized payload refused before the guest
  runs, undeclared secret never reaching the signer, and two half-frame
  streams proving instance isolation. The S3 acceptance lives in the
  conformance crate: `sandbox_parity_v1` loads the real official component
  and passes gate ② **byte-for-byte** against the fixture pack frozen on the
  native reference (stream incrementality re-splits at every byte boundary
  through the sandbox), plus gate ① and the tuple handshake on the shipped
  package.

## 3. Decisions — ruled 2026-08-21

- **D1 — the typed seam lives behind the conformance crate's `sandbox`
  feature**, not in the runtime and not in a new crate. The runtime cannot
  hold it (IR-free obligation, gate-enforced); a new crate would exist only
  to hold one wrapper. The feature exists because guests depend on the
  conformance crate for the `abi` shims and compile to `wasm32-wasip2` —
  wasmtime must never enter their build graph.
- **D2 — `StreamParserV1` gains a `Send` bound** (seam amendment, 0.10.0):
  host streams cross worker threads and the guest shell holds the parser in
  a `static`; a wasm guest is single-threaded, so the bound costs it
  nothing.
- **D3 — the official component wraps the reference by dependency, not by
  copy.** The donor keeps guest and native reference as two implementations
  pinned together by fixtures; here the guest depends on
  `south-component-conformance` and re-exports the same logic, so drift is
  structurally impossible, not merely detected.
- **D4 — deadline is the one classified trap.** `wasmtime::Trap::Interrupt`
  maps to `CallErrorV1::Deadline`; every other trap is `Trap(_)`. OOM is not
  separately classified: a guest allocation failure surfaces as a guest-side
  abort indistinguishable from any other trap, and pretending otherwise
  would promise more than the engine reports.
- **D5 — version: 0.10.0** (numbered at ship time per the retired
  pre-allocation rule). `compatibility.json` sets
  `provider_runtime.abi_version = "provider-adapter-v2"` — the runtime
  executes the world the manifest schema names.

## 4. Obligations

- The runtime's dependency tree stays IR-free; the boundary gate's carve-out
  (conformance crate only) is the standing enforcement, and any future
  runtime dependency on the kernel mirror is a red gate, not a review nit.
- Guest builds are child cargo invocations against `wasm32-wasip2` (CI
  installs the target); the official component's manifest declares the
  `south_runtime` it was verified with and is updated per release by the
  same tests that would fail on drift.
- The 64-stream cap, 10 ms epoch tick, and default limits are runtime
  policy, host-overridable via `RuntimeLimitsV1`; changing a default is a
  design-record note, not a silent bump.

## 5. Versioning

Ships as **0.10.0**. Hosts are unaffected until S4/S5: nothing consumes the
runtime yet. S4 packages the community host's migration onto
`SandboxedComponentV1`; S5's per-batch admission composes gate ① +
`compatibility_matches` + gate ② + this runtime's gate ④ enforcement.
