#!/usr/bin/env bash
# check-standards.sh — enforce "1 topic = 1 file = 1 truth" (anti-XKCD 927)
# Run locally before committing, and in CI on every PR.
set -euo pipefail

FAIL=0

# Rule 1: only one roadmap file at repo root
root_roadmaps=$(ls -1 *.md 2>/dev/null | grep -i roadmap || true)
count=$(echo "$root_roadmaps" | grep -c . || true)
if [ "$count" -gt 1 ]; then
  echo "FAIL: multiple roadmap files at root: $root_roadmaps"
  echo "  → Keep only ROADMAP.md. Merge others into it and delete them."
  FAIL=1
fi

# Rule 2: no -v2, -final, -consolidated suffixes on markdown files
bad_suffix=$(ls -1 *.md 2>/dev/null | grep -E '(-v[0-9]+|-final|-consolidated|-old|-new|-[0-9]{4})' || true)
if [ -n "$bad_suffix" ]; then
  echo "FAIL: files with forbidden suffixes: $bad_suffix"
  echo "  → No -v2, -final, -consolidated, -old, -new, or -YYYY suffixes allowed."
  FAIL=1
fi

# Rule 3: no duplicate architecture/design docs
arch_docs=$(find . -name "*.md" | grep -iE '(architecture|arch|design|tech-stack)' | grep -v node_modules | grep -v target || true)
count=$(echo "$arch_docs" | grep -c . || true)
if [ "$count" -gt 1 ]; then
  echo "FAIL: multiple architecture/design docs found:"
  echo "$arch_docs"
  echo "  → Keep one canonical file, delete or merge the rest."
  FAIL=1
fi

# Rule 4: no archive directories with md files (git is the archive)
archive_dirs=$(find . -type d -name "archive" | grep -v node_modules | grep -v target || true)
if [ -n "$archive_dirs" ]; then
  echo "FAIL: archive directories found: $archive_dirs"
  echo "  → Delete obsolete files instead of archiving them. Git is the archive."
  FAIL=1
fi

# Rule 5: docs/src/roadmap.md must not duplicate ROADMAP.md content
if [ -f "docs/src/roadmap.md" ] && [ -f "ROADMAP.md" ]; then
  if grep -q "current state\|positioning\|competitive" docs/src/roadmap.md 2>/dev/null; then
    echo "FAIL: docs/src/roadmap.md appears to duplicate ROADMAP.md content"
    echo "  → docs/src/roadmap.md should include/transclude ROADMAP.md, not duplicate it"
    FAIL=1
  fi
fi

if [ "$FAIL" -eq 1 ]; then
  echo ""
  echo "XKCD 927 violation: competing standards detected."
  echo "Fix the above issues before committing."
  exit 1
fi

echo "OK: no competing standards detected"
