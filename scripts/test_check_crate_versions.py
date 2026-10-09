"""Tests for scripts/check_crate_versions.py.

Run with `python3 -m unittest discover -s scripts -p 'test_*.py'` from the repository root. Each test builds
a throwaway git repository with a workspace and the three guest-linked crates, tags a release and edits it.
"""

from __future__ import annotations

import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
import unittest.mock
from pathlib import Path

# No __pycache__ beside the script in the working tree.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_crate_versions  # noqa: E402

CRATES = check_crate_versions.GUEST_LINKED_CRATES
# A fixed identity and no user or system configuration, so the tests do not depend on the machine.
GIT_ENV = {
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "test",
    "GIT_AUTHOR_EMAIL": "test@example.invalid",
    "GIT_COMMITTER_NAME": "test",
    "GIT_COMMITTER_EMAIL": "test@example.invalid",
}


def crate_manifest(name: str, version: str | None) -> str:
    line = 'version.workspace = true' if version is None else f'version = "{version}"'
    return f'[package]\nname = "{name}"\n{line}\n'


class CrateVersionCheckTest(unittest.TestCase):
    def setUp(self) -> None:
        self._env = unittest.mock.patch.dict(os.environ, GIT_ENV)
        self._env.start()
        self.addCleanup(self._env.stop)
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.repo = Path(self._tmp.name)
        self.git("init", "-q", "-b", "main")
        self.write("Cargo.toml", '[workspace.package]\nversion = "0.50.0"\n')
        for crate in CRATES:
            self.write(f"crates/{crate}/Cargo.toml", crate_manifest(crate, "0.49.0"))
            self.write(f"crates/{crate}/src/lib.rs", "pub fn f() {}\n")
        self.write("crates/south-core/Cargo.toml", crate_manifest("south-core", None))
        self.commit_and_tag("v0.50.0")
        self.write("Cargo.toml", '[workspace.package]\nversion = "0.51.0"\n')
        self.commit()

    def git(self, *args: str) -> str:
        return subprocess.run(
            ["git", "-C", str(self.repo), *args], check=True, capture_output=True, text=True
        ).stdout

    def write(self, path: str, text: str) -> None:
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")

    def commit(self) -> None:
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", "change")

    def commit_and_tag(self, tag: str) -> None:
        self.commit()
        self.git("tag", tag)

    def run_check(self, *args: str) -> tuple[int, str, str]:
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = check_crate_versions.main([f"--repo={self.repo}", *args])
        return code, stdout.getvalue(), stderr.getvalue()

    def test_unchanged_crates_pass(self) -> None:
        code, out, err = self.run_check()
        self.assertEqual(code, 0, err)
        self.assertIn("comparing guest-linked crates at HEAD with v0.50.0", out)
        for crate in CRATES:
            self.assertIn(f"{crate} 0.49.0: unchanged since v0.50.0 outside tests/", out)
        self.assertNotIn("exempt", out)

    def test_changed_without_bump_fails_naming_crate_and_files(self) -> None:
        self.write("crates/south-provider-api/src/lib.rs", "pub fn g() {}\n")
        self.write("crates/south-provider-api/src/more.rs", "\n")
        self.commit()
        code, _, err = self.run_check()
        self.assertEqual(code, 1)
        self.assertIn("south-provider-api keeps version 0.49.0 from v0.50.0 but 2 files", err)
        self.assertIn("crates/south-provider-api/src/lib.rs", err)
        self.assertIn("crates/south-provider-api/src/more.rs", err)
        self.assertNotIn("south-contracts keeps", err)

    def test_a_tests_only_change_passes_listing_the_exempt_files(self) -> None:
        self.write("crates/south-contracts/tests/a.rs", "\n")
        self.write("crates/south-contracts/tests/support/b.rs", "\n")
        self.commit()
        code, out, err = self.run_check()
        self.assertEqual(code, 0, err)
        self.assertIn(
            "south-contracts: 2 changed files under crates/south-contracts/tests/ are exempt:\n"
            "  crates/south-contracts/tests/a.rs\n  crates/south-contracts/tests/support/b.rs",
            out,
        )
        self.assertIn("south-contracts 0.49.0: unchanged since v0.50.0 outside tests/", out)

    def test_tests_plus_another_change_fails_naming_only_the_non_exempt_files(self) -> None:
        self.write("crates/south-contracts/tests/a.rs", "\n")
        self.write("crates/south-contracts/src/lib.rs", "pub fn g() {}\n")
        self.commit()
        code, out, err = self.run_check()
        self.assertEqual(code, 1)
        self.assertIn(
            "south-contracts keeps version 0.49.0 from v0.50.0 but 1 files under crates/south-contracts changed", err
        )
        self.assertIn("crates/south-contracts/src/lib.rs", err)
        self.assertNotIn("tests/a.rs", err)
        self.assertIn("crates/south-contracts/tests/a.rs", out)

    def test_a_fixture_outside_tests_without_bump_fails(self) -> None:
        # src/ may include_str! a fixture, so only tests/ is exempt.
        self.write("crates/south-component-conformance/fixtures-gemini/case.json", "{}\n")
        self.commit()
        code, _, err = self.run_check()
        self.assertEqual(code, 1)
        self.assertIn("crates/south-component-conformance/fixtures-gemini/case.json", err)

    def test_a_directory_merely_named_like_tests_is_not_exempt(self) -> None:
        self.write("crates/south-contracts/src/tests/x.rs", "\n")
        self.write("crates/south-contracts/tests-data/y.json", "\n")
        self.commit()
        code, _, err = self.run_check()
        self.assertEqual(code, 1)
        self.assertIn("crates/south-contracts/src/tests/x.rs", err)
        self.assertIn("crates/south-contracts/tests-data/y.json", err)

    def test_a_manifest_only_change_without_bump_fails(self) -> None:
        self.write(
            "crates/south-contracts/Cargo.toml",
            crate_manifest("south-contracts", "0.49.0") + "[dependencies]\nserde = \"1\"\n",
        )
        self.commit()
        code, _, err = self.run_check()
        self.assertEqual(code, 1)
        self.assertIn("crates/south-contracts/Cargo.toml", err)

    def test_changed_with_bump_passes(self) -> None:
        self.write("crates/south-contracts/src/lib.rs", "pub fn g() {}\n")
        self.write("crates/south-contracts/Cargo.toml", crate_manifest("south-contracts", "0.49.1"))
        self.commit()
        code, out, err = self.run_check()
        self.assertEqual(code, 0, err)
        self.assertIn("south-contracts: 0.49.0 -> 0.49.1, 2 files changed", out)

    def test_bump_without_change_passes(self) -> None:
        self.write(
            "crates/south-component-conformance/Cargo.toml",
            crate_manifest("south-component-conformance", "0.50.0"),
        )
        self.commit()
        code, out, err = self.run_check()
        self.assertEqual(code, 0, err)
        self.assertIn("south-component-conformance: 0.49.0 -> 0.50.0", out)

    def test_no_previous_tag_passes_with_notice(self) -> None:
        self.git("tag", "-d", "v0.50.0")
        code, out, err = self.run_check()
        self.assertEqual(code, 0, err)
        self.assertIn("no release tag earlier than v0.51.0", out)

    def test_missing_crate_directory_fails(self) -> None:
        self.git("rm", "-q", "-r", "crates/south-contracts")
        self.commit()
        code, _, err = self.run_check()
        self.assertEqual(code, 1)
        self.assertIn("south-contracts: crates/south-contracts/Cargo.toml is missing at HEAD", err)

    def test_the_tag_names_the_release_and_later_tags_are_ignored(self) -> None:
        # release.yml checks out the tag being released; a newer tag elsewhere is not the baseline.
        self.write("crates/south-contracts/src/lib.rs", "pub fn g() {}\n")
        self.commit_and_tag("v0.51.0")
        self.git("tag", "v0.52.0")
        code, out, err = self.run_check("--tag=v0.51.0")
        self.assertEqual(code, 1)
        self.assertIn("comparing guest-linked crates at HEAD with v0.50.0", out)
        self.assertIn("south-contracts keeps version 0.49.0 from v0.50.0", err)

    def test_the_previous_release_may_inherit_the_workspace_version(self) -> None:
        # A release before Q47: the crates inherited the workspace version, 0.50.0 here.
        for crate in CRATES:
            self.write(f"crates/{crate}/Cargo.toml", crate_manifest(crate, None))
        self.git("tag", "-d", "v0.50.0")
        self.write("Cargo.toml", '[workspace.package]\nversion = "0.50.0"\n')
        self.commit_and_tag("v0.50.0")
        for crate in CRATES:
            self.write(f"crates/{crate}/Cargo.toml", crate_manifest(crate, "0.50.0"))
        self.write("Cargo.toml", '[workspace.package]\nversion = "0.51.0"\n')
        self.commit()
        code, out, err = self.run_check()
        self.assertEqual(code, 1)
        # Same version, changed manifest: the switch to an own version must move it.
        self.assertIn("south-contracts keeps version 0.50.0 from v0.50.0", err)

    def test_inheriting_the_workspace_version_at_the_release_fails(self) -> None:
        self.write("crates/south-provider-api/Cargo.toml", crate_manifest("south-provider-api", None))
        self.commit()
        code, _, err = self.run_check()
        self.assertEqual(code, 1)
        self.assertIn("south-provider-api: crates/south-provider-api/Cargo.toml at HEAD inherits", err)

    def test_a_crate_new_since_the_previous_release_passes(self) -> None:
        self.git("tag", "-d", "v0.50.0")
        self.git("rm", "-q", "-r", "--cached", "crates/south-component-conformance")
        self.git("commit", "-q", "-m", "drop")
        self.git("tag", "v0.50.0")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "restore")
        code, out, err = self.run_check()
        self.assertEqual(code, 0, err)
        self.assertIn("south-component-conformance 0.49.0: not in v0.50.0, new crate", out)

    def test_a_shallow_clone_fails(self) -> None:
        clone = Path(self._tmp.name + "-shallow")
        self.addCleanup(subprocess.run, ["rm", "-rf", str(clone)], check=False)
        subprocess.run(
            ["git", "clone", "-q", "--depth=1", f"file://{self.repo}", str(clone)], check=True, capture_output=True
        )
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr), contextlib.redirect_stdout(io.StringIO()):
            code = check_crate_versions.main([f"--repo={clone}"])
        self.assertEqual(code, 1)
        self.assertIn("shallow", stderr.getvalue())

    def test_a_git_error_fails(self) -> None:
        code, _, err = self.run_check("--ref=no-such-ref")
        self.assertEqual(code, 1)
        self.assertIn("git rev-parse", err)

    def test_unreleased_passes_for_a_new_release(self) -> None:
        code, out, err = self.run_check("--unreleased")
        self.assertEqual(code, 0, err)
        self.assertIn("with v0.50.0", out)

    def test_unreleased_refuses_a_workspace_version_already_tagged(self) -> None:
        # A release branch that forgot the workspace bump would otherwise compare with the release before last.
        self.git("tag", "v0.51.0")
        code, _, err = self.run_check("--unreleased")
        self.assertEqual(code, 1)
        self.assertIn("v0.51.0 is already released", err)

    def test_a_non_release_tag_fails(self) -> None:
        code, _, err = self.run_check("--tag=main")
        self.assertEqual(code, 1)
        self.assertIn("main is not a vX.Y.Z release tag", err)

    def test_not_a_repository_fails(self) -> None:
        with tempfile.TemporaryDirectory() as empty:
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                code = check_crate_versions.main([f"--repo={empty}"])
        self.assertEqual(code, 1)
        self.assertIn("error: git", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
