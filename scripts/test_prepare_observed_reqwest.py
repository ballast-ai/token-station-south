"""Offline tests for the optional, checksum-pinned observation workspace preparer."""
from __future__ import annotations

import hashlib
import io
import json
from pathlib import Path
import sys
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import prepare_observed_reqwest as prepare


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class PrepareTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "source"
        self.root.mkdir()
        self.bundle = self.root / "patches/reqwest-0.13.5-local-write"
        self.bundle.mkdir(parents=True)
        files = {
            "Cargo.toml": '[workspace]\nmembers = ["crates/*"]\n[workspace.package]\nversion = "0.76.0"\n',
            "crates/south-transport-reqwest/Cargo.toml": '[package]\nname = "south-transport-reqwest"\nversion = "0.76.0"\n',
            "crates/south-transport-reqwest/src/lib.rs": "old south\n",
        }
        for relative, text in files.items():
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        self.archive = self.base / "reqwest.crate"
        self.make_archive()
        patches = {
            "reqwest.patch": "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old client\n+observed client\n",
            "south-transport.patch": "--- a/crates/south-transport-reqwest/src/lib.rs\n+++ b/crates/south-transport-reqwest/src/lib.rs\n@@ -1 +1 @@\n-old south\n+observed south\n",
        }
        for name, text in patches.items():
            (self.bundle / name).write_text(text)
        self.manifest = {
            "schema_version": 1,
            "reqwest_version": "0.13.5",
            "crate_sha256": digest(self.archive.read_bytes()),
            "patch_sha256": {name: digest(text.encode()) for name, text in patches.items()},
            "source_files": {relative: digest(text.encode()) for relative, text in files.items()},
            "south_base_revision": "fixture",
        }
        self.output = self.base / "prepared"
        self.write_manifest()

    def make_archive(self, extra: tarfile.TarInfo | None = None) -> None:
        with tarfile.open(self.archive, "w:gz") as archive:
            for relative, content in {
                "Cargo.toml": b'[package]\nname="reqwest"\nversion="0.13.5"\n',
                "src/lib.rs": b"old client\n",
            }.items():
                member = tarfile.TarInfo("reqwest-0.13.5/" + relative)
                member.size = len(content)
                archive.addfile(member, io.BytesIO(content))
            if extra is not None:
                archive.addfile(extra, io.BytesIO(b""))

    def write_manifest(self) -> None:
        (self.bundle / "manifest.json").write_text(json.dumps(self.manifest))

    def run_prepare(self) -> dict:
        return prepare.prepare(self.root, self.output, self.archive)

    def test_creates_both_explicit_roots_without_source_changes(self) -> None:
        result = self.run_prepare()
        self.assertEqual((self.output / "reqwest/src/lib.rs").read_text(), "observed client\n")
        self.assertEqual((self.output / "crates/south-transport-reqwest/src/lib.rs").read_text(), "observed south\n")
        self.assertEqual((self.root / "crates/south-transport-reqwest/src/lib.rs").read_text(), "old south\n")
        for cargo in [self.output / "Cargo.toml", self.output / "host-check/Cargo.toml"]:
            text = cargo.read_text()
            self.assertIn("[patch.crates-io]", text)
            self.assertIn("south-transport-reqwest", text)
            self.assertIn("reqwest", text)
        host_cargo = (self.output / "host-check/Cargo.toml").read_text()
        self.assertIn('south-core = { path = "../crates/south-core" }', host_cargo)
        self.assertIn('south-contracts = { path = "../crates/south-contracts" }', host_cargo)
        self.assertIn("does not verify a live gateway migration", (self.output / "OBSERVATION-WORKSPACE.md").read_text())
        self.assertEqual(result["schema_version"], 1)
        self.assertNotIn(str(self.root), json.dumps(result))
        self.assertEqual(result, json.loads((self.output / "prepared-manifest.json").read_text()))

    def test_checks_archive_patch_and_source_before_creating_output(self) -> None:
        for kind in ["crate", "patch", "source"]:
            with self.subTest(kind=kind):
                manifest = json.loads(json.dumps(self.manifest))
                if kind == "crate":
                    manifest["crate_sha256"] = "0" * 64
                elif kind == "patch":
                    manifest["patch_sha256"]["reqwest.patch"] = "0" * 64
                else:
                    manifest["source_files"]["Cargo.toml"] = "0" * 64
                (self.bundle / "manifest.json").write_text(json.dumps(manifest))
                with self.assertRaises(prepare.PreparationError):
                    self.run_prepare()
                self.assertFalse(self.output.exists())
        self.write_manifest()

    def test_rejects_existing_destination_without_modifying_it(self) -> None:
        self.output.mkdir()
        (self.output / "keep").write_text("preserve")
        with self.assertRaises(prepare.PreparationError):
            self.run_prepare()
        self.assertEqual((self.output / "keep").read_text(), "preserve")

    def test_archive_rejects_traversal_absolute_and_link_members(self) -> None:
        cases = [("../escape", tarfile.REGTYPE), ("/absolute", tarfile.REGTYPE),
                 ("reqwest-0.13.5/link", tarfile.SYMTYPE),
                 ("reqwest-0.13.5/hard", tarfile.LNKTYPE)]
        for name, kind in cases:
            with self.subTest(name=name):
                extra = tarfile.TarInfo(name)
                extra.type = kind
                extra.linkname = "../escape"
                self.make_archive(extra)
                self.manifest["crate_sha256"] = digest(self.archive.read_bytes())
                self.write_manifest()
                with self.assertRaises(prepare.PreparationError):
                    self.run_prepare()
                self.assertFalse(self.output.exists())
                self.assertFalse((self.base / "escape").exists())
                self.assertFalse(list(self.base.glob(".prepared.*")))

    def test_patch_offsets_fail_and_partial_output_is_removed(self) -> None:
        path = self.bundle / "south-transport.patch"
        path.write_text(path.read_text().replace("@@ -1 +1 @@", "@@ -2 +2 @@"))
        self.manifest["patch_sha256"][path.name] = digest(path.read_bytes())
        self.write_manifest()
        with self.assertRaises(prepare.PreparationError):
            self.run_prepare()
        self.assertFalse(self.output.exists())
        self.assertFalse(list(self.base.glob(".prepared.*")))

    def test_patch_cannot_escape_workspace(self) -> None:
        path = self.bundle / "reqwest.patch"
        path.write_text("--- /dev/null\n+++ b/../../escape\n@@ -0,0 +1 @@\n+escape\n")
        self.manifest["patch_sha256"][path.name] = digest(path.read_bytes())
        self.write_manifest()
        with self.assertRaises(prepare.PreparationError):
            self.run_prepare()
        self.assertFalse(self.output.exists())
        self.assertFalse((self.base / "escape").exists())

    def test_source_symlinks_are_not_followed(self) -> None:
        (self.root / "crates/private-link").symlink_to(self.base)
        with self.assertRaises(prepare.PreparationError):
            self.run_prepare()
        self.assertFalse(self.output.exists())

    def test_output_cannot_recurse_into_copied_patch_sources(self) -> None:
        self.output = self.bundle / "prepared"
        with self.assertRaisesRegex(prepare.PreparationError, "overlap"):
            self.run_prepare()
        self.assertFalse(self.output.exists())

    def test_generated_files_are_verified_before_publication(self) -> None:
        self.manifest["generated_files"] = {"reqwest/src/lib.rs": digest(b"observed client\n")}
        self.write_manifest()
        self.run_prepare()
        self.output.rename(self.base / "first-preparation")
        self.manifest["generated_files"]["reqwest/src/lib.rs"] = "0" * 64
        self.write_manifest()
        with self.assertRaises(prepare.PreparationError):
            self.run_prepare()
        self.assertFalse(self.output.exists())

    def test_exact_patcher_handles_new_files_and_multiple_hunks(self) -> None:
        root = self.base / "patch-target"
        root.mkdir()
        (root / "existing").write_text("one\ntwo\nthree\nfour\n")
        patch = ("--- a/existing\n+++ b/existing\n@@ -1 +1 @@\n-one\n+first\n"
                 "@@ -4 +4 @@\n-four\n+last\n"
                 "--- /dev/null\n+++ b/new\n@@ -0,0 +1 @@\n+created\n").encode()
        prepare.apply_exact_patch(root, patch)
        self.assertEqual((root / "existing").read_text(), "first\ntwo\nthree\nlast\n")
        self.assertEqual((root / "new").read_text(), "created\n")

    def test_copied_input_tree_captures_unpinned_sources_but_not_build_outputs(self) -> None:
        crate = self.root / "crates/south-transport-reqwest"
        (crate / "target").mkdir()
        (crate / "target/build-output").write_text("not a source input")
        before = self.run_prepare()
        self.assertEqual(before["copied_input_files"]["Cargo.toml"], digest((self.root / "Cargo.toml").read_bytes()))
        self.assertNotIn("crates/south-transport-reqwest/target/build-output", before["copied_input_files"])
        self.output.rename(self.base / "first-preparation")
        (crate / "source-note.txt").write_text("an additional copied source")
        after = self.run_prepare()
        self.assertIn("crates/south-transport-reqwest/source-note.txt", after["copied_input_files"])
        self.assertNotEqual(before["copied_input_tree_sha256"], after["copied_input_tree_sha256"])
        self.assertEqual(before["inputs"]["source_files"], after["inputs"]["source_files"])

    def test_download_and_local_archive_have_identical_copied_input_trees(self) -> None:
        local = self.run_prepare()
        self.output.rename(self.base / "local-preparation")
        with mock.patch.object(prepare.urllib.request, "urlopen",
                               return_value=io.BytesIO(self.archive.read_bytes())):
            downloaded = prepare.prepare(self.root, self.output)
        self.assertNotIn("source.crate", downloaded["copied_input_files"])
        self.assertEqual(local["copied_input_files"], downloaded["copied_input_files"])
        self.assertEqual(local["copied_input_tree_sha256"], downloaded["copied_input_tree_sha256"])

    def test_records_preparer_hash_and_absent_git_without_claiming_a_revision(self) -> None:
        result = self.run_prepare()
        self.assertEqual(result["prepare_script_sha256"], digest(Path(prepare.__file__).read_bytes()))
        self.assertIsNone(result["source_git_state"])
        self.assertEqual(result["south_base_revision"], "fixture")
        self.assertEqual(result["host_smoke_scope"], "basic_config")
        self.assertFalse(result["tests_executed"])

    @unittest.skipUnless(shutil.which("git"), "Git is unavailable")
    def test_records_actual_git_head_and_dirty_state_separately_from_patch_base(self) -> None:
        def git(*arguments: str) -> str:
            return subprocess.run(["git", "-C", str(self.root), *arguments],
                                  capture_output=True, text=True, check=True).stdout.strip()
        git("init", "-q")
        git("add", ".")
        git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "-qm", "fixture source")
        result = self.run_prepare()
        self.assertEqual(result["source_git_state"], {"head": git("rev-parse", "HEAD"), "dirty": False})
        self.assertEqual(result["south_base_revision"], "fixture")
        self.output.rename(self.base / "first-preparation")
        (self.root / "untracked-note").write_text("dirty")
        self.assertTrue(self.run_prepare()["source_git_state"]["dirty"])

    def test_pinned_host_smoke_is_copied_and_verified(self) -> None:
        smoke = b"fn main() { /* pinned host construction */ }\n"
        (self.bundle / "host-smoke.rs").write_bytes(smoke)
        self.manifest["host_smoke_sha256"] = digest(smoke)
        self.manifest["host_smoke_scope"] = "local_write_constructor"
        self.write_manifest()
        result = self.run_prepare()
        self.assertEqual((self.output / "host-check/src/main.rs").read_bytes(), smoke)
        self.assertEqual(result["host_smoke_scope"], "local_write_constructor")
        self.assertEqual(result["host_smoke_source"], "bundle_pinned")
        self.assertFalse(result["tests_executed"])


if __name__ == "__main__":
    unittest.main()
