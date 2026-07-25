---
name: anssi-rust
description: ANSSI Secure Rust Guidelines compliance checker and enforcer
---

## What I do

I enforce ANSSI Secure Rust Guidelines for the x.bin project. I check code for compliance with security rules and provide fixes for violations.

## When to use me

Use this when:
- Writing or reviewing Rust code
- Checking for unsafe code violations
- Verifying panic handling
- Auditing memory safety
- Running cargo audit

## Rules I enforce

### Critical (MUST fix)
- **DENV-STABLE**: Use stable toolchain only
- **DENV-CARGO-LOCK**: Cargo.lock must be tracked
- **LANG-UNSAFE**: No unsafe in xbin-core
- **UNSAFE-NOUB**: Zero undefined behavior
- **LANG-LIMIT-PANIC**: No panic!() in library code
- **LANG-LIMIT-PANIC-SRC**: No unwrap()/expect() without context
- **LANG-ARITH**: Use checked arithmetic
- **MEM-NO-LEAK**: No mem::forget or .leak()
- **FFI-SAFEWRAPPING**: All FFI must have safe wrappers

### How to check

```bash
# Check for unsafe code
grep -rn "unsafe" xbin-core/src/

# Check for unwrap/expect
grep -rn "unwrap\(\)\|expect(" xbin-core/src/

# Check for panic
grep -rn "panic!" xbin-core/src/

# Run cargo audit
cargo audit
```

## Fixes

For unsafe violations:
- Move unsafe code to stub/src/main.rs
- Add SAFETY comments
- Create safe wrappers

For panic violations:
- Replace panic!() with Result<T, E>
- Replace unwrap() with .ok() or .unwrap_or_else()
- Replace expect() with .map_err() or context

For arithmetic violations:
- Use .checked_add(), .checked_mul(), etc.
- Use .wrapping_add(), .saturating_add()
- Handle overflow explicitly
