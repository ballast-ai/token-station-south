#!/usr/bin/env bash
set -euo pipefail

readonly FORBIDDEN_NAME_PATTERN='(^|-)(token-station|sqlx|rusqlite|diesel|sea-orm|redis|deadpool-redis|postgres|tokio-postgres|mysql|mysql-async|mongodb|surrealdb|clickhouse|cassandra|scylla|rocksdb|sled|redb|lmdb|heed|duckdb|memcache)(-|$)'
readonly FORBIDDEN_SOURCE_PATTERN='GlimpseEngine/(token-station|token-station-server)(\.git)?([?#]|$)'

check_metadata() {
  local candidate_file="$1"

  jq -e \
    --arg name_pattern "$FORBIDDEN_NAME_PATTERN" \
    --arg source_pattern "$FORBIDDEN_SOURCE_PATTERN" \
    --argjson reqwest_features '["__rustls", "__rustls-ring", "__tls", "rustls-tls", "rustls-tls-webpki-roots", "rustls-tls-webpki-roots-no-provider", "stream"]' \
    '[
      (
        .packages[]
        | select(.name | gsub("_"; "-") | test($name_pattern; "i"))
      ),
      (
        .packages[].dependencies[]
        | select(
          (.name | gsub("_"; "-") | test($name_pattern; "i"))
          or ((.rename // "") | gsub("_"; "-") | test($name_pattern; "i"))
          or ((.source // "") | gsub("_"; "-") | test($source_pattern; "i"))
          or ((.path // "") | gsub("_"; "-") | test("/(token-station|token-station-server)(/|$)"; "i"))
        )
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
        | .dependencies[]
        | select(.name == "reqwest")
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
        | select(
            .req != "=0.12.28"
            or .uses_default_features != false
            or ((.features // []) | sort) != ["rustls-tls", "stream"]
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
            | select(
                if $transport_count == 0 then
                  ($reqwest_packages | length) != 0
                else
                  $transport_count != 1
                  or ($reqwest_packages | length) != 1
                  or $reqwest_packages[0].version != "0.12.28"
                  or ([
                        $metadata.resolve.nodes[]
                        | select(.id == $reqwest_packages[0].id)
                      ] | length) != 1
                  or ([
                        $metadata.resolve.nodes[]
                        | select(.id == $reqwest_packages[0].id)
                        | .features[]
                      ] | sort) != $reqwest_features
                end
              )
          end
      )
    ] | length == 0' "$candidate_file" >/dev/null
}

if [[ "${1:-}" == "--self-test" ]]; then
  for fixture in tests/fixtures/boundary/forbidden-*.json; do
    if check_metadata "$fixture"; then
      echo "boundary self-test failed: forbidden fixture was accepted: $fixture" >&2
      exit 1
    fi
  done
  if ! check_metadata "tests/fixtures/boundary/allowed-metadata.json"; then
    echo "boundary self-test failed: allowed fixture was rejected" >&2
    exit 1
  fi
  echo "boundary self-test passed"
  exit 0
fi

generated_metadata_file="$(mktemp)"
readonly generated_metadata_file
trap 'rm -f "$generated_metadata_file"' EXIT

for manifest in Cargo.toml fuzz/Cargo.toml; do
  cargo metadata --format-version 1 --manifest-path "$manifest" >"$generated_metadata_file"
  if ! check_metadata "$generated_metadata_file"; then
    echo "boundary check failed for $manifest: a host, database, or cache dependency is present" >&2
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

echo "boundary check passed"
