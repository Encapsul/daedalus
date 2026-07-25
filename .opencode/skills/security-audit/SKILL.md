---
name: security-audit
description: Comprehensive security audit for x.bin - dependencies, unsafe code, and ANSSI compliance
---

## What I do

I perform comprehensive security audits for x.bin. I check:
- Dependency vulnerabilities
- Unsafe code violations
- ANSSI-Rust compliance
- Memory safety
- FFI safety

## Audit checklist

### 1. Dependency audit

```bash
# Check for known vulnerabilities
cargo audit

# Check for outdated dependencies
cargo outdated

# Check license compliance
cargo license
```

### 2. Unsafe code audit

```bash
# Find all unsafe code
grep -rn "unsafe" xbin-core/src/
grep -rn "unsafe" stub/src/

# Check for SAFETY comments
grep -B5 "unsafe" xbin-core/src/*.rs
grep -B5 "unsafe" stub/src/*.rs
```

### 3. ANSSI-Rust compliance

```bash
# Check for panic in library code
grep -rn "panic!" xbin-core/src/

# Check for unwrap/expect without context
grep -rn "unwrap()\|expect(" xbin-core/src/

# Check for memory leaks
grep -rn "mem::forget\|\.leak()" xbin-core/src/

# Check for arithmetic overflow
grep -rn "wrapping_\|saturating_\|checked_" xbin-core/src/
```

### 4. Memory safety

```bash
# Check for buffer overflows
grep -rn "as usize\|as u32\|as u64" xbin-core/src/

# Check for null pointer dereferences
grep -rn "as *const\|as *mut" xbin-core/src/

# Check for use-after-free
grep -rn "into_raw\|from_raw" xbin-core/src/
```

### 5. FFI safety

```bash
# Check for unsafe FFI calls
grep -rn "extern \"C\"" xbin-core/src/
grep -rn "extern \"C\"" stub/src/

# Check for safe wrappers
grep -A10 "extern \"C\"" xbin-core/src/*.rs
```

## Severity levels

- **Critical**: Must fix before commit
- **High**: Should fix before merge
- **Medium**: Fix in next release
- **Low**: Track for future improvement

## Report format

```markdown
## Security Audit Report

### Critical Issues
- [ ] Issue 1: Description

### High Issues
- [ ] Issue 2: Description

### Medium Issues
- [ ] Issue 3: Description

### Low Issues
- [ ] Issue 4: Description
```

## Files to audit

- `xbin-core/src/*.rs`: Library code
- `xbin-cli/src/*.rs`: CLI code
- `stub/src/main.rs`: Stub launcher (unsafe allowed)
- `Cargo.toml`: Dependencies
- `Cargo.lock`: Locked versions
