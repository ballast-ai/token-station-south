# Response Header Visibility

Status: implemented

Date: 2026-08-28

## Problem

South returns four things about a provider's response: status, body, `content-type`, and
`retry-after` — plus, since 0.2.0, the nine approved quota fields. Everything else the upstream sent
is discarded at the transport, one line before the response is consumed.

That was the right shape for the reader South was built for: a host pacing itself against rate
limits reads values it *branches on*, and a closed, reviewed set is exactly what a control-flow
input should be.

It is the wrong shape for a second reader that has since appeared: the **operator** debugging a call
that went wrong. They ask "what did the provider actually answer?" — and an allow-list can never
answer it, because the header they need is by definition the one nobody thought to approve in
advance. A host surfacing a request/response inspector today shows a nearly empty response-header
pane, with no way to tell "the upstream sent nothing" apart from "South dropped it".

Serving that reader by widening the closed contracts would have been the wrong repair. It would
grow, one release at a time, into the unbounded and unreviewed trust boundary the original design
deliberately refused — with each addition justified by one debugging session and none of them ever
removed.

## Approach

Two mechanisms, because there are two readers with genuinely different needs.

**`ResponseDiagnosticsV1`** — a closed allow-list, identical in construction to
`ProviderQuotaMetadataV1`: eight named fields, per-value and total byte bounds, duplicate refusal.
These are the fields an operator quotes to a provider's support desk, and a host *may* read them
programmatically. Adding a field is a contract change with a version bump, as it should be.

**`ResponseTranscriptV1`** — a bounded, display-only capture of everything else. It exists to be
read by a person and by nothing else. Its safety rests on three properties rather than on a name
list:

1. **Denied by kind.** Credential-bearing headers (`set-cookie`, `set-cookie2`, `authorization`,
   `proxy-authenticate`, `proxy-authorization`) and hop-by-hop framing headers never enter, whatever
   the upstream sends. This is the response-side mirror of `RESERVED_HEADERS`.
2. **Bounded.** At most 64 headers within 16 KiB. A hostile or merely broken upstream cannot make a
   transcript grow without limit.
3. **Inert.** Nothing in South reads a transcript, and hosts must not branch on one. It carries no
   meaning to the machine.

Capture is *total*: a header that is denied, malformed, or past a bound is dropped, and anything
dropped for want of room sets `truncated()`. A transcript is a debugging aid, and a debugging aid
that can turn a good response into an outage is a bug. Denied headers deliberately do **not** set
`truncated()` — their exclusion is the contract working, and reporting it would send a reader
hunting for a header that is never coming.

The distinction is the whole point: **widening what a human may see does not widen what the host's
control flow may depend upon.** The reviewed trust boundary the closed contracts establish is left
exactly where it was.

## Goals

- One closed, versioned, bounded contract for eight approved diagnostic fields.
- One bounded, display-only transcript for everything else not denied by kind.
- Capture both on the buffered response and the headers-ready streaming head, in step.
- Preserve every existing constructor, so consumers that do not use either contract are untouched.
- Keep malformed or oversized response metadata from failing an otherwise valid response.

## Non-goals

- No parsing, interpretation, or normalisation of any captured value inside South.
- No host control flow keyed on a transcript. Hosts that need to branch must use the closed
  contracts, and a field they need but lack is a contract change, not a transcript lookup.
- No transcript on the WIT component boundary. `HttpResponseParts.headers` is already an open map;
  what a component may see is a separate decision from what a host may display, and this slice does
  not widen the component's view.

## Amendment to the quota metadata record

`docs/design/2026-08-17-provider-quota-response-metadata.md` lists as acceptance criterion 4:

> unknown response headers never appear in the South-adapted `HttpResponseParts`

That statement remains true of `HttpResponseParts` and of everything a host may branch on — this
slice does not put a transcript into the component's view. It is **no longer true of the host-facing
buffered response and streaming head**, which now also carry a bounded, display-only transcript that
is denied by kind rather than approved by name.

The non-goal in that record — "No arbitrary response-header map, string-key lookup, wildcard prefix
capture, or provider-defined extension field" — is likewise narrowed rather than repealed: it still
governs every *programmable* response contract. The transcript is none of those things for the host's
logic, because the host's logic may not read it.

## Verification

- `crates/south-contracts/tests/response_metadata_v1.rs` — allow-list round-trip, bounds, duplicate
  and illegal-value refusal, `ALL` exhaustiveness, and for the transcript: broad capture with
  upstream ordering, case-insensitive denial of credential and hop-by-hop headers, count and total
  bounds each marking truncation, malformed values dropped rather than rendered lossily, and
  `Debug` revealing only shape.
- `fuzz/fuzz_targets/contract_parsers.rs` — round-trip and bounds arms for both types, asserting
  over arbitrary input that a credential-bearing header never survives capture.
- `crates/south-contracts/tests/compatibility_manifest.rs` — every new manifest field asserted
  against its contract constant.
