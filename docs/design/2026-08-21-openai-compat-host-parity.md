# OpenAI-compatible component: host-parity slice

- Slice: behaviour changes to `OpenAiCompatibleReferenceV1` (and therefore to the
  official `provider-openai-compatible` component, which is a wit-bindgen shell
  around it)
- Origin: the enterprise host's S5-5a adoption ledger
  (`token-station-doc` `docs/design/2026-08-21-s5a-server-component-adoption.md`)
- Status: rulings taken 2026-08-21; this is the south half of them

## 1. Why this slice exists

The enterprise host (`token-station-server`) built a divergence ledger between
its own `translate_ir` family and this component, pinned it as assertions, and
then made the ledger's **completeness** machine-checked: normalise both sides so
that exactly the already-registered divergence classes disappear, then assert
byte equality. Anything new survives normalisation and turns the gate red.

That sweep — 18 curated cases plus 200,000 randomised cases per direction —
grew the ledger from 6 classes to 12, and two of the additions were worse than
most of the original list. The host's rulings assign seven of the twelve to
this repository. This slice implements those seven.

The framing that decided most of them: **a migration that only swaps
implementations must not change which requests succeed.** Where this component
refused something the host serves today, this component yields.

## 2. What changed

| Ref | Behaviour before | Behaviour now | Why |
|---|---|---|---|
| **R1** | a `Content::Parts` array holding only text collapsed to a bare string | `Content::Parts` always renders as an array | The IR distinguishes `Text` from `Parts`; collapsing discards a distinction the IR states. Not "more concise" — lossy. |
| **R5** | content that renders to nothing became `null`; an absent content omitted the key | `Parts` → array (possibly empty), `Text` → string, `None` → explicit `null` | One principle, not two patches. Three spellings of "empty" (`null` / omitted / `[]`) are not interchangeable to every upstream, so the shape now follows the IR rather than a heuristic. **Migration note in §4.** |
| **R3** | `ContentPart::RedactedThinking` returned `capability` 400 | dropped silently | Refusing turned a request the enterprise host serves today into a failure. The block has no OpenAI-compatible spelling either way; dropping is the same lossy-but-working choice the host already makes. |
| **R2** | `reasoning_content` emitted whenever the IR held a thinking block | emitted only when the model declares it (§3) | Legacy reasoner-style upstreams **reject** a request carrying the field. Emitting it unconditionally sends a field known to be refused. |
| **S1** | zero-length text deltas produced a `Delta` event | zero-length text **and** reasoning deltas produce nothing | OpenAI's opening frame is `delta:{role,content:""}` by convention. An event for it makes a northbound renderer open an empty content block, shifting every block index after it — measured end-to-end, this single behaviour was the only structural misalignment in the host's stream comparison. |
| **S4** | only `reasoning_content` was read from a stream delta | `reasoning_content` **or** a bare `reasoning` | Qwen, OpenWebUI and others spell it the second way. Reading one silently drops every thinking token from the other family: no error, no event, the client just never sees the reasoning. |
| **P1** | only `reasoning_content` was read from a buffered response | same two spellings | The non-streaming twin of S4. Fixing one side alone produces "thinking appears when streaming, vanishes when buffered", which is harder to diagnose than either failure alone. |
| **S5** | an event with empty `choices` and no `usage` returned `provider_protocol_error` (502) | ignored | It terminates the whole stream, and the shape is not exotic: **Azure OpenAI opens a stream with a `prompt_filter_results` frame carrying an empty `choices` array** — and `azure-openai-v1` is a dialect this component's own manifest declares. Ordinary keepalive frames look the same. |

Not in this slice (the host rules on its own side): the host adopting the IR's
`Finish`/`Done` split, the host unifying its tolerance of malformed 2xx bodies
with this component's refusal, and two host-side plumbing items.

## 3. How R2 decides

The gate is `ProviderConfig.models[].supported_parameters` containing
`reasoning_content`. That field is inside the S0 §6 policy fence, so a component
may read it: no new IR field, no `extensions` smuggling (S0 ruling D5).

**Declared wins; undeclared keeps the old heuristic.** A model listed in the
config without the parameter is a deliberate "no". A model the config does not
list at all falls back to the previous DeepSeek-prefix check, so a host that
ships no catalog — the community host's usual shape — sees no behaviour change,
while a host that declares one gets exact control.

## 4. Migration notes for adopting hosts

- **R5 changes a wire shape the community host sees today.** An assistant turn
  whose content is nothing but thinking blocks now renders `content: []` where
  it previously rendered `content: null`. OpenAI documents assistant `content`
  as string-or-null, so an empty array is outside that documented set. The
  enterprise host has rendered it this way in production all along, which is why
  its behaviour was adopted rather than a new shape invented — but a host that
  observes an upstream refusing `[]` should bring the case back to the ruling
  table rather than patching either side privately.
- **R2 changes which models receive `reasoning_content`** for hosts that declare
  a catalog. Declare `reasoning_content` in `supported_parameters` for every
  model that wants it before adopting this release.
- **Every other change is strictly more permissive**: requests and streams that
  previously failed now succeed, and reasoning that was previously dropped now
  arrives.

## 5. The fixture-pack finding

Gate ② is the layer that exists so behaviour is judged by frozen fixtures
rather than by review. It did not notice any of the seven changes above:
**the shipped pack covered none of these shapes.** All 329 tests stayed green
through the whole slice until the new fixtures were added.

That is not an argument against the gate — it is the reason these divergences
survived to be found by an adopting host instead of by us. Two consequences,
both landed here:

1. **Nine cases added**, one per behaviour, each written as intended output
   first and only then run — not recorded from whatever the implementation
   happened to produce.
2. **A presence gate**: the pack is discovered by scanning a directory, so a
   case whose file is renamed or mistyped simply stops existing while the suite
   still reports green over what remains. `the_shipped_pack_still_carries_every_host_parity_case`
   names these nine, so losing one is a red gate rather than a quiet hole.

The general lesson for future slices: **a fixture pack proves only what it
covers, and "all green" says nothing about the shapes nobody wrote down.**
