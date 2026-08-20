# Component Conformance Gates ① and ② (S2)

Status: D1–D5 ruled 2026-08-21 (the release number ruled explicitly; the rest
follow the S0/S1 contracts); shipped as 0.9.0.

Date: 2026-08-21

Predecessors: `2026-08-21-canonical-ir-inventory.md` (S0 — the four-gate
layering §, the gate-② obligations of rulings D1–D5), 
`2026-08-21-provider-api-promotion.md` (S1 — the manifest schema gate ① runs,
the frozen suite name `south.provider-component.v1`), and the community host's
`crates/conformance` (donor: three-gate runner, 1,418 lines) and
`plugins/official/provider-openai-compatible` (donor: the official adapter,
890 lines, WASM guest form).

## 1. Problem

S1 gave components an ABI and an admission schema; nothing yet judged what a
component *does*. The plan's ordering is deliberate — the judges exist before
the runtime (S3), so the runtime only ever has to prove "the sandboxed output
equals the native output" instead of simultaneously defining correctness and
running WASM.

## 2. What ships (`south-component-conformance`, new crate)

- **Gate ①, three halves**: `accepts_manifest` (the S1 schema's `validate`),
  `reported_identity_matches` (loaded `metadata()` == declared manifest
  identity), and `compatibility_matches` — the seven-field tuple handshake
  against `HostExpectationsV1` (the four values only a live host knows;
  the other three are exact-validated by the schema). Refusal in tuple order,
  never silent degradation.
- **Gate ②**: `run_provider_component_suite_v1` over a `FixturePackV1`
  (`provider.<family>.<case>.{input,expected}.json` pairs; 2 MiB per-file
  bound; unknown families refused by name as package errors, not conformance
  failures). Checks, per the donor's closed catalog: `Coverage`,
  `FixtureMatch` (byte-exact after canonical serialization), `Determinism`,
  `UnknownFieldTolerance`, `EndpointConfinement` (runs
  `ProviderConfig::authorize` on what the component *built*),
  `AuthErrorsAreNotRetriable` (with the donor's "the missing 401 fixture is
  itself a failure" rule), and `StreamIncrementality` — strengthened from the
  donor's char-boundary re-splitting to **every byte boundary**, because the
  v2 ABI carries raw bytes (S0 D2) and a socket split can land inside a UTF-8
  sequence.
- **The typed component seam**: `ProviderComponentV1` + `StreamParserV1`
  (bytes in, `Vec<StreamEvent>` out; EOF = empty chunk, and the suite always
  drives EOF so flush behavior is judged too). The runtime (S3) becomes an
  implementation of this seam over a WASM instance; a component author runs
  the same suite against a native build without a WASM toolchain.
- **The native reference**: `reference::OpenAiCompatibleReferenceV1`, the
  donor guest ported with two structural changes — stream state moves from a
  guest-global into a per-stream parser instance, and the SSE tail buffers
  bytes. Its translation logic is otherwise the donor's, line for line where
  possible, so S4's packaging step wraps this logic rather than rewriting it.
- **The fixture pack**: the donor's nine pairs ported verbatim, plus five
  S0-obligated rows — `stream.usage-terminal` (finish held until the final
  accounting event), `stream.duplicate-usage` (usage relayed exactly as often
  as the upstream reports; folding is the host's `absorb`),
  `stream.missing-terminal` (EOF flush emits the held finish),
  `response.reasoning` (thinking lift + `reasoning_tokens`, the
  thinking-marker derivation source), `response.cached-usage` (the OpenAI
  dialect's subset cache convention pinned). The D4 catalog-inclusion
  assertion (sanctioned headers ⊆ IR credential catalog) and the D5
  unknown-field round-trip run in the same test binary.

## 3. Decisions — ruled 2026-08-21

- **D1 — a new crate, not an extension of `south-provider-conformance`.**
  Gate ② is the one sanctioned typed consumer of the Canonical IR (S0
  invariant 6) and therefore carries `token-station-protocol` pinned to a
  kernel distribution tag (**v0.2.0** = token-station v1.2.3, the S0
  baseline). Folding that into the existing host-suite crate would push the
  kernel dependency into every host's dev tree and touch three frozen suites
  for no reason. The dependency-graph note in `ARCHITECTURE.md` names the
  edge as the sanctioned exception; `deny.toml` allows exactly this git
  source.
- **D2 — the reference lives in this crate for now.** S4's stated job is
  "`provider-openai-compatible` 搬家为第一个官方组件": the packaging step
  (wit-bindgen wrapper + component build) will lift `reference` into the
  official component crate. Splitting it out today would create a crate whose
  only consumer is this suite.
- **D3 — the suite always drives EOF.** The donor's runner fed fixture chunks
  and never called `finish()`; missing-terminal behavior was unjudgeable.
  Every stream's lifecycle includes EOF, so `feed` appends it — a no-op for
  `[DONE]`-terminated fixtures (verified by the ported pack passing
  unchanged) and the judge for the new missing-terminal row.
- **D4 — byte-level incrementality.** Supersedes char-level: strictly more
  splits, including mid-UTF-8-sequence ones the bytes ABI makes reachable.
- **D5 — version: 0.9.0** (ruled explicitly: the number previously reserved
  for the host-signed finalizer was released to this slice, and version
  pre-allocation is retired — future slices take the next minor at ship
  time).

## 4. Obligations

- Gate ② is frozen against the native reference from this release: the
  shipped pack passing `the_reference_implementation_passes_the_component_behavior_suite`
  is the reference run S3 must reproduce byte-for-byte from inside the
  sandbox.
- Growing the pack is additive; changing an existing pair is a contract
  change and needs a design record.
- No production crate gains the kernel edge; review plus the architecture
  note enforce it until S3 adds the CI gate the plan's risk table names.
- The S3 runtime inherits `StreamParserV1`'s EOF convention (empty chunk) and
  the per-stream instance rule.

## 5. Versioning

Ships as **0.9.0**. `compatibility.json` gains
`provider_component_suite_id = "south.provider-component.v1"` (version 1) and
the crate descriptor `provider_component_gates_reference_v1`. Hosts are
unaffected: nothing consumes the new crate yet; it exists so S3 can prove the
sandbox against it and S4/S5 can gate admissions with it.
