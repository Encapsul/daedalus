#!/usr/bin/env bash
# check-standards.sh - enforce "1 topic = 1 file = 1 truth" (anti-XKCD 927)
#
# Not wired into CI yet: run it locally before committing. It is deliberately
# stricter about duplication than the surrounding workflow, and the repo has
# not been swept for every intentional mirror, so it is opt-in for now.
#
# The rule is about *divergent truth*, not about file count. Five architecture
# docs describing five different subsystems is good structure; the same
# architecture documented twice is a problem. So every check below targets
# actual duplication, and a file that merely shares a word with another file
# is not a finding.
set -uo pipefail

FAIL=0

fail() {
  echo "FAIL: $1"
  echo "  → $2"
  FAIL=1
}

# Only real docs. node_modules/target/.git hold vendored or generated markdown
# that nobody curates, so including them produced pure noise.
DOCS=$(find . \( -name node_modules -o -name target -o -name .git -o -name benchmarks \) -prune -o -type f -name '*.md' -print 2>/dev/null | sort)

# Rule 1: only one roadmap file at repo root
root_roadmaps=$(ls -1 ./*.md 2>/dev/null | grep -i roadmap || true)
count=$(printf '%s' "$root_roadmaps" | grep -c . || true)
if [ "$count" -gt 1 ]; then
  fail "multiple roadmap files at root: $root_roadmaps" \
       "Keep only ROADMAP.md. Merge others into it and delete them."
fi

# Rule 2: no -v2, -final, -consolidated revision markers on tracked docs.
#
# A revision marker means "this supersedes another file". Documenting a
# version is not that: `v1-to-v2.md` is a migration guide and
# `daedalus-format-v2.md` is a format spec, so both end in `-v2` while naming
# a topic. Those are allowlisted explicitly. Anything else that ends in a
# version suffix is a likely superseded copy and gets flagged.
bad_suffix=$(python3 - <<'PY'
import re
import subprocess

pat = re.compile(r"(-v\d+|-final|-consolidated|-old|-new|-\d{4})$")
ALLOWED = {"docs/src/migration/v1-to-v2.md", "docs/src/spec/daedalus-format-v2.md"}

tracked = subprocess.run(
    ["git", "ls-files", "*.md"], capture_output=True, text=True, check=False
).stdout.split()
for path in tracked:
    if path in ALLOWED:
        continue
    stem = path[: -len(".md")] if path.endswith(".md") else path
    if pat.search(stem):
        print(f"  {path}")
PY
)
if [ -n "$bad_suffix" ]; then
  fail "files with forbidden suffixes:" \
       "No -v2, -final, -consolidated, -old, -new, or -YYYY suffixes allowed.

$bad_suffix"
fi

# Rule 3: no byte-identical duplicates among curated docs.
#
# The previous version flagged every file whose *path* contained
# "architecture", which reported the five legitimately separate subsystem
# docs (builder-pipeline, internal-crates, runtime-launcher, sisr-spec,
# concepts/architecture) as competing standards. That was a false positive
# on correct structure. Duplication is content, not filename: two tracked
# markdown files that are byte-identical are the same truth stored twice and
# will silently diverge the first time one is edited. Only byte-identical
# pairs are reported; two docs on different topics are not this check's job.
dupes=$(python3 - <<'PY'
import hashlib
import os
import subprocess

tracked = subprocess.run(
    ["git", "ls-files", "*.md"], capture_output=True, text=True, check=False
).stdout.split()

by_hash: dict[str, list[str]] = {}
for path in tracked:
    if os.path.islink(path) or not os.path.isfile(path):
        continue
    try:
        with open(path, "rb") as handle:
            digest = hashlib.sha256(handle.read()).hexdigest()
    except OSError:
        continue
    by_hash.setdefault(digest, []).append(path)

for digest, paths in sorted(by_hash.items()):
    if len(paths) > 1:
        print("  " + "  ".join(sorted(paths)))
PY
)
if [ -n "$dupes" ]; then
  fail "byte-identical markdown duplicates (same content tracked twice):" \
       "Keep one canonical file and delete the copy. Symlinks are not an
option here: the working tree is on vfat, which does not support them.

$dupes"
fi

# Rule 4: no archive directories with md files (git is the archive)
archive_dirs=$(find . \( -name node_modules -o -name target -o -name .git \) -prune -o -type d -name "archive" -print 2>/dev/null || true)
if [ -n "$archive_dirs" ]; then
  fail "archive directories found: $archive_dirs" \
       "Delete obsolete files instead of archiving them. Git is the archive."
fi

# Rule 5: a pointer file must not restate the content it points at.
#
# docs/ROADMAP.md is a three-line pointer to the root roadmap, which is the
# intended pattern. The failure mode this guards is the opposite: a "pointer"
# that grew into a second full copy.
for pointer in docs/ROADMAP.md; do
  if [ -f "$pointer" ] && [ "$(wc -l < "$pointer")" -gt 40 ]; then
    fail "$pointer has grown past a pointer and now duplicates the root doc" \
         "Reduce it to a link, or delete it and reference ROADMAP.md directly."
  fi
done

if [ "$FAIL" -eq 1 ]; then
  echo ""
  echo "XKCD 927 violation: competing standards detected."
  echo "Fix the above issues before committing."
  exit 1
fi

echo "OK: no competing standards detected"
