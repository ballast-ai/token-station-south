# The official Gemini provider component

**Date**: 2026-08-22
**Status**: proposed → landing with this change
**Related**: `2026-08-22-anthropic-provider-component.md` (the second component,
whose shape this one follows), `2026-08-21-component-conformance.md` (gates ① and ②)

---

## 1. Why now, and a correction that made it possible

The enterprise host's survey of the three remaining dialects concluded that
Gemini's component could not ship because its URL was inexpressible: the model
sits in the path and the operation is a `:method` suffix, while
`ProviderEndpoint::resolve` takes a four-variant `ProviderApi`.

**That conclusion was wrong.** `HttpRequestDescriptor::url` is a plain `String`,
`ProviderEndpoint::as_str` is public, and the host-side gate —
`ProviderConfig::authorize` → `permits(url)` — admits **any path under the
configured base**, stripping the query before it judges. `resolve` is a
convenience for four canonical shapes, not the only door.

The correction is recorded here rather than quietly acted on, because a wrong
"blocked" is more expensive than no claim at all: it stops people from trying.

What Gemini actually needed was nothing. `x-goog-api-key` is already a
sanctioned credential header, so the `HeaderSecret` arm covers direct keys and
`Bearer` covers Vertex.

## 2. What the component covers

All five families. The streaming direction is **new capability rather than a
migration**: the enterprise host never translated Gemini's SSE — it passes the
frames through byte for byte — so there is no incumbent behaviour to preserve
and the shape below is decided here.

## 3. Decisions

### G1 — The URL is built, not resolved

`{base}/v1beta/models/{model}:generateContent`, and for a streaming request
`:streamGenerateContent?alt=sse`. **Streaming is an operation, not a body
field**, which is why `ChatRequest::stream` reaches the URL rather than the
payload.

A request with an empty model is refused: the dialect addresses the model in
the path, so such a request has no target, and sending it to `models/:generate`
would ask the upstream to interpret a URL the caller never meant.

### G2 — A tool result is keyed by the called function's name

Gemini's `functionResponse` carries a name, not a call id; the IR carries only
the id on a tool turn, and the name lives on the assistant turn that made the
call. The component scans the exchange first and builds id→name, rather than
remembering while it translates: "the result follows the call" is a protocol
convention, not something the translation can assume.

A result whose call this exchange never made is **refused**, not dropped. The
alternative is sending a turn the model cannot attribute.

### G3 — Call ids are synthesised, stably

Gemini does not send one. The IR requires a non-empty id and the caller has to
quote it back, so the component synthesises `call_{position}_{name}`:
translating the same response twice yields the same id, and position + name is
enough for the next turn's id→name lookup to find its way back.

**A turn that produced a call finished because of the call**, whatever the
upstream labelled it — Gemini reports `STOP` for tool turns, and a caller
reading the reason alone would never look at the calls.

### G4 — Reasoning rides on a flag, not a block type

Gemini marks it `{"text": …, "thought": true}`. A translator that reads only
`text` therefore concatenates the model's private reasoning into the visible
answer — worse than dropping it, because it hands the user something that was
never meant for them. The component splits on the flag in both directions and
in the stream.

`thoughtSignature` round-trips as `RedactedThinking`.

### G5 — Unknown enum values survive

`RECITATION`, `MALFORMED_FUNCTION_CALL`, `OTHER` and anything added later become
`FinishReason::Other`, not a bucketed `Stop`. Telling a caller the model
finished normally when it was cut off for recitation is a wrong answer, not an
approximation.

### G6 — A remote image announces no mime type

A `data:` URL carries its own; an `https:` URL does not, and the extension is
not the content. The component emits `file_data.file_uri` without
`mime_type` and lets the upstream sniff. **Guessing `image/jpeg` for every
remote image — which the enterprise host does today — is a wrong answer where an
absent field would have been a question.**

## 4. Fixture pack

`fixtures-gemini/`, sixteen cases across all five families. Its own pack: three
components, three frozen packs.

Cases are written from the decisions above, not recorded from behaviour.

## 5. What this change does not do

- **No host adoption.** Adopting the component is each host's own slice.
- **No Vertex arm.** `Bearer` covers it at the contract level, but Vertex also
  moves the URL (`{region}-aiplatform…/publishers/google/models/…`), which is a
  second endpoint shape and belongs in its own slice with its own fixtures.
