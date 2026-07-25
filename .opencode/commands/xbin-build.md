---
description: Build x.bin with all optimizations
---

# Build Command

Build x.bin with all optimizations:

1. Run `cargo fmt` to format code
2. Run `cargo clippy --all-targets -- -D warnings` to check for issues
3. Run `cargo build --release` with proper environment
4. Verify the build output
5. Report build status and any warnings

## Usage

```
/xbin-build
```

## Output

```markdown
## Build Report

### Status
- [x] Code formatted
- [x] No clippy warnings
- [x] Build successful

### Output
- Binary: target/release/xbin
- Size: X MB
- Warnings: None
```
