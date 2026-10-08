#!/usr/bin/env python3
"""The machine-readable release index (host-zero-vendor-boundary §9) and the digest-stability check (§8.6).

Four subcommands, all used by .github/workflows/release.yml:

  generate           Reads the staged archives (and gate 2 reports) and catalogs in a dist directory,
                     cross-checks them against components/*/manifest.json and catalogs/*.json, and
                     writes south-release-index.json.
  previous-tag       Reads release tag names on stdin and prints the latest one earlier than --current.
  compare            Fails when a package keeps its version between two indexes but its component.wasm
                     or its manifest.json changed: the version must bump (§8.6, §13.6). Also fails
                     when a catalog schema the previous index listed is gone and not retired (§13.11).
  declared-runtimes  Prints `<south_runtime> <package>` for every staged package, so
                     scripts/check-declared-runtime.sh can load each one under the runtime it declares
                     (§13.6). Refuses a declaration the check could not honour.

Standard library only: the workspace is library-only and must not gain a binary for its release tooling.
The index is generated from the manifests inside the archives; nothing in it is written by hand.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tarfile
import tomllib
from pathlib import Path

INDEX_SCHEMA = "south.release-index.v1"
INDEX_FILE = "south-release-index.json"
GATE2_SCHEMA = "south.gate2-report.v1"
ARCHIVE_MEMBERS = {"manifest.json", "component.wasm"}
PROVIDER_WORLD_PREFIX = "provider-adapter-"
COMPATIBILITY_KEYS = ("south_runtime", "runtime_abi", "kernel_contracts", "contracts")
RELEASE_TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
VERSION_TRIPLE = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")
# The first runtime with the range handshake (B3, §8.3): nothing older can admit a package that
# declares `runtime_abi`, so no package may declare it as its minimum.
RANGE_HANDSHAKE_RUNTIME = (0, 43, 0)
# Catalog data (boundary record §7.5, §13.11). A catalog is listed by the schema id its document
# declares; the release refuses a schema it does not know, so a new catalog format is a deliberate
# change here and in the design record, never a file dropped into catalogs/.
KNOWN_CATALOG_SCHEMAS = frozenset({"south.model-catalog.v1"})
# A schema listed here may be absent from a release although the previous release published it.
# Retiring one is a breaking change for hosts that load it (they lose the data, §13.11), so it
# takes an entry here, made in the same change as its design record.
RETIRED_CATALOG_SCHEMAS: frozenset[str] = frozenset()


class ReleaseIndexError(Exception):
    """A refusal; the message names what to fix."""


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def archive_name(package: str, tag: str) -> str:
    return f"{package}-{tag}.tar.gz"


def gate2_report_name(package: str, tag: str) -> str:
    return f"{package}-{tag}.gate2.json"


def catalog_name(stem: str, tag: str) -> str:
    """The published name of catalogs/<stem>.json."""
    return f"{stem}-{tag}.json"


def workspace_version(cargo_toml: Path) -> str:
    with cargo_toml.open("rb") as handle:
        return tomllib.load(handle)["workspace"]["package"]["version"]


def read_archive(path: Path) -> dict[str, bytes]:
    """The two files of a package archive, refusing anything but exactly those two at the root."""
    with tarfile.open(path, "r:gz") as archive:
        members = archive.getmembers()
        names = sorted(member.name for member in members)
        if sorted(ARCHIVE_MEMBERS) != names or not all(member.isfile() for member in members):
            raise ReleaseIndexError(
                f"{path.name}: an archive holds exactly manifest.json and component.wasm as regular files "
                f"at its root; found {names}"
            )
        files = {}
        for member in members:
            extracted = archive.extractfile(member)
            assert extracted is not None  # isfile() was checked above
            files[member.name] = extracted.read()
        return files


def credential_recipes(manifest: dict) -> bool:
    credentials = manifest.get("credentials")
    return isinstance(credentials, dict) and bool(credentials.get("recipes"))


def world_default(manifest: dict, key: str, provider_default: str) -> str | None:
    """A provider-world declaration: the manifest's value, else its default; null outside the provider world.

    The manifest validator refuses `stream_framing` and `usage_evidence` in any other world, so reporting the
    provider default for a task package would state something the package cannot declare.
    """
    if key in manifest:
        return manifest[key]
    if str(manifest.get("api_version", "")).startswith(PROVIDER_WORLD_PREFIX):
        return provider_default
    return None


def verify_gate2_report(
    path: Path, manifest: dict, manifest_sha256: str, component_sha256: str, south_release: str
) -> bytes:
    """The report's bytes, after checking that it is a passing report about exactly this package.

    `south_release` is the workspace version being released. The report must name it: the suite that
    writes the report runs in a crate with a version of its own (boundary record §16 Q47), so a
    report that took that crate's version instead would name a release the run was not part of.
    """
    data = path.read_bytes()
    try:
        report = json.loads(data)
    except json.JSONDecodeError as error:
        raise ReleaseIndexError(f"{path.name}: not JSON: {error}") from error
    expected = {
        "schema": GATE2_SCHEMA,
        "south_release": south_release,
        "name": manifest.get("name"),
        "version": manifest.get("version"),
        "suite": manifest.get("conformance", {}).get("required_suite"),
        "manifest_sha256": manifest_sha256,
        "component_sha256": component_sha256,
        "passed": True,
        "failed": 0,
    }
    for key, value in expected.items():
        if report.get(key) != value:
            raise ReleaseIndexError(
                f"{path.name}: `{key}` is {report.get(key)!r}, expected {value!r}; the report does not "
                "describe a passing gate 2 run over the archived package"
            )
    return data


def package_entry(
    component_dir: Path, dist: Path, tag: str, require_gate2_reports: bool, south_release: str
) -> tuple[dict, str]:
    """One package's index entry, and the archive file name it consumed."""
    package = component_dir.name
    archive = archive_name(package, tag)
    archive_path = dist / archive
    if not archive_path.is_file():
        raise ReleaseIndexError(f"{package}: {archive} is missing from {dist}")

    archive_bytes = archive_path.read_bytes()
    files = read_archive(archive_path)
    source_manifest = (component_dir / "manifest.json").read_bytes()
    if files["manifest.json"] != source_manifest:
        raise ReleaseIndexError(
            f"{archive}: manifest.json differs from components/{package}/manifest.json; "
            "the archive must carry the manifest verbatim"
        )
    manifest = json.loads(files["manifest.json"])
    if manifest.get("name") != package:
        raise ReleaseIndexError(
            f"components/{package}/manifest.json declares name {manifest.get('name')!r}; "
            "the directory, the archive and the package must share one name"
        )

    manifest_sha256 = sha256_hex(files["manifest.json"])
    component_sha256 = sha256_hex(files["component.wasm"])

    report_path = dist / gate2_report_name(package, tag)
    if report_path.is_file():
        report = verify_gate2_report(report_path, manifest, manifest_sha256, component_sha256, south_release)
        gate2_report = report_path.name
        gate2_report_sha256 = sha256_hex(report)
    elif require_gate2_reports:
        raise ReleaseIndexError(f"{package}: {report_path.name} is missing from {dist}")
    else:
        gate2_report = None
        gate2_report_sha256 = None

    compatibility = manifest.get("compatibility", {})
    entry = {
        "name": package,
        "version": manifest["version"],
        "world": manifest["api_version"],
        "wit_package": compatibility.get("wit_package"),
        "providers": manifest.get("providers", []),
        "capabilities": manifest.get("capabilities", []),
        "auth_arms": manifest.get("auth_arms", []),
        "stream_framing": world_default(manifest, "stream_framing", "bytes"),
        "usage_evidence": world_default(manifest, "usage_evidence", "reported"),
        "credential_recipes": credential_recipes(manifest),
        "compatibility": {key: compatibility.get(key) for key in COMPATIBILITY_KEYS},
        "archive": archive,
        "archive_sha256": sha256_hex(archive_bytes),
        "manifest_sha256": manifest_sha256,
        "component_sha256": component_sha256,
        "gate2_report": gate2_report,
        "gate2_report_sha256": gate2_report_sha256,
    }
    return entry, archive


