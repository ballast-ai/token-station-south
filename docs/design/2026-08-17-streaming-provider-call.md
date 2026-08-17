# Streaming Provider Call Vertical Slice

Status: implemented on main and released as v0.1.0 (158 workspace tests, all gates green);
independently reviewed — APPROVED with two P1 findings, both fixed (per-pull deadline observation;
source script state moved inside the pull future + cancel-safety pinning tests at three layers).
Remaining P2 notes live in the review record. Host adoption for streaming is separate future work.

Date: 2026-08-17

Predecessor: `2026-08-16-minimal-provider-call.md` (the buffered slice; its boundaries, secret
handling, and conformance philosophy carry over unchanged unless this document says otherwise).

## 1. Problem

The buffered slice deliberately excluded streaming. Both hosts need it next, and their needs pin
the design from two sides:

- **token-station-server** (verified host): its production chat relay consumes upstream bytes as a
  pull-based `bytes_stream()` inside a `tokio::select!` that also drives a 30-second database
  lease heartbeat. Roughly a third of its streaming surface (AWS Bedrock) is **not SSE** — it is
  the binary `application/vnd.amazon.eventstream` wire, decoded host-side with CRC checks and a
  16 MiB frame bound. SSE framing, cross-frame translation state, exact usage evidence, terminal-
  frame withholding ("persist terminal before client visibility"), and settlement are all
  host-owned money paths.
- **token-station** (community host, provider-call adopted but streaming not adopted): its
  acceptance checklist (§1.8) sketches a South-driven loop that feeds an SSE frame decoder and a
  plugin `parse_stream_chunk`, returning `StreamEvent`s through a callback. That loop depends on
  the provider WIT runtime, which is still a placeholder crate in this workspace.

A single primitive cannot serve both altitudes at once without either baking SSE into the
transport contract (which locks Bedrock out) or shipping a plugin runtime prematurely.

## 2. Ruling D1 — two layers, this slice ships layer 1 only

**Layer 1 (this slice): a byte-level streaming transport contract.** South owns: binding
validation, credential resolution, the hardened transport, a headers-ready stage, bounded
error-body buffering on non-2xx, and a pull-based bounded chunk stream with cancellation and
stall-guard semantics. South does **not** parse SSE, does not know about frames, events, usage,
translation, or settlement.

**Layer 2 (a later slice, gated on the provider WIT runtime): the SSE/event loop for the
community host.** The `SseFrameDecoder` migration from `token-station` (checklist §1.7) lands
there, together with `parse_stream_chunk` orchestration. It composes on top of layer 1; nothing in
layer 1 anticipates it beyond staying byte-transparent.

Rationale: the verified host can adopt layer 1 immediately at its one clean seam (the
`http_client.execute` call inside its durable sender); layer 2 without a WIT runtime would have no
consumer; and byte-transparency is the only shape that serves SSE and eventstream alike.
Consequence for the community host's checklist: §1.8's "South drives the decoder and the plugin"
remains the layer-2 target, unchanged in substance, deferred in sequence.

## 3. Goals and non-goals

Goals (layer 1):

1. One streaming call: buffered JSON POST request, streamed response body.
2. Headers-ready stage: status and response metadata are handed to the host before any body byte
   is pulled, so the host can branch (non-2xx, settlement markers) without touching the stream.
3. Non-2xx responses never yield a stream: South buffers a bounded error body and returns a
   terminal outcome.
4. Pull-based chunk delivery that composes with the host's `tokio::select!` loops (lease
   heartbeats, client-side keepalive) — never a South-owned push loop.
5. Cancellation observed between pulls and honored mid-pull (drop = abort I/O, as in the buffered
   slice).
6. Stall-guard timeout semantics matching production reality: optional total deadline, mandatory
   idle guard.
7. A frozen mid-stream error taxonomy the host can map onto its own outcome machine
   (`delivery_unknown`, `FailedAfterPartial`-style classification stays host-side — the host
   counts what it has already forwarded; South does not).

Non-goals (layer 1): SSE parsing or frame bounds; eventstream decoding; `StreamEvent` or any
event model; retry/fallback; usage extraction; keepalive injection; multiplexing several calls on
one object; request-body streaming; trailers. The reserved-header policy, path containment,
secret handling, and redirect denial all carry over from the buffered slice verbatim.

## 4. Contract additions (`south-contracts`)

### 4.1 Stream contract version

`STREAM_CONTRACT_VERSION: Option<u16>` moves from `None` to `Some(1)`. `compatibility.json`
mirrors it (`contracts.stream: 1`).

### 4.2 New limits

| Constant | Value | Note |
|---|---|---|
| `MAX_STREAM_ERROR_BODY_BYTES` | `64 * 1024` | Bound for the buffered non-2xx error body. Matches the verified host's `UPSTREAM_ERROR_BODY_CAP`; deliberately far below the 32 MiB buffered-response cap because streaming error bodies feed failure classifiers, not clients. |
| `MAX_STREAM_CHUNK_BYTES` | `64 * 1024` | Upper bound on a single yielded chunk. Transport-level re-chunking is allowed and expected; this is a delivery-size guarantee to the host, not a protocol frame bound. |

