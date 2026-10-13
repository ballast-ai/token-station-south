# Request pass-through for the `generateContent` surface (D7)

Status: accepted (south side of the host's D7, ruled by lv on 2026-10-14: D7-Q1 to D7-Q5 as recommended in
the host's D7 plan, `server:docs/product-review-v2/plans/2026-10-14-D7-*.md` §6). Implemented on branch
`generate-content-request-passthrough` (§6.1); intended for release 0.60.0 together with image step I3-4.
`compatibility.json` lists `south.north-passthrough-delivery.v2` as `not_verified` for both hosts until a host runs it.

Date: 2026-10-14

Baseline: south `origin/main` = `2a95713` (release 0.59.0). Host references carry a `server:` prefix and refer to
token-station-server `origin/dev-v2` as read on 2026-10-14. Statements marked **unverified** are conclusions from reading
code, not from running anything.

Predecessors: [`2026-09-30-openai-responses-upstream-component.md`](2026-09-30-openai-responses-upstream-component.md)
§3.4 and §8.2 (`north_passthrough`, the response side of delivering upstream bytes to a client),
[`2026-10-11-responses-reasoning-replay-and-passthrough-suite.md`](2026-10-11-responses-reasoning-replay-and-passthrough-suite.md)
(the gate ③ suite `south.north-passthrough-delivery.v1`), [`2026-09-30-north-codec-render-gaps.md`](2026-09-30-north-codec-render-gaps.md)
(the north codec has no Gemini wire and the record proposes none).

## 1. Problem

Clients using the Gemini SDK call the host's `POST /v1beta/models/{model}:generateContent` and
`:streamGenerateContent`. The host forwards the client's body unchanged; it has no north codec for this surface and south
will not add one. Today only the host's native rows serve it. When a family moves to `component` rows (the Vertex text
family, later), the surface would be lost, because the host can learn a component's method, URL, headers and
authentication only by calling `build-http-request` with a `ChatRequest`, and has no contract that lets it keep those and
send a different body.

`north_passthrough` (the Responses record §3.4) covers the response side only: the request is still built by the
component from the IR (that record's §8.2). D7 needs the request side too.

## 2. Decision summary

1. `NorthProtocolV1` gains `generate_content`, named for the client's surface, never a provider.
2. A new manifest field `north_request_passthrough: { <family>: <protocol> }` declares that the host may send the
   **client's own body** to the URL the component builds, for that protocol. Only `generate_content` is admitted (a
   protocol the north codec does not speak); the family must also declare `north_passthrough` with the same protocol,
   because the response cannot be rendered either.
3. The descriptor a host keeps is the one `build-http-request` returns for the **probe IR** (§3.2). The component promises
   that method, URL, headers and authentication depend only on the upstream model, the stream flag, the provider
   configuration and the credential's exported attributes, never on the body's content. Gate ② proves it (§5).
4. The family's `request_facts` must place the output cap at `/generationConfig/maxOutputTokens`, the model in the URL
   and the stream flag in the URL, which is the client's own wire. Gate ① checks it.
5. `provider-gemini` declares both fields for family `gemini`. A Vertex text family declares the same when it exists.
6. Gate ③ gains the delivery rules of the `generate_content` protocol (§6), as a protocol-parameterised version of
   `south.north-passthrough-delivery`.

No WIT function is added (the Responses record §15 avoids new WIT exports where a declaration suffices).

## 3. The contract

### 3.1 Manifest

```json
"north_passthrough":         { "gemini": "generate_content" },
"north_request_passthrough": { "gemini": "generate_content" }
```

Gate ① refuses a manifest when, for some family in `north_request_passthrough`:

- the family is not in `providers`;
- the protocol is not `generate_content` (a protocol with a north codec renders through the IR instead);
- `north_passthrough` does not name the same protocol for the family;
- the family's `request_facts` are not exactly `output_cap: ["/generationConfig/maxOutputTokens"]`, `model.url` present,
  `stream: "url"`;
- the package is not a provider-world package.

### 3.2 The probe IR

The host builds the descriptor from this `ChatRequest` and discards its body:

- `model`: the row's upstream model; `stream`: the client's stream choice;
- one user message with the single text part `"."`; no tools, no response format, default sampling;
- the output cap set to the value the host authorised;
- the row's `host_values`, provider configuration and credential attributes as for any other request.

A component that declares `north_request_passthrough` must answer the probe with a descriptor whose method, URL,
headers and authentication are what it would build for **any** request with the same model and stream flag.

### 3.3 What the host promises

- It sends the client's body unchanged except where the host contract already rewrites a body: the output cap (written at
  the declared `output_cap` pointer) and operator request extras outside `immutable_body_paths`.
- It still applies its own checks on the client body against `request_facts` (R16 in host terms): exactly one cap at the
  declared pointer, equal to the authorised value; no body `model`; no body `stream`.
- It forwards the upstream response under `north_passthrough` (§6); usage evidence comes from the component's
  `parse-response` / `parse-stream-chunk` on the same bytes, as for every component row.

## 4. Changes