def catalog_entries(catalogs: Path, dist: Path, tag: str) -> tuple[list[dict], set[str]]:
    """The index's `catalogs` list, and the dist file names it consumed.

    Each catalogs/<stem>.json must be published verbatim as <stem>-<tag>.json. Only the document's
    `schema` is read here: the full shape check is the Rust contract (`south_contracts::ModelCatalogV1`)
    run by the test suite over the same source file, and the byte comparison below ties the published
    file to that source.
    """
    entries, consumed, schemas = [], set(), {}
    sources = sorted(catalogs.glob("*.json")) if catalogs.is_dir() else []
    for source in sources:
        published = catalog_name(source.stem, tag)
        path = dist / published
        if not path.is_file():
            raise ReleaseIndexError(f"catalogs/{source.name}: {published} is missing from {dist}")
        data = path.read_bytes()
        if data != source.read_bytes():
            raise ReleaseIndexError(
                f"{published}: differs from catalogs/{source.name}; the release must carry the catalog verbatim"
            )
        try:
            document = json.loads(data)
        except json.JSONDecodeError as error:
            raise ReleaseIndexError(f"catalogs/{source.name}: not JSON: {error}") from error
        schema = document.get("schema") if isinstance(document, dict) else None
        if schema not in KNOWN_CATALOG_SCHEMAS:
            raise ReleaseIndexError(
                f"catalogs/{source.name}: schema {schema!r} is not one of {sorted(KNOWN_CATALOG_SCHEMAS)}"
            )
        if schema in schemas:
            raise ReleaseIndexError(
                f"catalogs/{source.name}: schema {schema} is already published by catalogs/{schemas[schema]}; "
                "a release carries one catalog per schema"
            )
        schemas[schema] = source.name
        entries.append({"schema": schema, "file": published, "sha256": sha256_hex(data)})
        consumed.add(published)
    entries.sort(key=lambda entry: entry["schema"])
    return entries, consumed


