---
description: Run all tests for x.bin
---

# Test Command

Run the complete test suite:

1. Run `cargo test --workspace` for Rust tests
2. Run `cd cli && python -m pytest` for Python tests
3. Report test results with coverage if available
4. Highlight any failing tests and suggest fixes

## Usage

```
/xbin-test
```

## Output

```markdown
## Test Report

### Rust Tests
- Passed: X
- Failed: Y
- Skipped: Z

### Python Tests
- Passed: X
- Failed: Y
- Skipped: Z

### Coverage
- Lines: X%
- Branches: Y%

### Failures
- [ ] Test 1: Description
- [ ] Test 2: Description
```