Deliberately **no cumulative body cap** for 2xx streams: a long generation is legitimate wall-clock
work (the verified host runs streams with no total client timeout), and any global cap would be a
new product decision, not hardening. Hosts that want one enforce it by counting pulled bytes.

### 4.3 Streaming timeout shape

`ReqwestTransportConfigV1` (total, connect, read — all mandatory, total ≤ 24 h) cannot express
"no total limit", which is production reality for streams. New config type instead of overloading:

```rust
pub struct StreamTransportConfigV1 {
    total_timeout: Option<Duration>,   // None = unbounded wall clock (production streaming shape)
    connect_timeout: Duration,         // > 0
    idle_timeout: Duration,            // > 0; stall guard between reads; also caps TTFB
}
```

Validation: `connect_timeout > 0`, `idle_timeout > 0`; when `total_timeout` is `Some`, it obeys
the existing `MAX_TRANSPORT_TIMEOUT` cap and must be ≥ both others. The idle guard doubles as the
time-to-first-byte bound (matching the verified host's documented `read_timeout` semantics; no
separate TTFB knob in v1). The caller-supplied absolute deadline becomes `Option<Instant>` on the
streaming entry point: `None` is legal only because the idle guard is not — every silent upstream
dies within `idle_timeout` regardless.

### 4.4 Mid-stream error taxonomy

Pre-stream failures reuse the existing frozen codes unchanged (`INVALID_*`, `URL_OUTSIDE_BINDING`,
`CREDENTIAL_*`, `CANCELLED`, `DEADLINE_EXCEEDED`, `CONNECT_FAILED`, `REQUEST_FAILED`,
`REDIRECT_DENIED`, `CLIENT_BUILD_FAILED`, `TRANSPORT_TIMEOUT`). New closed enum for failures after
the stream opened:

```rust
pub enum StreamReadErrorV1 {
    StreamReadFailed,      // STREAM_READ_FAILED     — upstream broke mid-body
    StreamIdleTimeout,     // STREAM_IDLE_TIMEOUT    — idle guard fired between chunks
    StreamDeadlineExceeded,// STREAM_DEADLINE_EXCEEDED — caller deadline fired mid-stream
    StreamCancelled,       // STREAM_CANCELLED       — caller token fired mid-stream
    ChunkNotDeliverable,   // STREAM_CHUNK_INVALID   — transport produced an undeliverable chunk
}
```

Mid-stream codes are distinct from their pre-stream cousins on purpose: the host's money paths
treat "failed before any byte" and "failed after N bytes" differently, and the host is the one
counting bytes — South's contribution is making the phase unambiguous from the code alone.

Streaming responses are **not** `BufferedHttpResponseV1`: a new `StreamingResponseHeadV1` carries
`status`, `content_type`, `retry_after` under the existing metadata bounds. 3xx is refused at the
transport as today (`REDIRECT_DENIED`); 4xx/5xx short-circuit into
`StreamRejectedV1 { head, bounded error body }` before any streaming state exists.

## 5. Core orchestration (`south-core`)

One new entry point, mirroring the buffered one:

```rust
pub async fn open_streaming_provider_call_v1<R, T>(
    binding: &ProviderBindingV1,
    request: &JsonPostRequestV1,
    resolver: &R,
    transport: &T,
    deadline: Option<tokio::time::Instant>,
    cancellation: &CancellationToken,
) -> Result<StreamingCallV1, ProviderCallErrorV1>
```

Phases (1–3 identical to the buffered slice): URL containment → slot match → pre-flight
cancellation/deadline check → resolve secret → prepared request → transport opens the exchange and
returns at **headers-ready**. `StreamingCallV1` then exposes:

```rust
impl StreamingCallV1 {
    pub fn head(&self) -> &StreamingResponseHeadV1;            // always 2xx here
    pub async fn next_chunk(&mut self) -> Option<Result<StreamChunkV1, StreamReadErrorV1>>;
}
```

