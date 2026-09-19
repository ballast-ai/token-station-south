# `task-kling` fixture pack

Six families, one `south.task-component.v1` pack.

## Provenance — read this before trusting a case

**These cases were not captured from Kling.** Every input and expectation is
transcribed from the adopting host's production implementation and the word
table its own tests freeze (`normalize_kling`, `parse_kling_create`,
`KlingDispatch`). That code has served real traffic and carries the marks of
incidents, so the shapes here are evidenced — but they are evidenced
*second-hand*.

What a passing run therefore proves: **the component agrees with the host**.
What it does not prove: that the host agrees with Kling. That rests on
production traffic, not on a recording.

When someone captures real Kling traffic, these cases should be replaced by it
and this note deleted. Until then, a reader needing certainty about the wire
should consult the upstream's documentation, not this directory.

## The required row

`task.observation.query-404` exists because the 2026-08-27 vocabulary ruling
(D3) requires one non-2xx query row per family: a failed *query* is not a
failed *task*, and without such a row the check that enforces it never runs —
which the suite reports as a failure rather than a pass.
