# The Buffered Binary Response: Bytes Without a UTF-8 Proof

Status: **draft for ruling** — D1–D9 in §6 are proposals, not decisions

Date: 2026-09-09

Predecessors: `2026-08-16-minimal-provider-call.md` (the JSON POST shape everything else extends,
and the origin of the UTF-8 response body), `2026-09-08-buffered-get-request.md` (T1, whose D1
established that a new shape gets its own type rather than a field on the frozen one),
`2026-09-09-multipart-request-body.md` (T2, which applied that rule to the request body and whose
D4 is the reason §6 refuses to guess at a cap).

## 1. Problem

`BufferedHttpResponseV1::try_from_parts_with_response_metadata` ends with

```rust
let body = String::from_utf8(body).map_err(|_| TransportErrorV1::ResponseBodyNotUtf8)?;
```

Every buffered response South can produce is therefore UTF-8 by construction, and any upstream
that answers with audio or image bytes fails the call at the transport boundary. This is the
host's **T3**, the third and last of the transport-shape gaps its 37-号 ledger opened.

T3 is the smallest of the three by blocked-model count, and the ledger says so plainly: T1 blocked
70 models, T2 blocked 34, T3 blocks 11. Those 11 are 9 text-to-speech models whose upstream answers
in bytes, plus 2 `reve` image models without an edit path. A further 4 `reve` models are counted
`T2+T3`; §4.1 shows that count is wrong, because the `reve` native surface is a JSON POST on every
family. All 15 are blocked by T3 alone.

The ledger's own route table names the shapes:

| Route | Outbound body | Upstream response |
|---|---|---|
| `POST /v1/text-to-speech/{voice_id}` | JSON | binary `audio/mpeg` |
| `POST /v1/audio/speech` | JSON | binary `audio/mpeg` |
| `POST /v1/image/{create,edit,remix}` | JSON | JSON **or** binary, by `Accept` |
| ideogram image generation | multipart | JSON, then a second GET for the image |

Two things follow from that table, and both narrow the slice.

**The request side needs nothing.** `accept` is not in either reserved-header list, so a host that
must send `Accept: image/png` to make an upstream answer in bytes can already do it through
`SafeHeaders`. T3 is purely a response-side change.

**Streaming is already unaffected.** The ledger records binary streaming as unrestricted, and it
is: `StreamingCallV1` delivers bounded chunks with no UTF-8 step anywhere. Only the buffered path
has the constraint.

## 2. Boundary claim

Unchanged, and this slice narrows it rather than widening it. South gains no decoder, no media
sniffing, no allow-list of acceptable payload types, and no opinion about what an audio frame or a
PNG chunk is. It gains one thing: the ability to hand back a bounded response body it did not have
to prove was text.

The whole of the risk is in not letting that removal leak into the text path two hosts already
depend on. Two facts keep the door narrow:

1. **The bytes are never converted.** No lossy UTF-8, no normalisation, no re-encoding. What the
   wire carried is what the host receives, which is the byte-identity discipline the multipart
   request body established in the other direction.
2. **The UTF-8 guarantee stays where it is.** Nothing about the existing buffered response
   changes. A consumer of `BufferedHttpResponseV1` keeps the same `&str` it has today, proved the
   same way, and cannot be handed bytes by accident.

## 3. Design

### 3.1 Contract (`south-contracts`)

A new response type beside the frozen one, reusing every metadata contract it already carries:

- `BufferedBinaryResponseV1 { status, body: Vec<u8>, content_type, retry_after,
  provider_quota_metadata, response_diagnostics, response_transcript }`.
- `MAX_BINARY_RESPONSE_BODY_BYTES = 64 * 1024 * 1024`, beside the text path's 32 MiB and for the
  reason D4 gives. The text cap does not move.
- `try_from_parts_with_response_metadata` mirrors the text twin exactly, minus the
  `String::from_utf8` step: the same redirect refusal, the same
  `validate_response_metadata` on `content-type` and `retry-after`, and the byte check against the
  binary cap.
- `body() -> &[u8]` plus `body_len()`. No `as_str`, no `text()`, no `is_utf8()`. A host that wants
  text calls `str::from_utf8` itself and owns the failure.
- The `Debug` impl reports `body_byte_count` and never bytes, matching the text twin's redaction.

`ProviderQuotaMetadataV1`, `ResponseDiagnosticsV1` and `ResponseTranscriptV1` are reused verbatim.
No response metadata contract depends on the body being text, so duplicating the three would be
pure cost.

**No new error codes.** `ResponseBodyNotUtf8` never fires on this path; every other
`TransportErrorV1` variant applies unchanged. The frozen transport taxonomy is untouched, so the
host's error mapping needs no new arm and no wildcard.

### 3.2 Seam (`south-core`)

A third transport trait beside the two that exist:

```rust
pub type BinaryTransportFutureV1<'a> =
    Pin<Box<dyn Future<Output = Result<BufferedBinaryResponseV1, TransportErrorV1>> + Send + 'a>>;

pub trait AsyncBinaryHttpTransport: Send + Sync {
    fn execute_binary<'a>(
        &'a self,
        request: &'a PreparedHttpRequestV1<'_>,
        remaining_timeout: Duration,
    ) -> BinaryTransportFutureV1<'a>;
}
```

