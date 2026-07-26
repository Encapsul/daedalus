# CLAUDE.md

This file provides guidance to Claude Code when working with the x.bin codebase.

## Project Overview

x.bin packages any application into a single self-extracting ELF binary. Detects runtime (Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Hugo, Binary), bundles with your code, produces standalone executable.

## Architecture

- `xbin-core/` — Core library (format, detect, compress, encrypt, integrity, verify, assembly)
- `xbin-cli/` — CLI tool (clap-based, 12 commands)
- `xbin-stub/` — ELF launcher (unsafe FFI, mmap, fork, exec)
- `cli/` — Legacy Python CLI (being replaced by Rust)

## Build Commands

```bash
make preflight    # Check toolchain
make stub         # Build stub for current arch
make build        # Full pipeline: preflight + stub + cargo build
make test         # Full verification: lint + fmt + clippy + cargo test + python test
make lint         # Run all linters (cargo fmt + clippy + ruff + black)
make fmt          # Auto-format all code
```

## Verification Loop (MANDATORY)

Before finishing any code change, run:
1. `cargo fmt --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cd cli && ruff check xbin/ && black --check xbin/ && python3 -m pytest tests/ -q`
5. `xbin build examples/hello-web -o /tmp/test.xbin && xbin inspect /tmp/test.xbin`

## Claude Code Architecture (Command → Agent → Skill)

This project uses the **Command → Agent → Skill** orchestration pattern from the reference architecture:

### Commands (Entry Points)
| Command | Purpose |
|---------|---------|
| `/xbin-build` | Build a .xbin binary |
| `/xbin-verify` | Verify integrity |
| `/xbin-new-runtime` | Add new runtime |
| `/xbin-new-command` | Add new CLI command |
| `/xbin-security-audit` | Security audit |
| `/xbin-benchmark` | Performance benchmark |
| `/xbin-full-build` | Full build orchestration (Command → Agent → Skill) |
| `/xbin-test-app` | Test app from GitHub |
| `/xbin-new-runtime-full` | Add runtime with full orchestration |

### Agents (Specialized Workers)
| Agent | Purpose | Skills | Memory |
|-------|---------|--------|--------|
| `security-auditor` | ANSSI-Rust compliance | anssi-rust, security-audit | project |
| `runtime-detector` | Runtime detection logic | runtime-detection | project |
| `build-pipeline` | Build pipeline | xbin-format, verification-loop | project |
| `cli-command` | CLI command design | clig-conventions | project |
| `test-runner` | Verification loop | verification-loop | project |

### Skills (Knowledge Bundles)
| Skill | Purpose |
|-------|---------|
| `anssi-rust` | 41 ANSSI rules |
| `xbin-format` | Binary format specification |
| `runtime-detection` | Runtime detection rules |
| `verification-loop` | Verification checklist |
| `security-audit` | Security audit checklist |
| `clig-conventions` | CLI design conventions |
| `python-security` | Python security rules |

### Hooks (Automation)
- **PreToolUse**: Validates file edits for ANSSI-Rust compliance
- **PostToolUse**: Logs tool execution results
- **UserPromptSubmit**: Logs user prompts for audit trail
- **Stop**: Logs session stops
- **SubagentStart/SubagentStop**: Logs subagent invocations
- **SessionStart/SessionEnd**: Logs session lifecycle

### Agent Memory (Persistent)
All agents have `memory: project` — they remember findings across sessions:
- Security audit history
- Runtime detection accuracy
- Build performance trends
- CLI command evolution

### Context Forking
Use `context: fork` for isolated subagent contexts when running parallel tasks.

## Security Rules

- No `unsafe` in `xbin-core/` (only `stub/src/main.rs`)
- All `unsafe` blocks must have `SAFETY` comments
- Ed25519 keys must have the Ed25519 bit set (CVE-2023-48022)
- No hardcoded secrets anywhere in the codebase
- Use `cargo audit` periodically for dependency vulnerabilities

## Linting & Formatting

### Rust
- `cargo fmt --check` — formatting
- `cargo clippy --all-targets -- -D warnings` — linting
- Edition 2021, `opt-level = "z"`, LTO, strip, `panic = "abort"`

### Python (legacy CLI)
- `ruff check xbin/` — linting (E/W/F/I/UP/B/SIM/RUF)
- `black --check xbin/` — formatting (88-char line length)
- Type hints mandatory on all functions
- Function length ≤ 40 lines

### CI (GitHub Actions)
- `.github/workflows/ci.yml` runs on push/PR to main
- Jobs: preflight, rust, python, build (end-to-end)
- PRs that fail CI cannot be merged

## Benchmarking

Benchmarks in `benchmarks/` measure:
- Build time (seconds)
- Output size (MB)
- Peak RSS (MB)
- Cold/warm start time
- Native vs xbin comparison

Run: `bash benchmarks/run.sh`

Machine specs affect results:
- Xeon 32 cores: < 25s build
- Typical laptop: < 60s build
- Constrained hardware: < 2min build

## External References

- [clig.dev](https://clig.dev) — CLI design conventions
- [POSIX.1-2017 Ch.12](https://pubs.opengroup.org/onlinepubs/9799919797/) — Shell & Utilities
- [ANSSI-Rust](https://anssi-fr.github.io/rust-guide/) — Rust security guidelines
- [Google Doc Style](https://developers.google.com/style) — Documentation style
- [12-Factor CLI](https://medium.com/@jdxcode/12-factor-cli-apps-dd3c227a0e46) — CLI best practices
