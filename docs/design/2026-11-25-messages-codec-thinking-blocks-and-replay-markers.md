# Messages north codec: one wire block per thinking block, and opt-in Claude replay markers

Status: proposed.

Date: 2026-11-25

Baseline: south `origin/main` v0.87.1 (`8cda88b3`).

Origin: Token Station plan P4.1 (the first follow-up batch after its `agent-anthropic` moved to this codec, Token
Station #64). Token Station carries both items below as adapter shims today. This record moves them into the codec,
so both hosts share them and the shims can go.

## 1. Problem

Two gaps in the Anthropic Messages family of `south-north-codec`:

1. **Interleaved thinking renders into one block.** `AnthropicSseState` keeps a single `thinking_index`. A stream
   with two thinking blocks (thinking, a tool call, thinking again, which interleaved thinking produces) renders the
   second block's `thinking_delta` and `signature_delta` frames into the first wire block. The client receives one
   thinking block that carries two signatures, so the reasoning cannot be replayed on the next turn. At `Done`, only
   that one block is closed.
2. **The Messages decoder cannot mark a turn for Claude replay.** The Responses decoder writes the
   `reasoning_replay_protocol_family` and `reasoning_replay_block_layout` extensions that a component reads before it
   replays signatures. The Messages decoder writes none. A host that grants `reasoning_replay.claude.v1` to an
   official Anthropic target, as Token Station does, must compute the layout itself from the wire blocks.

## 2. Decisions

### A1. One wire block per source thinking block

`AnthropicSseState::thinking_index: Option<usize>` becomes `thinking_blocks: BTreeMap<u32, usize>`: the IR thinking
block (`block_index` of `ThinkingDelta` and `ThinkingSignatureDelta`) to its content-block number, as `tool_blocks`
already maps tool slots.

- A `ThinkingDelta` for a new `block_index` opens a new thinking block. One for a known `block_index` writes to that
  block.
- A `ThinkingSignatureDelta` writes to the block of its own `block_index`. With no such block open it renders nothing,
  as before.
- `Done` closes every thinking block, in IR block order, then text, then tool blocks, as before.

A stream with one thinking block renders exactly as before. Events that omit `block_index` deserialize to the
protocol's legacy default (`0`) and share one block.

### A2. Opt-in Claude replay markers on Messages decoding

`chat_request_from_anthropic_messages_with(body, AnthropicRequestOptions { claude_replay_markers: true })` marks each
assistant turn that can carry Claude replay. `chat_request_from_anthropic_messages(body)` is the same function with
the default options, and its output does not change.

A turn is marked when:
- it carries reasoning (a `thinking` or `redacted_thinking` block);
- every `thinking` block has a non-empty signature;
- every block is `text`, `thinking`, `redacted_thinking` or `tool_use`;
- its tool ids are present and distinct;
- it has at most 128 blocks.

The layout lists the wire blocks in order. A `tool_use` block appears as `{"kind": "tool_call", "call_id": …}`, and
every other block as `{"kind": "content", "ordinal": n}`, its ordinal among the turn's content parts. The decoder
keeps one part per non-`tool_use` block, in order, so the ordinals name the right parts. This is the layout the
components' `validated_layout` and the Responses carrier already read.

**Why opt-in.** A marker asks the target to trust reasoning state it issued earlier, and a component refuses a marked
turn for a target that does not declare `reasoning_replay.claude.v1`. Only a host that grants the capability per
target, and removes the markers for every other target, can turn this on. token-station-server decodes Messages with
this codec and keeps its own replay-carrier policy, so its default decode must not change.

## 3. Non-goals

- **`content: []` for a tool-only assistant turn.** The decoder gives an empty part list for an assistant turn with
  only `tool_use` blocks, and the OpenAI-compatible reference renders `content: []` on purpose (the R1 + R5
  host-parity slice). Token Station considered decoding such a turn as `content: None`. That would change the
  upstream request body on token-station-server's translated legs, which its dual-run legs compare against frozen
  snapshots that cannot be re-recorded. Token Station keeps its own adapter shim for this. Not changed here.
- The references' replay gating and rendering. Unchanged.
- Typed IR fields for the thinking configuration or the tool-result error flag. Those need a kernel protocol release.

## 4. Compatibility

- **Public API.**
  - `AnthropicSseState::thinking_index` is removed and `thinking_blocks` is added. A host that builds the state with
    `AnthropicSseState::new` is unaffected. token-station-server does. Token Station's adapter sets `thinking_index`
    today as its shim, and removes the shim when it moves to this release.
  - `AnthropicRequestOptions` and `chat_request_from_anthropic_messages_with` are new.
- **Client-visible stream change (A1).** A stream with more than one thinking block now renders one block per source
  block, each with its own signature and its own `content_block_stop`. token-station-server's north-stream snapshots
  that hold such a stream change. The server judges them when it re-pins.
- **Upstream wire.** Unchanged for a default decode. With the option on, a marked turn reaches a component with the
  two extensions. What the component then does is unchanged.

## 5. Tests

- `tests/anthropic_stream.rs`:
  - `interleaved_thinking_blocks_keep_their_own_wire_blocks_and_signatures` checks that thinking, a tool call and
    thinking again render as three blocks, that each signature lands in its own block, and that every block is
    closed once before `message_delta`, the thinking blocks first in IR block order.
  - `thinking_events_of_one_source_block_share_one_wire_block`.
- `tests/anthropic_messages.rs`:
  - `a_signed_turn_is_marked_with_its_wire_order_only_when_the_host_asks` checks the family and layout with the
    option, and no extensions with the default decode.
  - `a_turn_the_layout_cannot_describe_is_not_marked` covers:
    - a string turn;
    - no reasoning;
    - unsigned thinking, and an empty signature;
    - a `server_tool_use` block;
    - duplicate tool ids;
    - more than 128 blocks.

The new tests fail on `8cda88b3`: the stream test renders the second thinking block into block 0, and the marker
tests do not compile without the new API.

## 6. Release note

- Messages north codec: a stream with more than one thinking block renders one wire block per source thinking block,
  each with its own signature and stop. Streams with one thinking block are unchanged.
- New `chat_request_from_anthropic_messages_with` and `AnthropicRequestOptions` (`claude_replay_markers`, off by
  default) mark assistant turns for Claude signed-thinking replay. The default decode is unchanged.
- `AnthropicSseState::thinking_index` is replaced by `thinking_blocks`.
