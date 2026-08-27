# The Task Adapter Vocabulary: three semantics, proposed

Status: **proposed** — D1–D3 are recommendations, not rulings. Written ahead of
the slice because each one decides a field of a vocabulary that is meant to be
frozen, and a frozen contract argued after the fact is a contract nobody can
change. Issue: #52.

Date: 2026-08-27

Predecessors: `2026-08-21-canonical-ir-inventory.md` (S0 — the IR subset, the
seven-field tuple, and the `ProviderConfig` policy fence this vocabulary sits
beside), `2026-08-20-host-prelude.md` (`PreparedSecretResolverV1`, which D1
reuses), `2026-08-20-host-signed-request-finalizer.md` (the shape of a slice
that answers an issue: propose, rule, then ship). The adopting host's own
records are the evidence base: its plan 44 §10 (the overturned four-function
design), its plan 46 §2–§3 and §9 (the seven-half-function decomposition, the
three open semantics), and six real provider families already cut behind a
`VideoTaskAdapter` trait.

## 1. Problem

A chat call is one request and one response. A media task is a lifecycle —
submit, observe until terminal, render — with a durable row, a lease, a
reservation and a settlement between the steps. The adopting host has already
cut that seam inside its own binary and proven it across six families; what is
missing is the vocabulary the two halves speak once the seam becomes an ABI.

Three of its semantics cannot be settled by looking at the host's code, because
they are only decidable once there is a boundary. They are the subject of this
record.

Where the vocabulary lives is settled and recorded here so the record reads
standalone: **`south-contracts`**, in a `task` module, not the Canonical IR.
The IR's authority is the community host, which does not run asynchronous media
tasks; authoring a task lifecycle there would make that host define and version
types it never executes, behind a four-repo cadence (protocol release → kernel
sync → tag → south bump → `schema_id` bump). South already owns a parallel
execution vocabulary on exactly this reasoning. The transport shapes stay IR and
are reused verbatim: `HttpRequestDescriptor`, `HttpResponseParts`,
`ErrorEnvelope`, `ErrorCode`, `ProviderConfig`.

## 2. D1 — Pinned credential

**The hazard.** Polling and artifact fetch must run under the credential the
task was *submitted* with. Providers that scope a task id to the submitting
account — Kling and MiniMax among them — answer any other credential with
404/403. A component returns `Auth::Header{name, secret-ref}`, which says only
*which name*; a host holding that and nothing else will reasonably resolve it
the way it resolves a chat credential, by weight and failover. The symptom is
"polling fails intermittently, and tasks whose credential rotated are lost
forever": money spent, artifact unreachable, and in the log it is indis­tinguish­able
from an upstream that has not finished yet.

This is not hypothetical. The adopting host's own poller substituted
`creds.first()` at two sites — the observe query and the artifact fetch — with
a comment describing it as a fallback. It was fixed on 2026-08-27 after this
record's evidence was gathered.

**What South can and cannot do.** South cannot *enforce* pinning: which
credential a task was submitted with is host state, on the durable row, and
South has no memory across calls. Saying otherwise in a design record would be
promising a guarantee the code cannot keep. What South can do is decide where
the choice happens.

**Recommendation.** The task-side entry points take a **pre-resolved
credential** — the existing `PreparedSecretResolverV1` shape — and **not** a
`CredentialResolver`. The difference is not cosmetic:

- with a resolver, picking is a step that happens *inside* South's call, at the
  moment when only the host's routing policy is looking, and the default
  behaviour of a chat-shaped resolver is exactly the bug;
- with a pre-resolved secret, the choice is a statement the host makes at a
  named place, once, in code that a reviewer can see.

That does not make substitution impossible. It moves it from "what a resolver
does by default" to "a line someone wrote", which is the whole distance between
the reported failure mode and a deliberate act.

`PreparedSecretResolverV1::expecting_slot` supplies the slot check for free:
resolution for any slot other than the declared one fails.

**Rejected: a pinning marker on the descriptor.** The component cannot know
which credential was used at submit. Asking it to declare that is asking it to
declare something it does not have, and a field every component must fill with
a constant is a field that stops being read.

**Consequence for conformance.** The rule is a host obligation, so it belongs
in a host suite (the gate ③ family), not in the component suite. A component
cannot pass or fail it.

## 3. D2 — Host-minted values on submit

**The hazard.** Submit needs values only the host can produce and the wire
needs: the gateway task id, which Kling embeds as `external_task_id` so a
resubmission is idempotent upstream, and — where the provider supports it — a
callback URL already bearing the host's nonce. The component knows *where* they
go in its dialect's body; the host knows *what* they are. Neither can do the
other's half.

**Recommendation.** A third parameter on `build-submit-request`,
`south_contracts::HostMintedValuesV1`, carrying a required task id and an
optional callback URL, with three rules stated in the type's own documentation:

1. **The component places them; it never invents them.** A component that
   generates its own task id destroys the host's idempotency anchor — the
   upstream would accept a resubmission as new work, and the reservation the
   host is holding would pay for both.
