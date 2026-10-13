# Frozen samples: the Kiro upstream (`provider-kiro`, family `kiro`)

Gate ② samples for `provider-kiro` 1.0.0, judged by `south.provider-component.v1`
(`docs/design/2026-09-30-kiro-provider-component.md` §11.2). **No row is a capture of live traffic made for this
record**: the real-account capture is pending (lv, 2026-10-13: implement now, test with an account later), and the
rows marked provisional in the record's rulings (K-Q5, K-Q6, K-Q10, K-Q11, K-Q18) may change when it lands. Every
token, id and ARN below is fake.

Stream and response inputs are written in the canonical re-encoding (`south_contracts::reencode_eventstream_v1`):
the package declares `stream_framing: aws-eventstream`, and its family declares `request_facts.stream: "none"`, so a
non-streaming answer reaches `parse-response` as the same text, whole (the buffered path, boundary §5.2).

Each row's source:

- **Request rows**, from the host's native arm (server `crates/gateway-provider-protocol/src/translate_kiro.rs`, the
  composition of the Anthropic first hop and `anthropic_messages_to_kiro`, record §4.2):
  `system-tools-and-tool-results` is the host's
  `anthropic_to_kiro_builds_conversation_state_with_system_tools_and_results` written as IR (system prepended to the
  first user entry, `toolUses` with a parsed object `input`, the tool result as the current message, the tools on the
  current message only); `chat` and `no-profile-arn` are `anthropic_to_kiro_omits_profile_arn_and_history_for_single_turn`
  with and without the exported `profile_arn` attribute; `system-without-history-prepends-to-the-current-message` is
  `anthropic_to_kiro_prepends_system_to_current_when_no_history`; `system-replaces-an-empty-first-user-entry`
  (system parts joined by `\n`, empty parts skipped, a later system message joined in order, an empty first user entry
  replaced rather than prefixed); `parallel-tool-results-are-separate-user-entries` and `tool-result-then-user`
  (one entry per IR message, nothing merged: D11, K-Q18); `tool-choice-none-withholds-tools`,
  `tool-choice-auto-sends-tools`; `sampling-has-no-slot` (temperature, `top_p`, `top_k`, stop, the cap,
  `parallel_tool_calls` and the reasoning effort leave no trace: KR13, KR14); `model-is-verbatim` (a dashed id is not
  rewritten, KD1); `reasoning-parts-are-dropped` (KR12); `prompt-cache-markers-are-dropped` (KR15);
  `stream-is-the-same-request` (no stream switch). The headers are the native arm's (server
  `…/engine/upstream.rs` `apply_kiro_auth`, `…/engine/kiro.rs`), without `user-agent`, which the manifest declares,
  and without `content-type`, as natively (K-Q10). `conversationId` is the host's `attempt_id`.
- **Refused request rows**, one per numbered refusal of the record (KR1 to KR9): `refused-image`,
  `refused-image-in-history`, `refused-unknown-part`, `refused-json-response-format`, `refused-required-tool-choice`,
  `refused-named-tool-choice`, `refused-assistant-final-turn`, `refused-non-object-arguments`,
  `refused-empty-tool-call-id`, `refused-tool-result-without-id`, `refused-no-message`, `refused-missing-attempt-id`,
  `refused-malformed-attempt-id`.
- **Stream and response rows taken from the host's bytes**: `text` is server `gateway/tests/kiro_eventstream.rs`
  `normal()`; `metering-is-never-usage` is `translate_kiro.rs` `kiro_accumulate_builds_text_message_and_sums_credits`
  (with `contextUsageEvent` and a `modelId`); `tool-use` is
  `kiro_accumulate_builds_tool_use_from_streamed_input_fragments`, which that test calls "exactly the captured
  sequence"; `open-tool-call-at-eof` / `open-tool-call` is server `…/engine/kiro.rs` tests `normal()` (a tool fragment
  with no start and no stop, which the native arm completes and this package does not: KD5). The test
  `the_host_derived_stream_rows_are_the_hosts_bytes_re_encoded` rebuilds those binary bodies and checks that south's
  deframer and re-encoding produce these rows byte for byte.
- **Stream and response rows written for the record**: `text-then-tool`, `two-tool-calls`, `unknown-event-ignored`,
  `empty-content-ignored`, `metering-only-completes-empty`, `tool-call-without-stop-at-eof`, `no-frames` (KD4),
  `exception-frame` (the host corpus' `ThrottlingException` frame), `error-frame` (an `error` frame with its
  `:error-message`; the host corpus' bare `error` frame has none, so south's re-encoding refuses it before the
  component), `non-json-data-refused` (KR16), `tool-event-without-id-refused` (KR17),
  `tool-input-not-a-string-refused` (KR18), `tool-event-after-stop-refused` (KR19).
- **Error rows**: the status table of record §9 and the AWS exception-name arm (K-Q6, provisional:
  `rejected-credential` 403 with `AccessDeniedException`, `expired-token-is-auth` 401, `throttled-carries-retry-after`
  429 with `__type` `ThrottlingException` and `retry-after`, `validation-is-invalid-request` with a suffixed
  `x-amzn-errortype`, `opaque-500` (the upstream answers a malformed body with an opaque 500, server `kiro.rs`),
  `payment-required` 402, `unknown-status-is-internal` 418).
- **Credential rows** for the recipes `social` and `idc` behind the selector `kiro` (record §7.2), run by gate ②'s
  `CredentialRecipeMatch` with south's reference interpreter, from the host's `KiroMint` and `KiroExtras`
  (server `…/engine/token_refresh.rs`): `rotation.social-refresh`, `rotation.idc-refresh` (an `IdC` method in another
  case, the `eu-west-1` region in the endpoint, the camelCase JSON body with `grantType`),
  `rotation.social-without-new-token-keeps-the-stored-one`, `clock.expiry-default` (no `expiresIn`: 3600 s),
  `clock.expiry-clamped-to-a-day`, `clock.select-client-fields-pick-idc`,
  `clock.select-explicit-social-wins-over-client-fields`, `clock.select-enterprise-is-idc`,
  `clock.no-refresh-token-fails-as-configuration`, `clock.idc-missing-client-fields-sends-nothing`,
  `clock.region-not-region-shaped-sends-nothing`, and `on-status` 400 and 401 (`reauth_required`, the only terminal
  statuses natively), 403 and 429 (`transient`), 500 (`transient`, KD8).

There is no usage row: the package declares `usage_evidence: absent`, so every response and stream row is held to
`AbsentFamilyEmitsNoUsage` instead, and the usage judge (`usage_ir_contract_v1`) holds the reference to the
declaration's property for any numbers the wire carries.

Expectations were produced by the reference and the reference interpreter, then each was read against the record and
the host behaviour above; `request.chat`, `stream.tool-use` and `response.text` were written by hand first and the
pack test was seen to fail on the rest.
