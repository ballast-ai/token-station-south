#!/usr/bin/env bash
set -euo pipefail

# Enforce the English-only rule that CONTRIBUTING.md and AGENTS.md already state.
#
# Why this script exists at all: the rule was written down in **both** of the
# files a contributor reads first —
#
#   AGENTS.md:       "Use English for all code, comments, documentation,
#                     diagnostics, logs, tests, and commits."
#   CONTRIBUTING.md: the same sentence, spelled out to include commit messages.
#
# — and it still drifted to 1142 CJK lines across 46 files, including assertion
# messages in conformance tests (which *are* diagnostics) and six release commit
# titles. A rule with no judge is a preference, and preferences lose to whatever
# language the author happens to be thinking in. This is the judge.
#
# Two things it deliberately does NOT do:
#
#  * It does not try to detect "is this English". It detects **CJK codepoints**,
#    which is a fact about bytes rather than a guess about prose. False
#    negatives (romanised non-English) are accepted; a checker that argues about
#    language would get switched off.
#  * It does not demand the backlog be translated before anything else can
#    merge. `LANGUAGE_BASELINE` records the per-file counts that exist today and
#    the gate fails only when a count **rises** or a new file appears. The
#    backlog then drains at whatever pace the maintainers choose, and the
#    baseline can only ever be lowered (see --regen).
#
# Usage:
#   scripts/check-language.sh              # gate: no file may exceed its baseline
#   scripts/check-language.sh --self-test  # prove the gate actually catches things
#   scripts/check-language.sh --regen      # lower baselines after translating
#   scripts/check-language.sh --commits <range>   # gate commit messages in a range

readonly BASELINE_FILE='scripts/language-baseline.txt'

# The pattern, written as **codepoints** and matched by perl rather than as a
# literal character range matched by grep.
#
# That is not style. `grep -cE '[<CJK>-<CJK>]'` is what the first draft used, and
# it is unportable in a way that cost a CI round to find:
#
#   * GNU grep 3.11 on Linux, LC_ALL unset / C / en_US.UTF-8 — **over-matches**,
#     flagging any line with a single em dash as CJK. Correct English prose fails.
#   * GNU grep 3.11, LC_ALL=C.UTF-8 — `grep: Invalid collation character`, i.e.
#     the gate does not run at all.
#   * ugrep and BSD grep on macOS — correct, which is exactly why the bug was
#     invisible locally and only appeared on the runner.
#
# `perl -CSD` decodes UTF-8 itself and matches on codepoints, so it behaves the
# same on both platforms and needs no modules beyond perl-base (verified on a bare
# ubuntu:24.04 image, which has neither `Encode` nor python3). Invalid UTF-8 is
# skipped rather than fatal.
#
# Ranges: CJK symbols and punctuation, CJK Extension A, CJK Unified Ideographs,
# CJK compatibility ideographs, and fullwidth forms. `—` on its own is deliberately
# absent — see the note above — while `——`, the Chinese dash, is two of them.
#
# Fullwidth **digits** (U+FF10-U+FF19) are excluded, so the fullwidth range is
# split in two. This gate is about Chinese *prose*, and the only fullwidth digits
# in the repo are rejected-input fixtures (`１９`, `１２`) pinning that a pasted
# fullwidth number is refused — real coverage that an ASCII rewrite would delete.
# Nothing is lost by the exclusion: a genuine Chinese sentence carries ideographs
# or fullwidth punctuation too, and both are still matched.
readonly CJK_PATTERN='[\x{3000}-\x{303F}\x{3400}-\x{4DBF}\x{4E00}-\x{9FFF}\x{F900}-\x{FAFF}\x{FF01}-\x{FF0F}\x{FF1A}-\x{FF60}]|\x{2014}{2}'

# Lines matching the pattern, prefixed with their line number.
cjk_lines_of() {
  perl -CSD -ne 'BEGIN{$p=shift} print "$.:$_" if /$p/' "$CJK_PATTERN" -- "$1" 2>/dev/null
}