2. **They are opaque strings.** The component must not parse the callback URL,
   derive anything from the id, or reorder their bytes. A dialect that has no
   callback concept ignores the `Option`; South does not require it to be used.
3. **The nonce plaintext exists only inside that URL.** It is never stored and
   never logged — only its hash is persisted, and it is written before the
   upstream call because acceptance can trigger an immediate callback. So the
   type's `Debug` prints a byte count and not the value, the same discipline
   `CredentialSlotV1` and `JsonBodyV1` already follow. A component that echoes
   the URL into an error message leaks it, which is why the error channel
   carries an `ErrorEnvelope` and not free text.

**Rejected: the host post-processes the descriptor.** It would require the host
to know each dialect's field name — `external_task_id` here, `webhookConfig.uris`
there — which is the knowledge the component exists to hold.

## 4. D3 — Where `provider-expired` is decided

**The split.** Reading "the upstream said expired" off the wire is dialect
knowledge, so it is the component's. Deciding whether to try another upstream is
funds and routing policy, so it is the host's. Both halves have to be written
down; a component that reports "expired, retry elsewhere" has started making the
host's decisions, and a host that infers expiry from silence has started making
the component's.

**Recommendation.** `TaskObservationV1` carries
`running | succeeded | failed(kind) | unknown`, with
`kind ∈ {failed, cancelled, provider-expired}`, and:

1. **There is no `expired` observation variant.** Expiry is a failure kind. The
   client-facing `Expired` status word is a separate host mapping, and keeping
   them apart is what stops a public vocabulary change from becoming a contract
   change.
2. **`provider-expired` requires an explicit upstream statement.** Measured
   across six families: only BytePlus and xAI say it on the wire. Everything
   else that looks like expiry is either a plain failure or unknown.
3. **An unrecognised status word is `unknown`, never a synthesised failure.**
   Inventing a terminal renders "we cannot tell" as "it is over".
4. **A non-2xx query is `unknown`.** A failed *query* is not a failed *task* —
   429, 5xx, 404 and 401 all mean the observation did not happen.
5. **Neither side may synthesise a terminal from a timeout.** A component has
   no clock, so it can only report expiry when the wire says so. A host that
   maps its own deadline to a terminal failure settles a task that may still be
   running: the fee is released early and the caller is told a lie. A local
   deadline yields `unknown`, which is a reconciliation input, not an outcome.
6. **The host reads retry policy off the kind, and the component never expresses
   it.** There is no "retriable" flag on the observation.

**Consequence for conformance.** Unlike D1, this one *is* judgeable in the
component suite: per-dialect fixtures that feed a terminal body and assert the
kind, plus adversarial rows for the words that must fall through to `unknown`.
Those fixtures are the mechanism that keeps rule 2 true as families are added.

## 5. The fourth item of #52 — sized, deliberately not done

South cannot send the artifact fetch. The draft's claim that it is "a strict
subset of `HttpRequestDescriptor`" is true of the descriptor — `HttpMethod` has
`Get`, `body` is optional, `auth` is optional — and false of South's execution
path: `PreparedHttpRequestV1` sets `Method::POST` at both of its construction
sites, and `south-contracts` contains no `Method` at all. The whole buffered and
streaming surface is JSON POST by construction.

The shape of the fix is not in doubt: the request contract grows a method, with
its bounds and its conformance case, as a fourth `http` contract version.

It should nonetheless **land with the slice that needs it, not before**. Nothing
consumes a GET today, and this repository has already paid for the opposite
choice once: `south-transport-ureq` reserved a synchronous transport that no
host asked for, and was removed because "an empty crate did not constrain the
transport traits either way, so the reservation cost maintenance without
protecting anything". `AGENTS.md` says the same thing as a rule.

## 6. What this record does not decide

- **Whether gate ① can admit a task component at all.** It cannot today:
  `api_version` must equal `PROVIDER_WORLD`, `required_suite` must equal the
  provider suite, and `capabilities` is a chat-shaped enum. That is #53, and it
  is a manifest-schema question, not a vocabulary one.
- **The `host_signed` auth arm** — #43.
- **Per-dialect chunk grammars and fixture content**, which are authored per
  family, not frozen here.
- **The eighth function.** The draft exports eight lifecycle functions while its
  own header says seven; the extra one is `usage-intent`, and the two host
  records disagree about it — plan 44's ruling T-5 moved the metric into the
  component and left money in the host, while the ABI prospection note lists
  `reserve-bound` as host-only and counts seven. T-5 is the later and more
  specific ruling and the draft follows it. The count in that table needs
  correcting; that is the host's record to fix, not this one.

## 7. Versioning

No version. This record precedes its slice deliberately: D1–D3 each decide a
field of a vocabulary meant to be frozen, and the cost of ruling them after the
types exist is a contract change instead of a design choice. The slice that
implements them takes the next minor at ship time, per the retired
pre-allocation rule.
