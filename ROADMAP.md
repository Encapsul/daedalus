# x.bin Security & Technical Roadmap

Generated from security audit and code review. All tests pass, clippy passes, fmt passes — these are issues beyond the verification loop.

> **Second review pass (2026-08-10):** items #23–#42 added after a full codebase re-read. Note: #2 (static mut signal handler) and #18 (v2/v3 footer) are already resolved in the current code (`OnceLock<Mutex<Vec<i32>>>`, version-gated footer parse) — kept above for historical context.
>
> **Third review pass (2026-08-10):** items #43–#50 added after a follow-up review. Note: #6 (Landlock READ_ONLY allows EXECUTE) is resolved in the current code (`landlock.rs:55` = `READ_FILE | READ_DIR` only) — the remaining Landlock problems are covered by #44.

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

### 23. `--squashfs` Builds Are Broken (Payload Is Always zstd+tar)
**Location:** `xbin-cli/src/commands/build.rs:914, 1249`, `stub/src/main.rs:302-305`, `stub/src/squashfs_extract.rs`
- The CLI always creates the payload with `create_tar_zstd_with_level` (zstd+tar); `--squashfs` only sets `meta.payload_format = "squashfs"` + format version 5
- At runtime the stub sees `payload_format == "squashfs"` and tries to parse the zstd+tar bytes as a squashfs image with `backhand::v4::FilesystemReader` → parse error, extraction fails on first run
- **No `mksquashfs` invocation exists anywhere in the CLI** (doctor.rs checks for it; nothing uses it)
- **Action:** Either implement real squashfs creation (shell out to `mksquashfs`), or make `--squashfs` fail loudly at build time with "not implemented". Also fix the asymmetry: zstd+tar extraction limit is 1GB/50k files, squashfs path has different bounds.

### 24. `--encrypt --enable-sisr` Produces Undecryptable Binaries
**Location:** `xbin-cli/src/commands/build.rs:957-985`, `stub/src/main.rs:280-291`, `xbin-core/src/encrypt.rs:146-232`
- Build encrypts per-chunk (`encrypt_chunks`) and tags crypto meta with `"chunked": true`
- The stub only takes the chunked decrypt path when `meta.layers` is non-empty — but the CLI always writes `layers: []` (`build.rs:1254`) → stub falls back to full-payload `decrypt_aes_gcm` → GCM tag authentication fails → binary unusable
- The SISR manifest chunk table describes *plaintext* offsets/lengths, so the update engine slices ciphertext incorrectly even if decryption were fixed
- **Action:** Populate `meta.layers` from the SISR chunk table (with ciphertext-aware offsets), or reject the `--encrypt --enable-sisr` combination at build time until the chunk boundaries match the encrypted bytes.

