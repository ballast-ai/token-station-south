# Frozen samples: GMI Cloud media images

Expectations are hand-transcribed from token-station-server's native GMI arm
(`handler/images/gmi.rs` — `build_gmi_submit_body`, `gmi_reference_images`,
`gmi_reference_image_limit`, `gmi_media_urls`, `gmi_media_result_to_openai`;
`images/durable.rs::gmi_run` for the `b64_json` refusal and the two receipt
shapes `parse_gmi_create` accepts; `images/observe.rs::normalize_gmi`; and the
blocking surface's `code: detail` failure wording). **Expectations are never
regenerated from the implementation under test.**

The organisation header: the native arm reads `X-Organization-ID` out of the
credential's `account_id`, whereas a component never sees the credential row.
Since 0.36.0 the package declares the credential field `organization_id` and
exports it, so it arrives in `provider_config.declared` (`*.organization-header.*`).
For one release the provider-config extension `organization_id`, which hosts sent
before, still stands in (`*.organization-header-extension.*`; SA-Q3 of
`docs/design/2026-10-18-section-attributes-and-task-values.md`).

GMI prices per request and reports no usage: `n` is still `requested_outputs`,
the number of delivered URLs is `usage.outputs`, and pricing policy stays with
the host. A receipt that already carries artifacts is `accepted-terminal`.
