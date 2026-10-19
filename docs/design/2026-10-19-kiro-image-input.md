# Kiro image input: `provider-kiro` 1.3.0 takes images in user turns

Status: accepted 2026-10-19 (lv). Implemented on branch `kiro-images`.

Date: 2026-10-19

Predecessor: [the Kiro provider component](2026-09-30-kiro-provider-component.md) ("the Kiro record"); this record
revises its refusal KR1 and the `ContentPart::Unknown` row of its §4.2.

## 1. Why

The Kiro record's version 1 refuses every image in a user or assistant turn (KR1), as the server host's native arm
did when the record was written. On 2026-10-16 the native arm in the server's production deploy branch learned to send
images (server commit `b88383cb`, "support Kiro image inputs across API surfaces"). The server's development branch has
since deleted that arm: Kiro is served only by this package. Deploying it would take away an ability production
already has. lv ruled on 2026-10-19 that this package takes images the way that arm sends them, and that the host
prices an image it estimates by the image's size rather than by its base64 length (a host change, in the host's own
records).

## 2. What the package sends

The upstream's user input (`userInputMessage`, in `currentMessage` and in each user `history` entry) carries an
`images` array of `{"format": <format>, "source": {"bytes": <base64>}}` entries, after `content`. The component reads
an image as the native arm does:

| Input | Sent |
|---|---|
| An IR `image_url` part in a user turn, `data:<type>;base64,<bytes>` with a type of `image/png`, `image/jpeg`, `image/gif` or `image/webp` (case-insensitive) | One `images` entry, `format` `png` / `jpeg` / `gif` / `webp`, the bytes verbatim (never decoded) |
| An unmodelled `image` block in a user turn with a `base64` source (`media_type`, `data`) | The same |
| An `image` block inside an unmodelled `tool_result` block in a user turn | Promoted to that turn's `images`: the upstream's tool result has no image slot (the native arm promotes it too) |
| Several images in one turn | In order |

The text, tool results and everything else in the turn are unchanged. A turn without images sends no `images` key, so
every request without an image is byte-for-byte what 1.2.0 sends.

## 3. What stays refused or dropped

- **An image in an assistant turn** (an `image_url` part or an unmodelled `image` block, including a final assistant
  turn sent as the current message): capability error 400, "this upstream takes images in user turns only; an
  assistant turn carries one". The upstream's assistant entry has no image slot; the native arm refuses it too.
- **A remote image** (an `http(s)` URL, or an `image` block whose source is not `base64`): capability error 400.
  Inline bytes only, as the native arm and the Converse reference.
- **Another media type** (for example `image/bmp`): capability error 400 naming the type.
- **Empty or malformed base64 bytes**: `invalid_request` 400. The component checks the alphabet and the length; it does
  not decode.
- **An image in a system message**: dropped, the rest sent (unchanged from the Kiro record's revision of 2026-10-14).
- **An image in an IR tool message** (`role: tool`): dropped (unchanged). The north codec keeps only the text of a
  tool result, so no image reaches this part from a client's tool result; promoting one would need a codec change for
  every family and is not part of this record. Registered as a known difference from the native arm, which promoted
  images from Anthropic `tool_result` blocks.

KR1 is revised accordingly in the Kiro record (§18) and in its §4.2 row.

## 4. Package identities

`south-component-conformance` changes (0.73.0 → 0.74.0), and every guest links it, so every identity published with
0.74.0 retires (`kiro_images_retires_every_published_074_package_identity`). `provider-kiro` takes a minor
(1.2.0 → 1.3.0); every other package takes the Q47 patch. No package raises its `south_runtime` or its `contracts`: the
behaviour uses no manifest field or runtime meaning a 0.74.0 runtime lacks. The manifest's `capabilities` are
unchanged: no provider package declares image input, and a host reads the parts per component.

## 5. Fixtures

`fixtures-kiro`:
- revised to success: `image-in-the-current-turn` (was `refused-image`), `image-in-history` (was
  `refused-image-in-history`), `unmodelled-image-block-is-an-image` (was `refused-unmodelled-image-block`);
  `unmodelled-tool-result-block-is-a-tool-result` now carries a `base64` image in its `tool_result` content, promoted
  to the turn (its earlier source-less `image` block would now be refused as a remote image);
- new: `image-formats-map-to-the-upstream-names` (`image/JPEG`, `image/gif`, `image/webp`),
  `refused-image-in-an-assistant-turn`, `refused-remote-image`, `refused-unsupported-image-format`,
  `refused-image-without-base64-bytes`;
- `refused-unmodelled-image-block-in-an-assistant-turn` keeps its input and takes the new message.

Expected bodies were written from the native arm's shape (server `b88383cb`, `content_images`) and checked by gate ②
against the reference.

## 6. Host follow-up

- Re-pin the release that carries `provider-kiro` 1.3.0.
- The server's Kiro dual-run leg compares the component with a frozen recording of the deleted native arm, which
  refused images; an image row there is a registered intended difference (source: server `b88383cb`).
- The host's input estimate for a package declaring `usage_evidence: absent` counts an inline image by its size
  (server, 2026-10-19), so an image is not billed as hundreds of thousands of text tokens.

Unverified: the upstream's behaviour with images (nothing was called against an upstream; the native arm's shape is
the evidence).