The entry points reuse the `execute_buffered` flow verbatim: the same destination resolution, the
same `CredentialBindingMismatch` check, the same three credential arms, the same biased
cancellation race with the same `CANCELLED` precedence. Only the terminal transport call and the
return type differ. The shared body is extracted once so the two flows cannot drift.

### 3.3 Transport (`south-transport-reqwest`)

`ReqwestTransportV1` gains a second impl over the same client. The two execution bodies are the
same function up to the final statement: identical redirect refusal, identical header reads in the
order the existing comment pins ("every response-header read must happen before `read_bounded_body`
consumes `response`"), and the same streaming read, which already returns `Vec<u8>`. The binary arm
constructs the binary type instead of the text one.

Two things become cap-parameterised rather than constant, because the two paths no longer share one
limit: the `content-length` pre-check (`lib.rs:150`) and `read_bounded_body`'s running total
(`lib.rs:669`). Both take the cap as an argument; neither grows a branch.

No new client, no new hardening surface, no second connection pool.

### 3.4 Conformance

A dedicated `south.provider-binary.v1` suite, on the header-auth, controlled-query, provider-get
and provider-multipart precedents. The frozen `south.provider-call.v1` table is burned into two
hosts' evidence and a binary response is a different exchange shape.

The case table must include at least one **non-2xx carrying a JSON body**, because that is the
shape D3 exists to settle, and one body containing a byte sequence that is not valid UTF-8, whose
whole point is that it now succeeds.

## 4. Consumer

Read from the server host's source, because three of the decisions below turn on what is actually
there rather than on what the ledger's route table implies.

### 4.1 Every blocked call site is a JSON POST

| Call site | Outbound | Body read | Upstream answer |
|---|---|---|---|
| `/v1/audio/speech` | JSON POST | `handler/audio/tts.rs:667` | binary, `content-type` defaulted to `audio/mpeg` at `tts.rs:660` |
| `/v1/text-to-speech/{voice_id}` | JSON POST | `handler/elevenlabs.rs:172` | binary, same default at `elevenlabs.rs:165` |
| `/v1/image/{create,edit,remix}` | JSON POST | `handler/reve.rs:1190` | JSON **or** binary, decided by the client's `Accept`, forwarded verbatim at `reve.rs:1123` |

The ledger counted four `reve` models as `T2+T3`. The code says otherwise: the `reve` native
surface sends `content-type: application/json` (`reve.rs:1123`) and is a JSON POST on every family,
edit included. **Those four are blocked by T3 alone.** Nothing in this slice needs a multipart
binary twin.

The ledger also marks the ideogram image fetch `T1/T3`. Two host paths do read binary over a GET —
the Bailian Qwen3-TTS second leg (`handler/audio/tts_providers.rs:739-779`) and the ideogram CDN
fetch (`handler/images/ideogram.rs:596`) — and **neither can go through South at all**, with or
without T3. Both are absolute, ephemeral, non-provider URLs, and a `GetRequestV1` resolves a
relative path against a bound provider's base URL. They are host artifact fetches, not provider
calls. No binary GET twin.

### 4.2 The host's own cap is 64 MiB, and South's is 32

The gateway reads every buffered upstream success body — JSON and binary alike — through one
function, `read_json_body_capped` (`engine/body_cap.rs:130`), whose limit is
`UPSTREAM_JSON_BODY_CAP = 64 MiB` (`body_cap.rs:27`), hard-failing to 502 above it. South's
`MAX_RESPONSE_BODY_BYTES` is 32 MiB. On the text path the difference has never mattered. On the
binary path it is exactly T2's D4 hole in a new place, and the host's own artifact limits sit above
32 MiB: ideogram caps a single image at 50 MiB (`images/ideogram.rs:159`).

### 4.3 What the host must build to adopt

Three costs, none of them South's to pay, all worth stating so the slice is not mistaken for
turnkey:

1. **A ninth kill-switch surface.** `SouthSurface` (`engine/south_switch.rs:50-71`) has eight
   variants and none covers text-to-speech or image generation. Adoption adds one, default off
   until parity, as `TaskPoll` and `Multipart` both did.
2. **One `&str` assumption to unpick.** `south_adapter::truncate_error_body`
   (`engine/south_adapter.rs:650`) takes a `&str` and walks `is_char_boundary` backwards; four call
   sites pass `response.body()` straight in. On a binary response the host converts through
   `String::from_utf8_lossy` first, which is what the stream-rejection arm already does at
   `sender.rs:186`. The precedent is in the tree.
3. **A billing branch that reads the body.** `reve.rs:1196-1204` parses `credits_used` out of the
   JSON body when the answer is JSON and falls back to the `X-Reve-Credits-Used` header when it is
   binary. Neither present on a 2xx is a loud 502. That branch is untouched by this slice, but it
   is the reason a `reve` adoption is heavier than a text-to-speech one.

### 4.4 Scope honesty

T3 unblocks the **contract** for 15 models: the ledger's 11, plus the 4 `reve` it filed under
`T2+T3` and §4.1 reassigns. The host's own scope decides how many actually route on day one, and it
is far fewer:

- **Expressible today**: xAI 1, Groq 2, OpenAI `tts-1`/`tts-1-hd` 2. All three ride
  `ProviderType::Openai`, which maps to `SouthAuthKind::Bearer` (`south_adapter.rs:144`). **Five.**
- **Not expressible**: Azure Speech 1 and ElevenLabs 3, whose `south_auth_for` returns `None`
  (`south_adapter.rs:181`) on the same media-surface judgement that kept them out of T2, and which
  is not this record's to overturn. **Four.**
- **`reve` 6** (2 counted `T3`, 4 miscounted `T2+T3`) is Bearer-expressible but needs the new
  surface from §4.3 before any of it moves.

So the honest day-one number is **five**: four more wait on an auth-scope judgement that predates
this slice, and six on the new surface. 5 + 4 + 6 = 15.

## 5. Versioning

Additive; ships as **0.26.0** with `compatibility.json` `http: 8`, the new suite id and suite
version, and updated `crates.*` capability strings for `south-contracts`, `south-core`,
`south-provider-conformance`, `south-testkit` and `south-transport-reqwest`.

No public type changes shape and no enum gains a variant, so unlike 0.25.0 this release breaks no
downstream exhaustive match. Both hosts are annotated `provider_binary: not_verified` until each
runs the suite through its own adapter.

Fuzz obligations do not grow. There is no new grammar, because the binary body is never parsed.

## 6. Decisions — proposed, awaiting ruling

- **D1 — a separate `BufferedBinaryResponseV1` versus widening `BufferedHttpResponseV1`.**
  **Recommend separate.** Widening makes `body` a `Vec<u8>`, and then `body()` returns either
  `Option<&str>`, which breaks both hosts at every call site, or a lossy conversion, which
  silently corrupts. Worse than either: widening strips the UTF-8 *guarantee* from the text path,
  so every existing consumer inherits a weaker contract it never asked for. This is the rule T1's
  D1 and T2's D1 both settled, applied on the response side.

- **D2 — a third transport trait versus a method on `AsyncHttpTransport`.**
  **Recommend a third trait.** `AsyncHttpTransport::execute` returns
  `Result<BufferedHttpResponseV1, _>` by signature, so the response shape cannot vary at the call
  site. Adding a required method to a public trait breaks every host implementation, and a
  defaulted one returning "unsupported" ships a capability that silently is not there. The
  streaming path answered this once by taking `AsyncStreamingTransport` and its own head type.

- **D3 — bytes on every status, including a non-2xx.**
  **Recommend as designed.** A text-to-speech 200 is audio and its 400 is JSON. A type whose body
  shape depends on the status would be the worst of both worlds. The binary entry point returns
  bytes for every status, and a host reading an error body calls `str::from_utf8` and owns the
  failure. South does not inspect a response body today, and this slice does not start.

- **D4 — the byte cap.** **Recommend a dedicated `MAX_BINARY_RESPONSE_BODY_BYTES = 64 MiB`**,
  matching the host's `UPSTREAM_JSON_BODY_CAP` exactly rather than reusing the 32 MiB text cap.
  This is T2's D4 argument with the numbers from §4.2 substituted: a South cap below the host's own
  limit silently falls back to legacy for artifacts between 32 and 64 MiB, in exactly the models
  the slice exists to unblock, and the host already tolerates a 50 MiB single image. Reusing 32 MiB
  keeps one number for one concept, which is the only thing to be said for it. The text cap is not
  touched.

- **D5 — no content-type allow-list, no sniffing, no conversion.**
  **Recommend as designed.** South bounds the `content-type` value at 256 bytes and hands it over.
  Which media types are acceptable is host policy, and an allow-list would break the first time an
  upstream answers `audio/mpeg;codecs=mp3`.

- **D6 — which request shapes get a binary twin.** **Recommend JSON POST only.** §4.1 settles
  it from the source: all three blocked call sites are JSON POSTs, the `reve` edit path the ledger
  filed as `T2+T3` is a JSON POST too, and the two binary GETs are absolute-URL artifact fetches
  South cannot carry with or without this slice. A GET or multipart binary twin would be a
  reservation with no consumer, which is what `AGENTS.md` forbids and what got
  `south-transport-ureq` deleted. One entry point, `execute_binary_call_v1`.

- **D7 — a raw twin.** **Recommend none for now.** The host's binary call sites already hold a
  typed provider and model config, and its existing adapter wrappers around the JSON POST arm are
  typed (`south_adapter.rs:711`). If the adoption turns out to want the raw shape, it is an
  additive follow-up with a real consumer behind it, which is the right time to add it.

- **D8 — a dedicated `south.provider-binary.v1` suite.** **Recommend dedicated**, on the
  header-auth, controlled-query, provider-get and provider-multipart precedents.

- **D9 — streaming stays untouched.** **Recommend as designed.** Binary streaming is already
  unrestricted. No streaming binary type and no second head shape.
