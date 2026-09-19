# The Runtime's Second World: where a world begins and ends

Status: **proposed** 2026-09-19. Issue: the gap the task component slice
(#83) stopped at — a task guest builds, and nothing can instantiate it.

Date: 2026-09-19

Predecessors: `2026-08-27-manifest-schema-beyond-one-world.md` (made the
manifest world-parameterised and listed *"whether gate ① can admit a task
component"* as its question, leaving the runtime's side unasked),
`2026-09-18-task-adapter-world.md` (the world), `2026-09-19-task-world-fit-survey.md`
and `2026-09-19-task-vocabulary-fit-survey.md` (its signatures and types).

## 1. Problem

`south-provider-runtime` calls `bindgen!` once, for `provider-adapter-v2`, and
reaches a guest through eight accessors generated for that one world. Loading
never branches on the declared world: `admit()` builds a linker, instantiates
`ProviderAdapterV2`, and calls `metadata()` through the provider accessor.

So a `task-adapter-v1` component is admissible by gate ① (0.17.0 made the
manifest world-parameterised) and judgeable by gate ② in its native form
(#83), but cannot be loaded. The three gates disagree about whether a second
world exists.

The manifest-schema record parameterised *the manifest*. This record asks the
question it left alone: **what does the runtime share between worlds, and what
must it hold twice?**

## 2. What is actually world-specific

Measured against the two worlds that exist:

| Concern | Shared? | Why |
|---|---|---|
| Engine, epoch deadline, memory limiter | **shared** | Properties of the sandbox, not of an ABI |
| WASI linker setup | **shared** | Same locked-down p2 surface either way |
| Manifest gate, import scan, compatibility tuple | **shared** | Already world-parameterised since 0.17.0 |
| Identity probe (`metadata()` vs the manifest) | **shape shared, call specific** | Both worlds export `metadata`, through different accessors |
| `host.sign` | **identical in text, distinct in type** | §3 |
| The lifecycle calls | **specific** | Different functions, different payloads |

Only the last two rows force anything. Everything above them is why a second
`bindgen!` is not a second runtime.

## 3. The one genuine surprise: `host.sign` is two types

Both WIT files declare byte-identical `host` interfaces:

```wit
sign: func(secret-ref: string, payload: list<u8>, algorithm: string) -> result<list<u8>, string>;
```

They are nonetheless **different generated types**, because they live in
different WIT packages (`token-station:adapter` and
`token-station:task-adapter`). `wit_host::add_to_linker` registers one of
them; a task guest importing the other finds nothing.

Three ways out were considered:

**(a) One shared `host` package both worlds import.** The cleanest model —
`token-station:host@1.0.0`, imported by both. It is also a **breaking change to
a published world**: `provider-adapter-v2`'s import would move, and every
shipped provider component would have to be rebuilt against the new package.
Three components exist and their packages are released artifacts.

**(b) Register both, from the same `Ctx`.** Each world's `add_to_linker` is
called on its own linker; the implementation behind them is one `impl` written
once and delegated to twice. Duplication is two lines of registration, not two
implementations.

**(c) Task world imports nothing.** Kling signs nothing, so the first task
component would not notice. But Bedrock and Kling's HS256 JWT are both
task-side families — the world's own D4 says `host_signed` matters *more* here
than in chat. Removing the import would be a decision made on the first
family's convenience.

**Recommendation: (b).** (a) is where this should end up and costs a
coordinated rebuild that buys nothing today; (c) trades a real capability for
a saved line. Revisit (a) whenever a third world appears — at that point the
rebuild is amortised over a change that is happening anyway.

## 4. D1 — Two bindings, one loader, dispatch at instantiate

**Recommendation.** Keep `LoadedComponentV1` as the single admission path and
make the *instance* an enum:

```rust
enum InstanceKind {
    Provider(ProviderAdapterV2),
    Task(TaskAdapterV1),
}
```

`admit()` reads the already-validated `manifest.api_version`, instantiates the
matching world, and probes identity through that world's accessor. Everything
before instantiation is untouched, because none of it was ever world-specific.

**Rejected: a second `LoadedTaskComponentV1` beside the first.** It would
duplicate the manifest gate, the import scan, the compatibility handshake and
the limiter wiring — four things that are shared — to avoid an enum with two
variants. The bug that shape invites is the worst kind: a gate tightened in one
loader and not the other, with nothing to notice.

**Rejected: generic over the world.** A trait abstracting "a world's accessor"
would have to abstract over two sets of functions with nothing in common
beyond `metadata`. The abstraction would exist to serve one call.

## 5. D2 — A call into the wrong world is a load failure, not a call failure

`ProviderComponentV1` and `TaskComponentV1` are separate traits, so the type
system already prevents calling `build_http_request` on a task component
*through the typed seam*. But `LoadedComponentV1` also has a JSON face, and
there the mismatch is reachable.

**Recommendation.** The typed seams stay separate — `SandboxedComponentV1`
(provider) and a new `SandboxedTaskComponentV1` — and each is constructed only
from a `LoadedComponentV1` whose world matches, checked once at construction
and returning `None` otherwise. A wrong-world call then cannot be written,
rather than failing at runtime.

This is the same discipline the manifest already follows: a world mismatch is
an admission-time refusal with a named reason, never a late surprise.

## 6. D3 — `NotAComponent`'s message stops naming one world

Today: *"not a provider component"*. A task guest that fails to instantiate
would be told it is not a provider component, which is both wrong and the kind
of message that sends someone to look in the wrong place.

**Recommendation.** The error names the world the manifest declared: *"not a
`task-adapter-v1` component"*. One-line change; it is listed because a
diagnostic that misnames the thing it rejected costs more than it looks.

## 7. What this record does not decide

- **The shared `host` package** (§3 option (a)) — revisit at the third world.
- **Whether one package may ship two components**, one per world. Nothing here
  forbids it and no one has asked.
- **Streaming for the task world.** There is none: a task lifecycle is
  request/response at every stage, so `open_stream` stays provider-only.
- **Gate ③ host obligations** (the pinned-credential rule) — a host suite, and
  still unwritten.

## 8. Versioning

A south **minor**. No published contract changes: both worlds keep their WIT
packages, every shipped provider component keeps loading through the same path
it does today, and the task world gains an implementation of something it was
already specified to have.