def generate(
    dist: Path,
    components: Path,
    tag: str,
    cargo_toml: Path,
    compatibility_json: Path,
    require_gate2_reports: bool = False,
    catalogs: Path | None = None,
) -> dict:
    version = workspace_version(cargo_toml)
    if tag != f"v{version}":
        raise ReleaseIndexError(f"tag {tag} does not match workspace version {version}")
    compatibility = json.loads(compatibility_json.read_text(encoding="utf-8"))

    component_dirs = sorted(path.parent for path in components.glob("*/manifest.json"))
    if not component_dirs:
        raise ReleaseIndexError(f"no components/*/manifest.json under {components}")
    packages = []
    consumed = set()
    for component_dir in component_dirs:
        entry, archive = package_entry(component_dir, dist, tag, require_gate2_reports, version)
        packages.append(entry)
        consumed.add(archive)

    stray = sorted(path.name for path in dist.glob(f"*-{tag}.tar.gz") if path.name not in consumed)
    if stray:
        raise ReleaseIndexError(f"archives with no components/<name>/manifest.json: {stray}")

    catalog_list, catalog_files = catalog_entries(
        catalogs if catalogs is not None else components.parent / "catalogs", dist, tag
    )
    stray = sorted(path.name for path in dist.glob(f"*-{tag}.json") if path.name not in catalog_files)
    if stray:
        raise ReleaseIndexError(f"catalog files with no catalogs/<name>.json: {stray}")

    packages.sort(key=lambda entry: entry["name"])
    return {
        "schema": INDEX_SCHEMA,
        "south_release": version,
        "runtime_abi": compatibility.get("runtime_abi"),
        "packages": packages,
        "catalogs": catalog_list,
    }


def render(index: dict) -> str:
    """The index's bytes: a fixed key order, two-space indent, a trailing newline."""
    return json.dumps(index, indent=2, ensure_ascii=False) + "\n"


def parse_tag(tag: str) -> tuple[int, int, int] | None:
    match = RELEASE_TAG.match(tag.strip())
    return tuple(int(part) for part in match.groups()) if match else None


def previous_tag(current: str, tags: list[str]) -> str | None:
    """The latest `vX.Y.Z` tag strictly earlier than `current`; pre-release and other tags are ignored."""
    current_version = parse_tag(current)
    if current_version is None:
        raise ReleaseIndexError(f"{current} is not a vX.Y.Z release tag")
    earlier = [(version, tag.strip()) for tag in tags if (version := parse_tag(tag)) and version < current_version]
    return max(earlier)[1] if earlier else None


