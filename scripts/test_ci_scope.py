"""Tests for scripts/ci_scope.py.

Run with `python3 -m unittest discover -s scripts -p 'test_*.py'` from the repository root.
"""

from __future__ import annotations

import fnmatch
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

# No __pycache__ beside the script in the working tree.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import ci_scope  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent


class DecideTest(unittest.TestCase):
    def test_every_event_other_than_a_pull_request_runs_in_full(self) -> None:
        for event in ("push", "schedule", "workflow_dispatch", ""):
            with self.subTest(event=event):
                self.assertEqual(ci_scope.decide(event, "", ["docs/a.md"])[0], "full")

    def test_a_release_pull_request_runs_in_full(self) -> None:
        self.assertEqual(ci_scope.decide("pull_request", "release/0.84.0", ["docs/a.md"])[0], "full")

    def test_only_documentation_paths_are_docs(self) -> None:
        for paths in (
            ["docs/design/a.md"],
            ["docs/upstreams/b.md", "docs/releases.md"],
            ["README.md"],
            ["SECURITY.md", "docs/a.md"],
        ):
            with self.subTest(paths=paths):
                self.assertEqual(ci_scope.decide("pull_request", "x", paths)[0], "docs")

    def test_any_other_path_runs_in_full(self) -> None:
        for paths in (
            ["docs/a.md", "crates/south-core/src/lib.rs"],
            ["crates/south-component-conformance/fixtures-openai-responses/README.md"],
            ["components/provider-anthropic/README.md"],
            [".github/workflows/ci.yml"],
            [".github/notes.md"],
            ["docs"],
            ["Cargo.toml"],
        ):
            with self.subTest(paths=paths):
                self.assertEqual(ci_scope.decide("pull_request", "x", paths)[0], "full")

    def test_no_answer_from_git_runs_in_full(self) -> None:
        self.assertEqual(ci_scope.decide("pull_request", "x", None)[0], "full")
        self.assertEqual(ci_scope.decide("pull_request", "x", [])[0], "full")


class ChangedPathsTest(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = self.temporary_directory()
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "test")
        self.write("crates/a/src/lib.rs", "pub fn a() {}\n")
        self.write("docs/old.md", "old\n")
        self.commit("first")

    def temporary_directory(self) -> Path:
        directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, directory)
        return directory

    def git(self, *args: str) -> None:
        subprocess.run(["git", *args], cwd=self.repo, check=True, capture_output=True)

    def write(self, path: str, text: str) -> None:
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def commit(self, message: str) -> None:
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def test_lists_the_paths_of_the_last_commit(self) -> None:
        self.write("docs/old.md", "new\n")
        self.write("docs/café.md", "new\n")
        self.commit("second")
        self.assertEqual(sorted(ci_scope.changed_paths(self.repo)), ["docs/café.md", "docs/old.md"])

    def test_a_rename_lists_both_sides(self) -> None:
        self.git("mv", "crates/a/src/lib.rs", "docs/lib.rs")
        self.commit("second")
        paths = ci_scope.changed_paths(self.repo)
        self.assertIn("crates/a/src/lib.rs", paths)
        self.assertIn("docs/lib.rs", paths)

    def test_a_commit_without_a_parent_gives_no_answer(self) -> None:
        self.assertIsNone(ci_scope.changed_paths(self.repo))

    def test_a_directory_outside_git_gives_no_answer(self) -> None:
        self.assertIsNone(ci_scope.changed_paths(self.temporary_directory()))


