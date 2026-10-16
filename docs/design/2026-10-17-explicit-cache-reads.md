# Explicit cache reads in the IR: `provider-openai-compatible` reports Bailian's explicit-cache mode

Status: accepted (lv, 2026-10-17: "south kernel IR explicit/implicit two buckets", south half assigned to this
repository). Implemented; unreleased (0.72.0).

Date: 2026-10-17

Baseline: south `origin/main` = `2b4546c` (v0.71.0). Host references carry a `server:` prefix and refer to
token-station-server `dev-v2`; nothing here was run against an upstream.

Related: [host-zero-vendor boundary](2026-09-30-host-zero-vendor-boundary.md) §6 "Known differences from the host's
evidence" item 1, §11 (work item "IR explicit / implicit cache buckets"), §13.7;
[kernel re-pin to protocol 0.5.0](2026-10-08-kernel-repin-protocol-0.5.0.md).

## 1. What changed and why

The host prices two kinds of cache read apart: an implicit hit at `cached_input_price_per_million` and a read from a
cache the caller created explicitly at `cache_read_input_price_per_million`. Bailian (DashScope's OpenAI-compatible
mode) is the provider that makes the distinction. A component row bills only from the IR, and
`provider-openai-compatible`'s usage mapping put every hit into `cache_read_tokens` without saying which kind it was, so
an explicit read would have been billed at the implicit price. The host therefore keeps any model whose price card
prices the two reads differently on the native arm (`server:gateway/src/modules/inference/engine/ir_usage_evidence.rs`,
G7 `prices_cache_reads_two_ways`).

**No kernel change is needed.** The pinned kernel protocol (`token-station-protocol` 0.6.0) has carried
`Usage::explicit_cache_read_tokens` since protocol 0.5.0: a subset of `cache_read_tokens`, which stays the total of every
cache read, defaulted, and not serialized at zero. `Usage::is_partitioned` checks the subset, and gate ②'s partition
judge has judged it since the 0.5.0 re-pin (`suite.rs` `partition_violation`;
`usage_gate_v1::explicit_cache_reads_beyond_all_cache_reads_break_the_partition`). What was missing was a reference
that fills it. `provider-openai-compatible` 2.11.0 does.

## 2. The rule, ported from the host's native evidence

Source: `server:crates/gateway-provider-protocol/src/usage_evidence.rs`, the OpenAI Chat arm (around lines 458-512 on
`dev-v2`). The reference (`reference.rs` `usage_of`) now reads:

1. **Explicit mode** is the presence of the key `prompt_tokens_details.cache_creation_input_tokens`, even with the value
   0 (a request that only reads an explicit cache reports it at 0). When both write keys are present the Bailian key
   wins (unchanged).
2. **Cache writes**: in explicit mode `cache_creation_input_tokens`, otherwise `cache_write_tokens` (unchanged).
3. **Hits**: `prompt_tokens_details.cached_tokens`, else the top-level `prompt_cache_hit_tokens` (Kimi, DeepSeek), plus
   orchestration's cached subset (`orchestration_input_cached_tokens`), as `cache_read_tokens` (unchanged).
4. **New**: in explicit mode `explicit_cache_read_tokens = cache_read_tokens`: every hit, orchestration's included, is
   an explicit read. Native puts all of them in its explicit-read bucket (`cache_read_input_tokens`) and leaves the
   implicit bucket (`cached_input_tokens`) at 0. Otherwise `explicit_cache_read_tokens` is 0.
