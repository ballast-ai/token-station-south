# Provider quota metadata normalization in `south-core`

Status: **accepted** 2026-11-16 (lv, issue #342: the home is `south-core`, §3).

Date: 2026-11-16

Baseline: south `origin/main` = `a1b77783` (release 0.84.0 plus #334 and #335). Quota metadata contract 2 is
unchanged.

## 1. Problem

`ProviderQuotaMetadataV1` carries nine closed slots. Their names are vendor-named, but each name stands for a value
format:

| Slots | Value format of `reset` |
|---|---|
| `x-ratelimit-{limit,remaining,reset}-tokens` | Go-style duration (`6m0s`, `1s`, `250ms`) |
| `anthropic-ratelimit-tokens-{limit,remaining,reset}` | RFC 3339 instant |
| `anthropic-ratelimit-unified-{limit,remaining,reset}` | RFC 3339 instant; preferred over `tokens` |

A host that wants "limit, remaining, and when the window resets" therefore has to know which slot uses which format,
which slot wins, and how to read a Go duration. The server host does this in about 130 lines
(`credential_cooldown.rs`, `parse_go_duration_secs` and `quota_snapshot_from`). That is vendor knowledge in a host,
which the host-zero-vendor boundary (`docs/design/2026-09-30-host-zero-vendor-boundary.md`) assigns to South.

Renaming the slots to their formats would need quota metadata contract 3 and a re-release of every package. This
record does not do that.

## 2. Interface

```rust
pub fn normalize_provider_quota_v1(
    metadata: &ProviderQuotaMetadataV1,
    now: DateTime<Utc>,
) -> NormalizedQuotaV1;

pub struct NormalizedQuotaV1 {
    pub limit: Option<i64>,
    pub remaining: Option<i64>,
    /// Absolute reset instant (UTC).
    pub reset_at: Option<DateTime<Utc>>,
}
```

The function is pure: the caller passes `now`, and the crate takes `chrono` without its `clock` feature, so it cannot
read the clock even by accident.

Rules, taken from the server host's current behavior:

1. **Family.** If the `unified` reset or limit is present, read the `unified` slots; otherwise, if the `tokens` reset
   or limit is present, read the `tokens` slots; otherwise read the `x-ratelimit-*-tokens` slots. Only the chosen
   family is read.
2. **Limit and remaining.** The trimmed value parsed as `i64`; anything else is `None`.
3. **RFC 3339 reset.** Parsed after trimming. An instant in the past is still reported; the caller decides whether it
   is later than `now`. An unparseable value is `None`.
4. **Go duration reset.** After trimming, one or more `<number><unit>` components:
   - units are `h`, `m`, `s` and `ms` only, and `ms` is matched before `m`, so `250ms` is not 250 minutes;
   - the number is ASCII digits and dots and may have a fraction (`1.5s`);
   - each component is truncated to whole milliseconds, the components are summed, and the total is rounded up to
     whole seconds, so any positive sub-second reset is at least one second;
   - a total that is not positive, a missing or unknown unit, or a malformed number is `None`;
   - the result is `now` plus the rounded seconds.

## 3. Home

| Candidate | Why not |
|---|---|
| `south-contracts` (the issue's first proposal) | Every component links it. Any change to it bumps its version (§16 Q47), changes every `component.wasm` and re-identifies all packages, which the issue explicitly wanted to avoid. `chrono` would also enter every component lockfile. |
| `south-host-grammars` | It has no dependencies by rule, so it cannot see `ProviderQuotaMetadataV1`. A host would have to hand over the nine values by slot name, so the vendor-named slots would stay in the host. |
| **`south-core`** | Host-only (no component links it), already depends on `south-contracts`, and already holds string parsing both hosts used to hand-roll (`raw`). |

So the function lives in `south-core`. It adds the workspace dependency `chrono` (`default-features = false`,
`features = ["alloc"]`): host-side only, and already in the server host's graph.

## 4. Differences from the server host's implementation

The interface removes two edge cases in the server host's code; neither occurs with a real upstream value:

- The host added 999 to the millisecond total without a check, which overflows on a value such as a 255-digit
  number of hours. Here every step is checked and an overflow is `None`.
- When `now` plus the duration fell outside the representable range, the host reported a reset delay but no reset
  instant. Here both are absent, since the host derives the delay from `reset_at`.

## 5. Tests

- `crates/south-core/tests/provider_quota_normalization_v1.rs`: each family's format, the `unified` over `tokens`
  priority, `250ms` and `1h2m3s`, past instants, malformed values, and property tests (no panic on any slot values
  and any representable `now`; composed durations and millisecond durations give the expected instant; integers
  round-trip with surrounding whitespace).
- The scheduled fuzz target `contract_parsers` normalizes every quota metadata it builds, at an ordinary `now` and at
  both ends of the representable range.

## 6. Host follow-up

After the server host re-pins to a release that contains this, it deletes `parse_go_duration_secs` and
`quota_snapshot_from`, derives `reset_in_secs` from `reset_at` and `now`, and its tests move here (server rectification
plan, candidate 47 / D21).
