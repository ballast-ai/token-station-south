"""Tests for scripts/check_msrv.py.

Run with `python3 -m unittest discover -s scripts -p 'test_*.py'` from the repository root.
"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

# No __pycache__ beside the script in the working tree.
sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_msrv  # noqa: E402


class MsrvToolchainTest(unittest.TestCase):
    def test_a_pinned_toolchain_on_the_msrv_needs_no_second_check(self) -> None:
        self.assertIsNone(check_msrv.msrv_toolchain("1.96.0", "1.96"))
        self.assertIsNone(check_msrv.msrv_toolchain("1.96.2", "1.96"))
        self.assertIsNone(check_msrv.msrv_toolchain("1.96.0", "1.96.0"))

    def test_a_newer_pinned_toolchain_checks_the_msrv(self) -> None:
        self.assertEqual(check_msrv.msrv_toolchain("1.97.0", "1.96"), "1.96.0")
        self.assertEqual(check_msrv.msrv_toolchain("1.97.0", "1.96.1"), "1.96.1")

    def test_a_floating_channel_checks_the_msrv(self) -> None:
        self.assertEqual(check_msrv.msrv_toolchain("stable", "1.96"), "1.96.0")
        self.assertEqual(check_msrv.msrv_toolchain("nightly-2026-08-15", "1.96"), "1.96.0")

    def test_a_malformed_msrv_is_refused(self) -> None:
        for bad in ("", "1", "1.x", "v1.96", "1.96.0.1"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                check_msrv.msrv_toolchain("1.96.0", bad)


class RepositoryFilesTest(unittest.TestCase):
    def test_reads_the_channel_and_the_workspace_msrv(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.97.0"\n')
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = []\n\n[workspace.package]\nrust-version = "1.96"\n'
            )
            self.assertEqual(check_msrv.read_versions(root), ("1.97.0", "1.96"))

    def test_this_repository_pins_its_msrv(self) -> None:
        # The repository itself: CONTRIBUTING.md states "MSRV 1.96, exact toolchain 1.96.0".
        root = Path(__file__).resolve().parent.parent
        channel, msrv = check_msrv.read_versions(root)
        self.assertIsNone(check_msrv.msrv_toolchain(channel, msrv))


if __name__ == "__main__":
    unittest.main()
