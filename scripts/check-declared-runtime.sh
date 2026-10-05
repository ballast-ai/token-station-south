#!/usr/bin/env bash
# Loads every component package under the south runtime it declares (host-zero-vendor-boundary
# §13.6, SF10).
#
#   scripts/check-declared-runtime.sh --build              # components/*/manifest.json and the
#                                                          # release wasm the build scripts produced
#   scripts/check-declared-runtime.sh --dist DIR --tag vX.Y.Z   # the archives a release staged
#
# A package's `compatibility.south_runtime` is the oldest south runtime it needs, not the release
# that carried it. This script holds each package to that: it groups the packages by the runtime
# they declare and, for each group, runs crates/south-provider-runtime/tests/declared_runtime_v1.rs
# inside a checkout of that runtime's release tag, so the loader judging the package is the one
# the package claims to need. A group declaring the version being built uses this tree.
#
# Building an older runtime costs one more compile of the runtime crate and wasmtime per distinct
# older version; CARGO_TARGET_DIR defaults to this tree's target directory so the registry
# dependencies compile once.
#
# Requires git, python3, cargo and the wasm-free runtime toolchain only: the packages are already
# built. Fetches a missing release tag from `origin`.
set -euo pipefail
cd "$(dirname "$0")/.."
repo="$(pwd -P)"

usage() {
  sed -n '2,9p' "$0" >&2
  exit 2
}

mode=""
dist=""
tag=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --build) mode=build ;;
    --dist) mode=dist; dist="${2:-}"; shift ;;
    --tag) tag="${2:-}"; shift ;;
    *) usage ;;
  esac
  shift
done
[ -n "${mode}" ] || usage
if [ "${mode}" = dist ] && { [ -z "${dist}" ] || [ -z "${tag}" ]; }; then
  usage
fi

work="$(mktemp -d)"
trees=()
cleanup() {
  for tree in "${trees[@]+"${trees[@]}"}"; do
    git -C "${repo}" worktree remove --force "${tree}" >/dev/null 2>&1 || true
  done
  rm -rf "${work}"
}
trap cleanup EXIT

# Stage every package as <name>/manifest.json + <name>/component.wasm.
staged="${work}/staged"
mkdir -p "${staged}"
for manifest in components/*/manifest.json; do
  package="$(basename "$(dirname "${manifest}")")"
  mkdir -p "${staged}/${package}"
  if [ "${mode}" = build ]; then
    artifact="components/${package}/target/wasm32-wasip2/release/${package//-/_}.wasm"
    if [ ! -f "${artifact}" ]; then
      echo "${package}: ${artifact} is missing; run the component build scripts first" >&2
      exit 1
    fi
    cp "${manifest}" "${staged}/${package}/manifest.json"
    cp "${artifact}" "${staged}/${package}/component.wasm"
  else
    archive="${dist}/${package}-${tag}.tar.gz"
    if [ ! -f "${archive}" ]; then
      echo "${package}: ${archive} is missing" >&2
      exit 1
    fi
    tar -xzf "${archive}" -C "${staged}/${package}" manifest.json component.wasm
  fi
done

workspace="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml","rb"))["workspace"]["package"]["version"])')"
listing="${work}/declared.txt"
python3 scripts/release_index.py declared-runtimes --root "${staged}" > "${listing}"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${repo}/target}"
failed=0
for runtime in $(cut -d' ' -f1 "${listing}" | uniq); do
  group="${work}/runtime-${runtime}"
  mkdir -p "${group}"
  while read -r declared package; do
    if [ "${declared}" = "${runtime}" ]; then
      cp -R "${staged}/${package}" "${group}/${package}"
    fi
  done < "${listing}"

  if [ "${runtime}" = "${workspace}" ]; then
    tree="${repo}"
    echo "== south runtime ${runtime} (this tree): $(ls "${group}" | tr '\n' ' ')"
  else
    release="v${runtime}"
    if ! git -C "${repo}" rev-parse -q --verify "refs/tags/${release}^{commit}" >/dev/null; then
      git -C "${repo}" fetch --no-tags --depth 1 origin "refs/tags/${release}:refs/tags/${release}"
    fi
    tree="${work}/tree-${runtime}"
    git -C "${repo}" worktree add --detach "${tree}" "${release}" >/dev/null
    trees+=("${tree}")
    cp crates/south-provider-runtime/tests/declared_runtime_v1.rs \
      "${tree}/crates/south-provider-runtime/tests/declared_runtime_v1.rs"
    echo "== south runtime ${runtime} (${release}): $(ls "${group}" | tr '\n' ' ')"
  fi

  # From inside the tree, so its own rust-toolchain.toml picks the toolchain.
  if ! (cd "${tree}" && SOUTH_DECLARED_RUNTIME_PACKAGES="${group}" cargo test \
    -p south-provider-runtime --test declared_runtime_v1 \
    -- --ignored --exact every_package_loads_under_the_runtime_it_declares); then
    failed=1
  fi
done

if [ "${failed}" -ne 0 ]; then
  echo "a package is refused by the south runtime it declares as its minimum" >&2
  exit 1
fi
echo "every package loads under the south runtime it declares"
