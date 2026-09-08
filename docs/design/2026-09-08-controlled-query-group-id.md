# Controlled Query: Admitting `GroupId`

Status: D1–D2 ruled 2026-09-08 (see §5); shipped as 0.22.0

Date: 2026-09-08

Predecessor: `2026-08-18-controlled-query-support.md` — its D5 fixed the initial sanctioned set
at `api-version` and `alt` and named `GroupId` as "real but serving media surfaces this library's
call shapes do not yet cover", to be admitted only with a conformance case exercising it.

## 1. Problem

`MiniMax` runs two hosts. The international host (`api.minimax.io`) authenticates with a Bearer
token and nothing else. The China host (`api.minimaxi.com`) additionally rejects every request
that does not carry the account's group id as `?GroupId=<digits>` — on every path, including the
`OpenAI`-shaped text surface that South's call shapes already cover.

The server host already produces that URL and already routes `MiniMax` text through South's
Bearer arm. Its query lifter (`south_query_for`) only admits the frozen sanctioned set, so a
China-host request carrying `GroupId` is refused at pre-check and the whole request falls back to
the legacy path — silently, by design, and for every China-host `MiniMax` text call. D5's
condition ("a consumer on a covered surface") is therefore met by the text surface, not by the
media surfaces D5 was thinking of.

## 2. Boundary claim

Unchanged. The parameter set stays a frozen enum with a per-parameter grammar (record D1/D2); no
free-form names, no values from credential resolution. `GroupId` is operator configuration — an
account identifier, not a secret — and its grammar admits nothing a secret could be written in.

## 3. Design

```rust
pub enum QueryParameterV1 { ApiVersion, Alt, GroupId }   // ALL: [_; 3], canonical order
```

- Wire name `GroupId`, mixed case preserved: the upstream matches the name exactly.
- Grammar: a non-empty ASCII digit string of at most `MAX_QUERY_VALUE_BYTES`. No sign, no
  separators, no letters. Nothing narrower than a digit string is a real group id, and a digit
  string cannot smuggle a second parameter, a fragment, or a path segment.
- Canonical order appends it after `alt`, so every previously valid declaration serializes
  byte-identically.

### Conformance

`south.controlled-query.v1` gains a sixth case, `BufferedGroupIdQuerySuccess`, appended after
the frozen five: a buffered exchange declaring a nineteen-digit group id, expecting the wire to
carry `GroupId=…` exactly. It is the first case for a parameter admitted after the initial set,
and the one an adapter that lower-cases names or re-validates against a stale table fails on.

The suite version stays at `1`, by the precedent of the fifth case: the five existing cases are
byte-identical in declaration, upstream, outcome, and evidence, so recorded host evidence is not
invalidated. What changes is the class of adapter that can pass.

## 4. Versioning

`HTTP_CONTRACT_VERSION: 4 → 5`, additive: a version-four request is exactly a version-five
request that declares no `GroupId`. `compatibility.json` mirrors it. Ships as **0.22.0**.

Fuzz and property obligations carry over unchanged: both iterate `QueryParameterV1::ALL`, so the
new grammar is fuzzed and the join invariant is checked for it from the first run.

## 5. Decisions — ruled 2026-09-08

- **D1 — admit `GroupId` now.** D5's deferral condition is met on a covered surface; the
  alternative is a permanent legacy fallback for one host of one provider.
- **D2 — digits-only grammar.** Stricter than the adopting host's own handling (which
  percent-encodes a free-form string); the contract closes that gap rather than inheriting it,
  exactly as `api-version` did.