### 25. `--tree-shake` / `--minify` Destroy the User's Source Tree In Place
**Location:** `xbin-cli/src/commands/build.rs:571-585`, `xbin-core/src/treeshake.rs:178`, `xbin-core/src/minify.rs:70`
- `prune_node_modules(app_dir)` and `minify_app_dir(app_dir)` operate directly on `plan.app_dir` (the user's project), *before* the copy into the rootfs happens (`build.rs:808`)
- Unused node_modules packages are permanently deleted and JS/CSS rewritten in the user's source tree; irreversible without `npm install` again
- Runs once per target on multi-arch builds (second run is a no-op only by luck)
- **Action:** Copy the app to a staging dir first, then tree-shake/minify the staging copy. Never mutate the source directory.

### 43. Signature Does Not Cover the Footer — Downgrade Bypass
**Location:** `xbin-cli/src/commands/sign.rs:110-115`, `stub/src/main.rs:258-268`, `xbin-core/src/format.rs`
- The signed digest is only `SHA-256(payload ‖ meta)` — the footer bytes (`format_version`, `flags`, `payload_sha256`, offsets) are NOT covered by the signature
- The stub verifies the signature only when `format_version >= 3 && FLAG_SIGNED` (`main.rs:258-263`); the SHA-256 check (`main.rs:268`) is unkeyed and recomputable by anyone
- Attack: take any signed `.xbin`, flip `format_version` 5→2 (or clear `FLAG_SIGNED`), replace the payload, recompute `payload_sha256` → signature AND decryption (version < 4 = `CRYPTO_NONE`) are both skipped → arbitrary code execution from a "signed" file
- **Action:** include the packed footer (version + flags + payload offset/size) in the signed digest; reject mixed-state files (sig block present but `FLAG_SIGNED` unset); verify the signature whenever a sig block exists, regardless of flags.

### 44. Landlock Sandbox Is Dead Code and Fails Open
**Location:** `stub/src/exec.rs:411-415`, `stub/src/main.rs:981-1024`, `stub/src/landlock.rs:33, 49`
- `sandbox(rootfs)` (`exec.rs:411`) is called with the **pre-pivot absolute path after** `pivot_root_into` has already detached the old root (`umount2(MNT_DETACH)`, `main.rs:1018`) → path unreachable → `add_path_beneath` fails with ENOENT → warn-only (`exec.rs:412-414`) → app runs **unsandboxed**
- `HANDLED_FS` includes `LANDLOCK_ACCESS_FS_TRUNCATE` (bit 14), which requires Landlock **ABI v3 = kernel ≥ 6.2**, while the module claims ABI v1 / 5.13+ → `create_ruleset` returns EINVAL on 5.13–6.1 → sandbox fails → fail-open
- `REFER` (bit 13) is missing from `HANDLED_FS` → once the above is fixed, cross-directory rename/link inside rootfs is denied despite the `FULL_RW` grant
- **Action:** open an `O_PATH` dir fd of rootfs **before** `pivot_root` and pass it to `add_path_beneath`; drop TRUNCATE or gate it on kernel ≥ 6.2; add `REFER`; and **fail closed** — abort when the requested sandbox cannot be enforced.

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

### 26. Embedded Interpreters Fail on Hosts Without the Runtime
**Location:** `stub/src/exec.rs:452, 458-465`
- For interpreted runtimes, the pre-flight `is_executable(interpreter_name)` searches the **host** PATH (env not yet overridden — `setup_env`/`set_var` run later at `exec.rs:467-475`)
- With `--embed-interpreter` on a host lacking python3/node, execution fails with "interpreter 'python3' not found" even though the rootfs copy exists and `execvp` would have found it via the rootfs-first PATH
- This breaks the exact use case embedding was built for (run on machines without the runtime)
- **Action:** Use the rootfs PATH for the pre-flight check, and/or set argv[0] to the absolute rootfs path (also removes PATH ambiguity).

### 27. `xbin upgrade` Installs Unverified Binaries (With Sudo)
**Location:** `xbin-cli/src/commands/upgrade.rs:81-86, 124-171, 285-293`
- Checksum fetch failure is a **warning only** — the download proceeds unverified and is installed via `sudo cp`
- No signature verification (TOFU) of release tarballs; self-update + sudo = supply chain risk
- `is_writable()` only checks the POSIX readonly bit, which does not reflect actual writability
- **Action:** Fail closed when the checksum cannot be verified; verify the release signature; attempt a real write probe for writability.

### 28. `xbin sign` on a SISR-Enabled File Corrupts It
**Location:** `xbin-cli/src/commands/sign.rs:98`, `xbin-cli/src/commands/build.rs:1350`
- `sign.rs` only checks `is_signed()`, not `FLAG_SISR`; build.rs guards the combination, but the standalone `xbin sign <file>` does not
- Inserting the sig block between meta and footer shifts the SISR manifest/SisrFooterExt offsets → unbootable binary
- **Action:** Bail in `sign_file` when `footer.has_sisr()` (or support signature-insertion in the SISR layout properly).

### 45. SISR Binaries Are Never Signed at Rest; Updates Strip Signing
**Location:** `xbin-core/src/sisr/engine.rs:199`, `xbin-cli/src/commands/build.rs:1350-1370`, `xbin-core/src/legacy.rs:59-63`
- `apply_update_with_stats` rewrites the footer as `(flags & !FLAG_SIGNED) | FLAG_SISR` (`engine.rs:199`) — every applied update permanently removes binary signing
- With `--enable-sisr`, `--key` signs only the manifest (`build.rs:1367-1370`, `manifest_signed`), never the binary; signed binaries cannot be upgraded at all (`legacy.rs:59-63`)
- Result: SISR binaries have no authenticity at rest — only the manifest signature, verified during the update flow, protects them
- **Action:** resign after update (key available at update site) or build a signature chain — sign `SHA256(new_footer_hash ‖ parent_footer_hash)` so the stub verifies updates offline; at minimum document that SISR files are authenticated only during the update.

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

### 29. Build Cache Key Ignores Build Configuration
**Location:** `xbin-cli/src/commands/build.rs:595`, `xbin-core/src/paths.rs` (`BuildCache`)
- `BuildCache::find(&new_app_hash, target)` keys only on app hash (+ target) — not on encrypt/sign/isolation/seccomp/landlock/env/squashfs flags
- Two builds of the same app with different options reuse the same cached artifact → stale/wrong binary copied to output
- The hash is also computed *after* the destructive tree-shake/minify mutation (see #25)
- **Action:** Include a config-hash (canonicalized build options) in the cache key.

### 30. Stub Config Precedence Inverted (Global Overrides Local)
**Location:** `stub/src/config.rs:38-54, 98-107`
- `AppConfig::load()` merges local `xbin.toml` first, then global `~/.xbin/config.toml`; `merge()` *replaces* `database`/`secrets` wholesale
- Result: global config overrides local config — the opposite of the documented layering (local > env > global)
- **Action:** Swap merge order or make `merge()` fill-in (local wins), and add a precedence test.

### 31. Local Config Next to the Binary Is Silently Trusted
**Location:** `stub/src/config.rs:62-78, 101-116`
- The stub reads `xbin.toml` / `config.toml` from the *binary's directory* and injects `[secrets]` as `XBIN_SECRET_*` + `DATABASE_URL` into the app
- For a shared install dir (e.g. `/usr/local/bin`), anyone able to write that directory hijacks secrets for every user who runs the binary; the generic `config.toml` fallback name can also pick up unrelated files
- **Action:** Document the threat model; consider requiring `xbin.toml` only (not the generic `config.toml`), validating ownership/permissions, and warning when the file is world-writable.

### 32. Warm Start Skips Signature + Integrity Verification
**Location:** `stub/src/main.rs:225-228`
- On extraction-cache hit, `exec_app` runs without signature/Ed25519 or SHA-256 verification (verified only on the cold path)
- Cache poisoning at `~/.cache/xbin/<hash>/rootfs` = arbitrary code execution as the invoking user
- **Action:** Same-user only today (acceptable), but document the trust model, verify cache ownership/perms, and optionally re-verify the hash cheaply.

### 33. `--seccomp` / `--landlock` Are Silent No-Ops Without `--isolation 2`
**Location:** `stub/src/exec.rs:394-417, 642-660`
- Both sandbox features are gated behind `use_pivot` (isolation ≥ 2); a user passing `--seccomp` alone gets an unsandboxed build with no warning
- **Action:** Warn at build time and/or runtime when the flags cannot take effect.

### 34. `--xbin-update` Arg-Swallowing Heuristic
**Location:** `stub/src/update_url.rs:23-28`
- "Next non-dash argv is the update URL" consumes the app's first positional argument (e.g. `app.xbin --xbin-update serve` eats `serve`)
- **Action:** Require `--xbin-update=<URL>` or a dedicated separator; never guess from positionals.

### 35. App-Bundled `.env` Overrides Explicit Configuration
**Location:** `stub/src/exec.rs:113-116`
- `load_dotenv(rootfs, ...)` is applied last and overwrites `meta.env` (`--env` flags) and `DATABASE_URL` from config on key collisions
- **Action:** Decide and document precedence; `--env`/config should win over the app's own `.env`.

### 36. Unconditional `remove_dir_all("/tmp/xbin-build-tools")`
**Location:** `xbin-cli/src/commands/build.rs:907`
- Legacy hardcoded deletion of a shared `/tmp` path (potentially created by another user/process), plus the cache build-tools dir
- **Action:** Remove the `/tmp` branch; only clean the tool's own `cache_dir()/build-tools`.

### 37. Unverified Build-Time Tool Downloads
**Location:** `xbin-cli/src/commands/build.rs:1660` (node tarball), `build.rs:1693` (composer.phar via `php -r copy(...)`)
- Downloaded over HTTPS with no checksum pinning; composer runs without script suppression
- **Action:** Pin checksums, use `--no-scripts` / `--ignore-scripts` everywhere, or require a user-provided tool path.

### 46. Extraction `.ready` Marker Location Mismatch
**Location:** `stub/src/extraction.rs:89, 94-102`
- The marker is written to `tmp/.ready` (`extraction.rs:89`) — i.e. `cache_root/.ready` after the rename — but the rename-failure fallback checks `rootfs/.ready` (`extraction.rs:94`), which is never created
- Any rename failure therefore always returns `Err`; worse, a stale partial `cache_root` (no `.ready`) fails the rename on **every** cold start with no cleanup — the binary is permanently stuck until the user deletes the cache dir manually
- **Action:** check `cache_root/.ready` in the fallback; on rename failure wipe the stale `cache_root` and retry once.

### 47. `tar.rs` Drops Symlinks, Over-Broad `.git` Exclusion, Non-Deterministic Output
**Location:** `xbin-core/src/tar.rs:54, 103-117` (+ `xbin-core/src/compress.rs:27`)
- Symlinks are silently skipped (`tar.rs:107`); the comment at `tar.rs:103` claims they are "followed by the tar builder", which is wrong — apps relying on symlinks (node_modules/.bin, venvs, `usr/bin/python3 → python3.11`) break at runtime
- `rel_str.contains(".git")` (`tar.rs:117`) excludes any file whose path contains `.git` (e.g. `.gitignore`, `.gitattributes`)
- `multithread(num_cpus())` (`tar.rs:54`, `compress.rs:27`) makes compressed output machine-dependent → non-reproducible builds, defeating the incremental hash-reuse feature (#29)
- **Action:** store symlinks as symlink tar entries (with guarded targets); use per-component matching (`.git` only as a top-level dir); pin the zstd thread count for deterministic output.

### 48. `.env` Is Packaged Into the Binary (Secret Leak)
**Location:** `xbin-core/src/dotenv.rs:66-91`, skip lists in `xbin-core/src/include.rs:10, 93`, `xbin-core/src/tar.rs:117`
- `.env` is in no skip list → secrets land in the compressed payload of the redistributable and are extracted in plaintext to `~/.cache/xbin/<hash>/rootfs/.env` (runtime loads it, `stub/src/exec.rs:113`)
- `load_dotenv` only *warns* about secret-looking keys (`dotenv.rs:84`) — it still packages them
- **Action:** exclude `.env` by default (hard error unless `--include .env` is explicit), and document the risk. Related to #35 (precedence), but this is a packaging leak.

### 49. `wait_for_children` Reaps by Wildcard with Blind Accounting
**Location:** `stub/src/exec.rs:821-832`
- `waitpid(-1, ...)` waits for any child while `remaining` is decremented per reaped pid, without checking the pid is one of the supervised services
- Any spurious SIGCHLD or `ECHILD` (children already reaped elsewhere) decrements wrongly or breaks the loop early → wrong exit status, or a supervisor that reports success while services still run
- **Action:** `waitpid(pid, ...)` per tracked service (or verify the returned pid is in the set before decrementing), and treat `ECHILD` as an error only if `remaining > 0`.

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

### 38. SISR Footer Extension Fields Not Bounded at Parse Time
**Location:** `xbin-core/src/sisr_header.rs`, `stub/src/main.rs` (SisrFooterExt read)
- `chunk_table_len` is trusted from the footer ext without a sanity bound check before slicing
- **Action:** Bound `chunk_table_len` against the file size / max manifest size at read.

### 39. Signal Handler Uses `Mutex::lock()` + `signal()` Instead of `sigaction`
**Location:** `stub/src/exec.rs:895-927`
- `signal_forward` calls `Mutex::lock()` inside a signal handler (async-signal-unsafe; low risk today because only the main thread locks it) and `signal(2)` is legacy
- **Action:** Switch to `sigaction`, use a lock-free/atomic structure, or a self-pipe to defer work to the main loop.

### 40. Unknown Runtime Silently Maps to `bash`
**Location:** `stub/src/exec.rs:510-519`
- `resolve_entrypoint` maps any unrecognized runtime to `bash`; a typo'd runtime name launches via bash (or errors misleadingly)
- **Action:** Reject unknown runtimes at build time; stub should error with a clear message.

### 41. Dead Code: `meta.layers` Always Empty → Layered Cache Dormant
**Location:** `xbin-cli/src/commands/build.rs:1254` (`layers: &[]`), `stub/src/main.rs:229-233` (`cache_key_v2`, `slice_layers`)
- The layered extraction/cache path and `cache_key_v2` can never trigger with CLI-built binaries
- **Action:** Either wire layers up (see #24) or delete the dormant paths to reduce review surface.

### 42. `sign.rs` Hardcodes `sig_size = 64`
**Location:** `xbin-cli/src/commands/sign.rs:117`
- Works for Ed25519 today, but the sig block layout is duplicated with `SIG_BLOCK_SIZE`/`SIG_BLOCK_SIZE_FIELD` constants
- **Action:** Use the shared format constants instead of literals.

### 50. Java Entrypoint Emits Literal `$PORT`
**Location:** `xbin-core/src/detect.rs:392`
- `cmd.push("-Dserver.port=$PORT".into())` — the placeholder is never expanded; Java apps receive the literal argument `-Dserver.port=$PORT` (the server tries to bind port `"$PORT"` and fails)
- **Action:** expand `$PORT` / `$XBIN_PORT` at runtime from the environment, or drop the flag and rely on the web-port detection (`stub/src/exec.rs:154`).

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
11. **PR 11:** Stage the app copy before tree-shake/minify (#25) + cache key includes config (#29)
12. **PR 12:** Fix `--squashfs` (real mksquashfs or loud failure) + `--encrypt --enable-sisr` (#23, #24)
13. **PR 13:** Embedded interpreter exec fix (#26) + `xbin sign` SISR guard (#28)
14. **PR 14:** `xbin upgrade` fail-closed checksum + release signature verification (#27)
15. **PR 15:** Stub config precedence + trust model (#30, #31) + warm-start verification note (#32)
16. **PR 16:** Sandbox no-op warnings (#33) + update URL parsing (#34) + env precedence (#35) + cleanup (#36, #37, #38, #39, #40, #41, #42)
17. **PR 17:** Sign the footer version+flags so downgrades are impossible (#43)
18. **PR 18:** Landlock: pre-pivot fd, fail closed, ABI-aware flags (#44)
19. **PR 19:** SISR at-rest signing / post-update resign (#45)
20. **PR 20:** Extraction marker fix (#46) + packaging/dotenv/supervision cleanup (#47, #48, #49, #50)

---

## 🏷️ Labels for Tracking

| Label | Issues |
|-------|--------|
| `security` | #1, #2, #3, #4, #5, #6, #27, #31, #32, #37, #43, #45, #48 |
| `unsafe-audit` | #2, #7, #39 |
| `refactor` | #8, #9, #16, #41, #42 |
| `testing` | #10, #21 |
| `windows` | #11 |
| `configurable` | #12, #17 |
| `sandbox` | #6, #13, #33, #44 |
| `docs` | #1, #18 |
| `tech-debt` | #16, #17, #20, #22, #36, #40 |
| `broken-feature` | #23, #24, #25, #26, #28, #34, #35, #46, #47, #49, #50 |
| `cache-correctness` | #29, #30 |

---

## 🧭 Product Direction: Composable Execution Artifact

### The XKCD 927 trap (we are aware of it)

> *"There are 14 competing standards." → "14?! Ridiculous! We need one universal standard that covers all use cases." → "There are 15 competing standards."*

x.bin is a packaging format standing next to AppImage, Snap, Flatpak, deb, Docker, OCI, Nix, Wasm, pkg, PyInstaller. **Positioning x.bin as "a better AppImage", "a Docker killer", or "a universal format" guarantees we become the 15th standard.** The reframe is deliberately *not* a marketing slogan — it is a **feature-prioritization tool**: "compose existing formats instead of reinventing them", then prioritize by **architectural fit** (how directly a feature plugs into the existing CAS + layers + manifest + sandbox pipeline).

> Positioning statement: **x.bin is a portable execution artifact that composes existing software artifacts into a single verifiable, updateable, sandboxed unit.**

### Feature backlog, prioritized by architectural fit

| # | Feature | Fit | Priority | Why |
|---|---------|-----|----------|-----|
| F1 | **OCI import** (`xbin import docker://...`) | 🔴 high | **Now** | x.bin packages a rootfs; an OCI image *is* a layered rootfs. Reuses detection → layers → CAS → sign → sandbox → update unchanged. Widest app-catalog expansion with zero new infrastructure. |
| F2 | **Large payloads / mmap (AI model case)** | 🔴 high | **Now** | The flagship "local AI" use case (llama.cpp + multi-GB model + web UI) needs no GGUF support — a model is just a data file (`--include`). The real gap is streaming + mmap for multi-GB payloads without disk extraction. Already Phase 3 in docs. |
| F3 | **Wasm runtime embedding** | 🟡 medium | Later | Requires embedding a runtime (wasmtime) in the stub + a new isolation model = literally a 12th runtime. Competes with wasmer. Build only on demonstrated demand. |
| F4 | **Nix closure support** | 🟢 low | Later | `nix-store -qR` presupposes Nix installed. A build *input*, not a distribution format users consume as a single file. Only if users are already Nix users. |

### Design rules (anti-XKCD)

1. **Never invent a new on-disk standard.** ELF, tar, zstd, SquashFS, Ed25519, POSIX rootfs — all standard. Novelty is the pipeline, not a new container/archive format.
2. **One evolving format, not many.** New features extend `.xbin` backward-compatibly (v2→v3→v4→v5). Never fork the format or introduce a parallel one.
3. **Encapsulate, don't absorb.** OCI/Nix/Wasm are *referenced or embedded as blobs*, never rewritten into a new proprietary representation.
4. **Interop over lock-in.** A `.xbin` is unpackable with standard tools (`tar`, `unsquashfs`, `zstd`); no proprietary reader needed to access the payload.
5. **Narrow scope.** Headless web/server headless only. Desktop integration, orchestration, and package-registry are explicitly out — that is what keeps us off the 15th-standard slide.