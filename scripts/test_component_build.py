"""Tests for scripts/component-build.sh and the per-package build scripts that call it.

Run with `python3 -m unittest discover -s scripts -p 'test_*.py'` from the repository root. The
behaviour tests run the script in a throwaway repository layout with a stub `cargo` on PATH, so
they compile nothing.
"""

from __future__ import annotations

import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

# No __pycache__ beside the script in the working tree.
sys.dont_write_bytecode = True

REPO = Path(__file__).resolve().parent.parent
SCRIPT = REPO / "scripts" / "component-build.sh"

# Records its arguments, then writes the wasm cargo would have written under --target-dir.
STUB_CARGO = """#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "$@" > "${STUB_LOG}"
target_dir=""
manifest=""
while [ $# -gt 0 ]; do
  case "$1" in
    --target-dir) target_dir="$2"; shift ;;
    --manifest-path) manifest="$2"; shift ;;
  esac
  shift
done
name="$(basename "$(dirname "${manifest}")")"
mkdir -p "${target_dir}/wasm32-wasip2/release"
printf 'wasm of %s' "${name}" > "${target_dir}/wasm32-wasip2/release/${name//-/_}.wasm"
"""


class ComponentBuildTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        (self.tmp / "scripts").mkdir()
        shutil.copy(SCRIPT, self.tmp / "scripts" / SCRIPT.name)
        (self.tmp / "components" / "image-qwen").mkdir(parents=True)
        (self.tmp / "components" / "image-qwen" / "Cargo.toml").write_text("")
        bin_dir = self.tmp / "bin"
        bin_dir.mkdir()
        cargo = bin_dir / "cargo"
        cargo.write_text(STUB_CARGO)
        cargo.chmod(cargo.stat().st_mode | stat.S_IXUSR)
        self.log = self.tmp / "cargo-args"
        self.env = {
            "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
            "STUB_LOG": str(self.log),
        }

    def run_script(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(self.tmp / "scripts" / SCRIPT.name), *args],
            env=self.env,
            capture_output=True,
            text=True,
            check=False,
        )

    def test_builds_into_the_shared_target_directory(self) -> None:
        result = self.run_script("image-qwen")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            self.log.read_text().splitlines(),
            [
                "build",
                "--manifest-path",
                "components/image-qwen/Cargo.toml",
                "--target",
                "wasm32-wasip2",
                "--release",
                "--target-dir",
                "target/components",
            ],
        )

    def test_copies_the_wasm_to_the_package_target_directory(self) -> None:
        result = self.run_script("image-qwen")
        self.assertEqual(result.returncode, 0, result.stderr)
        artifact = self.tmp / "components/image-qwen/target/wasm32-wasip2/release/image_qwen.wasm"
        self.assertEqual(artifact.read_text(), "wasm of image-qwen")

    def test_a_prebuilt_run_builds_nothing(self) -> None:
        self.env["SOUTH_COMPONENTS_PREBUILT"] = "1"
        result = self.run_script("image-qwen")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.log.exists())

    def test_an_unknown_package_is_refused(self) -> None:
        for bad in ("", "no-such-package", "../image-qwen"):
            with self.subTest(bad=bad):
                result = self.run_script(bad)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.log.exists())


class PackageBuildScriptsTest(unittest.TestCase):
    def test_every_package_build_script_delegates_to_the_shared_build(self) -> None:
        scripts = sorted((REPO / "scripts").glob("build-*-component.sh"))
        self.assertGreater(len(scripts), 0)
        call = re.compile(r'^exec bash "\$\(dirname "\$0"\)/component-build\.sh" ([a-z0-9-]+)$')
        for script in scripts:
            with self.subTest(script=script.name):
                body = [
                    line
                    for line in script.read_text().splitlines()
                    if line.strip() and not line.startswith("#")
                ]
                self.assertEqual(body[0], "set -euo pipefail")
                self.assertEqual(len(body), 2, "the script does one thing: call component-build.sh")
                match = call.fullmatch(body[1])
                self.assertIsNotNone(match, body[1])
                self.assertTrue((REPO / "components" / match.group(1) / "Cargo.toml").is_file())


if __name__ == "__main__":
    unittest.main()
