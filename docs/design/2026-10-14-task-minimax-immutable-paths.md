# `task-minimax-v2` declares immutable body paths

Status: accepted (ruled by lv on 2026-10-14, host plan H5-5c); released in 0.64.0 (`task-minimax-v2` 0.33.0).

Date: 2026-10-14

Baseline: south `origin/main` = `aad018d` (after release 0.63.0 and speech step S-S-1). Host references carry a
`server:` prefix and refer to token-station-server as read on 2026-10-14. Statements marked **unverified** are
conclusions from reading code, not from running anything.

## 1. Problem

Task contract 6 (record `2026-09-27-task-contract-v6-facts.md` §4) gives `PreparedTaskV2` the field
`immutable_body_paths`: `null` means the component takes no position and a host must inject no extra request-body field
at all; a list names the body paths a host must leave untouched (with their ancestors and descendants) while it injects
everything else.

`task-minimax-v2` has prepared `immutable_body_paths: null` for both MiniMax generations since contract 6. The server
host follows the contract: on the generic component task path it injects none of a model's operator request extras
for this package and logs a warning (`server:gateway/src/modules/inference/handler/video/durable.rs`, the P13 C10
section). Its native MiniMax arm instead injects every extra except the fields of its protected list
`MINIMAX_VIDEO_OWNED` (`model`, `duration`, `resolution`, `content`, `first_frame_image`, `last_frame_image`), applied
to both the Hailuo v1 and the H3 body (`server:durable.rs` `MINIMAX_VIDEO_OWNED`, `apply_body`). The host's H5-5c
cutover record found the gap (`server:docs/product-review-v2/plans/2026-10-13-H5-*.md` §9.10 and the H5-5c cutover
handbook): once a MiniMax video model moves to a `component_task` route, extras such as `prompt_optimizer`,
`fast_pretreatment`, `aigc_watermark` or H3 `ratio` silently stop reaching the upstream. Money is unaffected; the
capability is lost. The host pins the current behaviour with
`server:unruled_minimax_tier_component_route_drops_request_extras`.

`task-kling-v2` had the same gap and closed it in 0.32.0 (#111) by declaring the six fields the native Kling arm
protects. This record does the same for MiniMax.

## 2. Ruling

lv, 2026-10-14 (H5-5c): `task-minimax-v2` declares immutable body paths matching the native arm's protection, so
operator extras keep working after cutover and still cannot overwrite gateway-owned fields.

## 3. The declared paths

The component writes two different bodies, and both declare the native arm's six fields, in its order:
`model`, `duration`, `resolution`, `content`, `first_frame_image`, `last_frame_image`.

| Generation | Endpoint | Declared | Declared but not in that body |
|---|---|---|---|
| Hailuo v1 (`MiniMax-Hailuo-02`, `-2.3`, `-2.3-Fast`) | `v1/video_generation` | all six | `content` |
| H3 (`MiniMax-H3`, `MiniMax-H3-Max`) | `v2/video_generation` | all six | `first_frame_image`, `last_frame_image` |

- `duration` and `resolution` are the fields the request estimate is read from; `first_frame_image` and
  `last_frame_image` (Hailuo) and `content` (H3, which carries the prompt and every input image with its role) set the
  estimate's input image count. An extra that rewrote any of them would decouple the reservation from what is sent,
  which is what contract 6 forbids.
- A declared path the body does not carry is legal under contract 6 (the host leaves the path untouched, so an extra
  cannot add it). Declaring the same table for both generations gives exact parity with the native arm, which applies
  one list to both bodies: an extra is skipped or injected on the component path exactly when the native arm skips or
  injects it. The Hailuo frame fields are absent from a request without frames and stay declared for the same reason.
- Fields the native arm leaves open stay open: Hailuo `prompt`, `prompt_optimizer`, `fast_pretreatment`, and H3
  `ratio`. An extra naming them is injected on both arms.

## 4. Package identity

The prepared output changes, so the published identity retires: `task-minimax-v2` 0.32.0 (published with 0.63.0) and
the unpublished S-S-1 patch 0.32.1 give way to 0.33.0, a minor version because the behaviour changes (as `task-kling-v2`
0.31.0 → 0.32.0 did). The manifest, the component crate, its lockfile root and the reference `metadata()` move
together; `south_runtime` stays 0.63.0 (the poll interval still needs it) and `contracts.task` stays 7.

No Q47 bump: the reference lives in `south-component-conformance`, which already moved 0.62.0 → 0.63.0 since v0.63.0
(speech step S-S-1), and `scripts/check_crate_versions.py` compares with the previous release tag.

## 5. Tests

- The four prepare fixtures in `fixtures-minimax-task-v2` expect the declared lists (Hailuo `defaults`; H3 `h3-defaults`,
  `h3-frames`, `h3-alias-refs`); the public suite and the real-Wasm sandbox parity run them against the reference and
  the built component.
- `minimax_task_reference_v2.rs` `both_generations_declare_the_native_protected_fields`: for both generations the
  declared list equals the native arm's six and is valid under contract 6, `prompt` / `prompt_optimizer` /
  `fast_pretreatment` / `ratio` stay open, and a request without optional fields gets the same list.
- `shipped_packages_v1.rs`
  `minimax_immutable_paths_retire_the_published_task_minimax_v2_identity` retires 0.32.0 and 0.32.1.

## 6. Host follow-up

After the host re-pins to the release carrying 0.33.0, its generic component path injects MiniMax extras outside the
declared paths. `server:unruled_minimax_tier_component_route_drops_request_extras` then fails by design and is replaced
by a test of the new behaviour; the H5-5c handbook's note on silently dropped extras no longer applies.
