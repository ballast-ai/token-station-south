"""Tests for scripts/release_index.py.

Run with `python3 -m unittest discover -s scripts -p 'test_*.py'` from the repository root.
"""

from __future__ import annotations

import contextlib
import hashlib
import io
import json
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

# No __pycache__ beside the script in the working tree.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import release_index  # noqa: E402

TAG = "v0.43.0"


def provider_manifest(name: str = "provider-gemini", version: str = "1.1.5", **extra) -> dict:
    manifest = {
        "name": name,
        "version": version,
        "api_version": "provider-adapter-v2",
        "providers": ["gemini"],
        "capabilities": ["chat", "stream", "tool_call"],
        "auth_arms": ["header_secret"],
        "permissions": {"network": False, "filesystem": False, "secrets": ["provider_api_key"]},
        "conformance": {"required_suite": "south.provider-component.v1", "fixtures": "fixtures-gemini/"},
        "compatibility": {
            "ir_schema_id": "token-station-protocol@0.5.0/v0.4.0",
            "kernel_version": "0.4.0",
            "kernel_revision": "8e34f5a089d0b9c7273b49ddb6952dd87e960019",
            "wit_package": "token-station:adapter@2.0.0",
            "south_runtime": "0.43.0",
        },
    }
    manifest.update(extra)
    return manifest


def task_manifest(name: str = "task-veo-v2", version: str = "0.35.1") -> dict:
    return {
        "name": name,
        "version": version,
        "api_version": "task-adapter-v2",
        "providers": ["veo"],
        "capabilities": ["submit", "observe", "render"],
        "auth_arms": ["bearer"],
        "conformance": {"required_suite": "south.task-component.v2", "fixtures": "fixtures-veo-task-v2/"},
        "compatibility": {"wit_package": "token-station:task-adapter@2.0.0", "south_runtime": "0.43.0"},
    }


