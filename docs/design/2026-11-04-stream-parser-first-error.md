# Stream parsers: the first error does not depend on chunking, and a failing scheduled run is visible

Status: **accepted** 2026-11-04 (lv: FQ-1 to FQ-5 as recommended, §7). Implemented on branch `design-stream-first-error`
(pull request #331); F2 ships in release 0.83.1.

Date: 2026-11-04

Baseline: south `origin/main` = `c8979f66` (release 0.83.0 plus #330). The fuzz figures come from the scheduled `ci.yml`
runs `39225533677` (2026-10-19), `40295276877` (2026-10-26) and `41433196607` (2026-11-02). The local reproduction in
§2.2 was run on this baseline.

## 1. Problem

The weekly `fuzz` job has failed on every scheduled run since 2026-10-05, and nothing reported it:

| Run | Failing step | Assertion |
|---|---|---|
| 2026-10-05, 2026-10-12 | Fuzz untrusted headers | the cargo-fuzz musl build could not run the address sanitizer (fixed since: the step names the runner's own target) |
| 2026-10-19 | Fuzz untrusted contract parsers | `contract_parsers.rs:281`, `fuzz_invoke_stream` |
| 2026-10-26 | Fuzz untrusted contract parsers | `contract_parsers.rs:178`, `fuzz_responses_stream` |
| 2026-11-02 | Fuzz untrusted contract parsers | `contract_parsers.rs:452`, `fuzz_kiro_stream` |

libFuzzer stops at the first crash, so each run reports one target; the three findings may hide others. All three check
one property: feeding the same upstream bytes to a stream parser in different chunk sizes gives the same result. They
are two different defects.

## 2. Findings

### 2.1 The invoke and Kiro checks compare what the contract does not promise

`StreamParserV1::parse_chunk` returns `ComponentResultV1<Vec<StreamEvent>>`
(`crates/south-component-conformance/src/component.rs`), and the WIT export has the same shape. A call that fails
returns only the error, so the events that call had already decoded are dropped by type. Where the bad frame sits in a
chunk therefore decides how many events precede the error: in small chunks the earlier frames went out in earlier calls;
in one chunk they are dropped with the error.

`fuzz_responses_stream` already states this ("an error discards the events of its own call, so only the error is
compared"). `fuzz_invoke_stream` and `fuzz_kiro_stream` compare the events before the error as well, and both failures
are exactly that difference. The errors are identical:

- 2026-10-19 (invoke): `[Usage, Delta "hi"]` then "the upstream sent an InvokeModel chunk that does not decode to a JSON
  event", against `[]` and the same error.
- 2026-11-02 (Kiro): `[Delta "Let me check.", ToolCallDelta get_weather]` then "the upstream sent a stream frame with
  invalid JSON", against `[]` and the same error.

These are defects of the fuzz checks, not of the parsers.

### 2.2 The SSE parsers report a chunking-dependent first error

The 2026-10-26 failure is different. The two runs fail with different errors:

- in small chunks: "the upstream stream event has no sequence_number";
- in one chunk: "the upstream sent a stream line that is not UTF-8".

`SseSplitterV1::push` (`crates/south-component-conformance/src/sse_split.rs`) returns `Err` on the first line that is
not UTF-8 and drops the frames it completed earlier in the same call. The parser never sees those frames, so a frame
that is itself invalid and precedes the bad line is never judged. In smaller chunks the earlier frame arrives in an
earlier call and its error wins.

Reproduced locally on the baseline with a throwaway test against `OpenAiResponsesReferenceV1`:

- a `response.created` frame with no `sequence_number`, then `data: \xff`;
- fed as two chunks, the error is "has no sequence_number";
- fed as one chunk, the error is "is not UTF-8".

Every stream parser that calls `self.splitter.push(chunk).map_err(...)?` has the same shape. That is four parsers in
five packages:

| Parser | Packages |
|---|---|
| `OpenAiSseParser` (`reference.rs`) | `provider-openai-compatible` |
| `AnthropicSseParser` (`reference_anthropic.rs`) | `provider-anthropic` |
| `GeminiSseParser` (`gemini_text.rs`) | `provider-gemini`, `provider-vertex` |
| `ResponsesStreamParser` (`reference_openai_responses.rs`) | `provider-openai-responses` |

The fuzz target exercises only the Responses parser this way. The effect on a client is narrow: which of two protocol
errors it receives, when one chunk holds both an invalid frame and a later line that is not UTF-8. Both are a 502
`provider_protocol_error`, so no status code or billing outcome changes. The defect is that the reported error is not
the first one in the stream.

### 2.3 Nothing reports a failing scheduled run

GitHub notifies a failed scheduled run only to the account that created the schedule or last changed its cron
line, and the repository has no branch protection, issue or status badge that would show it. The job failed five weeks running without anyone seeing it.

## 3. Decision

### F1. The invoke and Kiro fuzz checks compare what the contract promises

`fuzz_invoke_stream` and `fuzz_kiro_stream` compare, as `fuzz_responses_stream` does:

- the error alone when either run fails;
- all the events when neither fails.

The Kiro check's later assertions, which read the events of a successful stream, are unchanged. This changes no
shipped code.

### F2. The SSE splitter returns the frames it completed before an error

`SseSplitterV1::push` returns a value that carries both parts:

```rust
pub struct SsePushV1 {
    /// Every event completed before the first refused line, in order.
    pub frames: Vec<SseFrameV1>,
    /// The refusal that stopped the push, if any. The splitter is then unusable.
    pub error: Option<SseSplitErrorV1>,
}
```

The four parsers process `frames` in order and return the first frame error. Only when every frame passes do they
return the split error. The reported error is then the first one in byte order, whatever the chunking.

The return type changes rather than a new method being added beside `push`. That way every caller has to decide what
to do with the completed frames, and a fifth parser cannot keep the old shape by accident. `speech_suite.rs` (a gate
helper) and the tests that call `push` are updated the same way. `finish` keeps its signature, because it completes at
most one event and that event comes after any refused bytes.

Version effects, following the guest-linked crate rule (boundary record §13.12, Q47):

- `south-component-conformance` takes a new minor version.
- The five packages in §2.2 take a new patch version: the error they report changes only in the case above.
- Every other package takes a patch version because it links the crate, as in earlier rounds.
- `south_runtime` declarations and the host range are unchanged. The release still needs a host re-pin (§6).

### F3. A failing scheduled run opens an issue

The scheduled run gets a final job that runs only when an earlier job of that run failed. It opens an issue titled
"Scheduled CI failed", or comments on the open one, with the run link and the failing jobs and steps. The job needs
`issues: write`, scoped to that job alone. The rest of the workflow keeps `contents: read`.

### F4. The fuzz job reports every crash it finds

Each fuzz step runs libFuzzer through `scripts/fuzz-target.sh` with `-fork=1 -ignore_crashes=1` for the same 600 s,
and fails at the end if any finding was written. One week then reports every distinct finding instead of one. Each
finding is printed into the log as base64, so it can be reproduced without rerunning the fuzzer. Uploading the files
as a run artifact would need a new third-party action, which this repository pins by commit and does not yet use, so
the log carries them instead. The second and third fuzz steps run even when an earlier step found something.

## 4. What does not change

- The `StreamParserV1` trait and the WIT export. An error still discards its own call's events, which F1 accepts as
  the contract.
- The host's handling of stream errors, status codes and settlement.
- The eventstream-based parsers (Bedrock invoke, Converse, Kiro). Their reported failures are F1's; whether they share
  F2's defect is for the next full fuzz run (§5) to show, since F4 makes one run report all findings.

## 5. Tests

Each is written first and fails on the baseline:

- `sse_split`: a push holding a complete frame and then a line that is not UTF-8 returns the frame and `NotUtf8`. The
  same holds for an over-bound tail.
- For each of the four parsers: the bytes of §2.2 (an invalid frame, then a line that is not UTF-8) give the same
  error fed as one chunk and fed byte by byte, and that error is the frame's own.
- `fuzz_invoke_stream` and `fuzz_kiro_stream` under F1, then a local fuzz run of `contract_parsers` with F4's options,
  long enough to repeat the three findings. Its result goes into the implementing pull request. The run is CPU-heavy
  and is scheduled around the host's local matrix (§6).
- F3: the issue title and body are built by a small standard-library Python script, unit-tested in the Release
  tooling step like the other CI scripts. The job itself is first exercised by the next failing scheduled run.

## 6. Coordination with the host

The release that carries F2 is re-pinned by the session that owns token-station-server's re-pins. This session sends
it the tag and the release-index digest and does not edit the host. The host links `south-component-conformance` but
does not call `SseSplitterV1` (`server:gateway/tests/text_stream_sse_framing.rs` names it only in a comment), so the
re-pin needs no host code change.

## 7. Questions for lv

Ruled by lv on 2026-11-04: every question as recommended.

- **FQ-1.** Adopt F1, which fixes the fuzz checks to the contract's promise? Recommended: **yes**.
- **FQ-2.** Adopt F2 and change the return type of `SseSplitterV1::push`, rather than add a second method?
  Recommended: **yes, change the type**.
- **FQ-3.** Ship F2 in its own release, or with the next feature release? Recommended: **its own patch-level release**
  of the five packages, because the fuzz job stays red until it lands.
- **FQ-4.** Adopt F3, which opens an issue on a failing scheduled run? Recommended: **yes**. The alternative is a
  status badge on the README, which only shows the failure to someone who looks.
- **FQ-5.** Adopt F4, which makes one fuzz run report every crash it finds? Recommended: **yes**.