| Crate / package | Change |
|---|---|
| `south-provider-api` | `NorthProtocolV1::GenerateContent`; field `north_request_passthrough`; gate ① rules (§3.1); manifest error variant |
| `south-component-conformance` | gate ② row `descriptor_independent_of_body` (§5); `provider-gemini` reference unchanged in behaviour |
| `south-provider-conformance` | `north_passthrough` delivery suite parameterised by protocol, with `generate_content` rules (§6) |
| `components/provider-gemini` | declare both fields for `gemini` |
| every package | Q47 patch (the shared crate changes) |

Release: next minor (0.60.0). The host re-pins and implements its D7 step 2.

## 5. Gate ② row: the descriptor does not depend on the body

For each family that declares `north_request_passthrough`, the conformance runner calls `build-http-request` with the
probe IR and with a second IR that differs only in content (two messages, a tool, a system prompt, a different cap
value), for both stream flags, and requires method, URL, query, non-body headers and the authentication arm to be equal.
A component that put anything from the body into the URL or a header fails the row.

## 6. Gate ③: delivering `generate_content` bytes

The `generate_content` rules of the parameterised delivery suite:

- **Frames** are forwarded verbatim, one SSE frame at a time, cut by `SseDecoderV1::position`.
- **Terminal**: a frame whose `candidates[*].finishReason` is set **and** for which the component's `parse-stream-chunk`
  reports `Done`. The host withholds it until settlement.
- **Failure**: the protocol has no failure event. A host closes the stream without writing a frame of its own (today's
  behaviour of the host's native rows); the suite asserts no synthetic frame appears.
- **Non-2xx** responses are delivered to the client as the upstream sent them (D7-Q3), status and body.
- **Non-stream**: the body is delivered verbatim after the component has parsed it and the host has settled.

### 6.1 Implementation

The parameterised suite is a second identifier, `south.north-passthrough-delivery.v2` (version 2), next to an unchanged
v1. A host that ran v1 against Responses (the server's `host_passes_the_north_passthrough_delivery_suite`) keeps its
harness, its 22 cases and the meaning of its claim; adding `generate_content` cases under the v1 identifier would have
changed what "passes v1" means after the fact.

- `south-provider-conformance` adds `NorthPassthroughProtocolV2` (`Responses`, `GenerateContent`: the subset of
  `NorthProtocolV1` the suite has rules for, with `from_north_protocol` / `north_protocol`), the harness trait
  `NorthPassthroughHarnessV2` (v1's methods, with `stream` and `buffered` told the protocol),
  `run_north_passthrough_conformance_v2(harness, protocol)`, `north_passthrough_fixtures_v2(protocol)` and
  `north_passthrough_is_terminal_data_v2(protocol, data)`. For Responses, v2's table is v1's table and its checks are
  v1's checks; v1's runner is v2's runner over an adapter, so the two cannot drift. Reports gain `protocol()`.
- New public enum members: `NorthPassthroughBodyV1::Gemini*` (nine bodies; two are rows of `fixtures-gemini`, checked
  byte for byte), four case ids (`OtherFinishReasonIsTerminal`, `FinishReasonOnAnyCandidateIsTerminal`,
  `UpstreamErrorFrameNotRelayed`, `BufferedNon2xxDeliveredAsSent`) and the step `StatusWritten(u16)`.
  `NorthPassthroughFrameV1` gains `has_finish_reason()` and `is_terminal_frame(protocol)`.
- The `generate_content` table has 21 cases. Twelve reuse v1 ids whose rule does not depend on the protocol
  (forwarding, order, terminal withheld until settlement, parking on a terminal frame without `Done` and on `Done`
  without a terminal frame, a lagging component, component errors and traps, a failure on the first frame, truncation,
  a failed settlement, the end-of-input terminator); three are delivery cases (the client's tools decide nothing,
  since the request is passed through too); two are buffered cases as in v1; four are new. v1's `LossyToolMapRenders`,
  `IncompleteIsTerminal`, `UpstreamFailureReplaced`, `ErrorEventReplaced` and `BufferedNon2xxNotRelayed` have no
  `generate_content` form.
- Every `generate_content` failure case requires that nothing but comment lines follows the relayed frames (category
  `FailureEvent`), in place of v1's fixed-message `response.failed`. The non-2xx case requires the upstream's body
  byte for byte, exactly one `StatusWritten` equal to the upstream status before the first byte, `map-provider-error`
  returned before it, one funds transition before it and no persisted success settlement.
- `south-testkit`'s reference executor implements both harnesses and passes both protocols. Four new faults
  (`GenerateContentFailureFrame`, `GenerateContentNon2xxReplaced`, `GenerateContentNon2xxStatusRewritten`,
  `TerminalOnFirstCandidateOnly`) each fail exactly the cases that guard them; for Responses, v2 fails exactly what v1
  fails for every fault (`tests/north_passthrough_runner_v2.rs`).
- `compatibility.json` still names only the v1 suite; adding the v2 identifier there is part of the release.

## 7. Open points

None for south. `countTokens` stays out of scope (D7-Q4): there is no south operation for it and the host keeps
refusing component rows there.