class Workspace:
    """A temporary repository root: Cargo.toml, compatibility.json, components/ and dist/."""

    def __init__(self, root: Path, compatibility: dict | None = None) -> None:
        self.root = root
        root.mkdir(parents=True, exist_ok=True)
        self.components = root / "components"
        self.dist = root / "dist"
        self.components.mkdir()
        self.dist.mkdir()
        (root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.43.0"\n', encoding="utf-8")
        (root / "compatibility.json").write_text(json.dumps(compatibility or {"schema_version": 4}), encoding="utf-8")

    def add(self, manifest: dict, wasm: bytes = b"\0asm-component", members: dict | None = None) -> bytes:
        """Adds a component and its archive; returns the manifest bytes."""
        name = manifest["name"]
        manifest_bytes = json.dumps(manifest, indent=2).encode()
        (self.components / name).mkdir()
        (self.components / name / "manifest.json").write_bytes(manifest_bytes)
        files = members if members is not None else {"manifest.json": manifest_bytes, "component.wasm": wasm}
        with tarfile.open(self.dist / release_index.archive_name(name, TAG), "w:gz") as archive:
            for member, data in files.items():
                info = tarfile.TarInfo(member)
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
        return manifest_bytes

    def add_report(self, manifest: dict, manifest_bytes: bytes, wasm: bytes, **override) -> bytes:
        report = {
            "schema": "south.gate2-report.v1",
            "suite": manifest["conformance"]["required_suite"],
            "south_release": "0.43.0",
            "name": manifest["name"],
            "version": manifest["version"],
            "world": manifest["api_version"],
            "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
            "component_sha256": hashlib.sha256(wasm).hexdigest(),
            "passed": True,
            "checks": 1,
            "failed": 0,
            "outcomes": [{"check": "fixture_match", "case": "x", "verdict": "passed"}],
        }
        report.update(override)
        data = (json.dumps(report, indent=2) + "\n").encode()
        (self.dist / release_index.gate2_report_name(manifest["name"], TAG)).write_bytes(data)
        return data

    def add_catalog(self, stem: str = "model-catalog", document: dict | None = None, published: bytes | None = None) -> bytes:
        """Adds catalogs/<stem>.json and its published copy in dist; returns the source bytes."""
        data = (json.dumps(document or {"schema": "south.model-catalog.v1", "entries": []}, indent=2) + "\n").encode()
        catalogs = self.root / "catalogs"
        catalogs.mkdir(exist_ok=True)
        (catalogs / f"{stem}.json").write_bytes(data)
        (self.dist / release_index.catalog_name(stem, TAG)).write_bytes(data if published is None else published)
        return data

    def generate(self, **kwargs) -> dict:
        return release_index.generate(
            self.dist, self.components, TAG, self.root / "Cargo.toml", self.root / "compatibility.json", **kwargs
        )


class GenerateTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.ws = Workspace(Path(self.tmp.name))

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_entry_matches_the_section_9_2_shape(self) -> None:
        wasm = b"\0asm-gemini"
        manifest = provider_manifest()
        manifest_bytes = self.ws.add(manifest, wasm)
        report = self.ws.add_report(manifest, manifest_bytes, wasm)
        index = self.ws.generate()

        self.assertEqual(list(index), ["schema", "south_release", "runtime_abi", "packages", "catalogs"])
        self.assertEqual(index["schema"], "south.release-index.v1")
        self.assertEqual(index["south_release"], "0.43.0")
        self.assertIsNone(index["runtime_abi"])
        self.assertEqual(index["catalogs"], [])
        archive = (self.ws.dist / f"provider-gemini-{TAG}.tar.gz").read_bytes()
        self.assertEqual(
            index["packages"],
            [
                {
                    "name": "provider-gemini",
                    "version": "1.1.5",
                    "world": "provider-adapter-v2",
                    "wit_package": "token-station:adapter@2.0.0",
                    "providers": ["gemini"],
                    "capabilities": ["chat", "stream", "tool_call"],
                    "auth_arms": ["header_secret"],
                    "stream_framing": "bytes",
                    "usage_evidence": "reported",
                    "credential_recipes": False,
                    "compatibility": {
                        "south_runtime": "0.43.0",
                        "runtime_abi": None,
                        "kernel_contracts": None,
                        "contracts": None,
                    },
                    "archive": f"provider-gemini-{TAG}.tar.gz",
                    "archive_sha256": hashlib.sha256(archive).hexdigest(),
                    "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
                    "component_sha256": hashlib.sha256(wasm).hexdigest(),
                    "gate2_report": f"provider-gemini-{TAG}.gate2.json",
                    "gate2_report_sha256": hashlib.sha256(report).hexdigest(),
                }
            ],
        )

    def test_declared_values_and_range_fields_are_copied(self) -> None:
        self.ws = Workspace(Path(self.tmp.name) / "ranges", compatibility={"runtime_abi": 1})
        manifest = provider_manifest(
            stream_framing="aws-eventstream",
            usage_evidence="absent",
            credentials={"recipes": {"refresh": {}}},
        )
        manifest["compatibility"].update(
            runtime_abi=1,
            kernel_contracts={"canonical_ir": 3, "stream": 2, "error_catalog": 1},
            contracts={"task": 7},
        )
        self.ws.add(manifest)
        index = self.ws.generate()
        entry = index["packages"][0]
        self.assertEqual(index["runtime_abi"], 1)
        self.assertEqual(entry["stream_framing"], "aws-eventstream")
        self.assertEqual(entry["usage_evidence"], "absent")
        self.assertTrue(entry["credential_recipes"])
        self.assertEqual(
            entry["compatibility"],
            {
                "south_runtime": "0.43.0",
                "runtime_abi": 1,
                "kernel_contracts": {"canonical_ir": 3, "stream": 2, "error_catalog": 1},
                "contracts": {"task": 7},
            },
        )
        self.assertIsNone(entry["gate2_report"])
        self.assertIsNone(entry["gate2_report_sha256"])

    def test_provider_world_defaults_do_not_apply_to_task_packages(self) -> None:
        self.ws.add(task_manifest())
        entry = self.ws.generate()["packages"][0]
        self.assertEqual(entry["world"], "task-adapter-v2")
        self.assertIsNone(entry["stream_framing"])
        self.assertIsNone(entry["usage_evidence"])

    def test_packages_are_sorted_and_output_is_deterministic(self) -> None:
        self.ws.add(task_manifest("task-veo-v2"))
        self.ws.add(provider_manifest("provider-anthropic"))
        self.ws.add(provider_manifest("provider-gemini"))
        first = release_index.render(self.ws.generate())
        second = release_index.render(self.ws.generate())
        self.assertEqual(first, second)
        self.assertTrue(first.endswith("}\n"))
        names = [package["name"] for package in json.loads(first)["packages"]]
        self.assertEqual(names, ["provider-anthropic", "provider-gemini", "task-veo-v2"])

    def test_missing_archive_is_refused(self) -> None:
        self.ws.add(provider_manifest())
        (self.ws.dist / f"provider-gemini-{TAG}.tar.gz").unlink()
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "is missing"):
            self.ws.generate()

    def test_stray_archive_is_refused(self) -> None:
        self.ws.add(provider_manifest())
        (self.ws.dist / f"provider-ghost-{TAG}.tar.gz").write_bytes(b"")
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "provider-ghost"):
            self.ws.generate()

    def test_archive_with_an_extra_member_is_refused(self) -> None:
        manifest = provider_manifest()
        data = json.dumps(manifest, indent=2).encode()
        self.ws.add(manifest, members={"manifest.json": data, "component.wasm": b"w", "extra": b""})
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "exactly manifest.json and component.wasm"):
            self.ws.generate()

    def test_archive_manifest_must_equal_the_source_manifest(self) -> None:
        manifest = provider_manifest()
        stale = json.dumps(provider_manifest(version="1.1.4"), indent=2).encode()
        self.ws.add(manifest, members={"manifest.json": stale, "component.wasm": b"w"})
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "verbatim"):
            self.ws.generate()

    def test_directory_must_name_the_package(self) -> None:
        manifest = provider_manifest(name="provider-gemini")
        manifest_bytes = json.dumps(manifest, indent=2).encode()
        (self.ws.components / "provider-other").mkdir()
        (self.ws.components / "provider-other" / "manifest.json").write_bytes(manifest_bytes)
        with tarfile.open(self.ws.dist / f"provider-other-{TAG}.tar.gz", "w:gz") as archive:
            for member, data in (("manifest.json", manifest_bytes), ("component.wasm", b"w")):
                info = tarfile.TarInfo(member)
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "share one name"):
            self.ws.generate()

    def test_tag_must_match_the_workspace_version(self) -> None:
        self.ws.add(provider_manifest())
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "does not match"):
            release_index.generate(
                self.ws.dist,
                self.ws.components,
                "v0.42.0",
                self.ws.root / "Cargo.toml",
                self.ws.root / "compatibility.json",
            )

    def test_required_report_must_exist(self) -> None:
        self.ws.add(provider_manifest())
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "gate2.json is missing"):
            self.ws.generate(require_gate2_reports=True)

    def test_report_about_other_bytes_is_refused(self) -> None:
        manifest = provider_manifest()
        manifest_bytes = self.ws.add(manifest, b"released bytes")
        self.ws.add_report(manifest, manifest_bytes, b"other bytes")
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "component_sha256"):
            self.ws.generate()

    def test_failing_report_is_refused(self) -> None:
        manifest = provider_manifest()
        wasm = b"w"
        manifest_bytes = self.ws.add(manifest, wasm)
        self.ws.add_report(manifest, manifest_bytes, wasm, passed=False, failed=1)
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "`passed`"):
            self.ws.generate()

    def test_report_naming_another_release_is_refused(self) -> None:
        # A report that took the conformance crate's own version (§16 Q47) instead of the workspace
        # version names a release its run was not part of.
        manifest = provider_manifest()
        wasm = b"w"
        manifest_bytes = self.ws.add(manifest, wasm)
        self.ws.add_report(manifest, manifest_bytes, wasm, south_release="0.42.0")
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "`south_release`"):
            self.ws.generate()

    def test_report_for_another_suite_is_refused(self) -> None:
        manifest = provider_manifest()
        wasm = b"w"
        manifest_bytes = self.ws.add(manifest, wasm)
        self.ws.add_report(manifest, manifest_bytes, wasm, suite="south.task-component.v2")
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "`suite`"):
            self.ws.generate()

    def test_main_writes_the_index_into_dist(self) -> None:
        self.ws.add(provider_manifest())
        root = self.ws.root
        with contextlib.redirect_stdout(io.StringIO()):
            code = release_index.main(
                [
                    "generate",
                    f"--dist={root / 'dist'}",
                    f"--components={root / 'components'}",
                    f"--tag={TAG}",
                    f"--cargo-toml={root / 'Cargo.toml'}",
                    f"--compatibility={root / 'compatibility.json'}",
                ]
            )
        self.assertEqual(code, 0)
        written = (root / "dist" / "south-release-index.json").read_text(encoding="utf-8")
        self.assertEqual(json.loads(written)["packages"][0]["name"], "provider-gemini")