5. **Subset check** unchanged: hits + writes must not exceed the prompt (with orchestration's input), else a protocol
   error.

The streaming path builds its usage through the same function (`events_of_frame` calls `usage_of` for a frame with a
`usage` object, as `parse_response` does), so a stream follows the same rule.

**Families.** The rule is keyed on the wire, not on the family, exactly as native reads it: native applies it to every
OpenAI Chat-shaped upstream, not only to rows of the Bailian type. So it applies to every family of the package
(`openai-compatible`, `azure-openai-v1`, `azure-openai-dated`, `github-copilot`, `gemini-openai-compatible`,
`glm-coding`, `kimi-code`) and to Bedrock Mantle's `openai-compatible` row. An upstream that never sends
`cache_creation_input_tokens` sees no change: `explicit_cache_read_tokens` stays 0 and is not serialized, so its IR
wire is byte-identical to 2.10.1. No other package changes behaviour: `usage_of` is private to this reference, and the
Responses, Anthropic, Gemini, Converse, Invoke and Kiro references keep reporting 0.

## 3. The field crosses the component boundary unchanged

- **Guest.** `abi::parse_response_json` and `StreamAbiV1::parse_chunk_json` serialize the kernel's `ChatResponse` /
  `StreamEvent` with serde; `explicit_cache_read_tokens` is written whenever it is non-zero. The component's lock pins
  `token-station-protocol` 0.6.0 at the kernel revision the workspace pins.
- **WIT.** `parse-response` and `parse-stream-chunk` return that JSON as a string; nothing on the WIT side names usage
  fields.
- **Host side of the runtime.** `south-provider-runtime` hands the guest's JSON back as a string
  (`component.rs` `call_parse_response`); the conformance sandbox (`sandbox.rs`) and the host decode it into the same
  kernel type at the same revision (`server:Cargo.toml` pins rev `356445c`), so the field survives decoding.
- **North codecs.** `south-north-codec` renders `cache_read_tokens` as the client's cached count and does not read the
  explicit part, so no client-visible output changes (the explicit part is a subset of what it already renders).

Nothing between the reference and the host drops it. The host does not read it yet (§5).

## 4. Tests and fixtures

- `usage_ir_contract_v1`: explicit mode with hits and writes; with the creation key at 0; with the top-level hit
  count; both write keys present; orchestration's cached subset in explicit mode; hits and writes beyond the prompt
  refused; implicit mode by `cached_tokens`, by `prompt_cache_hit_tokens`, with OpenAI's `cache_write_tokens` and with
  orchestration; the stream path with and without the creation key.
- Gate ② pack (`fixtures/`): `provider.response.bailian-explicit-cache` (a read-only request: `cached_tokens` 2048,
  `cache_creation_input_tokens` 0) and `provider.stream.bailian-explicit-cache` (hits 1024 and writes 512 in one
  report). Their expected IR carries `explicit_cache_read_tokens`, so the sandbox parity run proves the field crosses
  the wasm boundary.

## 5. Host follow-up (not part of this change)

Once the host pins a south release carrying `provider-openai-compatible` 2.11.0:

- `server:parse_exact_usage_from_ir_with_thinking` maps `explicit_cache_read_tokens` to `cache_read_input_tokens`
  (billed at the explicit-read price) and `cache_read_tokens - explicit_cache_read_tokens` to `cached_input_tokens`
  (billed at the implicit price), refusing a report whose explicit part exceeds `cache_read_tokens`; the stream
  accumulator does the same with the folded usage.
- The G7 fallback (`prices_cache_reads_two_ways` in `ir_billing`) is dropped: the IR now says which reads are explicit.
- Its usage dual run gains the explicit-cache case.

## 6. Differences that remain

- A `null` count: native refuses a `null` value of a count it reads (`optional_nonnegative` requires an integer); the
  reference reads `null` as absent, as it already did. A `null` `cache_creation_input_tokens` is therefore present
  (explicit mode) with 0 writes in the reference, refused by native. Pre-existing, unchanged here; no upstream is
  known to send it.
- Folding: the kernel's `Usage::absorb` keeps the last non-zero value per field, so a stream that reported an explicit
  read and later a report without one would keep the earlier explicit count. This reference reports usage once per
  stream (a second report is the `provider.stream.duplicate-usage` shape, which carries no cache counts), so the case
  does not arise.

## 7. Versions (Q47)

Editing `south-component-conformance` opens the 0.72.0 cycle's Q47 round against v0.71.0: the crate moves 0.70.0 →
0.71.0. `provider-openai-compatible` reports a new bucket, a minor (2.10.1 → 2.11.0); every other official package takes
a patch with unchanged behaviour. No package moves its `south_runtime` or its `contracts`; no contract number changes;
the kernel pin is unchanged (`shipped_packages_v1::explicit_cache_reads_retires_every_published_071_package_identity`).
