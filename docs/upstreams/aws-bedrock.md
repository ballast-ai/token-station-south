# AWS Bedrock

Upstream facts for the Bedrock families: `bedrock` and `bedrock-bearer` (Converse,
[`provider-bedrock-converse`](../../components/provider-bedrock-converse/manifest.json) and
[`provider-bedrock-converse-bearer`](../../components/provider-bedrock-converse-bearer/manifest.json)),
`anthropic-bedrock-invoke` (InvokeModel,
[`provider-anthropic-bedrock-invoke`](../../components/provider-anthropic-bedrock-invoke/manifest.json)) and
`bedrock-mantle` ([`provider-openai-responses`](../../components/provider-openai-responses/manifest.json)).
Moved from the token-station-server host documentation (`gateway/README.md`) on 2026-11-04 and **not
re-checked** on that date; verify against AWS before relying on them for billing. InvokeModel and Mantle
request shapes are in the [InvokeModel record](../design/2026-10-08-bedrock-invoke-anthropic-component.md)
and the [Mantle record](../design/2026-10-15-bedrock-mantle.md).

- **Model ids.** Cross-region inference profiles (`global.anthropic.*`) are the recommended ids; the
  regional prefixes (`us.*`, `eu.*`) cost about 10% more. Check rates on the
  [Bedrock pricing page](https://aws.amazon.com/bedrock/pricing/).
- **Request size.** Bedrock rejects request bodies over 20 MB.
- **Not covered by the packages.** Provisioned throughput, batch-mode discounts and Guardrails.
