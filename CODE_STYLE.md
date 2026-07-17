# CODE_STYLE.md — x.bin coding conventions

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

## Rust (stub/)

### Formatting

```toml
# stub/rustfmt.toml
max_width = 100
use_small_heuristics = "Default"
```

We use `rustfmt` defaults otherwise. No nightly-only options.

### Clippy

We enable a pedantic subset, not the full `clippy::pedantic` (too many false
positives on a small codebase). Run via `cargo clippy -- -D warnings`.

```toml
# In stub/Cargo.toml, add:
[lints]
clippy:: pedantic = { level = "warn", priority = -1 }
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

### Function length: ≤ 40 lines

Measured as the line count from `fn` keyword to closing `}`. The reasoning:
40 lines fits on one screen at 100-col width with line numbers visible. If a
function exceeds 40 lines, it's a signal to extract a helper — not a hard ban,
but a review flag.

**Current state:** `supervise_services` is 104 lines — it should be split into
`fork_services()`, `wait_for_health()`, `wait_for_children()`.

### Unsafe rules

Every `unsafe` block must have a comment explaining **why it is sound**.
Format:

```rust
// SAFETY: execve(2) is safe here — prog_c and argv are valid CStrings,
// envp is null-terminated, and we never return on success.
unsafe {
    libc_execve(prog_c.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
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

---

## Python (cli/)

### Formatting

**Black** with default settings (88-char line length).

```toml
# In cli/pyproject.toml, add:
[tool.black]
target-version = ["py313"]
line-length = 88
```

Black is non-negotiable. It eliminates all formatting bikeshedding.

### Linting

**Ruff** replaces flake8, isort, pyupgrade, and dozens of plugins.

```toml
# In cli/pyproject.toml, add:
[tool.ruff]
target-version = "py313"
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
known-first-party = ["xbin"]
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
- This is a small codebase (~1500 LOC across 14 files). The annotation cost is
  negligible.
- Private functions are called from other modules (e.g., `_build_manifest` is
  called from `build`). They need types too.
- `from __future__ import annotations` is already used everywhere, so modern
  syntax (`str | None`) works with no cost.

### Function length: ≤ 60 lines

Measured from `def` to closing `}` (or end of body). 60 lines is roughly
one screen at standard terminal height. Python functions tend to be longer
than Rust because of indentation and verbosity, so the threshold is higher.

**Current state:** `_build_manifest` is 171 lines and `build` is 138 lines —
both need to be broken up.

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

We use **Makefile targets** — not CI, not pre-commit hooks. Reasoning:
this is a solo/small-team project where `make lint` is enough. CI adds
setup overhead (GitHub Actions config, secrets, runner costs) that isn't
justified yet. Pre-commit hooks require every contributor to install them.

```makefile
# Add to Makefile:

.PHONY: lint lint-rust lint-python fmt fmt-rust fmt-python

lint: lint-rust lint-python

lint-rust:
	cd stub && cargo clippy -- -D warnings

lint-python:
	cd cli && python -m ruff check xbin/
	cd cli && python -m black --check xbin/

fmt: fmt-rust fmt-python

fmt-rust:
	cd stub && cargo fmt

fmt-python:
	cd cli && python -m black xbin/
	cd cli && python -m ruff check --fix xbin/
```

**Workflow:**
1. Before committing: `make lint` to check, `make fmt` to auto-fix.
2. If you see a new warning, fix it or add an explicit `# noqa` / `#[allow]`
   with a comment explaining why.

### Future: when to add CI

If the project gets a second contributor or goes public, add a GitHub Actions
workflow that runs `make lint`. Until then, the Makefile is sufficient.

---

## Quick reference

| Rule | Rust | Python |
|---|---|---|
| Formatter | `rustfmt` (100 cols) | `Black` (88 cols) |
| Linter | `clippy` (pedantic subset) | `ruff` (E/W/F/I/UP/B/SIM/RUF) |
| Function length | ≤ 40 lines | ≤ 60 lines |
| Type hints | N/A (Rust is typed) | Mandatory on all functions |
| Comments explain | WHY, never WHAT | WHY, never WHAT |
| Nesting | Early returns / guard clauses | Early returns / guard clauses |
| Unsafe | Soundness comment required | N/A |
| Enforcement | `make lint` / `make fmt` | `make lint` / `make fmt` |
