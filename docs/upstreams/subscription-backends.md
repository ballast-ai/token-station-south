# Subscription backends: OpenAI Codex and GitHub Copilot

Upstream facts for the `openai-codex` family of
[`provider-openai-responses`](../../components/provider-openai-responses/manifest.json) and the
`github-copilot` family of
[`provider-openai-compatible`](../../components/provider-openai-compatible/manifest.json). Moved from the
token-station-server host documentation on 2026-10-30. Claude Code and Kiro facts are in the
[Claude Code family](../design/2026-10-15-claude-code-family.md) and
[Kiro provider component](../design/2026-09-30-kiro-provider-component.md) records; Codex request fields
are in the [Responses R3 record](../design/2026-10-12-responses-r3-codex-and-request-fields.md).

## OpenAI Codex (ChatGPT subscription)

- Base URL `chatgpt.com/backend-api`, path `/codex/responses` (not `/v1/responses`).
- **Tokens rotate.** When the access token's JWT `exp` is within a few minutes, refresh with
  `POST https://auth.openai.com/oauth/token` (`client_id=app_EMoamEEZ73f0CkXaXp7hrann`). OpenAI rotates the
  refresh token on every use, so the new refresh token must be persisted or the next refresh fails.
- **The backend wants streaming.** It requires `stream: true` and an `instructions` field. In a stream,
  `response.completed` carries an empty `output[]`; the output items arrive as `response.output_item.done`
  events. Usage sits at `response.usage` inside the `response.completed` event, not at the top level.
- **Failures arrive as HTTP 200.** A rejected request (for example `context_length_exceeded`) answers 200
  and delivers `event: error` (top-level `error` with `type`, `code`, `message`, `param`) followed by
  `event: response.failed` (`response.error` with `code` and `message`, the final verdict).
- **Context limits** seen on the Codex backend: `gpt-5.4` 1M, `gpt-5.4-mini` 400K, `gpt-5.3-codex` 400K,
  `gpt-5.3-codex-spark` 128K (Pro only), `gpt-5.2` 400K. `gpt-5.5` is gated at 272,000 prompt tokens there
  (last accepted 271,767; first refused within the next 128 tokens), whatever the standard API publishes.

## GitHub Copilot

- Hosts: `api.githubcopilot.com` (individual), `api.business.githubcopilot.com`,
  `api.enterprise.githubcopilot.com`. Paths have **no `/v1` prefix** (`/chat/completions`, `/models`,
  `/embeddings`).
- **Two ways a stored `gho_…` token authorizes** (the token itself never rotates):
  - *Minted* (classic editor OAuth, client id `Iv1.b507a08c87ecfe98`): `GET
    https://api.github.com/copilot_internal/v2/token` with `authorization: token <gho>` and editor headers
    returns 200 with a short-lived (about 30 minutes) Copilot bearer `{token, expires_at}`.
  - *Direct* (the GitHub Copilot CLI and Copilot Enterprise): the same mint answers **404**, and the `gho_`
    token is used directly as the chat bearer (verified 2026-06-13). `GET copilot_internal/user` (200 with
    `copilot_plan`) confirms the seat. Only a 404 means "use the token directly"; 401, 403 and 5xx mean a
    bad or revoked token. The default host accepts the direct bearer for `/models` and `/chat/completions`.
- **Editor headers gate access**: `copilot-integration-id`, `editor-version: vscode/…`,
  `editor-plugin-version: copilot-chat/…`, `user-agent: GitHubCopilotChat/…`, `openai-intent:
  conversation-panel`, `x-github-api-version: 2025-04-01`, `x-vscode-user-agent-library-version:
  electron-fetch` (values pinned at VS Code 1.123.0 / copilot-chat 0.43.0 in 2026-06).
- **`copilot-integration-id` selects the model set**, not just access. On a Copilot Enterprise seat
  (2026-06-13) `vscode-chat` resolved 8 models (the gpt-4o family) and refused every Claude, Gemini and
  gpt-5 request with 400 `model_not_supported`; `copilot-developer-cli` resolved 34, including
  `claude-opus-4.8`, `claude-sonnet-4.6`, `gemini-2.5-pro`, `gemini-3.5-flash`, `gpt-5.5` and `gpt-5.4`.
  Check a seat with `GET /models` and that header; org policy can still disable models per seat.
- **Usage is OpenAI-shaped for every model family**, Claude and Gemini included (input, cached subset,
  output; no Anthropic cache-write buckets). Copilot meters premium requests, not tokens.
- **Claude and Gemini streams omit `object`.** Their chunks carry `choices` / `delta` / `usage` /
  `[DONE]` without `"object": "chat.completion.chunk"`.
- **Rollout is per seat.** A model can be generally available yet absent from a seat's `/models`: Claude
  Sonnet 5 went GA on Copilot on 2026-06-30 with a gradual rollout, and two days later an
  under-provisioned seat still answered 400 `model_not_supported` for `claude-sonnet-5` (and for every
  spelling tried) while provisioned seats served it. Distinguish this from a global removal. Fable 5 on
  Copilot is a reasoning model: its responses carry `reasoning_text` / `reasoning_opaque`, and a small
  `max_tokens` (for example 50) yields empty `choices` while `completion_tokens` is still billed.