def compare(previous: dict, current: dict) -> tuple[list[str], list[str]]:
    """(log lines, violations). A violation is a package whose version held but whose component.wasm changed."""
    for name, index in (("previous", previous), ("current", current)):
        if index.get("schema") != INDEX_SCHEMA:
            raise ReleaseIndexError(f"the {name} index has schema {index.get('schema')!r}, expected {INDEX_SCHEMA}")
    before = {package["name"]: package for package in previous["packages"]}
    log, violations = [], []
    for package in current["packages"]:
        name, version, digest = package["name"], package["version"], package["component_sha256"]
        old = before.get(name)
        if old is None:
            log.append(f"{name} {version}: new package")
        elif old["version"] != version:
            log.append(f"{name}: {old['version']} -> {version}")
        elif old["component_sha256"] != digest:
            violations.append(
                f"{name} {version}: component.wasm changed ({old['component_sha256']} -> {digest}) but the "
                "version did not; bump the version"
            )
        elif (
            "manifest_sha256" in old
            and "manifest_sha256" in package
            and old["manifest_sha256"] != package["manifest_sha256"]
        ):
            # A package's south_runtime is the oldest runtime it needs, not the release that carried
            # it (§13.6), so an unchanged package keeps its manifest byte for byte; a changed manifest
            # is a changed package.
            violations.append(
                f"{name} {version}: manifest.json changed ({old['manifest_sha256']} -> "
                f"{package['manifest_sha256']}) but the version did not; bump the version"
            )
        else:
            log.append(f"{name} {version}: component.wasm unchanged")
    for name in sorted(set(before) - {package["name"] for package in current["packages"]}):
        log.append(f"{name}: no longer released")
    compare_catalogs(previous.get("catalogs", []), current.get("catalogs", []), log, violations)
    return log, violations


def compare_catalogs(previous: list[dict], current: list[dict], log: list[str], violations: list[str]) -> None:
    """Catalog data may change in any release; a schema may not silently disappear (§13.11).

    A changed digest under the same schema is ordinary: catalog data ships in releases. A schema the
    previous release published and this one does not is a violation unless it is retired, because a
    host loading that schema would lose the data on re-pin with nothing at the release saying so.
    """
    before = {catalog["schema"]: catalog for catalog in previous}
    for catalog in current:
        schema, digest = catalog["schema"], catalog["sha256"]
        old = before.get(schema)
        if old is None:
            log.append(f"catalog {schema}: new ({catalog['file']})")
        elif old["sha256"] != digest:
            log.append(f"catalog {schema}: data changed ({old['sha256']} -> {digest})")
        else:
            log.append(f"catalog {schema}: unchanged")
    for schema in sorted(set(before) - {catalog["schema"] for catalog in current}):
        if schema in RETIRED_CATALOG_SCHEMAS:
            log.append(f"catalog {schema}: retired")
        else:
            violations.append(
                f"catalog {schema}: published by the previous release but not by this one; hosts loading it "
                "would lose it on re-pin. Keep publishing it, or list it in RETIRED_CATALOG_SCHEMAS with a "
                "design record"
            )


