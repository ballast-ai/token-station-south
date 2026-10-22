# Documentation Simplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the oversized landing and architecture documents with concise English current-state documentation while preserving release and component detail in focused references.

**Architecture:** `README.md` becomes the onboarding entry point and `ARCHITECTURE.md` becomes the current invariant model. Historical release information moves to `docs/releases.md`, and the package catalog moves to `docs/components.md`; existing release and design records remain authoritative for detail.

**Tech Stack:** Markdown, Git, repository shell verification scripts, Cargo verification matrix.

---

### Task 1: Record the failing documentation checks

**Files:**
- Inspect: `README.md`
- Inspect: `ARCHITECTURE.md`
- Inspect: `Cargo.toml`
- Inspect: `compatibility.json`
- Inspect: `scripts/language-baseline.txt`

- [ ] **Step 1: Confirm the landing page exceeds its target**

Run: `test "$(wc -l < README.md)" -le 150`

Expected: FAIL because the current README is 320 lines.

- [ ] **Step 2: Confirm the architecture document exceeds its target**

Run: `test "$(wc -l < ARCHITECTURE.md)" -le 300`

Expected: FAIL because the current architecture document is 965 lines.

- [ ] **Step 3: Confirm the architecture document violates the English-only target**

Run: `rg -n '[\p{Han}]' ARCHITECTURE.md`

Expected: Matches in the task, crate, MiniMax, shared-task, and older release prose.

- [ ] **Step 4: Confirm the stale current-version statement**

Run: `rg -n 'v0\.74\.0|0\.62\.0 and 0\.73\.0' ARCHITECTURE.md`

Expected: Matches the stale version paragraph even though `Cargo.toml` and the guest-linked crate
manifests report 0.76.0, 0.56.0, 0.63.0, and 0.75.0.

- [ ] **Step 5: Fix the migration baseline**

Do not commit changes to `README.md` or `ARCHITECTURE.md` before Task 6. The link validator in Task
6 reads their original contents from `HEAD`, so those Git objects are the complete pre-migration
inventory against which direct-link preservation is checked.

### Task 2: Create the release index

**Files:**
- Create: `docs/releases.md`
- Modify: `README.md`
- Modify: `ARCHITECTURE.md`

- [ ] **Step 1: Move the release table out of the README**

Create `docs/releases.md` with a short authority statement and the existing release rows, keeping
links to release and design records.

- [ ] **Step 2: Add current 0.75.0 and 0.76.0 entries**

Summarize Kiro image input for 0.75.0 and Bailian reasoning caps, Qwen ASR, and Wan media tasks for
0.76.0. Preserve the release records' prepared/unpublished status and state explicitly that host
adoption remains authoritative in `compatibility.json`.

- [ ] **Step 3: Remove duplicate chronological prose from architecture**

Do not copy architecture's long per-release narrative. Ensure every unique still-valid target that
was linked from that narrative is linked directly from `docs/releases.md` or `docs/components.md`.

- [ ] **Step 4: Check the release-index diff**

Run: `git diff --check -- docs/releases.md`

Expected: no whitespace errors. Defer cross-document link and preservation validation until Task 6,
after all four documents exist.

### Task 3: Create the component catalog

**Files:**
- Create: `docs/components.md`
- Modify: `README.md`

- [ ] **Step 1: Move the package table out of the README**

Preserve package name, world/families, build command, and a concise present-purpose summary for all
41 packages. Group rows by provider, task, embeddings, image, and speech worlds.

- [ ] **Step 2: Move package artifact rules**

Move the current text about fixture provenance, release indexes, gate-2 reports, package digests,
declared runtime, and model catalogs below the package table.

- [ ] **Step 3: Remove embedded release history**

Describe only current package behavior. Link version-specific details to `docs/releases.md` and the
relevant design records instead of retaining chronological prose in each table cell.

- [ ] **Step 4: Add package source and evidence links**

Link each package name to its `components/<name>/manifest.json`. For every fixture pack that has a
README, link that evidence README from the package summary or a compact evidence column; packages
without a fixture README need no placeholder link.

- [ ] **Step 5: Verify catalog coverage**

Run: `find components -mindepth 1 -maxdepth 1 -type d | wc -l`

Expected: `41`. Compare the names with the first column of `docs/components.md`; each component
directory must appear exactly once.

