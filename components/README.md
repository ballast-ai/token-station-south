# Component packages

Each directory here is one official component package: a `wasm32-wasip2` guest crate plus the
`manifest.json` that describes it. A release archives it as `<name>-<tag>.tar.gz` holding
`manifest.json` beside `component.wasm`, and the host loads that directory shape unchanged.

This file explains how the directories are named. For each package's family, build script, status
and release history, read the **Component packages** table in the [root README](../README.md#component-packages).

## Reading a package name

A name is `<prefix>-<upstream>[-<variant>]`.

The directory name, the manifest `name`, and the release archive name are the same string. That
string is the package's identity (it keys the release index and the digest-stability check), so a
rename is a breaking change for every host that pins or lists packages by name. Treat the names
below as fixed.

### The prefix is the WIT world

The prefix tells you which WIT world the package implements, which is the ABI the runtime
calls and the conformance suite that judges it. It does not tell you which upstream vendor the
package talks to.

| Prefix | World (`api_version`) | WIT package | Suite | What the upstream exchange looks like |
| --- | --- | --- | --- | --- |
| `provider-` | `provider-adapter-v2` | `token-station:adapter@2.0.0` | `south.provider-component.v1` | Chat. One request, one response or one stream (`chat`, `stream`, `tool_call`). Imports `host.sign`. |
| `embeddings-` | `embeddings-adapter-v1` | `token-station:embeddings-adapter@1.0.0` | `south.embeddings-component.v1` | Embeddings. One request, one response. |
| `image-` | `image-adapter-v1` | `token-station:image-adapter@1.0.0` | `south.image-component.v1` | Image generation and edit that the upstream answers in the same HTTP exchange. |
| `speech-` | `speech-adapter-v1` | `token-station:speech-adapter@1.0.0` | `south.speech-component.v1` | Speech synthesis and recognition, answered in the same HTTP exchange. |
| `task-` | `task-adapter-v1` or `task-adapter-v2` | `token-station:task-adapter@1.0.0` or `@2.0.0` | `south.task-component.v1` or `.v2` | Asynchronous jobs: submit, then observe until a terminal state, then render the artifacts. |

Two of these prefixes are not named like the others, and that is the main source of confusion:

- **`provider-` means chat.** "Provider" is also the word this repository uses for every upstream
  (`ProviderConfig`, provider families), and every package here adapts a provider. The prefix names
  the chat world, and does not mean the other packages are not providers.
- **`task-` is named for the execution model, not for what is produced.** The other prefixes name
  the output (embeddings, image, speech). `task-` names how the exchange runs. So the same
  output can appear under two prefixes, and some outputs appear only under `task-`:
  - Image generation is `image-*` when the upstream answers synchronously, and
    `task-wan-image-v2` and `task-gmi-image-v2` when it is a submit-and-poll job.
  - Video generation is only available as `task-*` (`task-kling-v2`, `task-veo-v2`,
    `task-byteplus-v2`, `task-xai-v2`, `task-gemini-omni-v2`, and so on). There is no `video-`
    prefix.

  This is not a naming slip. A synchronous exchange and a polled job need different exports
  (`prepare` / `parse-response` against `submit` / `observe` / `render`), so they are different
  worlds, and an asynchronous image job cannot be an `image-adapter-v1` package.

### The rest of the name

The segment after the prefix is not governed by one rule. It is whatever identifies the upstream best
for that package, and it falls into these groups:

| Kind | Examples |
| --- | --- |
| Vendor | `provider-anthropic`, `provider-gemini`, `task-kling-v2`, `speech-elevenlabs` |
| Wire protocol shared by several vendors | `provider-openai-compatible`, `provider-openai-responses`, `provider-bedrock-converse` |
| Cloud platform | `provider-vertex`, `image-vertex`, `speech-vertex`, `image-azure`, `speech-azure` |
| Product | `provider-kiro` |
| Vendor on a platform, through one API | `provider-anthropic-bedrock-invoke` |

Optional suffixes:

- `-v2` appears only on `task-` packages and is the task world generation, not the package version.
  `task-kling` implements `task-adapter-v1`; `task-kling-v2` implements `task-adapter-v2`.
  Other worlds carry their generation in the WIT package and `api_version`, not in the directory name.
- `-bearer` marks an authentication-arm variant of a sibling package
  (`provider-bedrock-converse-bearer` beside `provider-bedrock-converse`).
- An image or video word before `-v2` (`task-wan-image-v2`, `task-gmi-image-v2`) names the output of
  a `task-` package, since the prefix does not.

## Finding a package

- **By output**: `ls components | grep '^image-'`, and add the `task-` packages whose family names
  say `image` or `video`. The families are listed in the root README table.
- **By vendor**: the vendor axis crosses the worlds. For example `vertex` appears as
  `provider-vertex`, `embeddings-vertex`, `image-vertex`, `speech-vertex`, and the Vertex families of
  `task-veo-v2` and `task-gemini-omni-v2`; `openai-compatible` appears under four prefixes.
  Search the **Family** column of the root README table, because one package can serve several
  families (`image-openai-compatible` serves `azure-openai-v1` and `azure-openai-dated`) and a
  family can exist in more than one world.
- **By ABI**: read `api_version` in the package's `manifest.json`. That field, not the directory
  prefix, is what the runtime and `scripts/release_index.py` act on.

## Adding a package

1. Pick the world from what the upstream exchange does, using the table above, and use that world's
   prefix. If the upstream needs a poll loop, it is a `task-` package even when it produces images.
2. Name the upstream the way the closest sibling in the same world does.
3. Add `scripts/build-<name>-component.sh`, a manifest whose `name` equals the directory name, a
   fixture pack, and the row in the root README table. Then register the package where it is
   listed by hand, because `shipped_packages_v1` fails when any of these disagree:
   - `OFFICIAL_COMPONENTS` in `crates/south-component-conformance/tests/shipped_packages_v1.rs`;
   - the build script list and the `package <name> <wasm>` line in `.github/workflows/release.yml`.

   `scripts/prebuild-components.sh` and the CI build loop pick the new script up by globbing
   `scripts/build-*-component.sh`, so they need no edit.
4. Read the world's design record and `docs/design/2026-09-30-host-zero-vendor-boundary.md`
   before writing the manifest.

Build script names do not mirror directory names exactly (`task-kling-v2` is built by
`build-kling-task-v2-component.sh`). The root README table lists the script for each package.
