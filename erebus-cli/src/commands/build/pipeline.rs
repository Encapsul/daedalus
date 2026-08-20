use crate::remote_cache::remote_cache_from_args;
use anyhow::{Context, Result};
use erebus_core::detect;
use erebus_core::embed;
use erebus_core::metadata::{BunFeatures, EmbeddedInterpreter};
use erebus_core::paths::cache_dir;
use erebus_core::pkgmgr;
use std::path::PathBuf;

use super::args::{config_fingerprint, parse_target, BuildArgs, BuildPlan};
use super::deps::{
    check_php_platform_reqs, ensure_composer, ensure_node, has_workspace_protocol,
    is_command_available,
};
use super::payload::{copy_dir_recursive_with, create_squashfs_payload, include_points_to_env};
use super::sign::sign_macos_binary;
use super::sisr::{build_sisr_config, report_sisr_bandwidth};
use super::stub::{find_stub, read_existing_hashes};

/// Warn when `--seccomp`/`--landlock` are requested but the stub will not
/// enforce them (they only apply at isolation >= 2, on the `pivot_root` path).
pub(crate) fn warn_sandbox_noops(isolation_num: u32, seccomp: bool, landlock: bool) {
    if isolation_num < 2 && (seccomp || landlock) {
        let flags = [("seccomp", seccomp), ("landlock", landlock)]
            .iter()
            .filter_map(|(name, on)| on.then_some(*name))
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "[erebus] warning: {flags} require --isolation sandbox (2) to take effect — \
             ignored at isolation level {isolation_num}"
        );
    }
}

