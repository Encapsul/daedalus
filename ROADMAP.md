# x.bin Security & Technical Roadmap

Generated from security audit and code review. All tests pass, clippy passes, fmt passes — these are issues beyond the verification loop.

---

## 🔴 Critical (Fix Immediately)

### 1. Encryption Is Obfuscation Only, Not Confidentiality
**Location:** `stub/src/main.rs:137-158` (CryptoMeta docs), `xbin-cli/src/commands/build.rs:924-1007`
- AES-256-GCM key stored in plaintext in metadata alongside ciphertext
- Anyone with the `.xbin` file can decrypt the payload
- **Action:** Update docs, CLI help, and marketing to state clearly: `--encrypt` provides obfuscation against casual inspection, NOT confidentiality against an attacker who holds the binary. Real confidentiality requires a key not stored in the file (env var, passphrase, HSM).

### 2. Static Mut in Signal Handler
**Location:** `stub/src/exec.rs:908, 889-921`
```rust
static mut CHILD_PIDS: Vec<i32> = Vec::new();
```
- Data race risk: written once at install, read from signal handler
- **Action:** Replace with `OnceLock<Mutex<Vec<i32>>>` or `std::sync::atomic` + lock-free structure.

### 3. Ed25519 Key Must Have Ed25519 Bit Set (CVE-2023-48022)
**Location:** `xbin-core/src/sisr_stage.rs:17-20`, `stub/src/crypto.rs:48-51`
- Using `ed25519-dalek` — verify version ≥ 2.1.0 has the fix
- **Action:** Add version pin check in CI, document requirement.

### 4. Constant-Time Comparison for All Secret Comparisons
**Location:** `stub/src/crypto.rs:160-172` has `ct_eq_sha256` but verify all uses
- SHA-256 integrity check uses constant-time — good
- **Action:** Audit all secret comparisons (keys, nonces, signatures) use constant-time.

---

## 🟠 High (Fix Before Next Release)

### 5. Path Traversal in Include Paths
**Location:** `xbin-cli/src/commands/build.rs:812-818` → `xbin_core::include::copy_include_paths`
- User-controlled `--include` paths copied into rootfs
- **Action:** Add path normalization and traversal check (resolve, ensure within app_dir).

### 6. Landlock Rules Too Permissive
**Location:** `stub/src/landlock.rs:54-56`
```rust
const READ_ONLY: u64 = EXECUTE | READ_FILE | READ_DIR; // allows execution outside rootfs
```
- READ_ONLY allows `EXECUTE` everywhere — defeats filesystem sandbox
- **Action:** Remove `EXECUTE` from READ_ONLY; only allow on rootfs.

