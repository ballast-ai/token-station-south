# Messages pass-through, forwarded client headers, and a count-tokens operation

Status: accepted (lv, 2026-10-17: Q1–Q6 all as recommended, §9; confirmed in this repository's session). Requested
by the host (server-14 session, 2026-10-17), relaying lv's ruling of the same day that the host's native `gemini` and
`anthropic` provider types are deleted once their families run on components. Three gaps stop that deletion; this
record decides the south half of all three. Implemented (#266); released in 0.73.0. The §3.5 correction (#268) is released in 0.73.1.

Date: 2026-10-17

Baseline: south `origin/main` = `b39d98f` (release 0.72.0). Statements about the upstream APIs come from the vendors'
public documentation and are **unverified** here: nothing was called.

Predecessors:
[`2026-10-14-generate-content-request-passthrough.md`](2026-10-14-generate-content-request-passthrough.md) (D7: request
pass-through, the probe IR, gate ②'s `descriptor_independent_of_body`; its §7 kept `countTokens` out of scope under
D7-Q4), [`2026-09-30-openai-responses-upstream-component.md`](2026-09-30-openai-responses-upstream-component.md) §3.4
and §8.2 (`north_passthrough`), [`2026-10-11-responses-reasoning-replay-and-passthrough-suite.md`](2026-10-11-responses-reasoning-replay-and-passthrough-suite.md)
(the gate ③ delivery suite), [`2026-10-15-immutable-request-headers.md`](2026-10-15-immutable-request-headers.md)
(`immutable_request_headers`).

## 1. Problem

1. **The Messages surface loses what the IR does not carry.** A client of the host's `POST /v1/messages` routed to a
   `component` row is decoded by the north codec into the IR, rebuilt by `provider-anthropic` from the IR, and its
   response rendered back through the codec. Whatever the IR has no field for is dropped on the way: top-level
   `cache_control`, top-level keys such as `context_management`, `container` and `mcp_servers`, and on the response
   `citations` and content block types the IR does not know. The host's native `anthropic` type forwards those today.
2. **The client's `anthropic-beta` header does not reach the upstream.** Beta features are switched on by that header,
   and a request body that uses one fails without it. The host forwards no client header to a component row, and a
   host rule "forward `anthropic-beta`" would be vendor logic in the host.
3. **The Gemini SDK's `countTokens` has no component path.** Clients call
   `POST …/models/{model}:countTokens` (response `{"totalTokens": n, …}`). The host learns a component's URL, headers
   and authentication only from `build-http-request`, which builds a generation request; replacing `:generateContent`
   with `:countTokens` in the host would again be vendor logic. (The host's native `anthropic` type never served
   Anthropic's `POST /v1/messages/count_tokens`, so only the Gemini families need it; §5 offers Anthropic too.)

## 2. Decision summary

1. **M1.** `north_request_passthrough` admits `messages`, on the same terms as `generate_content` plus request facts on
   the Messages wire. `provider-anthropic` declares `north_passthrough` and `north_request_passthrough` = `messages`
   for family `anthropic`, and gate ③'s delivery suite gains the `messages` rules.
2. **M2.** A new provider-world manifest field `forwarded_client_headers: { <family>: [<name>, …] }` names the client
   request headers a host forwards unchanged on a pass-through request. `provider-anthropic` declares `anthropic-beta`.
3. **M3.** A new provider-world manifest field `count_tokens: { <family>: <declaration> }` says the family serves the
   counting operation for one client protocol, and which suffix of the generation URL's path becomes the counting one.
   `provider-gemini` and `provider-vertex` declare it.

No WIT function is added: each of the three is a declaration the host reads, built on the probe descriptor D7 already
defines (§7 says why not a WIT function).

## 3. M1: Messages request and response pass-through

### 3.1 Manifest

```json
"request_facts":             { "anthropic": { "output_cap": ["/max_tokens"], "model": { "body": "/model" }, "stream": { "body": "/stream" } } },
"north_passthrough":         { "anthropic": "messages" },
"north_request_passthrough": { "anthropic": "messages" }
```

Gate ① today refuses any request pass-through protocol the north codec speaks, because D7 needed it only for the one it
does not. That rule becomes a closed list: `north_request_passthrough` admits `generate_content` and `messages`, and
still refuses `chat_completions` and `responses` (no host has asked; each would need its own facts rule). For
`messages`, the family's `request_facts` must be exactly `output_cap: ["/max_tokens"]`, `model: {"body": "/model"}`,
`stream: {"body": "/stream"}`, the client's own wire. The other D7 conditions are unchanged: a declared family, the same
protocol in `north_passthrough`, a provider-world package.

### 3.2 What the host does with the request

As D7 §3.3, with the facts of this wire: the host builds the probe descriptor (D7 §3.2) for the client's stream flag,
keeps its method, URL, headers and authentication, and sends the client's body with only the host contract's own
rewrites: the row's upstream model written at `/model`, the authorised cap at `/max_tokens`, and operator request extras
outside `immutable_body_paths`. The stream flag at `/stream` is the client's and selects the probe. Everything else in
the body, top-level `cache_control` and unknown top-level keys included, reaches the upstream as the client wrote it.

The component still parses the response for usage evidence (`parse-response`, `parse-stream-chunk`) on the same bytes,
as for every component row, so funds discipline does not change.

### 3.3 Which families

Only `anthropic`. The two Claude Code families cannot take a client's body as it is: the login form injects the Claude
Code identity into the system prompt (`inject_claude_code_identity`) and both send their own `anthropic-beta` set, so a
pass-through body would lose what the upstream requires of them. They keep rendering through the IR.

### 3.4 Gate ③: delivering `messages` bytes

`south.north-passthrough-delivery.v2` gains the protocol `messages` (`NorthPassthroughProtocolV2::Messages`), with:

- **Frames** forwarded verbatim, one SSE frame at a time, as for the other two protocols.
- **Terminal** (corrected 2026-10-17, §3.5): `Done` arrives on the `message_delta` that carries the stop reason, not on
  `message_stop`. The host withholds that frame, any `ping` after it, and `message_stop`, and releases them together
  after settlement.
- **Failure**: an upstream `event: error` frame is not relayed; the host writes the north codec's Messages error event
  of its own, as Responses does with its fixed `response.failed`, since the codec speaks this protocol (Q2).
- **Non-2xx**: not relayed; the host renders its own error through the codec, as for Responses (Q2).
- **Non-stream**: the body delivered verbatim after the component parsed it and the host settled. Content blocks the IR
  does not know, and `citations`, arrive as the upstream sent them.

Fixtures: a text stream, a tool-use stream, a stream with a `citations` delta and an unknown block type, an `error`
frame first, and a non-2xx body, each checked byte for byte against what the reference parses.

### 3.5 Correction (2026-10-17): `Done` arrives on `message_delta`

The host's read-back after release 0.73.0 (server-14 session) found that §3.4 and the suite described a component that
does not exist. Both assumed the component reports `Done` on `message_stop`. The shipped `provider-anthropic` reports
`Finish`, `Usage` and `Done` on the `message_delta` that carries usage, and nothing on `message_stop`. It does this by
design: the Anthropic component record D3 (`2026-08-22-anthropic-provider-component.md`) emits the terminal triple on
the frame that carries usage, or at EOF. A host that followed the suite literally would therefore park every real
Messages stream. Its case `DoneWithoutTerminalTypeParks` refuses `Done` on a frame that is not `message_stop`. The
host shipped tolerating both shapes.

Nothing checked the suite's scripts against the reference parser, which is why this was missed. The scripts were
written from §3.4, and §3.4 was written from the protocol's shape, not from the parser.

Ruled 2026-10-17 (confirmed in this repository's session): the suite follows the parser, and the component is
unchanged. Changing the parser would re-identify every package (Q47) and overturn D3 for no gain on the wire. The
delivery rule for `messages` is now:

1. `Done` must arrive on a `message_delta` frame. `Done` on any other frame parks the stream, as before.
2. From that frame on, the host withholds every frame: the `message_delta` itself, then any `ping` event whose call
   returned cleanly with no events, then `message_stop`, whose call must also return cleanly with no events.
3. When `message_stop` is in hand, the host settles. Once the settlement persists, it releases the withheld frames in
   order. If the settlement fails, it releases none of them and ends with its own `error` event.
4. Between `Done` and `message_stop`, any frame other than `ping` parks the stream and is not relayed, and so does
   any component `Error` or `Trap`. A frame after `message_stop` parks too.
5. A stream that ends after `Done` without `message_stop` parks, as a truncation does. The real upstream always sends
   `message_stop`; this fails closed, as Responses and `generate_content` do on truncation.
6. `message_stop` with no `Done` before it parks (`TerminalTypeWithoutDoneParks`), and so does `Done` that arrives only
   at EOF (`LaggingComponentParks`).

Suite changes: `south.north-passthrough-delivery.v2` moves to suite version 3. The `messages` scripts put `Done` on
`message_delta` and a clean empty call on `message_stop`. The Terminal check now requires every frame from the `Done`
frame on to be released after the settlement.

Case and body changes:

- `TruncatedStreamFails` ends right after the `Done` frame.
- `SettlementFailureWithholdsTerminal` fails the settlement after `Done`.
- `ErrorEventReplaced` fails before `Done`, because the parser ignores an `error` that comes after it. Its opening is
  the text body's four LF-framed frames.
- The citations body closes its first block and carries a `ping` event between `message_delta` and `message_stop`.

The reference executor gains the fault `MessagesForwardDoneFrame`, which forwards the `Done` frame at once, as the host
does today.

A new test drives `provider-anthropic`'s reference parser over the body of every `messages` case that stands for a
correct component, frame by frame, and requires its answers to equal the case's script. A script can no longer disagree
with the parser it stands in for.

Run on the Responses and `generate_content` bodies, the same test fails: their reference parsers refuse those synthetic
framing probes, or answer an error body with events. The Responses table is the frozen v1 suite. Making those bodies
parser-faithful is therefore left to a record of its own, and the test checks `messages` only.

The parity test also found a gap in the parser, which this record does not fix. `provider-anthropic` ends a frame
only at `\n\n` or `\r\n\r\n` (`sse_frame_boundary`), but SSE also allows a lone CR as a line end. The suite's
mixed-framing body has one CR-framed frame. The parser does not see that frame's end, and parses it together with
the frame after it. Its answers still match the scripts, because `Done` stays on the `message_delta` frame. The
Anthropic API frames with LF, so nothing on the wire is affected today. A fix changes a component and re-identifies
every package (Q47), so it waits for a release that has another reason to.

The host's v2 entry in `compatibility.json` stays `not_verified`. The host re-runs v2 against suite version 3.

## 4. M2: forwarded client headers

### 4.1 Manifest

```json
"forwarded_client_headers": { "anthropic": ["anthropic-beta"] }
```

Gate ① refuses the manifest when, for some family:

- the family is not in `providers`, or the package is not a provider-world package;
- the family does not declare `north_request_passthrough` (§4.3);
- a name is not a lowercase RFC 9110 `tchar` token, repeats, or there are more than 16 names;
- a name is a reserved header (`south_contracts`' reserved set: authentication, hop-by-hop, framing, `cookie`,
  `user-agent`, the SigV4 headers), a header the package declares in `permissions.secrets` presentation or in
  `signing`, or a name in the family's `immutable_request_headers`.

`south-provider-api` depends on no other south crate, so it mirrors the reserved set as it already mirrors the body path
bounds, and a conformance-crate test pins the two lists together.

### 4.2 What the host does

On a pass-through request (M1 or a counting request, M3) for a family that declares the field, the host copies each
declared header from the client's request to the upstream request, unchanged; repeated field lines are combined with
`", "` as RFC 9110 §5.3 allows. A header the client did not send is not added. Every value passes the descriptor's
header value rules (`MAX_PROVIDER_HEADER_VALUE_BYTES`, the total bound); a value that does not is refused with
`invalid_request`, never truncated. The host forwards no other client header, and writes no vendor name.

### 4.3 Why only with pass-through, and why no merge

- A client's beta flag goes with the client's body. On a request rebuilt from the IR the body is the component's, and a
  flag the client chose for a body the upstream never sees can only mislead. So the field requires
  `north_request_passthrough` on the family (Q3).
- The host never merges a client value into one the component set. Gate ② requires that the probe descriptor does not
  carry any forwarded header, for either stream flag (§6), so the two never meet. `anthropic` sets no `anthropic-beta`
  today.

## 5. M3: the count-tokens operation

### 5.1 Manifest

```json
"count_tokens": {
  "gemini": { "protocol": "generate_content", "path_suffix": { "from": ":generateContent", "to": ":countTokens" } }
}
```

- `protocol`: the client surface whose counting endpoint this serves; `generate_content` or `messages`.
- `path_suffix.from` is what the non-stream probe descriptor's URL path ends with; `to` replaces it. Both non-empty, at
  most 64 bytes, from `[A-Za-z0-9._~:/-]`, and different.

Gate ① refuses the manifest when the family is not declared, the package is not a provider-world package, the protocol
is another one, the family does not declare `north_request_passthrough` with the same protocol (the client's counting
body is that protocol's wire, which only a pass-through family takes as it is), or `path_suffix` breaks the rules.

### 5.2 What the host does

1. Build the probe descriptor (D7 §3.2) with `stream: false`; keep method, headers and authentication.
2. Require the URL path to end with `from` and replace that suffix with `to`, leaving scheme, host, port and query. A
   path that does not end with `from` is a component fault (`internal`), never a fallback. The rewrite is a public
   function of `south-provider-api` (`rewrite_count_tokens_path`), which gate ② also uses, so the host does not
   re-implement it.
3. Authorize the result against the configured endpoint as for any request; forward M2's declared headers; send the
   client's body unchanged. The generation rewrites (model, cap, extras) do not apply: a counting body has no cap, and
   the extras describe how to generate.
4. Deliver the upstream's status and body as they came. Counting is not billed: no reservation, no usage, no
   settlement, and no call to the response-side functions. The host may call `map-provider-error` on a non-2xx to
   decide whether to try another upstream (Q5).

### 5.3 Which families

| Family | Declaration | Note |
| --- | --- | --- |
| `gemini` | `generate_content`, `:generateContent` → `:countTokens` | required by the host |
| `vertex-ai` | the same | Vertex serves `:countTokens` on the same model path |
| `anthropic` | `messages`, `/v1/messages` → `/v1/messages/count_tokens` | not required (the native type never served it); Q4 |
| Claude Code families | none | not pass-through families (§3.3) |
| `anthropic-bedrock-invoke` | none | Bedrock's `CountTokens` wraps the body (`{"input": {"invokeModel": {"body": …}}}`); a suffix is not enough (Q6) |
| OpenAI-compatible families | none | no counting endpoint |

## 6. Gate ② rows

For every family with the declarations, the conformance runner calls `build-http-request` with the probe IR:

- `descriptor_independent_of_body` (D7 §5) now also runs for `messages` request pass-through families, with Messages
  content variations (a system prompt, a tool, a different cap).
- `forwarded_headers_absent`: for both stream flags, the descriptor carries none of the family's
  `forwarded_client_headers`.
- `count_tokens_path`: with `stream: false`, the method is `POST`, the path ends with `from`, and the rewritten URL keeps
  scheme, host, port and query.

Fault fixtures: a component whose descriptor sets `anthropic-beta` on a family that forwards it, and one whose
non-stream path ends with something other than `from`.

## 7. Alternatives

- **A WIT function per operation** (`build-count-tokens-request`). Explicit, and able to rewrite the body (Q6). But the
  runtime binds one world per `bindgen!`, so a new export in `provider-adapter-v2` would refuse every package built
  before it; it needs a new world that every host must adopt, for three families that need only a different URL.
- **A host value `operation: count_tokens`** that makes `build-http-request` return the counting URL. It changes what the
  function means, needs code in every declaring reference, and host values are values only the host can mint (D7), not
  a mode switch.
- **Widening the IR** for M1's fields instead of passing through. The IR is the community kernel's; and unknown
  top-level keys and block types are open-ended, which is what pass-through is for.
- **Forwarding client headers on every request** (M2 without the pass-through condition). Q3.

## 8. Changes

| Crate / package | Change |
| --- | --- |
| `south-provider-api` | `messages` admitted by `north_request_passthrough` with its facts rule; fields `forwarded_client_headers` and `count_tokens` with gate ① rules; `rewrite_count_tokens_path`; the mirrored reserved header list; manifest error variants |
| `south-provider-conformance` | delivery suite v2: protocol `messages`, its rules, fixtures and fault cases |
| `south-component-conformance` | gate ② rows of §6; the reserved-list pin test; references unchanged in behaviour |
| `components/provider-anthropic` | `request_facts`, `north_passthrough`, `north_request_passthrough`, `forwarded_client_headers` for `anthropic` (and `count_tokens` if Q4) |
| `components/provider-gemini`, `components/provider-vertex` | `count_tokens` |
| every package | Q47 patch (`south-provider-api` 0.60.0 → 0.61.0, `south-component-conformance` 0.71.0 → 0.72.0) |
| `compatibility.json` | the host's `north_passthrough_delivery_v2` entry goes back to `not_verified` (§8.1) |

### 8.1 Two consequences found while implementing

- **The three declaring packages need runtime 0.73.0.** `ComponentManifestV1` refuses unknown fields, so a runtime
  before 0.73.0 refuses `forwarded_client_headers` and `count_tokens`, and its gate ① refuses `messages` request
  pass-through. `provider-anthropic` (1.3.0), `provider-gemini` (1.4.0) and `provider-vertex` (1.1.0) therefore raise
  `south_runtime` to 0.73.0 (CONTRIBUTING, "a package declares the oldest runtime it needs") in the release pull request
  that ships them, as D7's packages moved to 0.60.0 in release 0.60.0 (#198): until the workspace is 0.73.0, no runtime
  of that number exists for the gates to load them under. A host must record runtime 0.73.0 to admit them.
- **The delivery suite v2 grew from 43 to 63 cases** (20 `messages` cases), so the host's recorded `verified` run no
  longer describes the table, and `host_capability_evidence_freshness_v1` refuses it. The entry is `not_verified`
  until the host re-runs v2 for all three protocols; editing the count without a run would fabricate the evidence.

Tests, written first and failing: gate ① acceptance and each refusal in §3.1, §4.1 and §5.1; the gate ② rows against the
declaring references and the fault fixtures; the delivery suite's `messages` cases against the reference executor and
its faults; `shipped_packages_v1` reads the new declarations.

Release: next minor (0.73.0). The host re-pins and implements its side in its own record.

## 9. Questions for lv, and the rulings

Ruled by lv on 2026-10-17: every question as recommended. Q2: a `messages` error frame and a non-2xx are rendered by
the host through the codec, as Responses. Q3: forwarded client headers apply only to pass-through requests. Q4:
`provider-anthropic` declares `count_tokens`, and the host opens the northbound counting route. Q5: a counting non-2xx
may fail over through `map-provider-error`; §5.2 step 3 stands. Q6: no `anthropic-bedrock-invoke` counting now.

- **Q1.** Adopt M1, M2 and M3 as written? Recommended: yes.
- **Q2.** On a `messages` pass-through, should an upstream `error` frame and a non-2xx be relayed as the upstream sent
  them (as `generate_content` does, D7-Q3) instead of rendered by the host through the codec (as Responses does)?
  Recommended: render through the codec, as Responses; the codec speaks Messages, and relaying would hand clients the
  upstream's request and organisation identifiers.
- **Q3.** Should `forwarded_client_headers` also apply to requests rebuilt from the IR? Recommended: no (§4.3).
- **Q4.** Declare `count_tokens` for `anthropic` although the native type never served it? It costs one manifest entry
  once M1 is in. Recommended: yes.
- **Q5.** On a counting non-2xx, fail over through `map-provider-error`, or deliver the first upstream's error?
  Recommended: the host decides; south only requires that what is delivered is an upstream's own response.
- **Q6.** `anthropic-bedrock-invoke` counting needs a body envelope. Recommended: leave it out until a host needs it;
  then decide between an envelope declaration and a WIT function.