pub(crate) fn build_single_target(
    args: &BuildArgs,
    plan: &BuildPlan,
    target: Option<String>,
    output: &PathBuf,
) -> Result<Option<serde_json::Value>> {
    let verbose = plan.verbose;
    let app_dir = &plan.app_dir;
    let runtime = plan.runtime;
    let runtime_name = &plan.runtime_name;
    let isolation_num = plan.isolation_num;
    let no_install = plan.no_install;
    let seccomp = plan.seccomp;
    let landlock = plan.landlock;
    let gui = plan.gui;
    let encrypt = plan.encrypt;
    let squashfs = plan.squashfs;
    let version_info = plan.version_info.clone();
    let author = plan.author.clone();
    let description = plan.description.clone();
    let license = plan.license.clone();
    let env_file = plan.env_file.clone();

    // ── Sandbox flags without isolation are silent no-ops ──────────────
    // seccomp/landlock are only enforced in the stub when isolation >= 2
    // (pivot_root + namespace path). Warn so the user isn't lulled into
    // believing the sandbox is active.
    warn_sandbox_noops(isolation_num, seccomp, landlock);

    // ── Reject the broken encrypt+SISR combination ─────────────────────
    // The stub's chunked-decrypt path requires per-layer sizes from
    // `meta.layers`, but the CLI always assembles with an empty layer list —
    // the produced binary would carry a chunk-encrypted payload that the
    // stub can only attempt to decrypt as one GCM blob, failing at runtime.
    if encrypt && args.enable_sisr {
        anyhow::bail!(
            "--encrypt and --enable-sisr cannot be combined: chunk-encrypted SISR \
             binaries are not decryptable by the current stub — use one or the other"
        );
    }

    // ── Reject a bundled `.env` unless it is included explicitly ───────
    // `.env` is excluded from the payload by default (secrets would be
    // extractable from the redistributable); silently dropping it could
    // break an app that relied on it, so make the builder decide.
    if app_dir.join(".env").is_file() && !include_points_to_env(app_dir, &args.include) {
        anyhow::bail!(
            "{} contains a .env file, which is excluded from the payload by default \
             (its secrets would be extractable from the binary). Bundle it explicitly \
             with `--include {}` to accept that risk, or provide configuration via \
             `--env-file` / `--env` instead.",
            app_dir.display(),
            app_dir.join(".env").display()
        );
    }

    // ── Compute hashes for incremental update ──────────────────────────
    // Hashed BEFORE tree-shake/minify run: those mutate a staging copy
    // below, and the cache/`--update` keys must reflect the pristine
    // source, not its by-products, or a rebuild would never match.
    let new_app_hash = erebus_core::include::hash_app_files(app_dir);
    let new_rt_hash = erebus_core::include::hash_lock_file(app_dir);

    // ── Intelligent build cache: skip rebuild if hash matches ──────────
    let cfg_hash = config_fingerprint(args, plan);
    if args.use_cache {
        let cache = erebus_core::paths::BuildCache::new(app_dir, 50);
        if let Some(cached) = cache.find(&new_app_hash, &cfg_hash, target.as_deref()) {
            if verbose {
                eprintln!("[erebus] cache hit — reusing cached build");
            }
            std::fs::copy(&cached, &output).context("failed to copy cached .erebus to output")?;
            if args.json {
                return Ok(Some(serde_json::json!({
                    "output": output.to_string_lossy(),
                    "runtime": runtime_name.as_str(),
                    "cache_hit": true,
                })));
            }
            return Ok(None);
        }
        if verbose {
            eprintln!("[erebus] cache miss — building from scratch");
        }
    }

    if args.clear_cache {
        let cache = erebus_core::paths::BuildCache::new(app_dir, 50);
        cache.clear().ok();
        if verbose {
            eprintln!("  cache: cleared");
        }
    }

    // ── Incremental update: skip rebuild if nothing changed ────────────
    if args.update && output.exists() {
        if let Some((old_app_hash, old_rt_hash)) = read_existing_hashes(output) {
            if old_app_hash == new_app_hash && old_rt_hash == new_rt_hash {
                if verbose {
                    eprintln!("[erebus] everything up to date, nothing to rebuild");
                }
                return Ok(None);
            } else if old_rt_hash == new_rt_hash && old_app_hash != new_app_hash {
                if verbose {
                    eprintln!("[erebus] app changed, reusing runtime layer (full layer reuse not yet supported in Rust CLI — doing full rebuild)");
                }
            } else if verbose {
                eprintln!("[erebus] runtime deps changed, full rebuild");
            }
        }
    }

    // ── PHP platform extensions check ──────────────────────────────────
    if runtime_name == "php" && !no_install {
        check_php_platform_reqs(app_dir, verbose)?;
    }

    // Detect pnpm workspace with workspace:* protocol (cannot use npm)
    if pkgmgr::detect_node_pkgmgr(app_dir) == Some(pkgmgr::PkgMgr::Npm)
        && has_workspace_protocol(app_dir)
    {
        eprintln!("[erebus] warning: package.json uses `workspace:*` protocol (pnpm-specific)");
        eprintln!("  but pnpm is not detected. Create `pnpm-workspace.yaml` or add a lockfile.");
    }

    // Detect and install all package managers (primary + secondary)
    let all_pkg_mgrs = pkgmgr::detect_all_pkgmgrs(app_dir, runtime_name);
    for mgr in &all_pkg_mgrs {
        if verbose {
            eprintln!("Package manager: {}", mgr.name());
        }

        if !no_install {
            let cmd = mgr.install_cmd();

            // Check if the binary exists before trying to run it. For
            // cross-target builds the builder's host toolchain has the wrong
            // arch/OS, so always download the target node instead.
            let need_target_node = target.is_some()
                && matches!(
                    mgr,
                    pkgmgr::PkgMgr::Npm
                        | pkgmgr::PkgMgr::Pnpm
                        | pkgmgr::PkgMgr::Yarn
                        | pkgmgr::PkgMgr::Bun
                );
            let mut node_bin_dir: Option<PathBuf> = None;
            if !is_command_available(cmd[0]) || need_target_node {
                // For node/npm: download static node to temp dir
                if matches!(
                    mgr,
                    pkgmgr::PkgMgr::Npm
                        | pkgmgr::PkgMgr::Pnpm
                        | pkgmgr::PkgMgr::Yarn
                        | pkgmgr::PkgMgr::Bun
                ) {
                    let bin_dir = ensure_node(target.as_deref(), verbose)?;
                    node_bin_dir = Some(bin_dir);
                } else {
                    eprintln!(
                        "[erebus] skipping {} — `{}` not found on PATH",
                        mgr.name(),
                        cmd[0]
                    );
                    continue;
                }
            }

            // For composer: auto-download composer.phar if not on PATH
            let (prog, extra_args) = if matches!(mgr, pkgmgr::PkgMgr::Composer) {
                ensure_composer(app_dir, verbose)?
            } else {
                (cmd[0].to_string(), Vec::new())
            };

            let mut full_args: Vec<String> = extra_args;
            full_args.extend(cmd.iter().skip(1).map(|s| s.to_string()));

            // When cross-compiling for a different arch, force source builds for pip
            let is_cross_pip = matches!(mgr, pkgmgr::PkgMgr::Pip)
                && args
                    .cross_compile
                    .as_ref()
                    .is_some_and(|c| c.split(',').any(|t| t.trim() != std::env::consts::ARCH));
            if is_cross_pip {
                full_args.push("--no-binary".into());
                full_args.push(":all:".into());
            }

            // When cross-compiling for a different OS/arch, tell npm/pnpm/yarn/bun
            // to install dependencies for the target platform, not the host.
            //
            // We also pass --ignore-scripts: the target node binary cannot run
            // on a foreign host (e.g. macOS node on Linux), so any install
            // scripts that compile native modules would fail. Pure-JS deps and
            // prebuilt binaries selected by --platform/--arch still work.
            let is_cross_node = matches!(
                mgr,
                pkgmgr::PkgMgr::Npm
                    | pkgmgr::PkgMgr::Pnpm
                    | pkgmgr::PkgMgr::Yarn
                    | pkgmgr::PkgMgr::Bun
            ) && target.as_ref().is_some_and(|t| {
                let (t_arch, t_os) = parse_target(t);
                t_arch != std::env::consts::ARCH || t_os != std::env::consts::OS
            });
            if is_cross_node {
                if let Some(target_str) = target.as_ref() {
                    let (t_arch, t_os) = parse_target(target_str);
                    let npm_arch = match t_arch.as_str() {
                        "x86_64" => "x64",
                        "aarch64" | "arm64" => "arm64",
                        "i686" | "i386" | "x86" => "ia32",
                        other => other,
                    };
                    let npm_platform = match t_os.as_str() {
                        "darwin" => "darwin",
                        "windows" => "win32",
                        "linux" => "linux",
                        other => other,
                    };
                    if matches!(mgr, pkgmgr::PkgMgr::Npm | pkgmgr::PkgMgr::Pnpm) {
                        full_args.push("--platform".into());
                        full_args.push(npm_platform.into());
                        full_args.push("--arch".into());
                        full_args.push(npm_arch.into());
                        full_args.push("--ignore-scripts".into());
                    } else if matches!(mgr, pkgmgr::PkgMgr::Yarn) {
                        full_args.push("--platform".into());
                        full_args.push(npm_platform.into());
                        full_args.push("--ignore-scripts".into());
                    } else if matches!(mgr, pkgmgr::PkgMgr::Bun) {
                        full_args.push("--platform".into());
                        full_args.push(npm_platform.into());
                        full_args.push("--arch".into());
                        full_args.push(npm_arch.into());
                        full_args.push("--ignore-scripts".into());
                    }
                }
            }

            let mut command = std::process::Command::new(&prog);
            command.args(&full_args).current_dir(app_dir);

            // If we downloaded node for npm/yarn/bun, prepend its bin dir to PATH
            // using Command::env() instead of mutating global std::env::PATH
            if let Some(ref bin_dir) = node_bin_dir {
                let current = std::env::var("PATH").unwrap_or_default();
                command.env("PATH", format!("{}:{}", bin_dir.display(), current));
            }

            let status = command
                .status()
                .context(format!("failed to run `{}` — is it installed?", prog))?;
            if !status.success() {
                eprintln!(
                    "[erebus] warning: {} installation failed (exit code {})",
                    mgr.name(),
                    status.code().unwrap_or(-1)
                );
            }
        }
    }

    // Clean up downloaded build tools (node/npm, composer.phar) from cache
    // (deferred: for cross-target builds the downloaded node must remain on
    // PATH until the interpreter is embedded below).

    // Find stub binary
    let stub = find_stub(&target)?;
    let stub_bytes = std::fs::read(&stub)
        .with_context(|| format!("failed to read stub binary at {}", stub.display()))?;

    // Create temp directory for layer building
    let tmp = tempfile::tempdir().context("failed to create temp directory")?;
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).context("failed to create rootfs directory")?;

    // Copy app files
    let include_node_modules = true;
    copy_dir_recursive_with(app_dir, &rootfs.join("app"), include_node_modules)
        .context("failed to copy app files")?;

    // ── Tree-shake / minify on the STAGING COPY ───────────────────────
    // Never mutate the source directory: a build must leave `app_dir`
    // byte-identical so repeat builds and `--update` hash comparisons see
    // the real input.
    if args.tree_shake {
        let removed = erebus_core::treeshake::prune_node_modules(&rootfs.join("app"), verbose)
            .context("tree-shaking failed")?;
        if verbose {
            eprintln!("  tree-shake: removed {removed} unused package(s)");
        }
    }

    if args.minify {
        let minified = erebus_core::minify::minify_app_dir(&rootfs.join("app"), verbose)
            .context("minification failed")?;
        if verbose {
            eprintln!("  minify: minified {minified} file(s)");
        }
    }

    // ── Include extra files ───────────────────────────────────────────
    if !args.include.is_empty() {
        let app_dest = rootfs.join("app");
        let count = erebus_core::include::copy_include_paths(&args.include, &app_dest, app_dir)
            .context("failed to copy include paths")?;
        if verbose {
            eprintln!("  include: copied {count} path(s) into rootfs");
        }
    }

    // ── Embed interpreter ──────────────────────────────────
    // NOTE: ensure_node() may have downloaded the target runtime to
    // <cache_dir>/build-tools/<target>/bin and added it to PATH —
    // embed_interpreter picks it up from there (`.exe` first for cross-OS).
    let embedded_interpreter_str = if let Some(ref interp) = args.embed_interpreter {
        Some(interp.clone())
    } else {
        match runtime_name.as_str() {
            "python" => Some("python3".to_string()),
            "node" => Some("node".to_string()),
            "php" => Some("php".to_string()),
            "ruby" => Some("ruby".to_string()),
            "deno" => Some("deno".to_string()),
            _ => None,
        }
    };

    let mut interpreter_embedded = false;
    if let Some(ref interpreter_name) = embedded_interpreter_str {
        if verbose {
            eprintln!("Embedding interpreter: {}...", interpreter_name);
        }

        let interpreter_path = interpreter_name.clone();
        match embed::embed_interpreter(&interpreter_path, &rootfs, Some(app_dir), verbose) {
            Ok(count) => {
                if verbose {
                    eprintln!("Embedded interpreter ({} files copied)", count);
                }
                interpreter_embedded = true;
            }
            Err(e) => {
                eprintln!("[erebus] warning: failed to embed interpreter: {}", e);
            }
        }
    }

    // Pre-compile Python bytecode for faster startup
    if runtime_name == "python" {
        let app_root = rootfs.join("app");
        if app_root.is_dir() {
            let status = std::process::Command::new("python3")
                .args(["-m", "compileall", "-f", "-q"])
                .arg(&app_root)
                .status();
            if let Ok(s) = status {
                if s.success() && verbose {
                    eprintln!("Bytecode compiled (.pyc)");
                }
            }
        }
    }

    // Embed N-API native addon dependencies (.node files → ldd → .so)
    if runtime_name == "node" {
        match embed::embed_napi_addons(&rootfs, verbose) {
            Ok(n) => {
                if verbose && n > 0 {
                    eprintln!("Embedded {} N-API shared library dependencies", n);
                }
            }
            Err(e) => {
                eprintln!("[erebus] warning: N-API addon embedding failed: {e}");
            }
        }
    }

    // Embed RoadRunner binary for Laravel Octane apps (rr.yaml or .rr.yaml)
    if app_dir.join("rr.yaml").is_file() || app_dir.join(".rr.yaml").is_file() {
        if which::which("rr").is_ok() {
            if verbose {
                eprintln!("Embedding RoadRunner...");
            }
            if let Err(e) = embed::embed_interpreter("rr", &rootfs, None, verbose) {
                eprintln!("[erebus] warning: failed to embed RoadRunner: {}", e);
            }
        } else if verbose {
            eprintln!("[erebus] warning: rr binary not found on PATH; RoadRunner won't be available at runtime");
        }
    }

    if !interpreter_embedded && verbose && embedded_interpreter_str.is_some() {
        eprintln!("  (interpreter embedding skipped)");
    }

    // Clean up downloaded build tools (node/npm, composer.phar). Only the
    // tool's own cache dir is removed — never a shared `/tmp` path, which
    // another user/process may own or have symlinked (roadmap #36).
    let _ = std::fs::remove_dir_all(cache_dir().join("build-tools"));

    // Build the payload: zstd(tar) by default, or a real SquashFS image
    // when `--squashfs` was requested (v5). Before this fix the flag only
    // flipped the metadata's payload_format while the payload stayed a
    // zstd+tar stream — the stub's squashfs extractor would fail on it.
    eprintln!("Creating payload...");
    let t0 = std::time::Instant::now();
    let mut payload = if squashfs {
        create_squashfs_payload(&rootfs, verbose).context("failed to create squashfs payload")?
    } else {
        erebus_core::tar::create_tar_zstd_with_level(&rootfs, args.compression_level)
            .context("failed to create tar+zstd payload")?
    };
    let compress_ms = t0.elapsed().as_millis();
    if verbose {
        eprintln!(
            "  compress: {compress_ms}ms, {} MB",
            payload.len() as f64 / 1_048_576.0
        );
    }

    // ── Encryption (v4) ──────────────────────────────────────────────────
    // AES-256-GCM, key derived from a random 32-byte encryption key via
    // HKDF-SHA256 (same key the stub derives at runtime).
    //
    // SECURITY: The encryption key is **separate** from the Ed25519 signing
    // seed.  `--key` is only used for SISR manifest signing (when
    // `--enable-sisr` is also set) and/or binary signing (`--sign`).
    // The encryption key itself is generated randomly at build time and
    // stored in meta `crypto` as `encryption_key_hex`.  The signing seed is
    // NEVER embedded in the binary — this prevents a key compromise from
    // breaking both confidentiality and authenticity.
    //
    // The ciphertext replaces the payload before assembly so the footer's
    // integrity hash and signature both cover the *encrypted* bytes.
    //
    // When combined with `--enable-sisr`, the payload is encrypted in
    // per-chunk mode: each SISR plaintext chunk gets an independent AES key
    // derived via HKDF(encryption_key, salt, chunk_index), and the manifest
    // tracks the ciphertext hashes.  The stub decrypts each chunk
    // independently at runtime before SISR extraction.
    let mut crypto_meta: Option<serde_json::Value> = None;
    let mut sisr_artifacts_opt: Option<erebus_core::sisr_stage::SisrArtifacts> = None;

    if encrypt {
        // Generate a fresh random encryption key — never reuse the signing seed.
        let encryption_key = erebus_core::encrypt::generate_encryption_key();

        if args.enable_sisr {
            let sisr_config = build_sisr_config(&args.key, args.embed_model.is_some())?;
            let artifacts = erebus_core::sisr_stage::build_artifacts(&payload, &sisr_config)
                .context("SISR stage failed during encrypt+SISR build")?;
            let chunk_sizes: Vec<usize> = artifacts
                .manifest
                .chunks
                .iter()
                .map(|c| c.length as usize)
                .collect();
            let salt = erebus_core::encrypt::generate_salt();
            let nonce = erebus_core::encrypt::generate_nonce();
            let ciphertext = erebus_core::encrypt::encrypt_chunks(
                &payload,
                &encryption_key,
                &salt,
                &nonce,
                &chunk_sizes,
            )
            .context("chunked AES-256-GCM payload encryption failed")?;
            payload = ciphertext;
            crypto_meta = Some(serde_json::json!({
                "nonce_hex": hex::encode(nonce),
                "encryption_key_hex": hex::encode(encryption_key),
                "encryption_salt_hex": hex::encode(salt),
                "chunked": true,
            }));
            sisr_artifacts_opt = Some(artifacts);
            if verbose {
                eprintln!(
                    "  encrypt+SISR: {} -> {} bytes (per-chunk AES-256-GCM, {} chunks)",
                    payload.len(),
                    payload.len(),
                    chunk_sizes.len()
                );
            }
        } else {
            let (ciphertext, em) = erebus_core::encrypt::encrypt_payload(&payload, &encryption_key)
                .context("AES-256-GCM payload encryption failed")?;
            payload = ciphertext;
            crypto_meta = Some(serde_json::json!({
                "nonce_hex": hex::encode(em.nonce),
                "tag_offset": em.tag_offset,
                "encryption_key_hex": hex::encode(encryption_key),
                "encryption_salt_hex": hex::encode(em.salt),
            }));
            if verbose {
                eprintln!(
                    "  encrypt: {} -> {} bytes (AES-256-GCM, tag at {})",
                    payload.len(),
                    payload.len(),
                    em.tag_offset
                );
            }
        }
    }

    // Build metadata
    let app_name = app_dir
        .file_name()
        .map_or_else(|| "app".to_string(), |n| n.to_string_lossy().into());

    let mut env_map = serde_json::Map::new();
    env_map.insert("ERE_RUNTIME".into(), runtime_name.clone().into());
    env_map.insert("ERE_APP_NAME".into(), app_name.clone().into());

    // Load env-file (KEY=VALUE per line, # comments, blank lines)
    if let Some(ref ef) = env_file {
        if let Ok(content) = std::fs::read_to_string(ef) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    env_map.insert(k.trim().into(), v.trim().into());
                }
            }
        }
    }

    // Inline --env KEY=VALUE flags (override env-file)
    for entry in &args.env {
        if let Some((k, v)) = entry.split_once('=') {
            env_map.insert(k.trim().into(), v.trim().into());
        }
    }

    // Build-time --define KEY=VALUE (injected as env vars, overrides --env)
    for entry in &args.define {
        if let Some((k, v)) = entry.split_once('=') {
            env_map.insert(k.trim().into(), v.trim().into());
        }
    }

    // ── Disable Xdebug for PHP apps (avoids "Could not connect to debugging client") ──
    if runtime_name == "php" && !env_map.contains_key("XDEBUG_MODE") {
        env_map.insert(
            "XDEBUG_MODE".into(),
            serde_json::Value::String("off".into()),
        );
        if verbose {
            eprintln!("  xdebug: XDEBUG_MODE=off (auto-injected for PHP)");
        }
    }

    // ── Persistent storage ────────────────────────────────────────────
    if args.persist {
        let persist_dir = erebus_core::persistent::get_persist_dir(&app_name);
        let _ = erebus_core::persistent::ensure_persist_dir(&app_name);
        env_map.insert(
            "ERE_PERSIST_DIR".into(),
            serde_json::Value::String(persist_dir.to_string_lossy().into()),
        );
        if verbose {
            eprintln!("  persistent storage: {}", persist_dir.display());
        }
    }

    // ── Health check port ─────────────────────────────────────────────
    if let Some(port) = args.health_port {
        env_map.insert(
            "ERE_HEALTH_PORT".into(),
            serde_json::Value::String(port.to_string()),
        );
        if verbose {
            eprintln!("  health: endpoint enabled on port {port}");
        }
    }

    // ── OpenTelemetry ─────────────────────────────────────────────────
    if let Some(ref endpoint) = args.otel_endpoint {
        let version = version_info.as_deref().unwrap_or("");
        let otel_env = erebus_core::otel::build_otel_env(
            &app_name,
            version,
            endpoint,
            &args.otel_protocol,
            "otlp",
            "otlp",
            "none",
        );
        for (k, v) in &otel_env {
            env_map.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        if verbose {
            eprintln!(
                "  otel: endpoint={endpoint} protocol={}",
                args.otel_protocol
            );
        }
    }

    // ── Cron/scheduled tasks ──────────────────────────────────────────
    if !args.cron.is_empty() {
        let mut tasks_json: Vec<serde_json::Value> = Vec::new();
        for ct in &args.cron {
            if let Some((name, schedule)) = ct.split_once(':') {
                let interval = erebus_core::cron::parse_schedule(schedule);
                tasks_json.push(serde_json::json!({
                    "name": name,
                    "schedule": schedule,
                    "interval_secs": interval,
                }));
                if verbose {
                    eprintln!("  cron: {name} -> every {interval}s (from {schedule})");
                }
            } else {
                anyhow::bail!("--cron format: NAME:SCHEDULE (got '{ct}')");
            }
        }
        env_map.insert(
            "ERE_CRON_TASKS".into(),
            serde_json::Value::Array(tasks_json),
        );
        if verbose {
            eprintln!("  cron: {} task(s) registered", args.cron.len());
        }
    }

    let env_pairs: Vec<(String, String)> = env_map
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect();

    let entrypoint =
        detect::resolve_entrypoint(app_dir, runtime).unwrap_or_else(|| vec!["run".to_string()]);

    let entrypoint = if runtime == detect::Runtime::Wasm && (args.wasi || args.component_model) {
        let mut ep = entrypoint.clone();
        if args.component_model {
            ep.insert(1, "--component-model".into());
        }
        if args.wasi {
            ep.insert(1, "--wasi".into());
        }
        ep
    } else {
        entrypoint
    };

    let mut bun_features = BunFeatures::default();

    // Set embedded interpreter based on either explicit --embed-interpreter or auto-detection
    let (interpreter_opt, interpreter_path_opt) = if let Some(ref interp) = args.embed_interpreter {
        (
            Some(match interp.to_lowercase().as_str() {
                "python3" | "python" => EmbeddedInterpreter::Python3,
                "node" => EmbeddedInterpreter::Node,
                "deno" => EmbeddedInterpreter::Deno,
                "ruby" => EmbeddedInterpreter::Ruby,
                "php" => EmbeddedInterpreter::Php,
                "perl" => EmbeddedInterpreter::Perl,
                "java" => EmbeddedInterpreter::Java,
                "go" => EmbeddedInterpreter::Go,
                "wasm" => EmbeddedInterpreter::Wasm,
                other => EmbeddedInterpreter::Custom(other.to_string()),
            }),
            args.interpreter_path.clone(),
        )
    } else {
        match runtime_name.as_str() {
            "python" => (Some(EmbeddedInterpreter::Python3), None),
            "node" => (Some(EmbeddedInterpreter::Node), None),
            "php" => (Some(EmbeddedInterpreter::Php), None),
            "ruby" => (Some(EmbeddedInterpreter::Ruby), None),
            "deno" => (Some(EmbeddedInterpreter::Deno), None),
            _ => (None, None),
        }
    };

    if let Some(interpreter) = interpreter_opt {
        bun_features.embedded_runtime.interpreter = Some(interpreter);
        if let Some(path) = interpreter_path_opt {
            bun_features.embedded_runtime.interpreter_path = Some(path);
        }
        if verbose {
            eprintln!(
                "  embedded runtime: {}",
                bun_features.embedded_runtime.interpreter.as_ref().unwrap()
            );
        }
    }

    if args.wasm {
        eprintln!("[erebus] warning: --wasm is not yet implemented in the stub; metadata will be written but ignored at runtime");
        bun_features.wasm.enabled = true;
        if let Some(ref path) = args.wasmtime_path {
            bun_features.wasm.wasmtime_path = Some(path.display().to_string());
        }
        bun_features.wasm.wasi = args.wasi;
        bun_features.wasm.component_model = args.component_model;
        if verbose {
            eprintln!(
                "  wasm: enabled (wasi={}, component_model={})",
                args.wasi, args.component_model
            );
        }
    }

    if let Some(port) = args.health_port {
        bun_features.health_check.enabled = true;
        bun_features.health_check.port = port;
        if let Some(ref ep) = args.health_endpoint {
            bun_features.health_check.endpoint.clone_from(ep);
        }
        if verbose {
            eprintln!("  health check: port {}", port);
        }
    }

    if let Some(ref cross) = args.cross_compile {
        eprintln!("[erebus] warning: --cross-compile is not yet implemented in the stub; metadata will be written but ignored at runtime");
        let targets: Vec<String> = cross.split(',').map(|s| s.trim().to_string()).collect();
        bun_features.cross_compile_targets = targets;
        if verbose {
            eprintln!("  cross-compile: {:?}", bun_features.cross_compile_targets);
        }
    }

    bun_features.build_cache.enabled = args.use_cache;

    bun_features
        .validate()
        .map_err(|e| anyhow::anyhow!("Invalid build options: {}", e))?;

    let meta = erebus_core::assembly::build_meta_json(
        &app_name,
        runtime_name,
        isolation_num,
        &entrypoint,
        &env_pairs,
        &erebus_core::assembly::MetaOptions {
            version: version_info,
            author,
            description,
            license,
            payload_format: Some(if squashfs { "squashfs" } else { "zstd-tar" }.to_string()),
            seccomp,
            landlock,
            gui,
            app_hash: Some(new_app_hash.clone()),
            rt_deps_hash: Some(new_rt_hash.clone()),
            update_url: args.update_url.clone(),
            crypto: crypto_meta,
            layers: None,
            entrypoint_layer: None,
        },
        &bun_features,
    )?;

    // Assemble
    eprintln!("Assembling {}...", output.display());

    // Save previous SISR manifest before assembly (for bandwidth reporting)
    let prev_manifest_bytes: Option<Vec<u8>> = if args.update && args.enable_sisr {
        let mut prev_manifest_path = output.clone();
        prev_manifest_path.set_extension("erebus.manifest");
        std::fs::read(&prev_manifest_path).ok()
    } else {
        None
    };

    let size = if args.enable_sisr {
        let sisr_config = build_sisr_config(&args.key, args.embed_model.is_some())?;
        let artifacts = match sisr_artifacts_opt {
            Some(a) => a,
            None => erebus_core::sisr_stage::build_artifacts(&payload, &sisr_config)
                .context("SISR stage failed during build")?,
        };
        let input = erebus_core::assembly::AssemblyInput {
            stub_bytes: &stub_bytes,
            payload: &payload,
            meta_bytes: &meta,
            encrypt,
            squashfs,
            target_arch: target.as_deref(),
            sisr: Some(artifacts),
        };
        erebus_core::assembly::assemble_erebus(output, &input)
            .context("failed to assemble erebus (SISR)")?
    } else {
        let input = erebus_core::assembly::AssemblyInput {
            stub_bytes: &stub_bytes,
            payload: &payload,
            meta_bytes: &meta,
            encrypt,
            squashfs,
            target_arch: target.as_deref(),
            sisr: None,
        };
        erebus_core::assembly::assemble_erebus(output, &input)
            .context("failed to assemble erebus")?
    };

    eprintln!(
        "Built {} ({:.1}MB)",
        output.display(),
        size as f64 / (1024.0 * 1024.0)
    );

    if args.enable_sisr {
        eprintln!(
            "warning: SISR binaries are NOT signed at rest — authenticity is only guaranteed \
             during an update, when the remote manifest signature is verified"
        );
    }

    // macOS code signing: re-sign the assembled binary since appending
    // payload + metadata invalidates any existing Mach-O signature.
    if target
        .as_ref()
        .is_some_and(|t| t.contains("darwin") || t.contains("apple") || t.contains("macos"))
    {
        sign_macos_binary(output, verbose)?;
    }

    if args.enable_sisr {
        let mut manifest_path = output.clone();
        manifest_path.set_extension("erebus.manifest");
        eprintln!("SISR manifest written: {}", manifest_path.display());

        // Bandwidth reporting: compare new chunk set against previous manifest
        if let Some(prev_bytes) = &prev_manifest_bytes {
            if let (Ok(prev), Ok(new_bytes)) = (
                erebus_core::sisr_stage::RemoteManifest::from_bytes(prev_bytes),
                std::fs::read(&manifest_path),
            ) {
                if let Ok(new_remote) =
                    erebus_core::sisr_stage::RemoteManifest::from_bytes(&new_bytes)
                {
                    report_sisr_bandwidth(&prev, &new_remote);
                }
            }
        }
    }

    // Binary signature is mutually exclusive with SISR in a single build:
    // `sign_file` rebuilds the file as `[..meta_end][sig][footer]`, which would
    // truncate the SISR section. With `--enable-sisr`, `--key` instead signs
    // the manifest (see `build_sisr_config`).
    //
    // Signing happens BEFORE the cache store so the cached artifact is the
    // complete, signed binary — a cache hit must not serve an unsigned copy.
    if !args.enable_sisr {
        if let Some(key_path) = &args.key {
            if verbose {
                eprintln!("Signing...");
            }
            crate::commands::sign::sign_file(output, key_path, !verbose)?;
        }
    }

    // ── Store in build cache ──────────────────────────────────────────
    if args.use_cache {
        let cache = erebus_core::paths::BuildCache::new(app_dir, 50);
        if cache
            .store(&new_app_hash, &cfg_hash, target.as_deref(), output)
            .is_ok()
            && verbose
        {
            eprintln!("  cache: stored build");
        }
    }

    // ── Store in remote cache (Depot-style) ───────────────────────────
    if let Some(remote) = remote_cache_from_args(args, app_dir) {
        if remote
            .store(&new_app_hash, &cfg_hash, target.as_deref(), output)
            .is_ok()
            && verbose
        {
            eprintln!("  remote cache: stored build");
        }
    }

    if args.json {
        return Ok(Some(serde_json::json!({
            "output": output.to_string_lossy(),
            "size_bytes": size,
            "runtime": runtime_name,
            "format": "zstd-tar",
            "signed": args.key.is_some() && !args.enable_sisr,
            "encrypted": encrypt,
            "sisr": args.enable_sisr,
            "manifest_signed": args.enable_sisr && args.key.is_some(),
        })));
    }
    Ok(None)
}