# Paths where CJK is **data, not prose** — the only sanctioned exception.
#
# One path per line with the reason it is exempt, so that adding an entry is a
# reviewed decision with a justification attached rather than a silent widening.
# A path prefix would not do: `crates/**/fixtures-*` as a blanket exemption would
# have swallowed the seven fixture READMEs, which are prose and do need fixing.
#
# Keep this list short. If it starts growing, the rule is being renegotiated by
# accident.
readonly DATA_ALLOWLIST='scripts/check-language.sh|the CJK character class this gate matches on, plus its self-test fixtures'

cjk_lines_in() {
  # Counts lines, not codepoints: one line with three Chinese words is one line
  # to fix.
  local file="$1"
  cjk_lines_of "$file" | wc -l | tr -d ' '
}

is_allowlisted() {
  local file="$1" entry
  while IFS= read -r entry; do
    [[ -z "$entry" ]] && continue
    [[ "${entry%%|*}" == "$file" ]] && return 0
  done <<<"$DATA_ALLOWLIST"
  return 1
}

# Text files git tracks. Binary files are skipped by asking git, not by guessing
# from the extension: `git grep -I` is the same "is this text" judgement the rest
# of the tooling already uses.
tracked_text_files() {
  git ls-files -z | while IFS= read -r -d '' f; do
    [[ -f "$f" ]] || continue
    if git grep -Iq --no-index -e '' -- "$f" >/dev/null 2>&1; then
      printf '%s\n' "$f"
    fi
  done
}

scan() {
  # Emits "count<TAB>path" for every tracked text file with at least one CJK
  # line, sorted by path so the baseline file has a stable diff.
  local f n
  while IFS= read -r f; do
    is_allowlisted "$f" && continue
    n="$(cjk_lines_in "$f")"
    # An `if` rather than `&&`: with `set -o pipefail`, a final iteration whose
    # `&&` short-circuits makes the whole `while … | sort` pipeline fail, and
    # `set -e` then kills --regen with no output at all. Cost me a debugging
    # round; an `if` with no else always returns 0.
    if [[ "$n" -gt 0 ]]; then
      printf '%s\t%s\n' "$n" "$f"
    fi
  done < <(tracked_text_files) | sort -t$'\t' -k2,2
}

# The baseline is read with awk rather than an associative array on purpose:
# `check-boundaries.sh` carries none either, because macOS still ships bash 3.2
# and `local -A` is a bash-4 feature. A gate that only runs on the CI runner
# would let every local check pass silently — which is the same "no judge"
# failure this script exists to fix.
baseline_for() {
  local file="$1"
  [[ -f "$BASELINE_FILE" ]] || { printf '0\n'; return 0; }
  awk -F'\t' -v k="$file" '$2==k{print $1; found=1} END{if(!found) print 0}' "$BASELINE_FILE"
}

gate() {
  local failed=0 n f allowed
  while IFS=$'\t' read -r n f; do
    allowed="$(baseline_for "$f")"
    if (( n > allowed )); then
      failed=1
      if (( allowed == 0 )); then
        echo "language: new CJK in $f ($n line(s))" >&2
        echo "  This repo is English-only — see CONTRIBUTING.md 'Language and design'." >&2
        echo "  Offending lines:" >&2
        cjk_lines_of "$f" | head -5 | sed 's/^/    /' >&2
      else
        echo "language: CJK grew in $f ($allowed -> $n line(s))" >&2
      fi
    fi
  done < <(scan)

  # A file whose count *dropped* is good news, but leaving the baseline stale
  # lets it creep back up to the old number for free. Report it and say what to
  # run, rather than quietly accepting a number nobody re-earned.
  local stale=0
  if [[ -f "$BASELINE_FILE" ]]; then
    while IFS=$'\t' read -r n f; do
      [[ -z "${f:-}" || "$n" == '#'* ]] && continue
      local now
      now="$(cjk_lines_in "$f")"
      (( now < n )) && stale=1
    done < "$BASELINE_FILE"
  fi
  if (( stale == 1 && failed == 0 )); then
    echo "language: baseline is stale (a file improved) — run scripts/check-language.sh --regen" >&2
    failed=1
  fi

  if (( failed == 1 )); then
    echo "language check failed" >&2
    return 1
  fi
  echo "language check passed"
}

