#!/usr/bin/env bash
set -euo pipefail

readonly FORBIDDEN_NAME_PATTERN='(^|-)(token-station|sqlx|rusqlite|diesel|sea-orm|redis|deadpool-redis|postgres|tokio-postgres|mysql|mysql-async|mongodb|surrealdb|clickhouse|cassandra|scylla|rocksdb|sled|redb|lmdb|heed|duckdb|memcache)(-|$)'
readonly FORBIDDEN_SOURCE_PATTERN='GlimpseEngine/(token-station|token-station-server)(\.git)?([?#]|$)'
# The sanctioned typed-IR edges (S0 invariant 6). `token-station-protocol` comes
# from the kernel distribution mirror, and only the crates named here may take
# it: conformance gate 2, and the north-bound codec whose whole job is mapping
# client wire formats onto that IR. Nothing else in this workspace may — in
# particular not `south-core`, `south-contracts`, the transport or the runtime,
# which must stay free of IR types both directly and transitively.
readonly KERNEL_SOURCE_PATTERN='ballast-ai/token-station-kernel(\.git)?([?#]|$)'
readonly KERNEL_IR_PACKAGE='token-station-protocol'
readonly KERNEL_IR_CONSUMERS='["south-component-conformance", "south-north-codec"]'
# Depending on the codec is how a crate would acquire the IR without naming it,
# so the allowlist above governs that edge too.
readonly CODEC_PACKAGE='south-north-codec'

# The feature set is **policy, not a moving target**: south takes rustls +
# stream and nothing else, so these two lists stay written down here. The
# *version* is the opposite — it moves every time upstream releases — which is
# why it is read from Cargo.toml (see `pinned_reqwest_req`) instead of being
# spelled out a second time in this file.
readonly REQWEST_DECLARED_FEATURES='["rustls", "stream"]'
readonly REQWEST_RESOLVED_FEATURES='["__rustls", "__rustls-aws-lc-rs", "__tls", "rustls", "stream"]'

# Fixtures are frozen inputs: each one encodes the *shape* of a violation, not
# today's pin. They stay on =0.13.4 after the workspace bumps reqwest, so the
# self-test passes its own expectation rather than the live one.
readonly FIXTURE_REQWEST_REQ='=0.13.4'

# Violations found by the last `check_metadata` call, one per line. The caller
# decides whether to print them: the self-test expects failures and stays
# quiet, the live check prints them because "something is wrong" without
# saying what costs the next reader a full investigation.
LAST_VIOLATIONS=''