### 7. Incomplete SAFETY Comments on Unsafe Blocks
**Locations:**
- `stub/src/config.rs:190` — "We're just modifying terminal settings temporarily" (too vague)
- `stub/src/exec.rs:763-766` — basic but could detail lifetime/validity
- `stub/src/exec.rs:889-904` — signal handler, static mut (see #2)
- `stub/src/main.rs:520, 640, 668, 725, 988, 1009, 1053` — syscall wrappers
- **Action:** Each `unsafe` block needs: what invariant is upheld, why it's sound, what could go wrong if wrong.

### 8. Large Functions Need Splitting
| File | Function | Lines |
|------|----------|-------|
| `xbin-cli/src/commands/build.rs` | `build_single_target` | ~700 |
| `xbin-cli/src/commands/build.rs` | `run` | ~150 |
| `stub/src/main.rs` | `run` | ~150 |
| `xbin-core/src/detect.rs` | `resolve_entrypoint` | ~140 |
| `xbin-core/src/assembly.rs` | `assemble_xbin_with_sisr_artifacts` | ~80 |

**Action:** Extract helpers, use config structs for >7 params.

### 9. Config Structs for Functions with >7 Parameters
- `assembly.rs:build_meta_json` (8 params)
- `assembly.rs:assemble_xbin` (7 params)
- `assembly.rs:assemble_xbin_with_sisr` (7 params)
- `assembly.rs:assemble_xbin_with_sisr_artifacts` (8 params)
- **Action:** Create `BuildMetaConfig`, `AssembleConfig` structs.

---

## 🟡 Medium (Track for Next Release)

### 10. Property-Based Testing for Critical Algorithms
- **Chunking** (`xbin-core/src/chunker.rs`) — FastCDC content-defined chunking
- **Merkle tree** (`xbin-core/src/sisr_stage.rs:164-190`) — pairing with duplication
- **Binary format parsing** (`xbin-core/src/format.rs`) — footer read/pack roundtrips
- **SISR engine** (`xbin-core/src/sisr/engine.rs`) — reuse index, chunk resolution
- **Action:** Add `proptest` tests for each; integrate into CI.

### 11. Windows Support Gaps
- **Flock:** `stub/src/main.rs:1063-1066` — `flock_exclusive` is no-op on Windows
- **Health gate service supervision:** `stub/src/main.rs:587-592` — returns `Unsupported`
- **Signal handling:** Unix-only `static mut CHILD_PIDS`, `signal_forward`
- **Action:** Implement proper Windows locking (named mutex), document limitations, or gate features.

### 12. HTTP Request Timeouts Configurable
**Location:** `stub/src/main.rs:882-894` (`http_get_bytes`)
- Hardcoded: connect 10s, recv response 30s, recv body 30s
- **Action:** Expose via `XBIN_HTTP_TIMEOUT_*` env vars or config.

### 13. Seccomp Denylist Completeness Review
**Location:** `stub/src/seccomp.rs` — blocks ~14 syscalls
- Review against current kernel syscall table
- Consider allowlist approach for stricter sandboxing
- **Action:** Document threat model, add missing dangerous syscalls (e.g., `bpf`, `userfaultfd`, `ptrace` variants).

### 14. Embedded Interpreter Integrity Verification
**Location:** `xbin-cli/src/commands/build.rs:845` (`embed_interpreter`)
- Interpreter binaries copied from host PATH without verification
- **Action:** Optional: verify checksum against known-good values, or require `--interpreter-path` with explicit hash.

### 15. Race Condition in Cache GC
**Location:** `stub/src/main.rs:943-967` (`gc_extraction_cache`)
- Reads dir, sorts by mtime, removes oldest — concurrent extraction can race
- **Action:** Use lock file or atomic rename marker for GC coordination.

---

## 🟢 Low (Technical Debt)

### 16. Remove `too_many_lines` Allowance
**Location:** `stub/Cargo.toml:85` — `too_many_lines = "allow"`
- Refactor functions >100 lines (see #8)

### 17. Replace Hardcoded Constants with Configuration
| Constant | Location | Suggested Config |
|----------|----------|------------------|
| PHP server port 8080 | `xbin-core/src/detect.rs:680` | `--php-port` / env |
| Health check timeout default | `xbin-core/src/sisr/health.rs` | `--health-timeout` |
| Cache max entries (16) | `stub/src/main.rs:300` | `--cache-max-entries` |
| Extraction limits (1GB, 50k files) | `stub/src/extraction.rs:15-17` | `--max-extract-size` |

### 18. Signed vs Unsigned Format Version Confusion
**Location:** `xbin-core/src/format.rs:113-116`, `stub/src/main.rs:258-263`
- v2 files have `sig_offset=0`, no signature verification
- v3+ verifies signature if `FLAG_SIGNED` set
- **Action:** Document clearly, consider rejecting v2 signed files.

### 19. Backward/Forward Compatibility Testing
- v2 → v5 format upgrades
- SISR manifest versioning
- **Action:** Add integration tests for cross-version compatibility.

### 20. Error Messages Leak Internal Details
- Some errors include internal paths, offsets, struct fields
- **Action:** Sanitize user-facing errors; log full details at debug level only.

### 21. Missing Integration Tests
- Landlock sandboxing E2E
- Seccomp with actual syscall blocking (not just "allows normal execution")
- Cross-compilation scenarios (linux→macos, linux→windows)
- **Action:** Add to `stub/tests/`.

### 22. ANSSI-Rust Compliance Gaps in xbin-core
Despite clippy allows:
- `unwrap_used = "allow"` — replace with proper error handling
- `expect_used = "allow"` — replace with context
- Check for `panic!()` in library code
- Check for `mem::forget` / `.leak()`
- **Action:** Audit and fix incrementally.

---

## ✅ Positive Findings (Maintain)

- Zero `unsafe` in `xbin-core` and `xbin-cli`
- All `unsafe` confined to `stub/src/` (launcher only)
- `zeroize` used for secrets (encryption keys, nonces)
- Constant-time SHA-256 comparison implemented
- Atomic extraction with flock + tmp→rename
- SISR delta updates with Merkle tree + signature verification
- Signature verified BEFORE decryption
- Good crate separation: core (safe) / stub (unsafe) / cli (safe)
- All tests pass, clippy pedantic passes, fmt passes
- Cargo.lock tracked
- Stable toolchain only

---

## 📋 Suggested PR Order

1. **PR 1:** Fix static mut signal handler (#2) + improve SAFETY comments (#7)
2. **PR 2:** Document encryption limitation clearly (#1) + Landlock EXECUTE fix (#6)
3. **PR 3:** Path traversal protection for includes (#5)
4. **PR 4:** Split large functions (#8) + config structs (#9)
5. **PR 5:** Property-based tests for chunking/merkle/format (#10)
6. **PR 6:** Windows flock + health gate service support (#11)
7. **PR 7:** Configurable timeouts + seccomp review (#12, #13)
8. **PR 8:** Remove hardcoded constants (#17) + error sanitization (#20)
9. **PR 9:** ANSSI compliance cleanup in xbin-core (#22)
10. **PR 10:** Interpreter integrity + cross-version tests (#14, #19)

---

## 🏷️ Labels for Tracking

| Label | Issues |
|-------|--------|
| `security` | #1, #2, #3, #4, #5, #6 |
| `unsafe-audit` | #2, #7 |
| `refactor` | #8, #9, #16 |
| `testing` | #10, #21 |
| `windows` | #11 |
| `configurable` | #12, #17 |
| `sandbox` | #6, #13 |
| `docs` | #1, #18 |
| `tech-debt` | #16, #17, #20, #22 |