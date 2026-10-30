# Reve (images)

Upstream facts for [`image-reve`](../../components/image-reve/manifest.json). Moved from the
token-station-server host documentation on 2026-10-30. The [image I3-6 record](../design/2026-10-13-image-i3-6-reve.md)
is authoritative for package behaviour.

- **Billing meter.** Each response reports `credits_used` in the JSON body, or in the `X-Reve-Credits-Used`
  header when the image is returned as binary. The meter already includes the `test_time_scaling` and
  `postprocessing` surcharges.
- **Content violations are charged.** A request refused for content still answers 200 and consumes credits.
