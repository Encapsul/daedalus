---
name: verification-loop
description: Test → Lint → Fix → Commit verification loop for x.bin
---

## What I do

I implement the verification loop for x.bin. Every code change must go through: Test → Lint → Fix → Commit.

## Verification loop

```
1. Test: cargo test --workspace
2. Lint: cargo clippy --all-targets -- -D warnings
3. Format: cargo fmt --check
4. Fix: Apply fixes from clippy/fmt
5. Commit: git commit -S -m "type: description"
```

## Step-by-step

### 1. Run tests

```bash
# Rust tests
cargo test --workspace

# Python tests (if applicable)
cd cli && python -m pytest
```

### 2. Run linter

```bash
# Clippy with pedantic lints
cargo clippy --all-targets -- -D warnings

# Check for unsafe violations
grep -rn "unsafe" xbin-core/src/
```

### 3. Check formatting

```bash
# Check Rust formatting
cargo fmt --check

# Check Python formatting
ruff check xbin/
black --check xbin/
```

### 4. Fix issues

```bash
# Auto-fix formatting
cargo fmt

# Auto-fix linting (careful!)
cargo clippy --fix --allow-dirty

# Manual fixes for:
# - unsafe violations
# - panic handling
# - arithmetic overflow
```

### 5. Commit

```bash
# Stage changes
git add .

# Commit with conventional format
git commit -S -m "fix: description"

# Push (if ready)
git push origin feat/branch-name
```

## Common fixes

### Unsafe violations
```rust
// Before
unsafe { &*ptr }

// After
// SAFETY: explanation
unsafe { &*ptr }
```

### Panic handling
```rust
// Before
let value = option.unwrap();

// After
let value = option.ok_or("Missing value")?;
```

### Arithmetic overflow
```rust
// Before
let result = a + b;

// After
let result = a.checked_add(b).ok_or("Overflow")?;
```

## Files to check

- `xbin-core/src/*.rs`: Library code
- `xbin-cli/src/*.rs`: CLI code
- `stub/src/main.rs`: Stub launcher
- `cli/xbin/*.py`: Python CLI