### Task 4: Rewrite the README as the landing page

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Write the concise project introduction and warning**

Keep the host-neutral southbound definition and the warning that this is a library slice, not a
complete host product.

- [ ] **Step 2: Write ownership and host-boundary summaries**

State what South owns and what remains host-owned without repeating protocol-level details.

- [ ] **Step 3: Add grouped crate and component-world summaries**

Use compact tables or bullets. Link detailed architecture to `ARCHITECTURE.md` and packages to
`docs/components.md`.

- [ ] **Step 4: Add the documentation map**

Link `ARCHITECTURE.md`, `compatibility.json`, `SECURITY.md`, `CONTRIBUTING.md`, `docs/releases.md`,
and `docs/components.md`.

- [ ] **Step 5: Preserve the complete local verification matrix**

Copy every command from the existing `Local verification` block without weakening or omitting any
command. Keep the English-only baseline explanation.

- [ ] **Step 6: Check the size and language target**

Run: `lines=$(wc -l < README.md); test "$lines" -ge 100 && test "$lines" -le 150 && ! rg -n '[\p{Han}]' README.md`

Expected: PASS.

### Task 5: Rewrite architecture as a current-state model

**Files:**
- Modify: `ARCHITECTURE.md`
- Reference: `Cargo.toml`
- Reference: `compatibility.json`
- Reference: `crates/*/Cargo.toml`

- [ ] **Step 1: Preserve dependency inversion and ownership boundaries**

Keep the host/South diagram and explain that South owns provider-facing contracts and runtime while
hosts own business policy.

- [ ] **Step 2: Replace per-crate prose with a current ownership table**

Cover all 12 workspace crates and distinguish the two 0.1.0 candidate task libraries from the
0.76.0 workspace-versioned crates and independently versioned guest-linked crates.

- [ ] **Step 3: Correct and label the dependency graph**

Show production and optional-feature edges, including the sanctioned kernel edges. State that
development-only dependencies are excluded.

- [ ] **Step 4: Describe the five current component worlds and four gates**

Summarize provider, task, embeddings, image, and speech responsibilities, explicit capabilities,
package admission, conformance, host integration, and upstream validation.

- [ ] **Step 5: Describe current host-owned and security boundaries**

Retain the no-database/no-secret-source/no-hidden-I/O rules, explicit credentials and I/O,
host-owned routing/pricing/persistence/settlement, and locked-down component runtime.

- [ ] **Step 6: Correct current version and capability facts**

Record workspace 0.76.0, `south-contracts` 0.56.0, `south-provider-api` 0.63.0, and
`south-component-conformance` 0.75.0. Describe Kiro image input, Bailian reasoning caps, Qwen ASR,
and Wan media as current capabilities, not as a chronology.

- [ ] **Step 7: Preserve the compatibility-range semantics**

Explain `runtime_abi`, exact kernel contract matching, host-decoded South contract ranges, and the
`south_runtime` floor/current-runtime interval. Preserve per-package isolation, no native fallback,
and digest pinning for duplicate family declarations. State that compatibility and adoption status
come from `compatibility.json`.

- [ ] **Step 8: Add focused reference links**

Link the release index, component catalog, compatibility manifest, security policy, and relevant
current design records.

- [ ] **Step 9: Check the size, language, and stale-fact targets**

Run:

```bash
test "$(wc -l < ARCHITECTURE.md)" -le 300
test "$(wc -l < ARCHITECTURE.md)" -ge 200
! rg -n '[\p{Han}]' ARCHITECTURE.md
! rg -n 'workspace-versioned crates are at v0\.74\.0|0\.62\.0 and 0\.73\.0' ARCHITECTURE.md
```

Expected: all commands PASS.

### Task 6: Update the language baseline and validate documentation

**Files:**
- Modify: `scripts/language-baseline.txt`
- Verify: `README.md`
- Verify: `ARCHITECTURE.md`
- Verify: `docs/releases.md`
- Verify: `docs/components.md`

- [ ] **Step 1: Lower the language baseline**

Run: `scripts/check-language.sh --regen`

Expected: the `ARCHITECTURE.md` entry is removed or reduced to zero; the command refuses any
increase elsewhere.

- [ ] **Step 2: Run the language gates**

Run:

```bash
scripts/check-language.sh --self-test
scripts/check-language.sh
scripts/check-language.sh --commits origin/main..HEAD
```

Expected: PASS.

- [ ] **Step 3: Validate links, anchors, and preservation with one reusable checker**

Run this checker after creating the four documents and again after every link-related correction.
It normalizes links relative to their source file, validates file targets and GitHub-style heading
anchors, and requires every valid local target from the original `README.md` and `ARCHITECTURE.md`
at `HEAD` to be linked directly from at least one resulting document.

```bash
python3 -c '
import pathlib, re, subprocess, sys, unicodedata

root = pathlib.Path.cwd()
current = [pathlib.Path(p) for p in (
    "README.md", "ARCHITECTURE.md", "docs/releases.md", "docs/components.md"
)]
link_re = re.compile(r"(?<!!)\[[^]]*\]\(([^)]+)\)")

def links(text, source):
    found = set()
    for raw in link_re.findall(text):
        raw = raw.strip().strip("<>")
        if not raw or raw.startswith(("http://", "https://", "mailto:")):
            continue
        path, mark, fragment = raw.partition("#")
        target = source if not path else (source.parent / path)
        normalized = pathlib.Path(target).as_posix()
        while normalized.startswith("./"):
            normalized = normalized[2:]
        normalized = pathlib.PurePosixPath(normalized).as_posix()
        found.add((normalized, fragment if mark else ""))
    return found

def slugs(text):
    result, counts = set(), {}
    for line in text.splitlines():
        match = re.match(r"^#{1,6}\s+(.+?)\s*#*\s*$", line)
        if not match:
            continue
        value = re.sub(r"[`*_~]", "", match.group(1)).strip().lower()
        value = "".join(ch for ch in unicodedata.normalize("NFKC", value)
                        if ch.isalnum() or ch in " -_")
        value = re.sub(r"[ _]+", "-", value)
        value = re.sub(r"-+", "-", value).strip("-")
        count = counts.get(value, 0)
        counts[value] = count + 1
        result.add(value if count == 0 else f"{value}-{count}")
    return result

original = set()
for name in ("README.md", "ARCHITECTURE.md"):
    text = subprocess.check_output(["git", "show", f"HEAD:{name}"], text=True)
    original |= links(text, pathlib.Path(name))

now = set()
errors = []
for source in current:
    text = source.read_text()
    now |= links(text, source)

for target, fragment in sorted(now):
    path = root / target
    if not path.is_file():
        errors.append(f"missing target: {target}")
        continue
    if fragment and fragment not in slugs(path.read_text()):
        errors.append(f"missing anchor: {target}#{fragment}")

for target in sorted(original - now):
    errors.append(f"original target no longer linked directly: {target[0]}" +
                  (f"#{target[1]}" if target[1] else ""))

if errors:
    print("\n".join(errors), file=sys.stderr)
    raise SystemExit(1)
print(f"documentation links valid; preserved {len(original)} original targets")
'
```

Expected: `documentation links valid; preserved <N> original targets` and exit zero.

- [ ] **Step 4: Check formatting and review the diff**

Run:

```bash
git diff --check
git add README.md ARCHITECTURE.md docs/releases.md docs/components.md scripts/language-baseline.txt
git diff --cached --stat
git diff --cached -- README.md ARCHITECTURE.md docs/releases.md docs/components.md scripts/language-baseline.txt
```

Expected: no whitespace errors and no changes outside the approved documentation scope.

- [ ] **Step 5: Commit the documentation migration**

```bash
git commit -m "docs: focus entry documents on current architecture"
```

### Task 7: Run the complete repository verification matrix

**Files:**
- Verify only: repository workspace

- [ ] **Step 1: Run every command listed under README `Local verification`**

Run the commands exactly as written, including formatting, Clippy, nextest, doctests, both feature
configurations, fuzz workspace check, rustdoc, MSRV check, boundary checks, Python unit tests,
language checks, `cargo deny`, `cargo audit`, and both `cargo machete` invocations.

Expected: every command exits zero. If a required tool is unavailable or an external advisory
database cannot be reached, report that limitation separately and do not claim that command passed.

- [ ] **Step 2: Confirm the final repository state**

Run: `git status --short --branch && git log -4 --oneline`

Expected: the documentation commits are present and no unintended tracked or untracked files
remain.