def declared_runtimes(root: Path, workspace: str) -> dict[str, list[str]]:
    """The staged packages under `root`, grouped by the `south_runtime` each declares.

    A package declares the oldest south runtime it needs (§13.6), so the release checks it by loading
    it under exactly that runtime. A declaration that check could not honour is refused here: newer
    than the release being built, older than the range handshake, malformed, or without the
    `runtime_abi` the range handshake requires.
    """
    workspace_triple = VERSION_TRIPLE.match(workspace)
    if workspace_triple is None:
        raise ReleaseIndexError(f"workspace version {workspace!r} is not a major.minor.patch triple")
    current = tuple(int(part) for part in workspace_triple.groups())
    groups: dict[str, list[str]] = {}
    directories = sorted(path for path in root.iterdir() if path.is_dir()) if root.is_dir() else []
    if not directories:
        raise ReleaseIndexError(f"no package directory under {root}")
    for directory in directories:
        manifest_path = directory / "manifest.json"
        if not manifest_path.is_file() or not (directory / "component.wasm").is_file():
            raise ReleaseIndexError(f"{directory.name}: a staged package holds manifest.json and component.wasm")
        compatibility = json.loads(manifest_path.read_text(encoding="utf-8")).get("compatibility", {})
        declared = compatibility.get("south_runtime")
        match = VERSION_TRIPLE.match(declared) if isinstance(declared, str) else None
        if match is None:
            raise ReleaseIndexError(
                f"{directory.name}: compatibility.south_runtime {declared!r} is not a major.minor.patch triple"
            )
        version = tuple(int(part) for part in match.groups())
        if version > current:
            raise ReleaseIndexError(
                f"{directory.name}: declares south runtime {declared}, newer than this release ({workspace})"
            )
        if version < RANGE_HANDSHAKE_RUNTIME:
            raise ReleaseIndexError(
                f"{directory.name}: declares south runtime {declared}, older than the range handshake "
                f"({'.'.join(map(str, RANGE_HANDSHAKE_RUNTIME))}), which no runtime could admit"
            )
        if not isinstance(compatibility.get("runtime_abi"), int):
            raise ReleaseIndexError(
                f"{directory.name}: declares no runtime_abi, so only an exact-tuple host could load it"
            )
        groups.setdefault(declared, []).append(directory.name)
    return dict(sorted(groups.items(), key=lambda item: tuple(int(p) for p in item[0].split("."))))


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)

    gen = commands.add_parser("generate", help="write south-release-index.json")
    gen.add_argument("--dist", type=Path, required=True, help="the staged release directory")
    gen.add_argument("--components", type=Path, default=Path("components"))
    gen.add_argument("--catalogs", type=Path, help="default: the catalogs/ directory beside --components")
    gen.add_argument("--tag", required=True, help="the release tag, vX.Y.Z")
    gen.add_argument("--cargo-toml", type=Path, default=Path("Cargo.toml"))
    gen.add_argument("--compatibility", type=Path, default=Path("compatibility.json"))
    gen.add_argument("--output", type=Path, help=f"default: <dist>/{INDEX_FILE}")
    gen.add_argument(
        "--require-gate2-reports",
        action="store_true",
        help="refuse a package with no gate 2 report instead of recording null",
    )

    prev = commands.add_parser("previous-tag", help="print the latest release tag earlier than --current")
    prev.add_argument("--current", required=True)

    cmp_ = commands.add_parser("compare", help="refuse a changed component.wasm under an unchanged version")
    cmp_.add_argument("--previous", type=Path, required=True)
    cmp_.add_argument("--current", type=Path, required=True)

    runtimes = commands.add_parser(
        "declared-runtimes", help="print `<south_runtime> <package>` for every staged package"
    )
    runtimes.add_argument("--root", type=Path, required=True, help="a directory of <package>/manifest.json")
    runtimes.add_argument("--cargo-toml", type=Path, default=Path("Cargo.toml"))

    args = parser.parse_args(argv)
    try:
        if args.command == "generate":
            index = generate(
                args.dist,
                args.components,
                args.tag,
                args.cargo_toml,
                args.compatibility,
                args.require_gate2_reports,
                args.catalogs,
            )
            output = args.output or args.dist / INDEX_FILE
            output.write_text(render(index), encoding="utf-8")
            print(f"wrote {output} ({len(index['packages'])} packages, {len(index['catalogs'])} catalogs)")
        elif args.command == "declared-runtimes":
            groups = declared_runtimes(args.root, workspace_version(args.cargo_toml))
            for runtime, packages in groups.items():
                for package in packages:
                    print(f"{runtime} {package}")
        elif args.command == "previous-tag":
            tag = previous_tag(args.current, sys.stdin.read().split())
            if tag:
                print(tag)
        else:
            previous = json.loads(args.previous.read_text(encoding="utf-8"))
            current = json.loads(args.current.read_text(encoding="utf-8"))
            log, violations = compare(previous, current)
            for line in log:
                print(line)
            for line in violations:
                print(f"error: {line}", file=sys.stderr)
            if violations:
                return 1
    except ReleaseIndexError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