regen() {
  # Only ever lowers. Raising a baseline is how a gate becomes decoration, so
  # that case is refused rather than written.
  local tmp
  tmp="$(mktemp)"
  {
    echo '# CJK-line counts this repo still carries, by file. Generated by'
    echo '# scripts/check-language.sh --regen. The gate fails when a count rises'
    echo '# or a new file appears; --regen refuses to raise a count, so this file'
    echo '# is a ratchet that only tightens. Translate, then regen.'
  } > "$tmp"
  scan >> "$tmp"

  if [[ -f "$BASELINE_FILE" ]]; then
    local n f old raised=0
    while IFS=$'\t' read -r n f; do
      [[ -z "${f:-}" || "$n" == '#'* ]] && continue
      old="$(awk -F'\t' -v k="$f" '$2==k{print $1}' "$BASELINE_FILE")"
      if [[ -n "$old" ]] && (( n > old )); then
        echo "refusing to raise the baseline for $f ($old -> $n)" >&2
        raised=1
      fi
    done < <(scan)
    if (( raised == 1 )); then
      rm -f "$tmp"
      echo "--regen is for recording progress, not for absorbing new CJK" >&2
      return 1
    fi
  fi
  mv "$tmp" "$BASELINE_FILE"
  echo "baseline written to $BASELINE_FILE"
}

check_commits() {
  # Commit messages are named in the rule and were the part that drifted
  # furthest: six release titles on main are Chinese. CI checks the push range
  # rather than all of history — history is immutable, the point is to stop the
  # next one.
  local range="$1" failed=0 sha subject
  while read -r sha; do
    [[ -z "$sha" ]] && continue
    # Counted in bash rather than signalled by perl's exit status: `exit` inside a
    # perl one-liner still runs its END block, so an `exit 0`/`END{exit 1}` pair
    # silently reports the opposite of what it found.
    local hits
    hits="$(git log -1 --pretty=%B "$sha" \
      | perl -CSD -ne 'BEGIN{$p=shift} print if /$p/' "$CJK_PATTERN" | wc -l | tr -d ' ')"
    if [[ "$hits" != 0 ]]; then
      subject="$(git log -1 --pretty=%s "$sha")"
      echo "language: CJK in commit message $sha" >&2
      echo "    $subject" >&2
      failed=1
    fi
  done < <(git rev-list "$range")
  if (( failed == 1 )); then
    echo "commit messages must be English — see CONTRIBUTING.md" >&2
    return 1
  fi
  echo "commit message language check passed"
}

