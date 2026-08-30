# CODE_STYLE.md — daedalus coding conventions

## References

| Document | What we follow |
|----------|---------------|
| [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) | Naming, interop, docs, predictability, type safety |
| [Rust CLI Book](https://rust-cli.github.io/book/) | Config files, exit codes, human/machine output, progress |
| [The Rustonomicon](https://doc.rust-lang.org/nomicon/) | Unsafe Rust patterns (FFI, memory, concurrency) |
| [clig.dev](https://clig.dev) | CLI UX: help, output, errors, flags, interactivity |
| [Better CLI](https://bettercli.org/) | Lifecycle, distribution, security, analytics |
| [clap docs](https://docs.rs/clap) | Arg parsing, derive macros, shell completion |
| [anyhow docs](https://docs.rs/anyhow) | Error handling with context |
| [Black](https://black.readthedocs.io/) | Python formatting (line-length 88, py312) |
| [ruff](https://docs.astral.sh/ruff/) | Python linting (E/W/F/I/UP/B/SIM/RUF) |

## Philosophy

We follow the principles behind 42/Epitech's "Norm" and the Linux kernel
coding style — adapted for Rust and Python, not copied from C.

1. **A function does ONE thing, and its name says exactly what that thing is.**
   If you can't name it precisely, it's doing too much.

2. **Many small, obviously-correct functions** over one clever one.
   Readability beats cleverness every time.

3. **No unnecessary abstraction.** Don't build a generic system for a case
   that appears once. Inline it; extract later when a second use appears.

4. **Comments explain WHY, never WHAT.** The code says what it does through
   naming and structure. Comments explain the non-obvious decision behind it.

5. **Flat is better than nested.** An early return beats a deeply nested
   if/else chain. Guard clauses first, logic second.

---

## Rust (daedalus-core/, daedalus-cli/, stub/)

### Build target

Repo lives on vfat (no exec bit). Build artifacts go to `/tmp/daedalus-stub-target`.
After `cargo build --release`, install manually:
```bash
cp /tmp/daedalus-stub-target/release/daedalus ~/.local/bin/daedalus
cp /tmp/daedalus-stub-target/release/daedalus-stub ~/.local/bin/daedalus-stub
```

### Formatting

```toml
# stub/rustfmt.toml
max_width = 100
use_small_heuristics = "Default"
```

We use `rustfmt` defaults otherwise. No nightly-only options.

### Clippy

We enable a pedantic subset, not the full `clippy::pedantic` (too many false
positives on a small codebase). Run via `cargo clippy -p daedalus-core --all-targets -- -D warnings`.

```toml
# In stub/Cargo.toml, add:
[lints]
clippy::pedantic = { level = "warn", priority = -1 }
# Then override specific noisy lints:
clippy::module_name_repetitions = "allow"
clippy::must_use_candidate = "allow"
clippy::missing_errors_doc = "allow"
clippy::missing_panics_doc = "allow"
clippy::doc_markdown = "warn"
clippy::unwrap_used = "warn"
clippy::expect_used = "warn"
```

**Rationale for allows:**
- `module_name_repetitions` — `format::Footer` is fine; `format::format_footer`
  would be worse.
- `must_use_candidate` — too noisy on private helper functions in a binary crate.
- `missing_errors_doc` / `missing_panics_doc` — only applies to `pub` items in
  library crates; this is a binary.
- `unwrap_used` / `expect_used` — warn (not deny); the codebase already avoids
  them, but a targeted `expect("reason")` is acceptable in parse paths.

### Function length: ≤ 30 lines

Measured as the line count from `fn` keyword to closing `}`. Rust is more
concise than Python; functions should be short. If a function exceeds 30
lines, it's a signal to extract a helper — not a hard ban, but a review
flag.

**Current state:** `supervise_services` is 104 lines — it should be split into
`fork_services()`, `wait_for_health()`, `wait_for_children()`.

### Unsafe rules

`daedalus-core/` and `daedalus-cli/` have **zero** `unsafe` — memory safety is
guaranteed by Rust's type system and borrow checker.

`stub/src/main.rs` is the only crate with `unsafe`. Every `unsafe` block
must have a `SAFETY` comment explaining **why it is sound**:

```rust
// SAFETY: execvp(3) is safe here — prog_c is a valid CString,
// argv_ptrs is null-terminated, and we never return on success.
// execvp searches PATH for bare command names.
unsafe {
    libc_execvp(prog_c.as_ptr(), argv_ptrs.as_ptr());
}
```

Additional rules:
- No `unsafe` outside of FFI calls and `static mut` access.
- `static mut` requires a justification comment explaining why the race
  condition is impossible (e.g., "single handler, installed once, never modified").
- `extern "C"` blocks are declared at module level, not inside functions.

### Module-level doc comments

Every `.rs` file starts with `//!` doc comments describing the module's
purpose and its role in the system. This is already the pattern in
`main.rs` and `format.rs`.

### Function documentation: Unix man-page format

Every function must have a doc comment in the Unix man-page style:

```rust
/// function_name - brief description of what the function does
/// @param_name: description of the parameter
///
/// Description: longer description explaining the algorithm, invariants,
/// and any non-obvious behavior.
///
/// Return: description of the return value, or "nothing" for ().
```

**Rules:**
- First line: `function_name - brief description` (no period at the end).
- One `@param` line per parameter, in declaration order.
- Blank line before `Description:` and `Return:`.
- `Return:` is mandatory even for `()` — write `Return: nothing`.
- For functions with no parameters, omit the `@param` lines.
- For functions returning `Result<T, E>`, describe both the success value
  and the error conditions.

**Examples:**

```rust
/// verify_ed25519 - verify an Ed25519 signature over the payload hash
/// @footer: parsed footer containing signature metadata
/// @trusted_keys: list of trusted public keys
///
/// Description: Reads the signature block from the binary, computes
/// SHA-256(payload || meta_bytes), and verifies the signature against
/// each trusted key. Returns Ok(()) on first match, Err on failure.
///
/// Return: Ok(()) if signature is valid, Err otherwise
fn verify_ed25519(footer: &Footer, trusted_keys: &[VerifyingKey]) -> Result<()> {
    ...
}

/// atomic_replace - atomically replace dst with contents of src
/// @src: temporary file containing the new content
/// @dst: final destination path
///
/// Description: Renames src to dst using std::fs::rename, which is
/// atomic on the same filesystem. Permissions are preserved from src.
///
/// Return: nothing
fn atomic_replace(src: &Path, dst: &Path) {
    ...
}

/// is_runtime_detected - check whether a runtime was auto-detected
///
/// Description: Returns true if the build pipeline detected a runtime
/// from the app directory (e.g., package.json, requirements.txt).
///
/// Return: true if runtime detected, false otherwise
fn is_runtime_detected() -> bool {
    ...
}
```

This format is enforced by `clippy::doc_markdown` warnings in CI. Functions
without doc comments will fail the lint check once `missing_docs` is enabled
for the crate. For now, this is a manual convention; the goal is to enable
`#![warn(missing_docs)]` once all public functions are documented.

---

## Python (cli/)

### Formatting

**Black** with default settings (88-char line length).

```toml
# In cli/pyproject.toml, add:
[tool.black]
target-version = ["py312"]
line-length = 88
```

Black is non-negotiable. It eliminates all formatting bikeshedding.

### Linting

**Ruff** replaces flake8, isort, pyupgrade, and dozens of plugins.

```toml
# In cli/pyproject.toml, add:
[tool.ruff]
target-version = "py312"
line-length = 88

[tool.ruff.lint]
select = [
    "E",     # pycodestyle errors
    "W",     # pycodestyle warnings
    "F",     # pyflakes
    "I",     # isort
    "UP",    # pyupgrade (modern syntax)
    "B",     # flake8-bugbear (common bugs)
    "SIM",   # flake8-simplify (flatten nesting, etc.)
    "RUF",   # Ruff-specific rules
]
ignore = [
    "E501",  # line too long — Black handles wrapping; long strings are fine
]

[tool.ruff.lint.isort]
known-first-party = ["daedalus"]
```

**Why these categories:**
- `E`/`W`/`F` — baseline, non-controversial.
- `I` — consistent import ordering (Black doesn't sort imports).
- `UP` — enforces modern Python syntax (`str | None` over `Optional[str]`).
- `B` — catches real bugs (`mutable-argument-default`, `bare-except`).
- `SIM` — suggests flattening (`if x: return y` over `if x: return y`).
- `RUF` — Ruff-specific rules that catch real issues.
- We do **not** enable `D` (pydocstyle) — docstrings are useful but
  enforcing format on a fast-moving project adds friction with no value.

### Type hints: mandatory on all functions

Every function gets parameter and return type annotations. No exceptions.

```python
def find_binary(name: str, extra_dirs: list[Path] | None = None) -> Path:
    ...

def read_footer(path: str) -> Footer:
    ...
```

**Why all functions, not just public ones:**
- This is a small codebase (~1800 LOC across 15 files). The annotation cost is
  negligible.
- Private functions are called from other modules (e.g., `_build_manifest` is
  called from `build`). They need types too.
- `from __future__ import annotations` is already used everywhere, so modern
  syntax (`str | None`) works with no cost.

### Function length: ≤ 40 lines

Measured from `def` to closing `}` (or end of body). 40 lines is roughly
one screen at standard terminal height.

**Current state:** Functions have been extracted to stay under the limit.
`_build_manifest` was 171 lines — now split into 10+ helpers.

### Comment style

```python
# GOOD: explains WHY
# The 4-byte field is an offset from file start, not from the sig block,
# because v2 readers need to skip past it without knowing the block layout.
sig_offset = read_u64(f, 0)

# BAD: explains WHAT (the code already says this)
# Read the signature offset
sig_offset = read_u64(f, 0)
```

Section separators use the existing `# ------` pattern. No trailing comments
on the same line as code unless they're a short `# type: ignore[code]`.

---

## Enforcement

We use **Makefile targets** and **GitHub Actions CI**.

### Local

```bash
make lint       # check both Rust + Python
make fmt        # auto-fix both
make preflight  # verify prerequisites
```

### CI (GitHub Actions)

`.github/workflows/ci.yml` runs on every push/PR to `main`:
1. **preflight** — verify system prerequisites
2. **rust** — `cargo build` + `cargo clippy -p daedalus-core --all-targets -- -D warnings`
3. **python** — `ruff check` + `black --check`
4. **build** — full end-to-end: build → inspect → keygen → sign → verify

PRs that fail CI cannot be merged.

### Workflow
1. Before committing: `make lint` to check, `make fmt` to auto-fix.
2. If you see a new warning, fix it or add an explicit `# noqa` / `#[allow]`
   with a comment explaining why.

---

## Brevity rules

**The codebase is too long.** These rules enforce conciseness:

### Python
- **Functions ≤ 40 lines** (down from 60). If longer, split.
- **No docstrings on private functions.** The name + type hints say enough.
  Only `_public_api()` or complex algorithms get docstrings.
- **No comments that restate the code.** `# read the footer` is noise.
  `# v2 readers expect exactly 84 bytes here` is signal.
- **No dead code.** If it's not called, delete it. Git remembers.
- **Inline small helpers.** Don't extract a 3-line function unless it's
  called from 2+ places.

### Rust
- **Functions ≤ 30 lines** (down from 40). Rust is more concise than Python;
  functions should be shorter.
- **No `#[allow]` without a comment.** Every lint suppression must explain
  why the lint is wrong in this specific case.
- **Prefer `?` over `match` for error propagation.** Only use `match` when
  you handle the error differently per arm.

### Both
- **No multi-file refactors unless the bug/feature requires it.** Moving code
  between files for "cleanliness" creates noise in diffs and breaks `git blame`.
- **One logical change per commit.** Don't mix refactors with features.
- **If you touch a file, check if related files need the same fix.** But don't
  touch files that don't need it.

---

## Anti-XKCD 927: one topic, one file, one truth

[XKCD 927](https://xkcd.com/927/) captures a universal truth: when a new standard
is created to solve a problem, it doesn't replace the old one — it adds to it.
The result is 14 competing standards, none of which work well.

We treat this as a **philosophical commitment**, not a checklist. The default
human impulse when something is messy is to create a new file to "organize" it.
That impulse is wrong. The correct response is to **consolidate**, not to add.

### The principle

**1 topic = 1 file = 1 truth.**

If you find yourself thinking "I'll create a new file to document this", stop
and ask:
- Does an existing file already cover this topic?
- If yes → update that file.
- If no → ask whether this topic deserves to exist at all.

### Anti-patterns and their corrections

| What you're tempted to do | What you actually do |
|---|---|
| "The roadmap is messy, I'll create ROADMAP-CONSOLIDATED.md" | Fix ROADMAP.md in place. Delete the mess. |
| "I'll keep the old MISSION_LOG.md for reference" | Delete it. Git is the archive. |
| "docs/src/roadmap.md needs its own copy of the content" | docs/src/roadmap.md includes ROADMAP.md. No copy-paste. |
| "I'll add a new ARCHITECTURE.md to be thorough" | Check if an existing doc already covers architecture. |
| "I'll create a -v2 suffix once this changes" | Edit the existing file. Versions live in git, not filenames. |

### Why this matters

Competing standards fragment knowledge. When two files claim to be "the
roadmap", no one knows which to trust. When architecture is split across
ARCHITECTURE.md, DESIGN.md, and TECH-STACK.md, they drift apart and become
wrong in different ways. The cognitive overhead of knowing which file is
current exceeds the cost of maintaining a single canonical one.

### The tool: `scripts/check-standards.sh`

We have a local script that catches the most common XKCD-927 anti-patterns:
- Multiple roadmap files
- Suffix-based versioning (`-v2`, `-final`, `-consolidated`)
- Duplicate architecture docs
- Archive directories

This is a **reminder of the philosophy**, not a substitute for it. The script
can't catch every case — you still need to apply judgment.

```bash
make check-standards   # run locally before committing
```

The real enforcement is cultural: if you create a competing standard, you
will be asked to consolidate it. The script just makes that conversation
easier by catching obvious cases automatically.

---

## Quick reference

| Rule | Rust | Python |
|---|---|---|
| Formatter | `rustfmt` (100 cols) | `Black` (88 cols) |
| Linter | `clippy` (pedantic subset) | `ruff` (E/W/F/I/UP/B/SIM/RUF) |
| Function length | ≤ 30 lines | ≤ 40 lines |
| Type hints | N/A (Rust is typed) | Mandatory on all functions |
| Comments explain | WHY, never WHAT | WHY, never WHAT |
| Nesting | Early returns / guard clauses | Early returns / guard clauses |
| Unsafe | Soundness comment required | N/A |
| Enforcement | `make lint` / `make fmt` | `make lint` / `make fmt` |
| Standards | 1 topic = 1 file = 1 truth | 1 topic = 1 file = 1 truth |