class CatalogGenerateTest(unittest.TestCase):
    """Boundary record §13.11: the release lists each catalog by schema, file and digest."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.ws = Workspace(Path(self.tmp.name))
        self.ws.add(provider_manifest())

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_no_catalogs_directory_lists_none(self) -> None:
        self.assertEqual(self.ws.generate()["catalogs"], [])

    def test_a_catalog_is_listed_with_schema_file_and_digest(self) -> None:
        data = self.ws.add_catalog()
        self.assertEqual(
            self.ws.generate()["catalogs"],
            [
                {
                    "schema": "south.model-catalog.v1",
                    "file": f"model-catalog-{TAG}.json",
                    "sha256": hashlib.sha256(data).hexdigest(),
                }
            ],
        )

    def test_a_missing_published_catalog_is_refused(self) -> None:
        self.ws.add_catalog()
        (self.ws.dist / f"model-catalog-{TAG}.json").unlink()
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "is missing"):
            self.ws.generate()

    def test_a_published_catalog_must_equal_its_source(self) -> None:
        self.ws.add_catalog(published=b'{"schema": "south.model-catalog.v1", "entries": []}\n')
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "verbatim"):
            self.ws.generate()

    def test_an_unknown_schema_is_refused(self) -> None:
        self.ws.add_catalog(document={"schema": "south.model-catalog.v2", "entries": []})
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "south.model-catalog.v2"):
            self.ws.generate()

    def test_a_catalog_that_is_not_an_object_is_refused(self) -> None:
        self.ws.add_catalog(document=["south.model-catalog.v1"])
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "schema None"):
            self.ws.generate()

    def test_two_catalogs_of_one_schema_are_refused(self) -> None:
        self.ws.add_catalog("model-catalog")
        self.ws.add_catalog("model-catalog-copy")
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "one catalog per schema"):
            self.ws.generate()

    def test_a_stray_catalog_file_is_refused(self) -> None:
        self.ws.add_catalog()
        (self.ws.dist / f"ghost-catalog-{TAG}.json").write_bytes(b"{}")
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "ghost-catalog"):
            self.ws.generate()

    def test_main_reads_the_catalogs_directory_beside_components(self) -> None:
        self.ws.add_catalog()
        root = self.ws.root
        with contextlib.redirect_stdout(io.StringIO()):
            code = release_index.main(
                [
                    "generate",
                    f"--dist={root / 'dist'}",
                    f"--components={root / 'components'}",
                    f"--tag={TAG}",
                    f"--cargo-toml={root / 'Cargo.toml'}",
                    f"--compatibility={root / 'compatibility.json'}",
                ]
            )
        self.assertEqual(code, 0)
        written = json.loads((root / "dist" / "south-release-index.json").read_text(encoding="utf-8"))
        self.assertEqual([catalog["schema"] for catalog in written["catalogs"]], ["south.model-catalog.v1"])

    def test_the_repository_catalog_is_listed(self) -> None:
        # The real catalogs/ directory: every file in it declares a known schema.
        repository = Path(__file__).resolve().parent.parent / "catalogs"
        for source in sorted(repository.glob("*.json")):
            published = self.ws.dist / release_index.catalog_name(source.stem, TAG)
            published.write_bytes(source.read_bytes())
        entries = self.ws.generate(catalogs=repository)["catalogs"]
        self.assertEqual([entry["schema"] for entry in entries], ["south.model-catalog.v1"])
        self.assertEqual(entries[0]["file"], f"model-catalog-{TAG}.json")


def index_of(*packages: tuple[str, ...]) -> dict:
    """An index of (name, version, component digest[, manifest digest]) entries."""
    entries = []
    for name, version, digest, *manifest in packages:
        entry = {"name": name, "version": version, "component_sha256": digest}
        if manifest:
            entry["manifest_sha256"] = manifest[0]
        entries.append(entry)
    return {"schema": "south.release-index.v1", "packages": entries}


def with_catalogs(index: dict, *catalogs: tuple[str, str]) -> dict:
    """The index with (schema, digest) catalog entries."""
    index["catalogs"] = [{"schema": schema, "file": f"{schema}.json", "sha256": digest} for schema, digest in catalogs]
    return index


class CompareTest(unittest.TestCase):
    def test_an_index_without_catalogs_compares_as_empty(self) -> None:
        log, violations = release_index.compare(
            index_of(("a", "1.0.0", "aa")), with_catalogs(index_of(("a", "1.0.0", "aa")), ("s.v1", "c1"))
        )
        self.assertEqual(violations, [])
        self.assertEqual(log, ["a 1.0.0: component.wasm unchanged", "catalog s.v1: new (s.v1.json)"])

    def test_catalog_data_may_change_between_releases(self) -> None:
        log, violations = release_index.compare(
            with_catalogs(index_of(), ("s.v1", "c1")), with_catalogs(index_of(), ("s.v1", "c2"))
        )
        self.assertEqual(violations, [])
        self.assertEqual(log, ["catalog s.v1: data changed (c1 -> c2)"])

    def test_an_unchanged_catalog_is_logged(self) -> None:
        log, violations = release_index.compare(
            with_catalogs(index_of(), ("s.v1", "c1")), with_catalogs(index_of(), ("s.v1", "c1"))
        )
        self.assertEqual((log, violations), (["catalog s.v1: unchanged"], []))

    def test_a_vanished_catalog_schema_fails(self) -> None:
        # A schema bump that drops the old schema: hosts still loading s.v1 would lose the data.
        log, violations = release_index.compare(
            with_catalogs(index_of(), ("s.v1", "c1")), with_catalogs(index_of(), ("s.v2", "c2"))
        )
        self.assertEqual(log, ["catalog s.v2: new (s.v2.json)"])
        self.assertEqual(len(violations), 1)
        self.assertIn("catalog s.v1: published by the previous release but not by this one", violations[0])

    def test_a_retired_catalog_schema_may_vanish(self) -> None:
        retired = release_index.RETIRED_CATALOG_SCHEMAS
        release_index.RETIRED_CATALOG_SCHEMAS = frozenset({"s.v1"})
        try:
            log, violations = release_index.compare(with_catalogs(index_of(), ("s.v1", "c1")), with_catalogs(index_of()))
        finally:
            release_index.RETIRED_CATALOG_SCHEMAS = retired
        self.assertEqual((log, violations), (["catalog s.v1: retired"], []))

    def test_no_catalog_schema_is_retired_today(self) -> None:
        self.assertEqual(release_index.RETIRED_CATALOG_SCHEMAS, frozenset())

    def test_unchanged_version_with_unchanged_bytes_passes(self) -> None:
        log, violations = release_index.compare(index_of(("a", "1.0.0", "aa")), index_of(("a", "1.0.0", "aa")))
        self.assertEqual(violations, [])
        self.assertEqual(log, ["a 1.0.0: component.wasm unchanged"])

    def test_unchanged_version_with_changed_bytes_fails_naming_the_package(self) -> None:
        _, violations = release_index.compare(
            index_of(("a", "1.0.0", "aa"), ("b", "2.0.0", "bb")),
            index_of(("a", "1.0.0", "aa"), ("b", "2.0.0", "cc")),
        )
        self.assertEqual(len(violations), 1)
        self.assertIn("b 2.0.0", violations[0])
        self.assertIn("bump the version", violations[0])

    def test_bumped_new_and_removed_packages_pass(self) -> None:
        log, violations = release_index.compare(
            index_of(("a", "1.0.0", "aa"), ("gone", "1.0.0", "gg")),
            index_of(("a", "1.0.1", "ab"), ("new", "0.1.0", "nn")),
        )
        self.assertEqual(violations, [])
        self.assertEqual(log, ["a: 1.0.0 -> 1.0.1", "new 0.1.0: new package", "gone: no longer released"])

    def test_unchanged_version_with_changed_manifest_fails_naming_the_package(self) -> None:
        # Boundary record §8.6 and §13.6: a package that keeps its version keeps its whole
        # identity, so its south_runtime is never re-stamped without a version bump.
        _, violations = release_index.compare(
            index_of(("a", "1.0.0", "aa", "m1")),
            index_of(("a", "1.0.0", "aa", "m2")),
        )
        self.assertEqual(len(violations), 1)
        self.assertIn("a 1.0.0: manifest.json changed", violations[0])
        self.assertIn("bump the version", violations[0])

    def test_unchanged_version_with_unchanged_manifest_passes(self) -> None:
        _, violations = release_index.compare(
            index_of(("a", "1.0.0", "aa", "m1")),
            index_of(("a", "1.0.0", "aa", "m1")),
        )
        self.assertEqual(violations, [])

    def test_unknown_schema_is_refused(self) -> None:
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "previous index"):
            release_index.compare({"schema": "other", "packages": []}, index_of())

    def test_main_exit_code(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            previous, current = Path(tmp) / "p.json", Path(tmp) / "c.json"
            previous.write_text(json.dumps(index_of(("a", "1.0.0", "aa"))), encoding="utf-8")
            current.write_text(json.dumps(index_of(("a", "1.0.0", "ab"))), encoding="utf-8")
            stderr = io.StringIO()
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
                code = release_index.main(["compare", f"--previous={previous}", f"--current={current}"])
            self.assertEqual(code, 1)
            self.assertIn("error: a 1.0.0", stderr.getvalue())


def staged(root: Path, manifest: dict) -> None:
    package = root / manifest["name"]
    package.mkdir(parents=True)
    (package / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
    (package / "component.wasm").write_bytes(b"\0asm")


def with_runtime(manifest: dict, south_runtime: str, runtime_abi: int | None = 1) -> dict:
    manifest["compatibility"]["south_runtime"] = south_runtime
    if runtime_abi is not None:
        manifest["compatibility"]["runtime_abi"] = runtime_abi
    return manifest


class DeclaredRuntimesTest(unittest.TestCase):
    """Boundary record §13.6 (SF10): each package is checked under the runtime it declares."""

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_packages_are_grouped_by_the_runtime_they_declare(self) -> None:
        staged(self.root, with_runtime(provider_manifest("provider-b"), "0.44.0"))
        staged(self.root, with_runtime(provider_manifest("provider-a"), "0.45.0"))
        staged(self.root, with_runtime(task_manifest("task-c"), "0.44.0"))
        self.assertEqual(
            release_index.declared_runtimes(self.root, "0.45.0"),
            {"0.44.0": ["provider-b", "task-c"], "0.45.0": ["provider-a"]},
        )

    def test_a_runtime_newer_than_the_workspace_is_refused(self) -> None:
        staged(self.root, with_runtime(provider_manifest(), "0.46.0"))
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "newer than this release"):
            release_index.declared_runtimes(self.root, "0.45.0")

    def test_a_runtime_older_than_the_range_handshake_is_refused(self) -> None:
        staged(self.root, with_runtime(provider_manifest(), "0.42.0"))
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "range handshake"):
            release_index.declared_runtimes(self.root, "0.45.0")

    def test_a_package_without_runtime_abi_is_refused(self) -> None:
        staged(self.root, with_runtime(provider_manifest(), "0.44.0", runtime_abi=None))
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "runtime_abi"):
            release_index.declared_runtimes(self.root, "0.45.0")

    def test_a_malformed_runtime_is_refused(self) -> None:
        staged(self.root, with_runtime(provider_manifest(), "0.44"))
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "major.minor.patch"):
            release_index.declared_runtimes(self.root, "0.45.0")

    def test_a_directory_without_a_package_is_refused(self) -> None:
        (self.root / "stray").mkdir()
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "stray"):
            release_index.declared_runtimes(self.root, "0.45.0")

    def test_an_empty_root_is_refused(self) -> None:
        with self.assertRaisesRegex(release_index.ReleaseIndexError, "no package"):
            release_index.declared_runtimes(self.root, "0.45.0")

    def test_main_prints_one_line_per_package(self) -> None:
        staged(self.root, with_runtime(provider_manifest("provider-a"), "0.44.0"))
        cargo_toml = self.root.parent / f"{self.root.name}-Cargo.toml"
        cargo_toml.write_text('[workspace.package]\nversion = "0.45.0"\n', encoding="utf-8")
        try:
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                code = release_index.main(
                    ["declared-runtimes", f"--root={self.root}", f"--cargo-toml={cargo_toml}"]
                )
        finally:
            cargo_toml.unlink()
        self.assertEqual(code, 0)
        self.assertEqual(stdout.getvalue(), "0.44.0 provider-a\n")


class PreviousTagTest(unittest.TestCase):
    def test_latest_earlier_release_tag(self) -> None:
        tags = ["v0.41.0", "v0.9.0", "v0.42.0", "v0.43.0", "v0.44.0", "v0.42.1-rc.1", "nightly"]
        self.assertEqual(release_index.previous_tag("v0.43.0", tags), "v0.42.0")

    def test_numeric_not_lexical_order(self) -> None:
        self.assertEqual(release_index.previous_tag("v0.10.0", ["v0.9.0", "v0.1.0"]), "v0.9.0")

    def test_no_earlier_release(self) -> None:
        self.assertIsNone(release_index.previous_tag("v0.25.0", ["v0.25.0", "v0.26.0"]))

    def test_current_must_be_a_release_tag(self) -> None:
        with self.assertRaises(release_index.ReleaseIndexError):
            release_index.previous_tag("main", [])


if __name__ == "__main__":
    unittest.main()