check_metadata() {
  local candidate_file="$1"
  local require_resolved_graph="${2:-false}"
  local expected_req="${3:-$FIXTURE_REQWEST_REQ}"
  local expected_version="${expected_req#=}"

  LAST_VIOLATIONS="$(
    jq -r \
      --arg name_pattern "$FORBIDDEN_NAME_PATTERN" \
      --arg source_pattern "$FORBIDDEN_SOURCE_PATTERN" \
      --arg kernel_source_pattern "$KERNEL_SOURCE_PATTERN" \
      --arg kernel_ir_package "$KERNEL_IR_PACKAGE" \
      --argjson kernel_ir_consumers "$KERNEL_IR_CONSUMERS" \
      --arg codec_package "$CODEC_PACKAGE" \
      --arg expected_req "$expected_req" \
      --arg expected_version "$expected_version" \
      --argjson declared_features "$REQWEST_DECLARED_FEATURES" \
      --argjson reqwest_features "$REQWEST_RESOLVED_FEATURES" \
      --argjson require_resolved_graph "$require_resolved_graph" \
      '[
        (
          select(
            $require_resolved_graph
            and (
              (.workspace_members | type) != "array"
              or .resolve == null
              or (.resolve.nodes | type) != "array"
            )
          )
          | "metadata-incomplete: workspace_members or resolve graph missing (cargo metadata must run with dependencies resolved)"
        ),
        (
          .packages[]
          | select(.name | gsub("_"; "-") | test($name_pattern; "i"))
          | select(
              (
                .name == $kernel_ir_package
                and ((.source // "") | test($kernel_source_pattern; "i"))
              )
              | not
            )
          | "forbidden-package: \(.name) \(.version) (host, database or cache crate in the graph)"
        ),
        (
          .packages[] as $package
          | $package.dependencies[]
          | select(
            (.name | gsub("_"; "-") | test($name_pattern; "i"))
            or ((.rename // "") | gsub("_"; "-") | test($name_pattern; "i"))
            or ((.source // "") | gsub("_"; "-") | test($source_pattern; "i"))
            or ((.path // "") | gsub("_"; "-") | test("/(token-station|token-station-server)(/|$)"; "i"))
          )
          | select(
              (
                ($kernel_ir_consumers | index($package.name)) != null
                and .name == $kernel_ir_package
                and ((.source // "") | test($kernel_source_pattern; "i"))
              )
              | not
            )
          | "forbidden-dependency: \($package.name) -> \(.name) (source=\(.source // .path // "local"))"
        ),
        (
          (.workspace_members // []) as $workspace_members
          | .packages[]
          | . as $package
          | select(
              (
                if ($workspace_members | length) > 0 then
                  $workspace_members | index($package.id) != null
                else
                  $package.name | startswith("south-")
                end
              )
              and ($kernel_ir_consumers | index($package.name)) == null
            )
          | $package.dependencies[]
          | select(
              (.name | gsub("_"; "-")) == $codec_package
              or ((.rename // "") | gsub("_"; "-")) == $codec_package
            )
          | "codec-dependency: \($package.name) -> \($codec_package) (the typed IR would leak transitively)"
        ),
        (
          (.workspace_members // []) as $workspace_members
          | .packages[]
          | select(
              (
                if ($workspace_members | length) > 0 then
                  .id as $package_id | $workspace_members | index($package_id) != null
                else
                  .name | startswith("south-")
                end
              )
              and .name != "south-transport-reqwest"
            )
          | . as $package
          | .dependencies[]
          | select(.name == "reqwest")
          | "reqwest-outside-transport: \($package.name) declares reqwest; only south-transport-reqwest may own the transport"
        ),
        (
          (.workspace_members // []) as $workspace_members
          | .packages[]
          | select(
              (
                if ($workspace_members | length) > 0 then
                  .id as $package_id | $workspace_members | index($package_id) != null
                else
                  .name | startswith("south-")
                end
              )
              and .name == "south-transport-reqwest"
            )
          | select(([.dependencies[] | select(.name == "reqwest")] | length) != 1)
          | "transport-reqwest-count: south-transport-reqwest declares \([.dependencies[] | select(.name == "reqwest")] | length) reqwest dependencies, expected exactly 1"
        ),
        (
          (.workspace_members // []) as $workspace_members
          | .packages[]
          | select(
              (
                if ($workspace_members | length) > 0 then
                  .id as $package_id | $workspace_members | index($package_id) != null
                else
                  .name | startswith("south-")
                end
              )
              and .name == "south-transport-reqwest"
            )
          | .dependencies[]
          | select(.name == "reqwest")
          | (
              (
                select(.req != $expected_req)
                | "transport-reqwest-req: declared \(.req), Cargo.toml pins \($expected_req)"
              ),
              (
                select(.uses_default_features != false)
                | "transport-reqwest-default-features: default features must stay off"
              ),
              (
                select(((.features // []) | sort) != ($declared_features | sort))
                | "transport-reqwest-features: declared \((.features // []) | sort | join(",")), expected \($declared_features | sort | join(","))"
              )
            )
        ),
        (
          . as $metadata
          | if .resolve == null then
              empty
            else
              (.workspace_members // []) as $workspace_members
              | ([
                  .packages[]
                  | select(
                      .name == "south-transport-reqwest"
                      and (
                        if ($workspace_members | length) > 0 then
                          .id as $package_id | $workspace_members | index($package_id) != null
                        else
                          true
                        end
                      )
                    )
                ] | length) as $transport_count
              | [.packages[] | select(.name == "reqwest")] as $reqwest_packages
              | if $transport_count == 0 then
                  (
                    select(($reqwest_packages | length) != 0)
                    | "reqwest-without-transport: reqwest is in the graph but south-transport-reqwest is not"
                  )
                else
                  (
                    (
                      select($transport_count != 1)
                      | "transport-crate-count: \($transport_count) copies of south-transport-reqwest, expected exactly 1"
                    ),
                    (
                      select(($reqwest_packages | length) != 1)
                      | "reqwest-package-count: \($reqwest_packages | length) reqwest versions in the graph, expected exactly 1"
                    ),
                    (
                      select(
                        ($reqwest_packages | length) == 1
                        and $reqwest_packages[0].version != $expected_version
                      )
                      | "reqwest-version: resolved \($reqwest_packages[0].version), Cargo.toml pins \($expected_version)"
                    ),
                    (
                      select(
                        ($reqwest_packages | length) == 1
                        and ([
                              $metadata.resolve.nodes[]
                              | select(.id == $reqwest_packages[0].id)
                            ] | length) != 1
                      )
                      | "reqwest-resolve-nodes: expected exactly one resolve node for \($reqwest_packages[0].id)"
                    ),
                    (
                      select(
                        ($reqwest_packages | length) == 1
                        and ([
                              $metadata.resolve.nodes[]
                              | select(.id == $reqwest_packages[0].id)
                              | .features[]
                            ] | sort) != $reqwest_features
                      )
                      | "reqwest-resolved-features: \([$metadata.resolve.nodes[] | select(.id == $reqwest_packages[0].id) | .features[]] | sort | join(",")), expected \($reqwest_features | join(","))"
                    )
                  )
                end
            end
        )
      ] | .[]' "$candidate_file"
  )"

  [[ -z "$LAST_VIOLATIONS" ]]
}

# The pinned requirement lives in exactly one place — the workspace manifest —
# and this script reads it from there. Spelling the version out here as well
# used to make every upstream bump red for a reason the message never
# mentioned: dependabot can change Cargo.toml but not this file, so
# `=0.13.4 -> =0.13.5` failed the boundary gate with "a host, database, or
# cache dependency is present" and nothing else. The invariant being enforced
# is "exactly one reqwest, exact-pinned, owned only by south-transport-reqwest"
# — which does not require this file to know *which* version that is.
pinned_reqwest_req() {
  local manifest="$1"
  local req
  req="$(
    sed -n 's/^[[:space:]]*reqwest[[:space:]]*=[[:space:]]*{.*version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
      "$manifest" | head -n 1
  )"
  if [[ ! "$req" =~ ^=[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "boundary check failed: reqwest must stay exact-pinned in $manifest (found: ${req:-no reqwest dependency})" >&2
    return 1
  fi
  printf '%s' "$req"
}

print_violations() {
  local line
  while IFS= read -r line; do
    [[ -n "$line" ]] && echo "  - $line" >&2
  done <<<"$LAST_VIOLATIONS"
}

if [[ "${1:-}" == "--self-test" ]]; then
  for fixture in tests/fixtures/boundary/forbidden-*.json; do
    if check_metadata "$fixture"; then
      echo "boundary self-test failed: forbidden fixture was accepted: $fixture" >&2
      exit 1
    fi
    # A rejection that cannot say what it rejected is the defect this gate had:
    # every failure named "a host, database, or cache dependency" regardless of
    # which rule actually fired.
    if [[ -z "$LAST_VIOLATIONS" ]]; then
      echo "boundary self-test failed: rejection carried no reason: $fixture" >&2
      exit 1
    fi
  done
  for fixture in tests/fixtures/boundary/allowed-*.json; do
    if ! check_metadata "$fixture"; then
      echo "boundary self-test failed: allowed fixture was rejected: $fixture" >&2
      print_violations
      exit 1
    fi
  done
  if check_metadata "tests/fixtures/boundary/incomplete-live-metadata.json" true; then
    echo "boundary self-test failed: incomplete live metadata was accepted" >&2
    exit 1
  fi
  echo "boundary self-test passed"
  exit 0
fi

expected_reqwest_req="$(pinned_reqwest_req Cargo.toml)"
readonly expected_reqwest_req

generated_metadata_file="$(mktemp)"
readonly generated_metadata_file
trap 'rm -f "$generated_metadata_file"' EXIT

for manifest in Cargo.toml fuzz/Cargo.toml; do
  cargo metadata --format-version 1 --all-features --manifest-path "$manifest" >"$generated_metadata_file"
  if ! check_metadata "$generated_metadata_file" true "$expected_reqwest_req"; then
    echo "boundary check failed for $manifest:" >&2
    print_violations
    exit 1
  fi
done

if find . \
  -path './.git' -prune -o \
  -path './target' -prune -o \
  -type d -name migrations -print \
  | grep -q .; then
  echo "boundary check failed: migration directories are host-owned" >&2
  exit 1
fi

echo "boundary check passed (reqwest pinned $expected_reqwest_req)"
