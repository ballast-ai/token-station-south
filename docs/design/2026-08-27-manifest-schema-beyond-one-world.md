# The Manifest Schema Beyond One World: two extensions, proposed

Status: **ruled 2026-08-27**; D1 shipped in 0.17.0 — the manifest declares its
world, and the suite name, capability vocabulary, auth arms and WIT package
are validated against that world (`KNOWN_WORLDS`, one world today). D2–D4
(the `host_signed` arm, #43) await their slice. Issues: #43 and #53, each
closed by its implementing slice. Written as one record because they are the
same question asked twice.

Date: 2026-08-27

Predecessors: `2026-08-21-provider-api-promotion.md` (S1 — the manifest schema,
the closed enums, and the seven-field tuple), `2026-08-20-host-signed-request-finalizer.md`
(the host half of `HostSigned`, shipped in 0.14.0 without its component half),
`2026-08-27-task-adapter-vocabulary.md` (the second world that wants in).

## 1. Problem: one schema, written for exactly one component

`ComponentManifestV1` is not generic over worlds. It is the provider world's
schema with a general-sounding name, and it says so in three places, all in
`crates/south-provider-api/src/manifest.rs`:

- `validate_identity` refuses any `api_version` that is not `PROVIDER_WORLD`
  (`"provider-adapter-v2"`), by the named error
  `ApiVersionIsNotTheProviderWorld`;
- `conformance.required_suite` must equal `COMPONENT_BEHAVIOR_SUITE`
  (`"south.provider-component.v1"`), by `ConformanceSuiteIsNotTheComponentSuite`;
- `ComponentCapabilityV1` is `chat | stream | tool_call | json_schema`, and
  `AuthArmV1` is `bearer | header_secret | oauth`. Both are closed on purpose:
  an arm the schema does not know is a version mismatch, not a request to
  honour.

Two things now want to enter through that door, and they are usually filed as
unrelated:

- **#43**: a provider component that declares `host_signed` and the header set
  its host finalizer will emit. The 0.14.0 slice shipped the host half — the
  arm, the seam, the allow-list diff, both entry points — and left the
  component-facing half undone. The `AuthArmV1` doc comment had promised it
  would arrive "with the finalizer slice … as a schema bump carrying its
  `emits` declaration alongside"; that slice touched this crate by one version
  literal in a test.
- **#53**: a task component, whose world is `task-adapter-v1` and whose suite is
  not the provider suite.

They are the same question. The compatibility tuple's field 4 **is**
`api_version` and its field 6 **is** `conformance.required_suite`
(`compatibility_tuple()` reads them straight off the manifest). So "can the
schema describe something else" and "what does the tuple mean when it can" are
one decision, and #43's own fourth question already lands on #53's ground.

Answering them separately risks doing the arm twice: once inside today's
provider-only schema, and again after the schema learns there is more than one
world.

## 2. D1 — Generalise by adding a world descriptor, not by loosening the checks

Two shapes were considered.

**(a) One schema, world-parameterised.** `api_version` validated against a set
of known worlds; `required_suite` keyed by world; `capabilities` widened to a
union. One admission path, one tuple.

**(b) A schema per world.** `TaskComponentManifestV1` beside the existing one,
sharing the identity, permissions and compatibility halves.

**Recommendation: (a), with the parameterisation made explicit rather than
implicit.** Not by relaxing `validate_identity` to accept a wider set — that
turns three exact, named errors into one vague one and loses what the current
messages buy. Instead the manifest declares **which world it is for**, and the
schema validates the rest *against that world*: the suite name, the capability
vocabulary, and the auth arms all become properties of the declared world
rather than constants of the file.

The reason to prefer (a) over (b) is that the tuple is shared either way. Two
schemas would mean two places that must agree about what the seven fields mean,
and the S0 freeze exists precisely because a tuple that means different things
in two places is not a handshake. (b) is closer to how this repository split
crates before — a new crate for gate ①/②, two frozen fixture packs — but those
splits separated things that had no shared invariant. This one does.

**Consequence:** the errors stay exact. `ApiVersionIsNotTheProviderWorld`
becomes "this world is not one this South knows" and, separately, "this suite
is not the suite that world is judged by" — still two failures with two names,
now both parameterised by the declared world.

## 3. D2 — `host_signed` is an arm plus a declaration, and the declaration is the contract

`AuthArmV1` gains `host_signed`. On its own that is not enough: the other three
arms are complete statements ("attach the named credential this way"), while a
signed request is only complete once you also know **which headers the finalizer
will emit** — that set is what South diffs the finalizer's output against, in
both directions.

**Recommendation.** The manifest carries the arm *and* its `emits` set, reusing
the frozen `SignedHeaderV1` vocabulary the host half already ships
(`authorization`, `x-amz-date`, `x-amz-content-sha256`, `x-amz-security-token`).
An empty `emits` on a `host_signed` declaration is refused at gate ①: an empty
allow-list makes the diff vacuous, so a finalizer could emit nothing and still
satisfy it — an unsigned request that looks finalized. `SignedHeaderSetV1`
already refuses `Empty` and `Duplicate` for the same reason on the host side;
the manifest should refuse the same shapes with the same words.

## 4. D3 — What `build-http-request` puts in `auth`: nothing new in the IR

This is #43's second question, and the one with a real trap.

The IR's `Auth` is closed at `Bearer | Header | OAuth`, and its own doc comment
anticipates the answer: *"a `SigV4` signature over the request body … would need
`Auth` to carry the output of the `host.sign` ABI call — goes to `-v2` rather
than being approximated here."* But S0 §7 ruled that `host.sign` is
**orthogonal** to the Finalizer and that components "must not use it to imitate
SigV4". Both statements are in force and they point at different designs. That
contradiction has to be resolved before anything is written, or the resolution
becomes an accident.

**Recommendation: resolve it in favour of S0 §7, and add nothing to the IR.**
The descriptor omits `auth` (`None`), and the *manifest's* `host_signed`
declaration is what tells the host this component's requests are finalized. The
reasoning:

- The component genuinely has nothing to say. It does not sign, it does not
  hold a key, and it cannot name a credential value. The one thing it knows —
  "these requests get signed, and these headers will appear" — is a property of
  the component, not of an individual request, so it belongs in the manifest,
  which is read once at admission, rather than in every descriptor.
- An IR change is the four-repo cadence of S0 ruling D5: community `protocol`
  release → kernel sync → `schema_id` bump → both hosts re-pin. Paying that for
  a field whose value would be constant per component is a bad trade.
- `auth: None` already has a meaning — "unauthenticated upstream, such as a
  local Ollama endpoint" — so this overloads it. That is the cost, and it is
  why the manifest declaration must be the thing the host reads: a host that
  looks only at the descriptor cannot tell a signed request from an
  unauthenticated one. **South's task is to make that indistinguishability
  unreachable**, by refusing at gate ① any component whose manifest declares
  `host_signed` but whose descriptors carry an `auth`, and vice versa.

**Rejected: a `host.sign`-shaped `Auth` variant.** It is the shape the IR
comment anticipates, and S0 §7 already ruled against components imitating SigV4
through `host.sign`. Reviving it here would overturn a ruling by implication
rather than by argument.

## 5. D4 — Version and blast radius

Adding an arm to a closed enum in a `deny_unknown_fields` schema is a **wire**
change: a manifest declaring `host_signed` fails to deserialize on an older
South, which is exactly the intended behaviour (refusal, not silent
degradation), and an older manifest still validates on a newer South.

**Recommendation:** a south minor, not a WIT package or world bump. Neither the
WIT package version (tuple 3) nor the world name (tuple 4) changes for #43 — the
provider world's functions are untouched. The tuple's `south_runtime` (tuple 5)
moves with the release, which is what already refuses a component built against
an older schema.

For #53 the world name *is* new, so tuple 4 differs by construction and tuple 6
follows it; no additional versioning mechanism is needed beyond D1's
parameterisation.

## 6. What this record does not decide

- **The task vocabulary itself** — `2026-08-27-task-adapter-vocabulary.md` (#52).
- **The `emits` half of the host suite**: whether a third-party host's finalizer
  gets a runnable fixture pack. Deferred deliberately in the finalizer record §8
  until a real SigV4 signer exists.
- **Capability vocabulary for the task world.** D1 makes `capabilities` a
  property of the declared world; what the task world's set *is* belongs with
  that world's own slice.

## 7. Versioning

No version. Both extensions are additive and neither has a consumer until its
slice lands; the slice takes the next minor at ship time.