class ComponentFilterTest(unittest.TestCase):
    def temporary_directory(self) -> Path:
        directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, directory)
        return directory

    def test_reads_the_prebuild_script_filter(self) -> None:
        directory = self.temporary_directory()
        (directory / "nextest.toml").write_text(
            "[[profile.default.scripts]]\n"
            "filter = 'binary(other)'\n"
            "setup = 'other'\n"
            "[[profile.default.scripts]]\n"
            "filter = 'binary(~parity) | binary(world_v1)'\n"
            "setup = 'prebuild-components'\n"
        )
        self.assertEqual(
            ci_scope.component_filter(directory / "nextest.toml"),
            "binary(~parity) | binary(world_v1)",
        )

    def test_a_missing_prebuild_script_is_refused(self) -> None:
        directory = self.temporary_directory()
        (directory / "nextest.toml").write_text("[[profile.default.scripts]]\nfilter = 'all()'\nsetup = 'x'\n")
        with self.assertRaises(ValueError):
            ci_scope.component_filter(directory / "nextest.toml")


_MARKERS = re.compile(r"wasm32-wasip2|SOUTH_COMPONENTS_PREBUILT|component-build\.sh|build-[a-z0-9-]+-component\.sh")
_TERM = re.compile(r"binary\((~?)([A-Za-z0-9_*?\[\]-]+)\)")
_PATH_MODULE = re.compile(r'#\[path\s*=\s*"([^"]+)"\]')


def _filter_terms() -> list[tuple[bool, str]]:
    text = ci_scope.component_filter(ROOT / ".config/nextest.toml")
    terms = [(contains == "~", name) for contains, name in _TERM.findall(text)]
    rebuilt = " | ".join(f"binary({'~' if contains else ''}{name})" for contains, name in terms)
    if rebuilt != text.strip():
        raise AssertionError(f"the filter is not a union of binary() terms: {text!r}")
    return terms


def _matched(binary: str, terms: list[tuple[bool, str]]) -> bool:
    return any(name in binary if contains else fnmatch.fnmatchcase(binary, name) for contains, name in terms)


def _test_binaries() -> dict[str, tuple[str, str]]:
    """Every integration test binary: name -> (package directory, the text it compiles)."""
    binaries = {}
    for source in sorted(ROOT.glob("crates/*/tests/*.rs")):
        text = source.read_text()
        for module in _PATH_MODULE.findall(text):
            included = source.parent / module
            if included.is_file():
                text += included.read_text()
        binaries[source.stem] = (source.parent.parent.name, text)
    return binaries


class RepositoryTest(unittest.TestCase):
    def test_every_test_that_builds_a_component_is_in_the_filter(self) -> None:
        terms = _filter_terms()
        missing = [
            name
            for name, (_, text) in _test_binaries().items()
            if _MARKERS.search(text) and not _matched(name, terms)
        ]
        self.assertEqual(missing, [], "these tests build components but quality would run them")

    def test_every_filtered_test_belongs_to_a_package_the_components_job_runs(self) -> None:
        terms = _filter_terms()
        outside = [
            f"{package}::{name}"
            for name, (package, _) in _test_binaries().items()
            if _matched(name, terms) and package not in ci_scope.COMPONENT_PACKAGES
        ]
        self.assertEqual(outside, [])

    def test_every_filter_term_matches_a_test(self) -> None:
        binaries = _test_binaries()
        stale = [name for contains, name in _filter_terms() if not _matched_any(contains, name, binaries)]
        self.assertEqual(stale, [])

    def test_ci_and_the_readme_run_the_same_two_test_commands(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        readme = (ROOT / "README.md").read_text()
        for command in (ci_scope.QUALITY_TEST_COMMAND, ci_scope.COMPONENTS_TEST_COMMAND):
            with self.subTest(command=command):
                self.assertTrue(f"run: {command}\n" in workflow, f"ci.yml does not run: {command}")
                self.assertTrue(f"\n{command}\n" in readme, f"README.md does not list: {command}")
        self.assertFalse(
            "\ncargo nextest run --workspace --all-features\n" in readme,
            "README.md still lists the single nextest command",
        )


def _matched_any(contains: bool, name: str, binaries: dict[str, tuple[str, str]]) -> bool:
    return any(name in binary if contains else fnmatch.fnmatchcase(binary, name) for binary in binaries)


if __name__ == "__main__":
    unittest.main()