self_test() {
  # The gate must be shown to catch what it claims, in a throwaway tree — the
  # same reason check-boundaries.sh carries fixtures rather than trusting its
  # own patterns.
  local tmp checker
  # Resolve the checker before any `cd`: the subshells below run inside the
  # throwaway tree, where a relative path or $OLDPWD would not survive.
  checker="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)/check-language.sh"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN

  git -C "$tmp" init -q .
  git -C "$tmp" config user.email t@example.com
  git -C "$tmp" config user.name t

  printf 'plain english line\n' > "$tmp/clean.txt"
  printf 'hello\n' > "$tmp/prose.md"
  git -C "$tmp" add -A
  git -C "$tmp" commit -qm 'english subject'

  # 1. A clean tree with no baseline must pass.
  # No baseline file exists in the throwaway tree yet, so this exercises the
  # "no baseline at all" path without needing to override the constant.
  if ! ( cd "$tmp" && bash "$checker" >/dev/null 2>&1 ); then
    echo "language self-test failed: a clean tree was rejected" >&2
    return 1
  fi

  # 2. CJK prose must be rejected, and the reason must name the file.
  printf '这一行是中文\n' >> "$tmp/prose.md"
  git -C "$tmp" add -A
  local out
  if out="$( cd "$tmp" && bash "$checker" 2>&1 )"; then
    echo "language self-test failed: CJK prose was accepted" >&2
    return 1
  fi
  if ! grep -q 'prose.md' <<<"$out"; then
    echo "language self-test failed: rejection did not name the file" >&2
    return 1
  fi

  # 3. The Chinese dash alone must be caught — that is the shape of the release
  #    titles that slipped through ("0.34.0 —— Bedrock ...").
  printf 'release 0.34.0 —— bedrock\n' > "$tmp/prose.md"
  git -C "$tmp" add -A
  if ( cd "$tmp" && bash "$checker" >/dev/null 2>&1 ); then
    echo "language self-test failed: the Chinese double dash was accepted" >&2
    return 1
  fi

  # 3b. **The negative control, and the most important case here.** Correct
  #     English typography must pass: a single em dash, an ellipsis, a middle
  #     dot. The first draft of this gate put all three in the CJK class and
  #     flagged 60+ lines of perfectly good English on its first real run — and
  #     a gate that cries wolf is a gate someone disables, which would leave the
  #     repo as unguarded as it was before. Without this case, that bug could
  #     come back and every other self-test case would stay green.
  printf 'a dash — an ellipsis … a middle dot · all fine\n' > "$tmp/prose.md"
  git -C "$tmp" add -A
  if ! ( cd "$tmp" && bash "$checker" >/dev/null 2>&1 ); then
    echo "language self-test failed: correct English typography was rejected" >&2
    return 1
  fi

  # 4. A baseline must license exactly the lines it records and no more.
  printf 'english\n中文一\n' > "$tmp/prose.md"
  git -C "$tmp" add -A
  printf '1\tprose.md\n' > "$tmp/scripts-baseline.txt"
  mkdir -p "$tmp/scripts"
  cp "$tmp/scripts-baseline.txt" "$tmp/scripts/language-baseline.txt"
  git -C "$tmp" add -A
  if ! ( cd "$tmp" && bash "$checker" >/dev/null 2>&1 ); then
    echo "language self-test failed: a baselined line was rejected" >&2
    return 1
  fi
  printf 'english\n中文一\n中文二\n' > "$tmp/prose.md"
  git -C "$tmp" add -A
  if ( cd "$tmp" && bash "$checker" >/dev/null 2>&1 ); then
    echo "language self-test failed: growth past the baseline was accepted" >&2
    return 1
  fi

  # 4b. The data allowlist must actually exempt the path it names — and must not
  #     exempt a path it does not. It was dead code in the first draft (an empty
  #     list plus a lookup that never matched), and dead exemption machinery is
  #     worse than none: the day someone needs it they get a gate that refuses
  #     their entry for reasons nobody has debugged.
  # Case 4 left a baseline that licenses prose.md; drop it so this case tests the
  # allowlist rather than re-testing the baseline.
  rm -f "$tmp/scripts/language-baseline.txt"
  printf 'english\n中文一\n' > "$tmp/prose.md"
  printf '中文\n' > "$tmp/exempt.txt"
  git -C "$tmp" add -A
  # Outside the repo tree on purpose: a copy of this checker inside `$tmp` would
  # be picked up by `git add -A`, and it carries CJK of its own, so the fixture
  # would fail the gate for a reason that has nothing to do with the allowlist.
  local patched
  patched="$(mktemp)"
  sed "s#^readonly DATA_ALLOWLIST=.*#readonly DATA_ALLOWLIST='exempt.txt|self-test fixture'#" \
    "$checker" > "$patched"
  if ( cd "$tmp" && bash "$patched" >/dev/null 2>&1 ); then
    echo "language self-test failed: an unlisted CJK file was exempted" >&2
    return 1
  fi
  rm -f "$tmp/prose.md"
  git -C "$tmp" add -A
  if ! ( cd "$tmp" && bash "$patched" >/dev/null 2>&1 ); then
    echo "language self-test failed: the allowlisted path was not exempted" >&2
    return 1
  fi
  rm -f "$tmp/exempt.txt"
  rm -f "$patched"
  git -C "$tmp" add -A

  # 5. A CJK commit message must be rejected.
  printf 'english\n中文一\n' > "$tmp/prose.md"
  git -C "$tmp" add -A
  git -C "$tmp" commit -qm '发布记录'
  if ( cd "$tmp" && bash "$checker" --commits 'HEAD~1..HEAD' >/dev/null 2>&1 ); then
    echo "language self-test failed: a CJK commit message was accepted" >&2
    return 1
  fi

  echo "language self-test passed"
}

case "${1:-}" in
  --self-test) self_test ;;
  --regen)     regen ;;
  --commits)   check_commits "${2:?usage: --commits <git range>}" ;;
  '')          gate ;;
  *)           echo "unknown argument: $1" >&2; exit 2 ;;
esac
