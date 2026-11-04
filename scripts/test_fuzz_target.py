"""Tests for scripts/fuzz-target.sh.

Run with `python3 -m unittest discover -s scripts -p 'test_*.py'` from the repository root.
"""

from __future__ import annotations

import base64
import os
import shutil
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "fuzz-target.sh"

# A stub `rustup` that records its arguments, writes the artifacts named by STUB_ARTIFACTS into
# the target's artifact directory, and exits with STUB_EXIT.
STUB_RUSTUP = """#!/usr/bin/env bash
printf '%s\\n' "$@" > "${STUB_LOG}"
target=""
for arg in "$@"; do
  case "${arg}" in
    safe_headers|contract_parsers|local_write_parser) target="${arg}" ;;
  esac
done
mkdir -p "fuzz/artifacts/${target}"
for name in ${STUB_ARTIFACTS:-}; do
  printf 'input of %s' "${name}" > "fuzz/artifacts/${target}/${name}"
done
exit "${STUB_EXIT:-0}"
"""


class FuzzTargetTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        (self.tmp / "scripts").mkdir()
        shutil.copy(SCRIPT, self.tmp / "scripts" / SCRIPT.name)
        seeds = self.tmp / "fuzz" / "seeds" / "contract_parsers"
        seeds.mkdir(parents=True)
        (seeds / "seed-1").write_text("seed")
        bin_dir = self.tmp / "bin"
        bin_dir.mkdir()
        rustup = bin_dir / "rustup"
        rustup.write_text(STUB_RUSTUP)
        rustup.chmod(rustup.stat().st_mode | stat.S_IXUSR)
        self.log = self.tmp / "rustup-args"
        self.env = {"PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}", "STUB_LOG": str(self.log)}

    def run_script(self, target: str, **env: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(self.tmp / "scripts" / SCRIPT.name), target, "600"],
            cwd=self.tmp,
            env={**self.env, **env},
            capture_output=True,
            text=True,
            check=False,
        )

    def test_runs_every_crash_in_fork_mode_for_the_given_time(self) -> None:
        result = self.run_script("contract_parsers")
        self.assertEqual(result.returncode, 0, result.stderr)
        arguments = self.log.read_text().splitlines()
        self.assertEqual(arguments[:3], ["run", "nightly-2026-08-15", "cargo"])
        self.assertIn("x86_64-unknown-linux-gnu", arguments)
        for flag in ("-fork=1", "-ignore_crashes=1", "-max_total_time=600"):
            self.assertIn(flag, arguments)

    def test_copies_the_seeds_into_the_corpus(self) -> None:
        self.run_script("contract_parsers")
        self.assertEqual((self.tmp / "fuzz/corpus/contract_parsers/seed-1").read_text(), "seed")

    def test_a_target_without_seeds_still_runs(self) -> None:
        result = self.run_script("safe_headers")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_every_finding_fails_the_step_and_is_printed(self) -> None:
        result = self.run_script("contract_parsers", STUB_ARTIFACTS="crash-aa timeout-bb")
        self.assertEqual(result.returncode, 1)
        for name in ("crash-aa", "timeout-bb"):
            encoded = base64.b64encode(f"input of {name}".encode()).decode()
            self.assertIn(name, result.stdout)
            self.assertIn(encoded, result.stdout)
        self.assertIn("2 finding(s)", result.stdout)

    def test_a_failure_without_a_finding_keeps_its_exit_status(self) -> None:
        result = self.run_script("contract_parsers", STUB_EXIT="101")
        self.assertEqual(result.returncode, 101)

    def test_an_unknown_target_is_refused(self) -> None:
        result = self.run_script("../etc")
        self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
