#!/usr/bin/env python3
"""Prepare the opt-in reqwest write-observation workspace without changing South.

The default workspace keeps the registry reqwest. Cargo patches apply at the final
root, so the generated host check declares both patched paths independently.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import sys
import subprocess
import tarfile
import tempfile
import urllib.error
import urllib.request

BUNDLE = "patches/reqwest-0.13.5-local-write"
VERSION = "0.13.5"
MAX_ARCHIVE_BYTES = 50 * 1024 * 1024
MAX_MEMBER_BYTES = 10 * 1024 * 1024
EXCLUDED = {".git", ".worktrees", "target", "__pycache__", "Cargo.lock"}
ROOT_FILES = (
    "Cargo.toml", "rust-toolchain.toml", "rustfmt.toml", "LICENSE",
    "README.md", "ARCHITECTURE.md", "CONTRIBUTING.md", "compatibility.json", "deny.toml",
)
HUNK = re.compile(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(?:.*)?\n?$")


class PreparationError(Exception):
    """A pinned input or preparation invariant failed."""


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def checked_relative(value: str) -> Path:
    if not isinstance(value, str) or not value or "\\" in value or "\x00" in value:
        raise PreparationError("Invalid relative path")
    path = PurePosixPath(value)
    if path.is_absolute() or ".." in path.parts or path == PurePosixPath("."):
        raise PreparationError("Path must stay inside its workspace")
    return Path(*path.parts)


def check_digest(data: bytes, expected: str, label: str) -> None:
    if not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise PreparationError(f"Invalid SHA-256 for {label}")
    if digest(data) != expected:
        raise PreparationError(f"SHA-256 mismatch for {label}")


def read_input(root: Path, relative: str) -> bytes:
    path = root / checked_relative(relative)
    current = path
    while current != root:
        if current.is_symlink():
            raise PreparationError("Source symlinks are not accepted")
        current = current.parent
    if not path.is_file():
        raise PreparationError(f"Missing pinned input: {relative}")
    return path.read_bytes()


def patch_path(value: str, prefix: str) -> str | None:
    value = value.split("\t", 1)[0].rstrip("\r\n")
    if value == "/dev/null":
        return None
    if not value.startswith(prefix + "/"):
        raise PreparationError("Patch paths must use a/ and b/ prefixes")
    relative = value[2:]
    checked_relative(relative)
    return relative


def apply_exact_patch(root: Path, data: bytes, allowed_sources: set[str] | None = None) -> list[str]:
    """Apply text hunks at their declared positions; never search or accept offsets."""
    try:
        lines = data.decode("utf-8").splitlines(keepends=True)
    except UnicodeError as error:
        raise PreparationError("Patches must be UTF-8 unified text") from error
    index = 0
    touched = []
    while index < len(lines):
        if not lines[index].startswith("--- "):
            if lines[index].startswith(("diff --git ", "index ", "new file mode ", "deleted file mode ")) or not lines[index].strip():
                index += 1
                continue
            raise PreparationError("Unsupported patch metadata")
        old = patch_path(lines[index][4:], "a")
        index += 1
        if index >= len(lines) or not lines[index].startswith("+++ "):
            raise PreparationError("Missing unified patch destination")
        new = patch_path(lines[index][4:], "b")
        index += 1
        if old is None and new is None or old is not None and new is not None and old != new:
            raise PreparationError("Renames and empty patch paths are not supported")
        if old is not None and allowed_sources is not None and old not in allowed_sources:
            raise PreparationError("A patched South source is not pinned in source_files")
        relative = new or old
        assert relative is not None
        path = root / checked_relative(relative)
        if old is None:
            if os.path.lexists(path):
                raise PreparationError("Patch creation would overwrite an existing file")
            original = []
        else:
            if not path.is_file() or path.is_symlink():
                raise PreparationError("Patch source is missing or is a link")
            try:
                original = path.read_text(encoding="utf-8").splitlines(keepends=True)
            except UnicodeError as error:
                raise PreparationError("Patch source is not UTF-8") from error
        result = []
        cursor = 0
        hunks = 0
        while index < len(lines) and lines[index].startswith("@@ "):
            match = HUNK.fullmatch(lines[index])
            if match is None:
                raise PreparationError("Invalid unified hunk header")
            old_start, old_count, new_start, new_count = match.groups()
            old_count = int(old_count or 1)
            new_count = int(new_count or 1)
            old_index = int(old_start) - (1 if old_count else 0)
            new_index = int(new_start) - (1 if new_count else 0)
            if old_index < cursor or old_index > len(original):
                raise PreparationError("Patch hunk position does not match its source")
            result.extend(original[cursor:old_index])
            if len(result) != new_index:
                raise PreparationError("Patch destination position is inconsistent")
            index += 1
            pieces = []
            while index < len(lines) and lines[index][:1] in {" ", "+", "-", "\\"}:
                if lines[index].startswith("--- ") and len([p for p in pieces if p[0] in " -"]) == old_count:
                    break
                line = lines[index]
                if line.startswith("\\ No newline at end of file"):
                    if not pieces:
                        raise PreparationError("Invalid no-newline marker")
                    kind, text = pieces[-1]
                    pieces[-1] = (kind, text.removesuffix("\n"))
                elif line.startswith("\\"):
                    raise PreparationError("Unsupported unified patch marker")
                else:
                    pieces.append((line[0], line[1:]))
                index += 1
            expected = [text for kind, text in pieces if kind in " -"]
            replacement = [text for kind, text in pieces if kind in " +"]
            if len(expected) != old_count or len(replacement) != new_count:
                raise PreparationError("Unified hunk counts do not match")
            if original[old_index:old_index + old_count] != expected:
                raise PreparationError("Patch context differs at the declared position")
            result.extend(replacement)
            cursor = old_index + old_count
            hunks += 1
        if not hunks:
            raise PreparationError("A patch file has no text hunks")
        result.extend(original[cursor:])
        if new is None:
            if result:
                raise PreparationError("A deletion patch left content")
            path.unlink()
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("".join(result), encoding="utf-8")
        touched.append(relative)
    if not touched:
        raise PreparationError("The patch is empty")
    return touched


def extract_archive(archive_path: Path, destination: Path) -> None:
    """Extract regular files under the single pinned package prefix."""
    prefix = f"reqwest-{VERSION}"
    total = 0
    seen = set()
    with tarfile.open(archive_path, "r:gz") as archive:
        for member in archive:
            relative = checked_relative(member.name)
            if relative.parts[0] != prefix:
                raise PreparationError("Archive package prefix differs from the pinned version")
            if not (member.isdir() or member.isfile()):
                raise PreparationError("Archive links and special files are not accepted")
            if relative in seen:
                raise PreparationError("Duplicate archive member")
            seen.add(relative)
            if len(relative.parts) == 1:
                if not member.isdir():
                    raise PreparationError("Archive package root must be a directory")
                continue
            target = destination.joinpath(*relative.parts[1:])
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            if member.size > MAX_MEMBER_BYTES:
                raise PreparationError("Archive member exceeds its size bound")
            total += member.size
            if total > MAX_ARCHIVE_BYTES:
                raise PreparationError("Expanded archive exceeds its size bound")
            stream = archive.extractfile(member)
            if stream is None:
                raise PreparationError("Archive member cannot be read")
            content = stream.read(MAX_MEMBER_BYTES + 1)
            if len(content) != member.size:
                raise PreparationError("Archive member length differs from its header")
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(content)
            target.chmod(0o755 if member.mode & 0o111 else 0o644)
    if not (destination / "Cargo.toml").is_file():
        raise PreparationError("Pinned archive has no Cargo.toml")


def copy_workspace(root: Path, destination: Path) -> dict[str, str]:
    copied = {}
    for name in ROOT_FILES:
        if (root / name).is_file():
            content = read_input(root, name)
            (destination / name).write_bytes(content)
            copied[name] = digest(content)
    for relative in ["crates", "patches"]:
        source = root / relative
        if not source.is_dir() or source.is_symlink():
            raise PreparationError("Required source directory is missing or is a link")
        for current, directories, files in os.walk(source, followlinks=False):
            directories[:] = [name for name in directories if name not in EXCLUDED]
            for name in directories + files:
                if (Path(current) / name).is_symlink():
                    raise PreparationError("Source symlinks are not accepted")
            target = destination / Path(current).relative_to(root)
            target.mkdir(parents=True, exist_ok=True)
            for name in files:
                if name not in EXCLUDED:
                    path = target / name
                    shutil.copyfile(Path(current) / name, path)
                    copied[str(path.relative_to(destination))] = digest(path.read_bytes())
    return dict(sorted(copied.items()))


def create_roots(destination: Path, smoke: bytes | None) -> None:
    cargo = destination / "Cargo.toml"
    text = cargo.read_text(encoding="utf-8")
    # Only inspect the pinned workspace's two sections; no TOML dependency is needed.
    sections = dict(re.findall(r"^\[([^]\n]+)\][ \t]*\n(.*?)(?=^\[|\Z)", text, re.MULTILINE | re.DOTALL))
    if re.search(r"^exclude\s*=", sections.get("workspace", ""), re.MULTILINE) or "patch.crates-io" in sections:
        raise PreparationError("Source workspace already has unsupported exclusion or patch settings")
    found = re.findall(r'^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"[ \t]*$', sections.get("workspace.package", ""), re.MULTILINE)
    if len(found) != 1:
        raise PreparationError("South workspace has no supported package version")
    version = found[0]
    if text.count("[workspace]") != 1:
        raise PreparationError("South workspace header is ambiguous")
    text = text.replace("[workspace]", '[workspace]\nexclude = ["reqwest", "host-check"]', 1)
    text += '\n[patch.crates-io]\nreqwest = { path = "reqwest" }\nsouth-transport-reqwest = { path = "crates/south-transport-reqwest" }\n'
    cargo.write_text(text, encoding="utf-8")
    host = destination / "host-check"
    (host / "src").mkdir(parents=True)
    (host / "Cargo.toml").write_text(
        '[workspace]\n[package]\nname = "south-observed-host-check"\nversion = "0.1.0"\n'
        'edition = "2024"\npublish = false\n[dependencies]\n'
        f'south-transport-reqwest = "={version}"\n'
        'south-core = { path = "../crates/south-core" }\n'
        'south-contracts = { path = "../crates/south-contracts" }\n'
        '[patch.crates-io]\nreqwest = { path = "../reqwest" }\n'
        'south-transport-reqwest = { path = "../crates/south-transport-reqwest" }\n',
        encoding="utf-8",
    )
    if smoke is None:
        smoke = b'fn main() { let _ = std::mem::size_of::<south_transport_reqwest::ReqwestTransportConfigV1>(); }\n'
    (host / "src/main.rs").write_bytes(smoke)
    (destination / "OBSERVATION-WORKSPACE.md").write_text(
        '# Prepared optional observation workspace\n\n'
        'This generated workspace is not a released SDK. It does not modify the source checkout.\n'
        'Both this root and `host-check/Cargo.toml` explicitly patch reqwest and the transport;\n'
        'Cargo patches do not propagate from a dependency into its host. The host check also\n'
        'names south-core and south-contracts from this snapshot so its smoke can check type identity.\n'
        'An existing Git-source host must move all South crates to one path snapshot, or apply a\n'
        'complete patch for that Git source. This fixture does not verify a live gateway migration.\n\n'
        '```sh\ncargo test -p south-transport-reqwest --lib\n'
        'cargo check --manifest-path host-check/Cargo.toml\n```\n\n'
        'The host check is compile-only and never sends a network request. A pinned bundle smoke\n'
        'may exercise the optional constructor; the fallback only resolves the public config type.\n'
        'Local flush completion does not prove remote application receipt or queue admission.\n',
        encoding="utf-8",
    )


def source_git_state(root: Path) -> dict | None:
    """Inspect an actual checkout without treating a patch-base label as its HEAD."""
    def git(*arguments: str) -> subprocess.CompletedProcess:
        return subprocess.run(["git", "--no-optional-locks", "-C", str(root), *arguments],
                              capture_output=True, text=True, timeout=10, check=False)
    try:
        top = git("rev-parse", "--show-toplevel")
        if top.returncode or Path(top.stdout.strip()).resolve() != root:
            return None
        head = git("rev-parse", "HEAD")
        status = git("status", "--porcelain=v1", "--untracked-files=all")
        if head.returncode or status.returncode:
            return None
        return {"head": head.stdout.strip(), "dirty": bool(status.stdout.strip())}
    except (OSError, subprocess.TimeoutExpired):
        return None


def tree_hashes(root: Path) -> dict[str, str]:
    return {str(path.relative_to(root)): digest(path.read_bytes())
            for path in sorted(root.rglob("*")) if path.is_file()}


def prepare(root: Path, output: Path, archive: Path | None = None) -> dict:
    root = root.resolve()
    output = output.absolute()
    if os.path.lexists(output):
        raise PreparationError("Output already exists; refusing to overwrite it")
    if output.resolve() == root or any((root / name) in output.resolve().parents for name in ["crates", "patches"]):
        raise PreparationError("Output must not overlap copied South sources")
    try:
        manifest = json.loads(read_input(root, f"{BUNDLE}/manifest.json"))
        if manifest.get("schema_version") != 1 or manifest.get("reqwest_version") != VERSION:
            raise PreparationError("Unsupported observation bundle manifest")
        sources = manifest["source_files"]
        if not isinstance(sources, dict) or "Cargo.toml" not in sources:
            raise PreparationError("Bundle must pin its source workspace")
        for relative, expected in sources.items():
            check_digest(read_input(root, relative), expected, relative)
        patches = {}
        for name in ["reqwest.patch", "south-transport.patch"]:
            patches[name] = read_input(root, f"{BUNDLE}/{name}")
            check_digest(patches[name], manifest["patch_sha256"][name], name)
        smoke_scope = manifest.get("host_smoke_scope", "basic_config")
        if smoke_scope not in {"basic_config", "local_write_constructor"}:
            raise PreparationError("Unsupported host smoke scope")
        if smoke_scope == "local_write_constructor" and "host_smoke_sha256" not in manifest:
            raise PreparationError("Local-write constructor smoke must be checksum-pinned")
        smoke = None
        if "host_smoke_sha256" in manifest:
            smoke = read_input(root, f"{BUNDLE}/host-smoke.rs")
            check_digest(smoke, manifest["host_smoke_sha256"], "host-smoke.rs")
    except (KeyError, TypeError, ValueError, OSError) as error:
        raise PreparationError("Invalid or unreadable observation bundle") from error
    actual_git_state = source_git_state(root)
    preparer_digest = digest(Path(__file__).read_bytes())
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=f".{output.name}.", dir=output.parent))
    try:
        if archive is None:
            archive = staging / "source.crate"
            url = f"https://static.crates.io/crates/reqwest/reqwest-{VERSION}.crate"
            with urllib.request.urlopen(url, timeout=30) as response:
                data = response.read(MAX_ARCHIVE_BYTES + 1)
            if len(data) > MAX_ARCHIVE_BYTES:
                raise PreparationError("Downloaded crate exceeds its size bound")
            archive.write_bytes(data)
        elif archive.stat().st_size > MAX_ARCHIVE_BYTES:
            raise PreparationError("Crate archive exceeds its size bound")
        check_digest(archive.read_bytes(), manifest["crate_sha256"], "reqwest crate archive")
        copied_inputs = copy_workspace(root, staging)
        copied_tree_digest = digest(json.dumps(copied_inputs, sort_keys=True, separators=(",", ":")).encode())
        extract_archive(archive, staging / "reqwest")
        if archive.parent == staging:
            archive.unlink()
        apply_exact_patch(staging / "reqwest", patches["reqwest.patch"])
        apply_exact_patch(staging, patches["south-transport.patch"], set(sources))
        create_roots(staging, smoke)
        for relative, expected in manifest.get("generated_files", {}).items():
            check_digest(read_input(staging, relative), expected, "generated " + relative)
        result = {
            "schema_version": 1,
            "reqwest_version": VERSION,
            "south_base_revision": manifest.get("south_base_revision"),
            "source_git_state": actual_git_state,
            "prepare_script_sha256": preparer_digest,
            "copied_input_files": copied_inputs,
            "copied_input_tree_sha256": copied_tree_digest,
            "inputs": {"crate_sha256": manifest["crate_sha256"],
                       "patch_sha256": manifest["patch_sha256"], "source_files": sources,
                       "host_smoke_sha256": manifest.get("host_smoke_sha256")},
            "host_smoke_scope": smoke_scope,
            "host_smoke_source": "bundle_pinned" if smoke is not None else "basic_config",
            "tests_executed": False,
            "generated_files": tree_hashes(staging),
        }
        (staging / "prepared-manifest.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        if os.path.lexists(output):
            raise PreparationError("Output appeared during preparation; refusing to overwrite it")
        staging.rename(output)
        return result
    except (OSError, ValueError, tarfile.TarError, urllib.error.URLError) as error:
        raise PreparationError("Workspace preparation failed; no prepared output was published") from error
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="New output directory; it must not already exist")
    parser.add_argument("--crate-archive", type=Path, help="Optional local reqwest-0.13.5 .crate archive")
    args = parser.parse_args()
    try:
        result = prepare(Path(__file__).resolve().parent.parent, args.output, args.crate_archive)
    except PreparationError as error:
        print(f"Preparation failed: {error}", file=sys.stderr)
        return 1
    print(f"Prepared checksum-verified workspace ({len(result['generated_files'])} files); no tests or API calls were run")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
