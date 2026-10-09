#!/usr/bin/env python3
"""The Q47 release check (host-zero-vendor-boundary §13.12): a guest-linked crate that changed since the
previous release must carry a new version.

`south-contracts`, `south-provider-api` and `south-component-conformance` carry versions of their own,
independent of `[workspace.package] version`, so that a release which leaves them alone re-identifies no
package. The other half of that rule is that a release which does change one of them moves its version: a
host's `Cargo.lock` names these crates by version, and an unchanged version over changed source would tell
the host it links the same crate it linked before. Nothing else catches that lie. The digest-stability check
(`release_index.py compare`) fails the packages, whose bytes change, but not the crate.

The check compares each crate's directory at --ref with the latest `vX.Y.Z` tag earlier than the release
being cut. When any file under the directory differs (its `Cargo.toml` included) and the crate's
`package.version` did not change, the release fails, naming the crate and the changed files. A version that
moved without a source change is allowed. With no earlier release tag the check is skipped with a notice.

Files under the crate's `tests/` directory are exempt (lv, 2026-10-09): integration tests are not compiled into
any component, so they do not change component bytes. A change only there passes with a notice that lists the
exempted files. Nothing else is exempt: fixtures and other directories stay strict, because `src/` may embed them
with `include_str!` or `include_bytes!`.

Fail-closed: any git error, a shallow clone (whose missing tags would read as "no earlier release"), a crate
directory or manifest missing at --ref, or a version that cannot be read fails the check.

  scripts/check_crate_versions.py                      # the release named by the workspace version, at HEAD
  scripts/check_crate_versions.py --tag vX.Y.Z         # release.yml: the tag being released
  scripts/check_crate_versions.py --unreleased         # ci.yml on release/* pull requests: the workspace
                                                       # version must name a release not yet tagged

Standard library only, like release_index.py, whose previous-tag rule it reuses.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tomllib
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))

from release_index import ReleaseIndexError, previous_tag  # noqa: E402

# The workspace crates every component links (shipped_packages_v1 enforces that no other is linked).
GUEST_LINKED_CRATES = ("south-contracts", "south-provider-api", "south-component-conformance")


class CrateVersionError(Exception):
    """A refusal; the message names what to fix."""


def git(repo: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ["git", "-C", str(repo), *args], capture_output=True, text=True, check=False
        )
    except OSError as error:
        raise CrateVersionError(f"cannot run git: {error}") from error
    if result.returncode != 0:
        raise CrateVersionError(f"git {' '.join(args)} failed ({result.returncode}): {result.stderr.strip()}")
    return result.stdout


def read_toml(repo: Path, ref: str, path: str) -> dict:
    text = git(repo, "show", f"{ref}:{path}")
    try:
        return tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise CrateVersionError(f"{path} at {ref} is not valid TOML: {error}") from error


def workspace_version(repo: Path, ref: str) -> str:
    try:
        version = read_toml(repo, ref, "Cargo.toml")["workspace"]["package"]["version"]
    except (KeyError, TypeError) as error:
        raise CrateVersionError(f"Cargo.toml at {ref} has no [workspace.package] version") from error
    if not isinstance(version, str):
        raise CrateVersionError(f"Cargo.toml at {ref} has a non-string [workspace.package] version")
    return version


def path_exists(repo: Path, ref: str, path: str) -> bool:
    """Whether `path` exists at `ref`. An empty listing means absent; a git failure is an error."""
    return bool(git(repo, "ls-tree", "--name-only", ref, "--", path).strip())


def crate_version(repo: Path, ref: str, crate: str, *, inherit: bool, label: str | None = None) -> str:
    """The crate's version at `ref`. `inherit` allows `version.workspace = true` (a release before Q47)."""
    manifest = f"crates/{crate}/Cargo.toml"
    label = label or ref
    version = read_toml(repo, ref, manifest).get("package", {}).get("version")
    if isinstance(version, dict) and version.get("workspace") is True:
        if not inherit:
            raise CrateVersionError(
                f"{crate}: {manifest} at {label} inherits the workspace version; "
                "this crate must declare its own (§13.12)"
            )
        return workspace_version(repo, ref)
    if not isinstance(version, str) or not version:
        raise CrateVersionError(f"{crate}: {manifest} at {label} has no package version")
    return version


def check(repo: Path, ref: str, tag: str | None, unreleased: bool = False) -> tuple[list[str], list[str]]:
    """(log lines, violations). `unreleased` refuses a tag that already exists (a release pull request)."""
    if git(repo, "rev-parse", "--is-shallow-repository").strip() != "false":
        raise CrateVersionError("the clone is shallow, so earlier release tags may be missing; fetch full history")
    commit = git(repo, "rev-parse", "--verify", "--end-of-options", f"{ref}^{{commit}}").strip()
    if tag is None:
        tag = f"v{workspace_version(repo, commit)}"
    if unreleased and git(repo, "tag", "--list", tag).strip():
        raise CrateVersionError(
            f"{tag} is already released; a release pull request must bump [workspace.package] version first"
        )
    try:
        previous = previous_tag(tag, git(repo, "tag", "--list", "v*").split())
    except ReleaseIndexError as error:
        raise CrateVersionError(str(error)) from error
    if previous is None:
        return [f"no release tag earlier than {tag}; guest-linked crate version check skipped"], []

    log, violations = [f"comparing guest-linked crates at {ref} with {previous}"], []
    for crate in GUEST_LINKED_CRATES:
        directory = f"crates/{crate}"
        if not path_exists(repo, commit, f"{directory}/Cargo.toml"):
            raise CrateVersionError(f"{crate}: {directory}/Cargo.toml is missing at {ref}")
        current = crate_version(repo, commit, crate, inherit=False, label=ref)
        if not path_exists(repo, previous, f"{directory}/Cargo.toml"):
            log.append(f"{crate} {current}: not in {previous}, new crate")
            continue
        before = crate_version(repo, previous, crate, inherit=True)
        changed = git(repo, "diff", "--name-only", "--no-renames", previous, commit, "--", directory).splitlines()
        exempt = [path for path in changed if path.startswith(f"{directory}/tests/")]
        changed = [path for path in changed if path not in exempt]
        if exempt:
            listing = "".join(f"\n  {path}" for path in exempt)
            log.append(f"{crate}: {len(exempt)} changed files under {directory}/tests/ are exempt:{listing}")
        if not changed:
            log.append(f"{crate} {current}: unchanged since {previous} outside tests/")
        elif before != current:
            log.append(f"{crate}: {before} -> {current}, {len(changed)} files changed")
        else:
            listing = "".join(f"\n  {path}" for path in changed)
            violations.append(
                f"{crate} keeps version {current} from {previous} but {len(changed)} files under {directory} "
                f"changed; bump its version (and the requirements on it and the component lockfiles, "
                f"§13.12):{listing}"
            )
    return log, violations


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", type=Path, default=Path("."), help="the repository (default: .)")
    parser.add_argument("--ref", default="HEAD", help="the tree being released (default: HEAD)")
    parser.add_argument("--tag", help="the release tag, vX.Y.Z (default: v<workspace version at --ref>)")
    parser.add_argument(
        "--unreleased",
        action="store_true",
        help="refuse a release tag that already exists (ci.yml, on a release pull request)",
    )
    args = parser.parse_args(argv)
    try:
        log, violations = check(args.repo, args.ref, args.tag, args.unreleased)
    except CrateVersionError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    for line in log:
        print(line)
    for line in violations:
        print(f"error: {line}", file=sys.stderr)
    return 1 if violations else 0


if __name__ == "__main__":
    sys.exit(main())
