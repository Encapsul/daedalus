---
description: Run full security audit on x.bin
agent: security-audit
---

# Security Audit Command

Run a comprehensive security audit on x.bin:

1. Run `cargo audit` for dependency vulnerabilities
2. Run `cargo clippy --all-targets -- -D warnings` for lint issues
3. Check for unsafe code violations in xbin-core
4. Verify ANSSI-Rust compliance
5. Report all findings with severity levels

## Usage

```
/security-audit
```

## Output

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
