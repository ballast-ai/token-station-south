#!/usr/bin/env python3
"""Decides how much of CI a run needs, and prints the component test filter.

    python3 scripts/ci_scope.py --event <github.event_name> --head-ref <github.head_ref>
    python3 scripts/ci_scope.py --component-filter

The first form prints `scope=docs` or `scope=full` for `$GITHUB_OUTPUT` (CI scoped-runs record C1,
docs/design/2026-10-29-ci-scoped-runs-and-component-job.md). A pull request whose merge commit
changes only paths under `docs/` or Markdown files at the repository root is `docs`: no crate
embeds or reads those files, so the Rust steps would only repeat the previous result. Everything
else is `full`: every event other than a pull request, a `release/*` pull request, and every case
the script cannot decide (no parent commit, a failing `git`, an empty diff). The rule only ever
removes work whose answer is known.

The second form prints the filter of the `prebuild-components` setup script in
`.config/nextest.toml`, the one place that names the tests which load real components (record C2).
CI's `quality` job runs every other test and the `components` job runs these.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tomllib
from pathlib import Path

PREBUILD_SCRIPT = "prebuild-components"
COMPONENT_PACKAGES = ("south-component-conformance", "south-provider-runtime")
_FILTER = "$(python3 scripts/ci_scope.py --component-filter)"
QUALITY_TEST_COMMAND = f'cargo nextest run --workspace --all-features -E "not ({_FILTER})"'
COMPONENTS_TEST_COMMAND = (
    "cargo nextest run "
    + " ".join(f"-p {package}" for package in COMPONENT_PACKAGES)
    + f' --all-features -E "{_FILTER}"'
)


def is_documentation(path: str) -> bool:
    """Whether nothing compiles or reads `path`: under `docs/`, or a Markdown file at the root."""
    return path.startswith("docs/") or ("/" not in path and path.endswith(".md"))


def decide(event: str, head_ref: str, paths: list[str] | None) -> tuple[str, str]:
    """The scope (`docs` or `full`) and the reason for it."""
    if event != "pull_request":
        return "full", f"event {event!r} always runs in full"
    if head_ref.startswith("release/"):
        return "full", "a release pull request always runs in full"
    if not paths:
        return "full", "the changed paths are unknown"
    other = [path for path in paths if not is_documentation(path)]
    if other:
        return "full", f"{len(other)} changed path(s) outside docs/ and root-level Markdown, e.g. {other[0]}"
    return "docs", f"all {len(paths)} changed path(s) are documentation"


def changed_paths(root: Path) -> list[str] | None:
    """The paths the checked-out commit changes against its first parent, or None if unknown.

    A pull request run checks out the merge commit, whose first parent is the base branch, so
    these are the pull request's changes. Renames are listed as a deletion and an addition, so a
    file moved into `docs/` still names its old path.
    """
    try:
        result = subprocess.run(
            ["git", "diff", "--no-renames", "--name-only", "-z", "HEAD^1", "HEAD"],
            cwd=root,
            capture_output=True,
            check=False,
        )
    except OSError:
        return None
    if result.returncode != 0:
        return None
    return [path for path in result.stdout.decode("utf-8", "surrogateescape").split("\0") if path]


def component_filter(nextest_config: Path) -> str:
    """The filter of the `prebuild-components` setup script in the nextest configuration."""
    config = tomllib.loads(nextest_config.read_text())
    for entry in config.get("profile", {}).get("default", {}).get("scripts", []):
        if entry.get("setup") == PREBUILD_SCRIPT:
            return entry["filter"]
    raise ValueError(f"{nextest_config} has no profile.default.scripts entry for {PREBUILD_SCRIPT!r}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--component-filter", action="store_true", help="print the component test filter")
    parser.add_argument("--event", default="", help="github.event_name")
    parser.add_argument("--head-ref", default="", help="github.head_ref (empty outside pull requests)")
    arguments = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if arguments.component_filter:
        print(component_filter(root / ".config/nextest.toml"))
        return 0
    paths = changed_paths(root) if arguments.event == "pull_request" else None
    scope, reason = decide(arguments.event, arguments.head_ref, paths)
    print(f"scope={scope}")
    print(f"scope {scope}: {reason}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
