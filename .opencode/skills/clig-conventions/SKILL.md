---
name: clig-conventions
description: CLI design conventions following clig.dev, POSIX, and 12-factor app principles
---

## What I do

I enforce CLI design conventions for x.bin based on clig.dev, POSIX, and 12-factor app principles.

## Core principles

### 1. Human-first design

- stdout = data (machine-readable)
- stderr = logs/errors (human-readable)
- No colors in piped output
- No prompts in CI (require `--force`)

### 2. Standard flags

Every command MUST support:
- `-h` / `--help`: Help message
- `--version`: Version info
- `-v` / `--verbose`: Verbose output
- `-q` / `--quiet`: Suppress output
- `-o` / `--output`: Output file/path
- `--dry-run`: Show what would happen
- `--json`: JSON output (when applicable)

### 3. Exit codes

- `0`: Success
- `1`: General error
- `2`: Usage error
- `3`: Data error
- `4`: Permission error
- `5`: Not found

### 4. Error messages

```rust
// Good
eprintln!("Error: failed to read file: {}", path.display());

// Bad
eprintln!("Error!");
```

## x.bin specific conventions

### Commands

```bash
xbin build <app>      # Build self-extracting binary
xbin inspect <binary> # Inspect binary metadata
xbin verify <binary>  # Verify integrity
xbin sign <binary>    # Sign binary
xbin keygen           # Generate signing key
xbin doctor           # Check environment
xbin env              # Show environment
xbin clean            # Clean cache
xbin completion        # Generate completions
xbin man              # Generate man page
```

### Flag conventions

```bash
# Short flags (single char)
-v, --verbose
-q, --quiet
-o, --output
-f, --force

# Long flags (descriptive)
--dry-run
--json
--runtime <runtime>
--entrypoint <entrypoint>
--encrypt
--sign
```

## Files to modify

- `xbin-cli/src/main.rs`: CLI definition
- `xbin-cli/src/commands/*.rs`: Command implementations
- `xbin-core/src/error.rs`: Error types

## Testing

```bash
# Test help output
xbin --help
xbin build --help

# Test error handling
xbin build nonexistent/
xbin inspect nonexistent/

# Test JSON output
xbin inspect --json <binary>
```
