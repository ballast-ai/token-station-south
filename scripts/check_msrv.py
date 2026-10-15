#!/usr/bin/env python3
"""Checks the workspace on its MSRV, unless the pinned toolchain already is the MSRV.

    python3 scripts/check_msrv.py

The workspace states its MSRV as `rust-version` in `[workspace.package]` and pins an exact
toolchain in `rust-toolchain.toml`. While the pinned toolchain is a patch release of the MSRV, the
Clippy step has already compiled every target with it, and a second `cargo check` on the same
compiler proves nothing. This script then says so and exits 0. Once the pinned toolchain moves
ahead of the MSRV (or floats), it runs `rustup run <msrv> cargo check --workspace --all-targets`,
so the MSRV cannot drift silently.
"""

from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path

_VERSION = re.compile(r"(\d+)\.(\d+)(?:\.(\d+))?")


def msrv_toolchain(channel: str, msrv: str) -> str | None:
    """The toolchain to check the MSRV with, or None when `channel` is a release of the MSRV."""
    wanted = _VERSION.fullmatch(msrv)
    if wanted is None:
        raise ValueError(f"rust-version {msrv!r} is not MAJOR.MINOR or MAJOR.MINOR.PATCH")
    major, minor, patch = wanted.group(1), wanted.group(2), wanted.group(3) or "0"
    pinned = _VERSION.fullmatch(channel)
    if pinned is not None and (pinned.group(1), pinned.group(2)) == (major, minor):
        return None
    return f"{major}.{minor}.{patch}"


def read_versions(root: Path) -> tuple[str, str]:
    """The pinned toolchain channel and the workspace `rust-version`."""
    toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text())
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    return toolchain["toolchain"]["channel"], manifest["workspace"]["package"]["rust-version"]


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    channel, msrv = read_versions(root)
    toolchain = msrv_toolchain(channel, msrv)
    if toolchain is None:
        print(
            f"MSRV {msrv}: the pinned toolchain {channel} is a release of it, so the Clippy step "
            "has already compiled every target on the MSRV; no second check"
        )
        return 0
    print(f"MSRV {msrv}: the pinned toolchain is {channel}; checking with {toolchain}")
    command = ["rustup", "run", toolchain, "cargo", "check", "--workspace", "--all-targets"]
    return subprocess.run(command, cwd=root, check=False).returncode


if __name__ == "__main__":
    sys.exit(main())
