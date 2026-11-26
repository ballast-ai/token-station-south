# Public usage rules: OpenAI Responses `response.usage` and Gemini `usageMetadata`

Status: implemented in #364 (`53e01b1d`) and the #365 round (`3ac2a757`); released in 0.88.1.

Date: 2026-11-26

Baseline: south `origin/main` v0.88.0 (`bc07c516`).

Origin: Token Station plan P4.1, item B. Token Station ships its own components for subscription
plans (`plan-responses`, shared by its ChatGPT and Copilot plan components, and
`provider-codeassist-v2`). Each one carries a copy of a South usage rule. This record makes the two
rules public, so those components call South's rule rather than keep a copy.

## 1. Problem

Usage is funds evidence. South holds a strict rule for each wire: a missing count or a total that
does not add up is a protocol error, never a zero (south #131). Two of these rules are private:

- `reference_openai_responses::usage_of`, the terminal `response.usage` of OpenAI Responses;
- `gemini_text::usage_of`, Gemini `usageMetadata`. `reference_gemini` and `reference_vertex` share
  it through the crate-private `gemini_text` module.

A host component that reads the same wire must copy them. Token Station has two copies. They match
South's logic today, but nothing keeps them in step. Their error texts name the product ("Code
Assist usage …"), and one copy treats an explicit `null` count as an error where South reads zero.

## 2. Decision

Make both rules public where they already live. Move no code.

- `reference_openai_responses::usage_of(usage: Option<&Value>) -> ComponentResultV1<Usage>` becomes
  `pub`.
- `gemini_text::usage_of(meta: &Value) -> ComponentResultV1<Usage>` becomes `pub` inside its private
  module, and `reference_gemini` re-exports it as `reference_gemini::usage_of`.

The references keep calling these same functions, so the public rule and the judged rule cannot
drift. Each function's documentation states its contract:

- every refusal is `provider_protocol_error` (502);
- its text names the upstream or its wire ("the upstream …", "a Gemini response …"), never a
  product. A host that needs its product name in a message adds it where it reports the error.

The rules themselves do not change.

## 3. Non-goals

- The usage rules of the other references (Chat Completions, Anthropic Messages, Bedrock Converse).
  No host copies them today.
- A shared `usage` module. Moving the functions would change no behavior and would make the diff
  larger.

## 4. Compatibility

- Public API: two new public functions in `south-component-conformance`. Nothing is removed or
  renamed.
- Behavior: unchanged. No fixture changes.
- Package identities: they change. South #364 changed `south-component-conformance/src` without
  the crate bump that CONTRIBUTING requires in the same change, and an earlier version of this
  record said no identity changes. The 0.88.1 round (§7) adds the bump.

## 5. Tests

`tests/public_usage_rules_v1.rs` calls both functions only through their public paths:

- Responses: every count maps; absent or null details read zero; refusals of a missing usage
  object, each missing or negative required count, a total that does not add up, details that are
  not an object, an invalid detail count, cache counts above input, and reasoning above output.
- Gemini: thoughts move into output and tool-use prompts into input; an omitted
  `candidatesTokenCount` reads zero only when the total closes; refusals of a missing
  `usageMetadata`, each missing required count, an invalid count, cached above prompt, a total that
  does not add up, and an overflow.
- Every refusal is checked to be a 502 `provider_protocol_error`, and its text to name the upstream
  or its wire.

## 6. Release note

- `south-component-conformance`: `reference_openai_responses::usage_of` and
  `reference_gemini::usage_of` are public. A host component that reads OpenAI Responses or Gemini
  usage can apply South's rule rather than copy it.

## 7. The 0.88.1 round

A change to `south-component-conformance` enters every `component.wasm` (CONTRIBUTING, "The crates
every component links carry their own versions"; boundary record §13.12, §16 Q47). So:

- `south-component-conformance` 0.82.0 → 0.82.1: a compatible change, so the patch.
- Every one of the 41 official packages takes the Q47 patch: manifest, `Cargo.toml`, `Cargo.lock`,
  and the version literal its reference reports. No runtime, contract or kernel tuple moves.
- The requirements on the crate (`south-testkit`, `fuzz`), the synthetic-speech guest lockfile and
  `ARCHITECTURE.md` follow.
- `public_usage_rules_retire_every_published_088_package_identity` pins the round. The identities it
  retires were published with 0.87.0 and again unchanged with 0.87.1 and 0.88.0. The 0.87.0 round
  test now only refuses its identities. The tests that pin one package's version follow.

A host that vendors a South package takes the new version with its next pin. Token Station's
`provider-openai-compatible-v2` and `provider-anthropic-v2` are two such packages.