- `next_chunk` is a plain async method: its future is `select!`-compatible and cancel-safe
  (dropping the future between chunks loses nothing; the host's heartbeat legs poll alongside it).
  `None` means clean upstream EOF. After any `Err`, subsequent calls return `None`; the host still
  owns whatever async work (settlement writes) it does with the error before dropping the call.
- The deadline and cancellation token given at open time stay armed for the whole stream:
  cancellation and deadline are checked on every pull, and firing mid-pull aborts the in-flight
  read (mapped to `STREAM_CANCELLED` / `STREAM_DEADLINE_EXCEEDED`).
- Dropping `StreamingCallV1` aborts the connection. There is no detached work to leak.
- Non-2xx path: `open_…` returns `Ok`? No — ruling: **non-2xx is an `Err` variant carrying
  `StreamRejectedV1`** (`ProviderCallErrorV1` gains a `Rejected(StreamRejectedV1)` arm scoped to
  the streaming entry point). The host gets status + bounded body in one shot and no stream object
  ever exists for a rejected exchange. This keeps "a `StreamingCallV1` is always a live 2xx
  stream" as a type invariant.

The one-shot `CredentialResolver` and `SecretValue` zeroization story is unchanged; the bearer
header owner lives as long as the exchange, so the transport zeroizes it when the stream closes
(same guarantee wording and caveats as the buffered slice).

## 6. Transport (`south-transport-reqwest`)

`ReqwestStreamingTransportV1`, constructed from `StreamTransportConfigV1`. Hardening carries over:
no proxy, `redirect::Policy::none()`, retry never, referer off, **all decompression disabled** —
byte transparency is part of the contract for streams, not just hygiene (host-side eventstream CRC
checks would break under transparent decompression). One additional request header obligation on
the host, not the transport: the host sets its own `accept` header (`text/event-stream`,
`application/vnd.amazon.eventstream`, …) through the normal `SafeHeaders` path — `accept` is not
on the reserved list and South does not guess it.

Timeout wiring: `connect_timeout` on the builder; the idle guard implemented per read await; the
optional total as an outer bound. reqwest's own `read_timeout` covers the header-wait phase, which
is what makes the idle guard double as the TTFB bound.

## 7. Conformance (`south-provider-conformance` + `south-testkit`)

New suite `south.provider-stream.v1`, same philosophy: fixed fixture table, assembled-executor
runner, adapter-reported evidence (resolver/transport call counts, pending-drop flags — now plus
`chunks_pulled` and `poststream_error_code`), redacted debug everywhere, no internal watchdog,
caller-owned virtual-clock pattern. Draft case list:

| # | Case | Verifies |
|---|---|---|
| 1 | `StreamSuccess` | headers-ready → N chunks byte-identical → clean EOF (`None`) |
| 2 | `RejectedUpstreamStatus` | 4xx short-circuits into bounded error body; no stream object; evidence: one resolve, one transport open, zero chunks |
| 3 | `RedirectDenied` | 3xx refused before any body pull |
| 4 | `CancelBetweenChunks` | token fires between pulls → `STREAM_CANCELLED`, in-flight future dropped |
| 5 | `IdleTimeoutMidStream` | silent upstream after chunk 1 → `STREAM_IDLE_TIMEOUT` at the virtual idle bound |
| 6 | `DeadlineMidStream` | absolute deadline fires while a pull is pending → `STREAM_DEADLINE_EXCEEDED` |
| 7 | `UpstreamBreakMidStream` | transport error after chunk 1 → `STREAM_READ_FAILED`; subsequent pulls return `None` |
| 8 | `InvalidRelativePath` | contract parse fails before resolver/transport (shared with buffered suite semantics) |
| 9 | `ErrorBodyTooLargeIsTruncated` | rejected-path error body is truncated at the bound, not failed |

Evidence discipline: as before, a passing report is insufficient — the host-adoption review must
confirm the counters and drop guards wrap the real boundaries.

## 8. Host adoption criteria (mirrors the buffered slice)

`token-station-server` becomes verified **for streaming** only when: (a) its real chat SSE seam
(the durable sender's execute call) runs through `open_streaming_provider_call_v1` for at least
the pure-Bearer SSE scope, (b) its assembled executor passes `south.provider-stream.v1`, (c) a
wiring review confirms the evidence, and (d) the settlement invariants (marker before send, no
refund after marker, terminal-frame withholding) are shown unchanged by the host's own test legs.
Bedrock's eventstream legs are in scope *as consumers of the byte stream* but out of scope for
auth (SigV4 stays host-side; those legs keep their existing transport until the Auth roadmap
lands). `compatibility.json` uses `host_capabilities.<host>.provider_stream` for this status while
the legacy top-level host summary continues to mirror provider-call status.

## 9. Decisions (ruled 2026-08-17, all as recommended)

- **D1 — layering ruling (§2).** Recommended as written; the alternative (SSE-aware primitive)
  locks out Bedrock and is not recommended.
- **D2 — chunk type.** `StreamChunkV1` wrapping `bytes::Bytes` (zero-copy from reqwest, `bytes`
  becomes a public dependency of the contracts) **vs** `Vec<u8>` (no new public dependency, one
  copy per chunk). Recommendation: `Bytes` — the copy cost lands on every streamed token at the
  hottest path of the busiest surface, and `bytes` is a stable, ubiquitous crate.
- **D3 — non-2xx as `Err(Rejected)` (§5).** Recommended as written; the alternative (an `Ok`
  two-armed outcome enum) makes the happy path pattern-match for a stream it statically knows
  exists.
- **D4 — conformance evidence extension.** `chunks_pulled` + `poststream_error_code` as suite
  evidence (recommended), vs keeping the buffered slice's four-field evidence and trusting host
  test legs for stream-phase coverage.

Implementation follows the buffered slice's process: RED conformance fixtures first, then core,
then transport, then the enterprise adoption slice as a separate piece of work.
