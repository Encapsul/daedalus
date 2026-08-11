---
globs: "**/*.rs"
alwaysApply: false
disable: false
---

# Rust Security Rules (ANSSI-Rust)

Based on [ANSSI Secure Rust Guidelines](https://anssi-fr.github.io/rust-guide). These are **rules** (MUST) not suggestions.

## Critical Rules

- **DENV-STABLE**: Use stable toolchain only. Never nightly/beta.
- **DENV-CARGO-LOCK**: `Cargo.lock` MUST be tracked in version control.
- **LANG-UNSAFE**: No `unsafe` blocks in `erebus-core`. Only in `stub/src/main.rs`.
- **UNSAFE-NOUB**: Zero Undefined Behavior. No exceptions.
- **LANG-LIMIT-PANIC**: No `panic!()` in library code. Prefer `Result<T, E>`.
- **LANG-LIMIT-PANIC-SRC**: No `unwrap()`/`expect()` in `erebus-core` without context.
- **LANG-ARITH**: Use checked/wrapping/saturating arithmetic where overflow is possible.
- **MEM-NO-LEAK**: No `mem::forget` or `.leak()`.
- **FFI-SAFEWRAPPING**: All FFI calls MUST have safe wrappers.
- **LIBS-AUDIT**: Run `cargo audit` periodically.

## Code Style

- Edition 2021, `cargo fmt` is authoritative
- Release profile: `opt-level = "z"`, LTO, strip, `panic = "abort"` — tiny binaries
- Clippy pedantic with many allows (see `erebus-core/Cargo.toml [lints.clippy]`)
- Prefer `Result::ok()` over `|e| e.ok()`
- Prefer `r"..."` over `r#"..."#` when no `#` in string
- Use `'\n'` not `"\n"` for single-char pattern matching
- Use `.contains_key()` over `.get().is_none()`
- Use `if let Some(v)` over `match` with `None => {}`
- Functions with >7 params: use a config struct

## Safety Comments

All `unsafe` blocks MUST have SAFETY comments explaining:
1. Why `unsafe` is necessary
2. What invariants are upheld
3. Why the code is sound

Example:
```rust
// SAFETY: We trust the input from /proc/self/exe and have validated
// the footer magic before accessing this memory.
unsafe { &*ptr }
```
