# Renderer refusal for unmappable content blocks

- Slice: behaviour change to `OpenAiCompatibleReferenceV1` and `GeminiReferenceV1`
  (and therefore to the official `provider-openai-compatible` and `provider-gemini`
  components, which are wit-bindgen shells around them); `AnthropicReferenceV1` unchanged
- Origin: the community host's stage A′ (`token-station-doc`
  `docs/design/2026-08-22-stage-a-prime-and-server-tools-rulings.md`, ruling A′-1:
  "the south must learn to refuse before the north can stop refusing")
- Status: implemented 2026-08-23
- Release: south **0.15.0**; `provider-openai-compatible` **2.1.0**; `provider-gemini` **1.1.0**

## 0. Versioning

South goes `0.14.0 → 0.15.0`. The two component packages take a minor bump: the
ABI is unchanged (`token-station:adapter@2.0.0`, world `provider-adapter-v2`), the
change is strictly *less* permissive for one class of input, and a host that
upgrades sees a named 400 where it previously saw either an upstream 400 or a
wrong answer. The compatibility tuple's `south_runtime` moves with the release.

## 1. The problem

`ContentPart::Unknown(Value)` is the IR's "never drop" guarantee: a part whose
`type` the IR does not model survives verbatim (canonical IR inventory, §ContentPart;
kernel v0.2.0 `chat.rs`). The Anthropic wire produces several such parts —
`server_tool_use`, `web_search_tool_result`, `web_fetch_tool_result`,
`code_execution_tool_result`, `mcp_tool_use`, `mcp_tool_result`, `document`,
`search_result` — and the Anthropic component writes them back verbatim, which is
the round trip working as designed.

The OpenAI-compatible and Gemini renderers, however, also wrote them verbatim:
`multimodal.push(value.clone())` into the Chat Completions `content` array, and
`value.clone()` into Gemini `parts`. Neither wire has a spelling for these blocks.
What the upstream does with one is its own business: OpenAI-compatible providers
mostly reject the request with their own 400 (opaque, and attributed to the
host), a few read the block as empty content and answer as if it had never been
there; Gemini `parts` carry no `type` discriminator at all, so a `{"type": …}`
object is an unknown-field error or, worse, an empty part.

The community host's stage A′ needs the north-bound Anthropic adapter to stop
refusing these blocks on the south's behalf — it refuses today because it cannot
know which renderer will serve the request (normalisation precedes routing). It
can only stop once the renderer that cannot serve them says so itself.

## 2. The ruling

**The IR never drops; the renderer never smuggles.** A renderer that cannot
spell a part refuses at `build_http_request` with `capability` (400), naming the
block's `type` and nothing else:

```text
content block `web_search_tool_result` has no OpenAI Chat Completions rendering;
route the request to a provider that speaks its wire
```

| Wire | Unmodelled part | Behaviour now |
|---|---|---|
| OpenAI Chat Completions | `type` ∈ {`input_audio`, `file`} | forwarded verbatim — it is the wire's own vocabulary the IR has no typed field for yet |
| OpenAI Chat Completions | any other `type`, or no `type` | **refused**, `capability` 400 |
| Gemini | any | **refused**, `capability` 400 — `parts` have no discriminator, so there is nothing to forward *as* |
| Anthropic | any | verbatim, unchanged |

### 2.1 Why this is not R3 in reverse

The host-parity slice (0.11.0, R3) ruled the opposite way for `RedactedThinking`:
drop it, do not refuse, because "a migration that only swaps implementations must
not change which requests succeed". That ruling stands. The distinction is what
the block *is*:

- `redacted_thinking` is side metadata. Dropping it leaves the answer the model
  will give intact; the request succeeds and is correct.
- A `web_search_tool_result` or `document` block **is the content the answer
  depends on**. Smuggling it produces either an upstream refusal the host cannot
  explain, or a confident answer computed without the content. Neither is "the
  request succeeds"; the second is silently wrong, which is the worst outcome a
  translation layer can produce.

Loud failure with a name beats quiet wrong answers. That is the line: metadata
may be dropped, content may not be smuggled.

### 2.2 Why this is not a D5 violation

D5 (canonical IR inventory) says `extensions` are data, not contract: a
component must not branch on an extensions key. This change branches on
`ContentPart::Unknown`'s `type`, which is a typed IR variant, not an extensions
key — and it branches only to *refuse*, never to render differently. The
`input_audio` / `file` allow-list is the renderer recognising its own wire's
vocabulary, the same judgement it already makes for every modelled part.

## 3. What the host does with it

Before this release the north-bound Anthropic adapter refused these blocks at
`normalize_inbound` so that an OpenAI-compatible upstream would never see them.
With the renderer refusing, the host can let them into the IR: routed to an
Anthropic upstream they round-trip verbatim; routed to an OpenAI-compatible or
Gemini upstream they are refused at render time with the same error class and a
clearer name. The host's receipt records the failure at the `provider_request`
conversion stage instead of `inbound_normalize`.

## 4. Tests

`crates/south-component-conformance/tests/renderer_refusal_v1.rs` pins: the
OpenAI refusal names the block and carries no content; `document`,
`search_result` and an untyped object are refused; `input_audio` is still
forwarded; Gemini refuses the same block; Anthropic writes it back verbatim.

The fixture protocol expresses expected *outputs*, not expected refusals, so
these are native assertions rather than fixture pairs. The sandbox-parity gates
are unaffected: the components are the same code compiled to wasm, and the
shipped fixture packs carry no unmodelled parts.
