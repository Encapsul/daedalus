use crate::remote_cache::remote_cache_from_args;
use anyhow::{Context, Result};
use daedalus_core::detect;
use daedalus_core::embed;
use daedalus_core::encrypt;
use daedalus_core::layer::{Capability, RuntimeLayer, SerializableLayer};
use daedalus_core::metadata::{BunFeatures, EmbeddedInterpreter};
use daedalus_core::paths::cache_dir;
use daedalus_core::pkgmgr;
use hex;
use std::path::{Path, PathBuf};

use super::args::{config_fingerprint, parse_target, BuildArgs, BuildPlan};
use super::deps::{
    check_php_platform_reqs, ensure_composer, ensure_go, ensure_node, ensure_python,
    has_workspace_protocol, is_command_available,
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
            "[daedalus] warning: {flags} require --isolation sandbox (2) to take effect — \
             ignored at isolation level {isolation_num}"
        );
    }
}

/// Everything [`assemble_and_sign`] needs to write the final artifact.
struct AssembleInputs<'a> {
    stub_bytes: &'a [u8],
    payload: &'a [u8],
    meta_bytes: &'a [u8],
    squashfs: bool,
    /// Pre-built SISR artifacts from the build stage, if any.
    sisr_artifacts: Option<daedalus_core::sisr_stage::SisrArtifacts>,
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
    let seccomp = plan.seccomp;
    let landlock = plan.landlock;
    let gui = plan.gui;
    let squashfs = plan.squashfs;
    let version_info = plan.version_info.clone();
    let author = plan.author.clone();
    let description = plan.description.clone();
    let license = plan.license.clone();

    // ── Sandbox flags without isolation are silent no-ops ──────────────
    // seccomp/landlock are only enforced in the stub when isolation >= 2
    // (pivot_root + namespace path). Warn so the user isn't lulled into
    // believing the sandbox is active.
    warn_sandbox_noops(isolation_num, seccomp, landlock);

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
    let new_app_hash = daedalus_core::include::hash_app_files(app_dir);
    let new_rt_hash = daedalus_core::include::hash_lock_file(app_dir);

    // ── Intelligent build cache: skip rebuild if hash matches ──────────
    let cfg_hash = config_fingerprint(args, plan);
    if args.use_cache {
        let cache = daedalus_core::paths::BuildCache::new(app_dir, 50);
        if let Some(cached) = cache.find(&new_app_hash, &cfg_hash, target.as_deref()) {
            if verbose {
                eprintln!("[daedalus] cache hit — reusing cached build");
            }
            std::fs::copy(&cached, &output).context("failed to copy cached .daedalus to output")?;
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
            eprintln!("[daedalus] cache miss — building from scratch");
        }
    }

    if args.clear_cache {
        let cache = daedalus_core::paths::BuildCache::new(app_dir, 50);
        cache.clear().ok();
        if verbose {
            eprintln!("  cache: cleared");
        }
    }

    // ── Incremental update: skip rebuild if nothing changed ────────────
    let reuse_binary: Option<PathBuf> = if args.update && output.exists() {
        if let Some((old_app_hash, old_rt_hash)) = read_existing_hashes(output) {
            if old_app_hash == new_app_hash && old_rt_hash == new_rt_hash {
                if verbose {
                    eprintln!("[daedalus] everything up to date, nothing to rebuild");
                }
                return Ok(None);
            } else if old_rt_hash == new_rt_hash && old_app_hash != new_app_hash {
                if verbose {
                    eprintln!("[daedalus] app changed, reusing runtime from existing binary");
                }
                Some(output.clone())
            } else {
                if verbose {
                    eprintln!("[daedalus] runtime deps changed, full rebuild");
                }
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // ── Package managers: detect + install deps for every manifest ────
    // Skip when reusing the rootfs — the runtime + deps are already embedded.
    let mut reuse_rootfs: Option<(tempfile::TempDir, PathBuf)> = None;
    if let Some(ref old_bin) = reuse_binary {
        reuse_rootfs = Some(reuse_rootfs_from_binary(old_bin, app_dir, args, verbose)?);
    }

    if reuse_rootfs.is_none() {
        install_package_managers(args, plan, target.as_deref())?;
    }

    // Find stub binary
    let stub = find_stub(&target)?;
    let stub_bytes = std::fs::read(&stub)
        .with_context(|| format!("failed to read stub binary at {}", stub.display()))?;

    // Stage the rootfs: temp dir, app copy, tree-shake/minify, includes.
    let (_tmp, rootfs) = if let Some((tmp, rootfs)) = reuse_rootfs {
        (tmp, rootfs)
    } else {
        stage_rootfs(args, app_dir, verbose)?
    };

    // ── Build Go / Rust binaries, Maven/Gradle JARs, .NET binaries ────────
    let go_binary_name = build_go_binary(plan, target.as_deref(), &rootfs)?;
    let rust_binary_name = build_rust_binary(plan, target.as_deref(), &rootfs)?;
    let java_jar_name = build_java_binary(plan, &rootfs)?;
    let dotnet_binary_name = build_dotnet_binary(plan, target.as_deref(), &rootfs)?;

    // ── Embed interpreter / N-API addons / RoadRunner into the rootfs ──
    // Skip when reusing rootfs — interpreter already present in payload.
    if reuse_binary.is_none() {
        embed_interpreters(args, plan, &rootfs, target.as_deref());
    }

    // ── Payload: compress (tar+zstd or squashfs) ──────────────────────
    let payload = compress_payload(args, plan, &rootfs)?;

    // Build metadata
    let app_name = app_dir
        .file_name()
        .map_or_else(|| "app".to_string(), |n| n.to_string_lossy().into());
    let env_pairs = build_env_map(args, plan, &app_name)?;

    let entrypoint = resolve_entrypoint_argv(
        args,
        app_dir,
        runtime,
        go_binary_name
            .as_deref()
            .or(rust_binary_name.as_deref())
            .or(java_jar_name.as_deref())
            .or(dotnet_binary_name.as_deref()),
    );

    let bun_features = build_bun_features(args, plan)?;

    let layers = build_layers(runtime_name, &entrypoint, &env_pairs, app_dir);

    let pre_hooks: Option<serde_json::Value> = if let Some(s) = plan.pre_hooks.as_deref() {
        if !s.trim().is_empty() {
            let v: serde_json::Value = parse_hooks_json(s)
                .map_err(|e| anyhow::anyhow!("invalid --pre-hooks: {e}"))?
                .unwrap_or(serde_json::Value::Null);
            Some(v)
        } else {
            None
        }
    } else {
        None
    };
    let post_hooks: Option<serde_json::Value> = if let Some(s) = plan.post_hooks.as_deref() {
        if !s.trim().is_empty() {
            let v: serde_json::Value = parse_hooks_json(s)
                .map_err(|e| anyhow::anyhow!("invalid --post-hooks: {e}"))?
                .unwrap_or(serde_json::Value::Null);
            Some(v)
        } else {
            None
        }
    } else {
        None
    };

    let meta = daedalus_core::assembly::build_meta_json(
        &app_name,
        runtime_name,
        isolation_num,
        &entrypoint,
        &env_pairs,
        &daedalus_core::assembly::MetaOptions {
            version: version_info,
            author,
            description,
            license,
            payload_format: Some(if squashfs { "squashfs" } else { "zstd-tar" }.to_string()),
            seccomp,
            landlock,
            gui,
            cpu_limit: plan.cpu_limit,
            memory_limit_mb: plan.memory_limit_mb,
            pid_limit: plan.pid_limit,
            app_hash: Some(new_app_hash.clone()),
            rt_deps_hash: Some(new_rt_hash.clone()),
            update_url: args.update_url.clone(),
            pre_hooks,
            post_hooks,
            layers: Some(layers),
            entrypoint_layer: Some(runtime_name.clone()),
        },
        &bun_features,
    )?;

    // ── Assemble, package and (macOS) re-sign the artifact ────────────
    let size = assemble_and_sign(
        args,
        plan,
        target.as_deref(),
        output,
        AssembleInputs {
            stub_bytes: &stub_bytes,
            payload: &payload,
            meta_bytes: &meta,
            squashfs,
            sisr_artifacts: None,
        },
    )?;

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
        let cache = daedalus_core::paths::BuildCache::new(app_dir, 50);
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
            "format": if squashfs { "squashfs" } else { "zstd-tar" },
            "signed": args.key.is_some() && !args.enable_sisr,
            "sisr": args.enable_sisr,
            "manifest_signed": args.enable_sisr && args.key.is_some(),
        })));
    }
    Ok(None)
}

/// Build the layer list for the artifact metadata.
fn parse_hooks_json(s: &str) -> Result<Option<serde_json::Value>, serde_json::Error> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if s.starts_with('@') {
        let path = s.trim_start_matches('@');
        let content = std::fs::read_to_string(path).map_err(serde_json::Error::io)?;
        serde_json::from_str(&content).map(Some)
    } else {
        serde_json::from_str(s).map(Some)
    }
}

///
/// Always includes a `RuntimeLayer` for the detected runtime. Adds a
/// `ConfigLayer` for `.daedalus.toml` when present.
fn build_layers(
    runtime_name: &str,
    entrypoint: &[String],
    env: &[(String, String)],
    app_dir: &Path,
) -> Vec<SerializableLayer> {
    let mut layers: Vec<SerializableLayer> = vec![SerializableLayer::Runtime(RuntimeLayer {
        name: runtime_name.to_string(),
        interpreter: runtime_name.to_string(),
        entrypoint: entrypoint.to_vec(),
        version: None,
        env: env.to_vec(),
        capabilities: vec![
            Capability::ReadFile,
            Capability::WriteFile,
            Capability::Network,
            Capability::Exec,
            Capability::Syscall,
            Capability::Env,
        ],
    })];

    let config_path = app_dir.join(".daedalus.toml");
    if config_path.exists() {
        let config_data = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|content| {
                toml::from_str::<toml::Value>(&content)
                    .ok()
                    .and_then(|v| serde_json::to_value(v).ok())
            })
            .unwrap_or_else(|| serde_json::json!({"path": ".daedalus.toml"}));
        layers.push(SerializableLayer::Config(
            daedalus_core::layer::ConfigLayer {
                name: "daedalus-config".to_string(),
                format: "toml".to_string(),
                data: config_data,
            },
        ));
    }

    layers
}

// ─────────────────────────────────────────────────────────────────────────────
// Extracted build phases (audit action #6): each function below is a verbatim
// move of a `build_single_target` section — behavior, message order and
// quirks are preserved.
// ─────────────────────────────────────────────────────────────────────────────

/// Detect every package-manager manifest in the app and install its deps.
fn install_package_managers(
    args: &BuildArgs,
    plan: &BuildPlan,
    target: Option<&str>,
) -> Result<()> {
    let app_dir = &plan.app_dir;
    let runtime_name = &plan.runtime_name;
    let verbose = plan.verbose;

    // ── PHP platform extensions check ──────────────────────────────────
    if runtime_name == "php" && !plan.no_install {
        check_php_platform_reqs(app_dir, verbose)?;
    }

    // Detect pnpm workspace with workspace:* protocol (cannot use npm)
    if pkgmgr::detect_node_pkgmgr(app_dir) == Some(pkgmgr::PkgMgr::Npm)
        && has_workspace_protocol(app_dir)
    {
        eprintln!("[daedalus] warning: package.json uses `workspace:*` protocol (pnpm-specific)");
        eprintln!("  but pnpm is not detected. Create `pnpm-workspace.yaml` or add a lockfile.");
    }

    // Detect and install all package managers (primary + secondary)
    for mgr in &pkgmgr::detect_all_pkgmgrs(app_dir, runtime_name) {
        if verbose {
            eprintln!("Package manager: {}", mgr.name());
        }
        if !plan.no_install {
            install_pkgmgr_deps(args, mgr, app_dir, target, verbose)?;
        }
    }
    Ok(())
}

/// Run the install command for one package manager, downloading the target
/// toolchain first when needed.
fn install_pkgmgr_deps(
    args: &BuildArgs,
    mgr: &pkgmgr::PkgMgr,
    app_dir: &Path,
    target: Option<&str>,
    verbose: bool,
) -> Result<()> {
    // `None` = tool unavailable; the skip warning was already printed.
    let Some((prog, full_args, node_bin_dir)) =
        pkgmgr_install_command(mgr, app_dir, target, args.cross_compile.as_deref(), verbose)?
    else {
        return Ok(());
    };

    let mut command = std::process::Command::new(&prog);
    command.args(&full_args).current_dir(app_dir);

    // If we downloaded node for npm/yarn/bun, prepend its bin dir to PATH
    // using Command::env() instead of mutating global std::env::PATH
    if let Some(ref bin_dir) = node_bin_dir {
        let current = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{}:{}", bin_dir.display(), current));
    }

    if *mgr == pkgmgr::PkgMgr::Bundler {
        command.env("BUNDLE_WITHOUT", "development");
    }

    let status = command
        .status()
        .context(format!("failed to run `{}` — is it installed?", prog))?;
    if !status.success() {
        eprintln!(
            "[daedalus] warning: {} installation failed (exit code {})",
            mgr.name(),
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

/// Resolve the install command for one manager as `(prog, args, node_bin_dir)`.
/// Returns `None` when the tool is missing from PATH and cannot be fetched.
#[allow(clippy::type_complexity)] // (prog, args, downloaded-node bin dir) triple
fn pkgmgr_install_command(
    mgr: &pkgmgr::PkgMgr,
    app_dir: &Path,
    target: Option<&str>,
    cross_compile: Option<&str>,
    verbose: bool,
) -> Result<Option<(String, Vec<String>, Option<PathBuf>)>> {
    let cmd = mgr.install_cmd();

    // Check if the binary exists before trying to run it. For
    // cross-target builds the builder's host toolchain has the wrong
    // arch/OS, so always download the target node instead.
    let is_node_mgr = matches!(
        mgr,
        pkgmgr::PkgMgr::Npm | pkgmgr::PkgMgr::Pnpm | pkgmgr::PkgMgr::Yarn | pkgmgr::PkgMgr::Bun
    );
    let need_target_node = target.is_some() && is_node_mgr;
    let mut node_bin_dir: Option<PathBuf> = None;
    if !is_command_available(cmd[0]) || need_target_node {
        if is_node_mgr {
            node_bin_dir = Some(ensure_node(target, verbose)?);
        } else {
            eprintln!(
                "[daedalus] skipping {} — `{}` not found on PATH",
                mgr.name(),
                cmd[0]
            );
            return Ok(None);
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

    push_install_flags(&mut full_args, mgr, target, cross_compile);

    Ok(Some((prog, full_args, node_bin_dir)))
}

/// Target-platform install flags: force source builds for pip and select the
/// target platform/arch for node package managers when cross-compiling.
fn push_install_flags(
    full_args: &mut Vec<String>,
    mgr: &pkgmgr::PkgMgr,
    target: Option<&str>,
    cross_compile: Option<&str>,
) {
    // When cross-compiling for a different arch, force source builds for pip
    let is_cross_pip = matches!(mgr, pkgmgr::PkgMgr::Pip)
        && cross_compile.is_some_and(|c| c.split(',').any(|t| t.trim() != std::env::consts::ARCH));
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
    let is_node_mgr = matches!(
        mgr,
        pkgmgr::PkgMgr::Npm | pkgmgr::PkgMgr::Pnpm | pkgmgr::PkgMgr::Yarn | pkgmgr::PkgMgr::Bun
    );
    let is_cross_node = is_node_mgr
        && target.is_some_and(|t| {
            let (t_arch, t_os) = parse_target(t);
            t_arch != std::env::consts::ARCH || t_os != std::env::consts::OS
        });
    if is_cross_node {
        if let Some(t) = target {
            push_cross_node_flags(full_args, mgr, t);
        }
    }
}

/// Append `--platform/--arch/--ignore-scripts` so native deps resolve for
/// the target platform instead of the build host.
fn push_cross_node_flags(full_args: &mut Vec<String>, mgr: &pkgmgr::PkgMgr, target: &str) {
    let (t_arch, t_os) = parse_target(target);
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

/// Stage the rootfs into a temp dir: copy the app, then tree-shake/minify
/// and copy `--include` paths on the staging copy only.
fn stage_rootfs(
    args: &BuildArgs,
    app_dir: &Path,
    verbose: bool,
) -> Result<(tempfile::TempDir, PathBuf)> {
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
        let removed = daedalus_core::treeshake::prune_node_modules(&rootfs.join("app"), verbose)
            .context("tree-shaking failed")?;
        if verbose {
            eprintln!("  tree-shake: removed {removed} unused package(s)");
        }
    }

    if args.minify {
        let minified = daedalus_core::minify::minify_app_dir(&rootfs.join("app"), verbose)
            .context("minification failed")?;
        if verbose {
            eprintln!("  minify: minified {minified} file(s)");
        }
    }

    // ── Include extra files ───────────────────────────────────────────
    if !args.include.is_empty() {
        let app_dest = rootfs.join("app");
        let count = daedalus_core::include::copy_include_paths(&args.include, &app_dest, app_dir)
            .context("failed to copy include paths")?;
        if verbose {
            eprintln!("  include: copied {count} path(s) into rootfs");
        }
    }

    Ok((tmp, rootfs))
}

/// Build the Go binary into `rootfs/app` and strip source files. Returns the
/// binary name, or `None` when the app is not Go or `--no-install` is set.
fn build_go_binary(
    plan: &BuildPlan,
    target: Option<&str>,
    rootfs: &Path,
) -> Result<Option<String>> {
    if plan.runtime != detect::Runtime::Go || plan.no_install {
        return Ok(None);
    }
    let app_dir = &plan.app_dir;
    let verbose = plan.verbose;

    let go_bin_dir = ensure_go(target, verbose)?;
    let prev_path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{}", go_bin_dir.display(), prev_path));

    // Cross-compilation: set GOOS/GOARCH from --target
    if let Some(target_str) = target {
        let (arch, os) = parse_target(target_str);
        let go_arch = match arch.as_str() {
            "x86_64" | "amd64" => "amd64",
            "aarch64" | "arm64" => "arm64",
            other => other,
        };
        std::env::set_var("GOOS", os.as_str());
        std::env::set_var("GOARCH", go_arch);
        if verbose {
            eprintln!("  cross-compiling Go for {}/{go_arch}", os.as_str());
        }
    }

    go_mod_download(app_dir, verbose)?;
    let bin_name = go_module_bin_name(app_dir);
    go_build_and_strip(target, rootfs, app_dir, &bin_name, verbose)?;

    if verbose {
        eprintln!("  Go binary built successfully");
    }
    Ok(Some(bin_name))
}

/// Build the Cargo binary into `rootfs/app` and strip source files. Returns
/// the binary name, or `None` when the app is not Rust or `--no-install` is
/// set (Phase 8 Step 2).
///
/// Unlike Go, no toolchain is auto-downloaded: the canonical Rust installer
/// is rustup and a toolchain weighs hundreds of MB — we require `cargo` on
/// PATH and fail with guidance instead.
fn build_rust_binary(
    plan: &BuildPlan,
    target: Option<&str>,
    rootfs: &Path,
) -> Result<Option<String>> {
    if plan.runtime != detect::Runtime::Rust || plan.no_install {
        return Ok(None);
    }
    let app_dir = &plan.app_dir;
    let verbose = plan.verbose;

    ensure_cargo()?;
    let bin_name = cargo_bin_name(app_dir);

    // Cross-compilation: synthesize a full Rust triple from --target.
    let triple = target.map(rust_target_triple);
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(["build", "--release"]);
    if let Some(triple) = &triple {
        cmd.args(["--target", triple]);
        if verbose {
            eprintln!("  cross-compiling Rust for {triple}");
        }
    }
    // Isolation level >= 2 pivots into the artifact rootfs, which carries no
    // glibc loader — a dynamically linked binary fails execve with ENOENT.
    // Mirror Go's implicit static linking via crt-static (Linux only: other
    // platforms reject the flag and their launchers don't pivot anyway).
    let os_is_linux = match target {
        Some(t) => !t.contains("windows") && !t.contains("darwin") && !t.contains("macos"),
        None => std::env::consts::OS == "linux",
    };
    if os_is_linux {
        let prev = std::env::var("RUSTFLAGS").unwrap_or_default();
        let flags = format!("{prev} -C target-feature=+crt-static");
        cmd.env("RUSTFLAGS", flags.trim_start());
    }
    cmd.current_dir(app_dir);
    cmd.env_remove("CARGO_TARGET_DIR");
    if verbose {
        eprintln!("  cargo build --release...");
    }
    let status = cmd
        .status()
        .context("failed to run `cargo build` — is Rust installed? (https://rustup.rs)")?;
    if !status.success() {
        anyhow::bail!("`cargo build` failed with exit code {status}");
    }

    let exe_suffix = if rust_target_is_windows(target) {
        ".exe"
    } else {
        ""
    };
    let built = match &triple {
        Some(t) => app_dir.join("target").join(t).join("release"),
        None => app_dir.join("target").join("release"),
    }
    .join(format!("{bin_name}{exe_suffix}"));
    if !built.is_file() {
        anyhow::bail!(
            "cargo build succeeded but {} is missing — explicit [[bin]] names or workspace layouts may need --entrypoint",
            built.display()
        );
    }

    let staged = rootfs.join("app").join(format!("{bin_name}{exe_suffix}"));
    std::fs::copy(&built, &staged)
        .with_context(|| format!("failed to stage {}", built.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }

    strip_compiled_sources(rootfs, &bin_name);
    if verbose {
        eprintln!("  Rust binary built successfully");
    }
    Ok(Some(bin_name))
}

/// Build the Maven/Gradle JAR into `rootfs/app` and strip source files.
/// Returns the JAR file name, or `None` when the app is not Java or
/// `--no-install` is set (Phase 8 Step 3).
///
/// Java bytecode is portable — `--target` is a no-op here (jlink minimal
/// runtimes are a separate roadmap item). Wrapper scripts (`mvnw`,
/// `gradlew`) win over system tools so projects pin their own build; they
/// must carry the executable bit.
fn build_java_binary(plan: &BuildPlan, rootfs: &Path) -> Result<Option<String>> {
    if plan.runtime != detect::Runtime::Java || plan.no_install {
        return Ok(None);
    }
    ensure_java()?;
    let app_dir = &plan.app_dir;
    let verbose = plan.verbose;

    let tool = java_build_tool(app_dir)?;
    run_java_build(&tool, app_dir, verbose)?;
    let jar_path = find_built_jar(app_dir).ok_or_else(|| {
        anyhow::anyhow!("build succeeded but no usable JAR found in target/ or build/libs/")
    })?;
    let jar_name = jar_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .context("JAR path has no file name")?;
    std::fs::copy(&jar_path, rootfs.join("app").join(&jar_name))
        .with_context(|| format!("failed to stage {}", jar_path.display()))?;

    strip_compiled_sources(rootfs, &jar_name);
    if verbose {
        eprintln!("  Java JAR built successfully: {jar_name}");
    }
    Ok(Some(jar_name))
}

/// Build tool for a Java project: Maven (`pom.xml`) or Gradle
/// (`build.gradle[.kts]`). The variant payload is the command to invoke.
#[derive(Debug, PartialEq)]
enum JavaBuildTool {
    Maven { cmd: String },
    Gradle { cmd: String },
}

/// Pick the build tool: the project wrapper wins over a system install.
/// Maven markers are checked first when both ecosystems are present.
fn java_build_tool(app_dir: &Path) -> Result<JavaBuildTool> {
    let has = |name: &str| app_dir.join(name).is_file();
    if has("pom.xml") || has("mvnw") {
        let cmd = if has("mvnw") {
            "./mvnw".to_string()
        } else {
            system_tool("mvn", "Maven")?
        };
        return Ok(JavaBuildTool::Maven { cmd });
    }
    let cmd = if has("gradlew") {
        "./gradlew".to_string()
    } else {
        system_tool("gradle", "Gradle")?
    };
    Ok(JavaBuildTool::Gradle { cmd })
}

/// Resolve a system build tool from PATH (`.exe` suffix for Windows).
fn system_tool(name: &str, label: &str) -> Result<String> {
    let found = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .any(|dir| dir.join(name).is_file() || dir.join(format!("{name}.exe")).is_file())
        })
        .unwrap_or(false);
    if !found {
        anyhow::bail!("{label} not found — use the project wrapper ({name}w) or install {label}");
    }
    Ok(name.to_string())
}

/// Run the package phase. Tests are skipped — packaging is the goal here,
/// verification stays in CI. Gradle `build` covers Spring Boot's bootJar.
fn run_java_build(tool: &JavaBuildTool, app_dir: &Path, verbose: bool) -> Result<()> {
    let (cmd, args): (&str, Vec<&str>) = match tool {
        JavaBuildTool::Maven { cmd } => (cmd, vec!["-q", "-DskipTests", "package"]),
        JavaBuildTool::Gradle { cmd } => {
            (cmd, vec!["-q", "build", "-x", "test", "--console=plain"])
        }
    };
    if verbose {
        eprintln!("  {cmd} {}...", args.join(" "));
    }
    let status = std::process::Command::new(cmd)
        .args(args)
        .current_dir(app_dir)
        .status()
        .with_context(|| format!("failed to run `{cmd}`"))?;
    if !status.success() {
        anyhow::bail!("`{cmd}` failed with exit code {status}");
    }
    Ok(())
}

/// Locate the built JAR in `target/` (Maven) or `build/libs/` (Gradle),
/// skipping auxiliary artifacts (sources/javadoc/plain/original). When
/// several candidates remain, the largest wins — fat jars carry deps.
fn find_built_jar(app_dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    for rel in ["target", "build/libs"] {
        let entries = std::fs::read_dir(app_dir.join(rel)).into_iter().flatten();
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_jar_file(&path) || is_auxiliary_jar(&path) {
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if best.as_ref().map_or(true, |(s, _)| size > *s) {
                best = Some((size, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

/// Check if the path has a `.jar` extension (case-insensitive).
fn is_jar_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("jar"))
}

/// Shade originals, source/javadoc bundles and Spring Boot `-plain` jars
/// are build by-products, never runnable entrypoints.
fn is_auxiliary_jar(path: &Path) -> bool {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    stem.starts_with("original-")
        || name.ends_with("-sources.jar")
        || name.ends_with("-javadoc.jar")
        || stem.contains("-plain")
}

/// Build the .NET project via `dotnet publish --self-contained` into
/// `rootfs/app` and strip source files. Returns the binary name, or
/// `None` when the app is not .NET or `--no-install` is set.
///
/// Self-contained publishes produce a native binary that runs without a
/// .NET runtime on the target machine — aligned with the Go/Rust approach.
/// The RID (Runtime Identifier) is synthesized from `--target` shorthands
/// (linux-x64, linux-arm64, win-x64, macos-x64, macos-arm64) or passed
/// through as a full RID (e.g. `linux-musl-x64`).
fn build_dotnet_binary(
    plan: &BuildPlan,
    target: Option<&str>,
    rootfs: &Path,
) -> Result<Option<String>> {
    if plan.runtime != detect::Runtime::Dotnet || plan.no_install {
        return Ok(None);
    }
    ensure_dotnet()?;
    let app_dir = &plan.app_dir;
    let verbose = plan.verbose;

    let rid = dotnet_rid_from_target(target);
    let publish_dir = app_dir.join("publish");
    if verbose {
        eprintln!("  dotnet publish --self-contained -r {rid}...");
    }
    let status = std::process::Command::new("dotnet")
        .args([
            "publish",
            "-c",
            "Release",
            "--self-contained",
            "-r",
            &rid,
            "-o",
            publish_dir.to_string_lossy().as_ref(),
        ])
        .current_dir(app_dir)
        .status()
        .context("failed to run `dotnet publish`")?;
    if !status.success() {
        anyhow::bail!("`dotnet publish` failed with exit code {status}");
    }

    let binary_name = detect::find_dotnet_self_contained(app_dir).ok_or_else(|| {
        anyhow::anyhow!("dotnet publish succeeded but no native binary found in publish/")
    })?;
    let binary_path = app_dir.join(&binary_name);
    let staged = rootfs.join("app").join(&binary_name);
    std::fs::copy(&binary_path, &staged)
        .with_context(|| format!("failed to stage {}", binary_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }

    strip_compiled_sources(rootfs, &binary_name);
    if verbose {
        eprintln!("  .NET binary built successfully: {binary_name}");
    }
    Ok(Some(binary_name))
}

/// Map daedalus `--target` shorthand to .NET Runtime Identifier (RID).
/// Full RIDs (e.g. `linux-musl-x64`) are passed through unchanged.
fn dotnet_rid_from_target(target: Option<&str>) -> String {
    match target {
        Some(t)
            if t.contains('-')
                && (t.contains("linux")
                    || t.contains("win")
                    || t.contains("macos")
                    || t.contains("osx")
                    || t.contains("darwin")) =>
        {
            // Already looks like a full RID or our shorthand
            t.replace("macos", "osx").replace("darwin", "osx")
        }
        Some(t) => t.to_string(), // unknown, pass through
        None => {
            // Host default
            if cfg!(target_os = "windows") {
                "win-x64".to_string()
            } else if cfg!(target_os = "macos") {
                "osx-x64".to_string()
            } else {
                "linux-x64".to_string()
            }
        }
    }
}

fn ensure_dotnet() -> Result<()> {
    let ok = std::process::Command::new("dotnet")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        anyhow::bail!(
            "dotnet SDK not found on PATH — install from https://dotnet.microsoft.com/download"
        );
    }
    Ok(())
}

fn ensure_java() -> Result<()> {
    let ok = std::process::Command::new("java")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        anyhow::bail!(
            "java not found on PATH — install a JDK (https://adoptium.net) to build Java apps"
        );
    }
    Ok(())
}

/// Fails with actionable guidance when `cargo` is not on PATH.
fn ensure_cargo() -> Result<()> {
    let ok = std::process::Command::new("cargo")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        anyhow::bail!(
            "cargo not found on PATH — install Rust via https://rustup.rs to build Rust apps"
        );
    }
    Ok(())
}

/// The output binary name: first `[[bin]] name` wins, else `[package] name`
/// (Cargo keeps hyphens for the default binary). Workspace virtual manifests
/// have no `[package]` — fall back to "app" (workspace support is Phase 8
/// Step 5).
fn cargo_bin_name(app_dir: &Path) -> String {
    let parsed = std::fs::read_to_string(app_dir.join("Cargo.toml"))
        .ok()
        .and_then(|content| content.parse::<toml::Table>().ok());
    let table = parsed.as_ref();
    table
        .and_then(|t| t.get("bin"))
        .and_then(|bins| bins.as_array())
        .and_then(|bins| bins.first())
        .and_then(|bin| bin.get("name"))
        .and_then(|name| name.as_str())
        .or_else(|| {
            table
                .and_then(|t| t.get("package"))
                .and_then(|pkg| pkg.get("name"))
                .and_then(|name| name.as_str())
        })
        .map_or_else(|| "app".to_string(), str::to_string)
}

/// Maps a `--target` value to a full Rust target triple. Full triples pass
/// through untouched; shorthands are synthesized (linux defaults to gnu,
/// `musl` in the shorthand selects the musl triple).
fn rust_target_triple(target: &str) -> String {
    if target.contains("-unknown-")
        || target.contains("-apple-")
        || target.contains("-pc-")
        || target.contains("-linux-")
    {
        return target.to_string();
    }
    let (arch, os) = parse_target(target);
    match os.as_str() {
        "darwin" => format!("{arch}-apple-darwin"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        _ => {
            if target.contains("musl") {
                format!("{arch}-unknown-linux-musl")
            } else {
                format!("{arch}-unknown-linux-gnu")
            }
        }
    }
}

/// Whether the effective build target is Windows (drives the `.exe` suffix).
fn rust_target_is_windows(target: Option<&str>) -> bool {
    target.is_some_and(|t| parse_target(t).1 == "windows")
        || (target.is_none() && std::env::consts::OS == "windows")
}

fn go_mod_download(app_dir: &Path, verbose: bool) -> Result<()> {
    if verbose {
        eprintln!("  go mod download...");
    }
    let status = std::process::Command::new("go")
        .args(["mod", "download"])
        .current_dir(app_dir)
        .status()
        .context("failed to run `go mod download` — is Go installed?")?;
    if !status.success() {
        anyhow::bail!("`go mod download` failed with exit code {status}");
    }
    Ok(())
}

/// Output binary name from the go.mod module path (last path segment).
fn go_module_bin_name(app_dir: &Path) -> String {
    std::fs::read_to_string(app_dir.join("go.mod"))
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|l| l.starts_with("module "))
                .and_then(|l| l.strip_prefix("module ").map(|s| s.trim().to_string()))
        })
        .unwrap_or_else(|| "app".to_string())
        .rsplit('/')
        .next()
        .unwrap_or("app")
        .to_string()
}

fn go_build_and_strip(
    target: Option<&str>,
    rootfs: &Path,
    app_dir: &Path,
    bin_name: &str,
    verbose: bool,
) -> Result<()> {
    let binary_path = rootfs.join("app").join(bin_name);
    if verbose {
        eprintln!("  go build -o {}...", binary_path.display());
    }
    let build_status = std::process::Command::new("go")
        .args(["build", "-o", &binary_path.to_string_lossy(), "."])
        .current_dir(app_dir)
        .status()
        .context("failed to run `go build`")?;
    if !build_status.success() {
        anyhow::bail!("`go build` failed with exit code {build_status}");
    }

    // On Windows, append .exe
    let is_windows_target = target.is_some_and(|t| parse_target(t).1 == "windows")
        || (target.is_none() && std::env::consts::OS == "windows");
    if is_windows_target {
        let win_path = rootfs.join("app").join(format!("{bin_name}.exe"));
        if binary_path.exists() && !win_path.exists() {
            std::fs::rename(&binary_path, &win_path)?;
        }
    }

    strip_compiled_sources(rootfs, bin_name);
    Ok(())
}

/// Strip source files from the staged app — only the compiled binary (and
/// directories, which may hold configs) is needed. Shared by the Go and Rust
/// build paths.
fn strip_compiled_sources(rootfs: &Path, bin_name: &str) {
    let app_root = rootfs.join("app");
    if !app_root.is_dir() {
        return;
    }
    let entries: Vec<_> = std::fs::read_dir(&app_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            // Keep the binary and directories (might have configs)
            if name_str == bin_name
                || name_str == format!("{bin_name}.exe")
                || e.file_type().map(|t| t.is_dir()).unwrap_or(false)
            {
                None
            } else {
                Some(e.path())
            }
        })
        .collect();
    for path in &entries {
        if path.is_file() {
            std::fs::remove_file(path).ok();
        }
    }
}

/// Embed the interpreter, `N-API` addons and `RoadRunner` into the staged rootfs.
///
/// When `target` is set and differs from the host architecture, downloads a
/// target-specific interpreter (Python via `python-build-standalone`, Node.js
/// via official binaries) and embeds that. Otherwise uses the host's
/// interpreter from PATH.
fn embed_interpreters(args: &BuildArgs, plan: &BuildPlan, rootfs: &Path, target: Option<&str>) {
    let app_dir = &plan.app_dir;
    let verbose = plan.verbose;

    let host_arch = std::env::consts::ARCH;
    let target_arch = target.map(|t| parse_target(t).0);
    let is_cross = target_arch.as_deref() != Some(host_arch);

    let embedded_interpreter_str = resolve_embed_interpreter(args, &plan.runtime_name);

    let mut interpreter_embedded = false;
    if let Some(ref interpreter_name) = embedded_interpreter_str {
        let interp_path = if is_cross {
            match interpreter_name.as_str() {
                "python3" => match ensure_python(target, verbose) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        eprintln!(
                            "[daedalus] warning: failed to download cross-compiled python: {e}"
                        );
                        None
                    }
                },
                "node" => match ensure_node(target, verbose) {
                    Ok(p) => Some(p.join("node")),
                    Err(e) => {
                        eprintln!(
                            "[daedalus] warning: failed to download cross-compiled node: {e}"
                        );
                        None
                    }
                },
                other => {
                    eprintln!("[daedalus] warning: no cross-compiled {other} available, falling back to host");
                    None
                }
            }
        } else {
            None
        };

        if let Some(path) = interp_path {
            interpreter_embedded = embed_primary_interpreter(&path, rootfs, app_dir, verbose);
        } else {
            let host_path = which::which(interpreter_name)
                .ok()
                .or_else(|| embed::find_interpreter_host(interpreter_name));
            if let Some(path) = host_path {
                interpreter_embedded = embed_primary_interpreter(&path, rootfs, app_dir, verbose);
            }
        }
    }

    compile_python_bytecode(&plan.runtime_name, rootfs, verbose);
    embed_napi_addons_if_node(&plan.runtime_name, rootfs, verbose);
    embed_roadrunner(app_dir, rootfs, verbose);

    if !interpreter_embedded && verbose && embedded_interpreter_str.is_some() {
        eprintln!("  (interpreter embedding skipped)");
    }

    // Clean up downloaded build tools (node/npm, composer.phar). Only the
    // tool's own cache dir is removed — never a shared `/tmp` path, which
    // another user/process may own or have symlinked (roadmap #36).
    let _ = std::fs::remove_dir_all(cache_dir().join("build-tools"));
}

/// Explicit `--embed-interpreter` wins, else the runtime's default binary.
fn resolve_embed_interpreter(args: &BuildArgs, runtime_name: &str) -> Option<String> {
    if let Some(ref interp) = args.embed_interpreter {
        return Some(interp.clone());
    }
    match runtime_name {
        "python" => Some("python3".to_string()),
        "node" => Some("node".to_string()),
        "php" => Some("php".to_string()),
        "ruby" => Some("ruby".to_string()),
        "deno" => Some("deno".to_string()),
        _ => None,
    }
}

/// Embed the requested interpreter; returns whether embedding succeeded.
fn embed_primary_interpreter(
    interpreter_path: &Path,
    rootfs: &Path,
    app_dir: &Path,
    verbose: bool,
) -> bool {
    if verbose {
        eprintln!("Embedding interpreter: {}...", interpreter_path.display());
    }
    match embed::embed_interpreter_from_path(interpreter_path, rootfs, Some(app_dir), verbose) {
        Ok(count) => {
            if verbose {
                eprintln!("Embedded interpreter ({} files copied)", count);
            }
            true
        }
        Err(e) => {
            eprintln!("[daedalus] warning: failed to embed interpreter: {}", e);
            false
        }
    }
}

/// Pre-compile Python bytecode for faster startup.
fn compile_python_bytecode(runtime_name: &str, rootfs: &Path, verbose: bool) {
    if runtime_name != "python" {
        return;
    }
    let app_root = rootfs.join("app");
    if !app_root.is_dir() {
        return;
    }
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

/// Embed N-API native addon dependencies (.node files → ldd → .so).
fn embed_napi_addons_if_node(runtime_name: &str, rootfs: &Path, verbose: bool) {
    if runtime_name != "node" {
        return;
    }
    match embed::embed_napi_addons(rootfs, verbose) {
        Ok(n) => {
            if verbose && n > 0 {
                eprintln!("Embedded {} N-API shared library dependencies", n);
            }
        }
        Err(e) => {
            eprintln!("[daedalus] warning: N-API addon embedding failed: {e}");
        }
    }
}

/// Embed the `RoadRunner` binary for Laravel Octane apps (`rr.yaml`/`.rr.yaml`).
fn embed_roadrunner(app_dir: &Path, rootfs: &Path, verbose: bool) {
    if !(app_dir.join("rr.yaml").is_file() || app_dir.join(".rr.yaml").is_file()) {
        return;
    }
    if which::which("rr").is_ok() {
        if verbose {
            eprintln!("Embedding RoadRunner...");
        }
        if let Err(e) = embed::embed_interpreter("rr", rootfs, None, verbose) {
            eprintln!("[daedalus] warning: failed to embed RoadRunner: {}", e);
        }
    } else if verbose {
        eprintln!("[daedalus] warning: rr binary not found on PATH; RoadRunner won't be available at runtime");
    }
}

/// Extract the payload from an existing .de binary into a rootfs directory,
/// then overlay new app files (with tree-shaking/includes) on top.
fn reuse_rootfs_from_binary(
    bin_path: &Path,
    app_dir: &Path,
    args: &BuildArgs,
    verbose: bool,
) -> Result<(tempfile::TempDir, PathBuf)> {
    use daedalus_core::format::Footer;
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(bin_path)?;
    let footer = Footer::read_from(&mut file)?;
    if footer.meta_offset > 0 {
        // Check if squashfs (can't extract incrementally)
        let meta_bytes = {
            let mut buf = vec![0u8; footer.meta_size as usize];
            file.seek(SeekFrom::Start(footer.meta_offset))?;
            file.read_exact(&mut buf)?;
            buf
        };
        let meta: serde_json::Value =
            serde_json::from_slice(&meta_bytes).context("failed to parse metadata JSON")?;
        let is_squashfs = meta
            .get("payload_format")
            .and_then(|v| v.as_str())
            .map(|s| s == "squashfs")
            .unwrap_or(false);
        if is_squashfs {
            return Err(anyhow::anyhow!(
                "cannot reuse squashfs payload incrementally; use tar+zstd build"
            ));
        }
    }

    // Extract payload bytes
    let len: usize = footer
        .payload_csize
        .try_into()
        .map_err(|_| anyhow::anyhow!("payload size does not fit in usize"))?;
    let start = footer.payload_offset;
    file.seek(SeekFrom::Start(start))?;
    let mut payload = vec![0u8; len];
    file.read_exact(&mut payload)?;

    // Decompress into rootfs
    let tmp = tempfile::tempdir().context("failed to create temp directory")?;
    let rootfs = tmp.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).context("failed to create rootfs directory")?;

    // Decompress zstd payload into tar bytes, then extract tar
    let tar_bytes = daedalus_core::compress::decompress(&payload)?;
    let mut archive = tar::Archive::new(&tar_bytes[..]);
    archive.unpack(&rootfs)?;

    // Overlay new app files with tree-shake/minify/includes
    let app_dest = rootfs.join("app");
    copy_dir_recursive_with(app_dir, &app_dest, true)
        .context("failed to copy app files into reused rootfs")?;

    if args.tree_shake {
        let removed = daedalus_core::treeshake::prune_node_modules(&app_dest, verbose)
            .context("tree-shaking failed")?;
        if verbose {
            eprintln!("  tree-shake: removed {removed} unused package(s)");
        }
    }

    if args.minify {
        let minified = daedalus_core::minify::minify_app_dir(&app_dest, verbose)
            .context("minification failed")?;
        if verbose {
            eprintln!("  minify: minified {minified} file(s)");
        }
    }

    if !args.include.is_empty() {
        let count = daedalus_core::include::copy_include_paths(&args.include, &app_dest, app_dir)
            .context("failed to copy include paths")?;
        if verbose {
            eprintln!("  include: copied {count} path(s) into rootfs");
        }
    }

    Ok((tmp, rootfs))
}

/// Compress the staged rootfs: `SquashFS` image (v5) or tar+zstd stream.
fn compress_payload(args: &BuildArgs, plan: &BuildPlan, rootfs: &Path) -> Result<Vec<u8>> {
    // Build the payload: zstd(tar) by default, or a real SquashFS image
    // when `--squashfs` was requested (v5). Before this fix the flag only
    // flipped the metadata's payload_format while the payload stayed a
    // zstd+tar stream — the stub's squashfs extractor would fail on it.
    eprintln!("Creating payload...");
    let t0 = std::time::Instant::now();
    let payload = if plan.squashfs {
        create_squashfs_payload(rootfs, plan.verbose)
            .context("failed to create squashfs payload")?
    } else {
        daedalus_core::tar::create_tar_zstd_with_level(rootfs, args.compression_level)
            .context("failed to create tar+zstd payload")?
    };
    if plan.verbose {
        eprintln!(
            "  compress: {}ms, {} MB",
            t0.elapsed().as_millis(),
            payload.len() as f64 / 1_048_576.0
        );
    }
    Ok(payload)
}

/// Build the metadata env map: env-file < `--env` < `--define` < built-ins.
fn build_env_map(
    args: &BuildArgs,
    plan: &BuildPlan,
    app_name: &str,
) -> Result<Vec<(String, String)>> {
    let mut env_map = serde_json::Map::new();
    env_map.insert("DAEDALUS_RUNTIME".into(), plan.runtime_name.clone().into());
    env_map.insert("DAEDALUS_APP_NAME".into(), app_name.to_string().into());

    load_env_file(&mut env_map, plan.env_file.as_deref());
    insert_kv_flags(&mut env_map, &args.env);
    insert_kv_flags(&mut env_map, &args.define);

    inject_builtin_env(&mut env_map, args, plan, app_name)?;

    Ok(env_map
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
        .collect())
}

/// Load an env-file (KEY=VALUE per line, # comments, blank lines).
fn load_env_file(
    env_map: &mut serde_json::Map<String, serde_json::Value>,
    env_file: Option<&Path>,
) {
    let Some(ef) = env_file else { return };
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

/// Inline `KEY=VALUE` flags (`--env`, then `--define` — later calls win).
fn insert_kv_flags(env_map: &mut serde_json::Map<String, serde_json::Value>, entries: &[String]) {
    for entry in entries {
        if let Some((k, v)) = entry.split_once('=') {
            env_map.insert(k.trim().into(), v.trim().into());
        }
    }
}

fn inject_builtin_env(
    env_map: &mut serde_json::Map<String, serde_json::Value>,
    args: &BuildArgs,
    plan: &BuildPlan,
    app_name: &str,
) -> Result<()> {
    inject_xdebug_default(env_map, &plan.runtime_name, plan.verbose);
    inject_persist_dir(env_map, args.persist, app_name, plan.verbose);
    inject_health_port(env_map, args.health_port, plan.verbose);
    inject_cron_tasks(env_map, &args.cron, plan.verbose)?;
    Ok(())
}

fn inject_xdebug_default(
    env_map: &mut serde_json::Map<String, serde_json::Value>,
    runtime_name: &str,
    verbose: bool,
) {
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
}

fn inject_persist_dir(
    env_map: &mut serde_json::Map<String, serde_json::Value>,
    enabled: bool,
    app_name: &str,
    verbose: bool,
) {
    // ── Persistent storage ────────────────────────────────────────────
    if !enabled {
        return;
    }
    let persist_dir = daedalus_core::persistent::get_persist_dir(app_name);
    let _ = daedalus_core::persistent::ensure_persist_dir(app_name);
    env_map.insert(
        "DAEDALUS_PERSIST_DIR".into(),
        serde_json::Value::String(persist_dir.to_string_lossy().into()),
    );
    if verbose {
        eprintln!("  persistent storage: {}", persist_dir.display());
    }
}

fn inject_health_port(
    env_map: &mut serde_json::Map<String, serde_json::Value>,
    port: Option<u16>,
    verbose: bool,
) {
    // ── Health check port ─────────────────────────────────────────────
    let Some(port) = port else { return };
    env_map.insert(
        "DAEDALUS_HEALTH_PORT".into(),
        serde_json::Value::String(port.to_string()),
    );
    if verbose {
        eprintln!("  health: endpoint enabled on port {port}");
    }
}

fn inject_cron_tasks(
    env_map: &mut serde_json::Map<String, serde_json::Value>,
    crons: &[String],
    verbose: bool,
) -> Result<()> {
    // ── Cron/scheduled tasks ──────────────────────────────────────────
    if crons.is_empty() {
        return Ok(());
    }
    let mut tasks_json: Vec<serde_json::Value> = Vec::new();
    for ct in crons {
        let Some((name, schedule)) = ct.split_once(':') else {
            anyhow::bail!("--cron format: NAME:SCHEDULE (got '{ct}')");
        };
        let interval = daedalus_core::cron::parse_schedule(schedule);
        tasks_json.push(serde_json::json!({
            "name": name,
            "schedule": schedule,
            "interval_secs": interval,
        }));
        if verbose {
            eprintln!("  cron: {name} -> every {interval}s (from {schedule})");
        }
    }
    env_map.insert(
        "DAEDALUS_CRON_TASKS".into(),
        serde_json::Value::Array(tasks_json),
    );
    if verbose {
        eprintln!("  cron: {} task(s) registered", crons.len());
    }
    Ok(())
}

/// Final entrypoint argv: explicit flag > Go binary > runtime detection;
/// WASI/component-model flags are inserted after the interpreter for wasm runs.
fn resolve_entrypoint_argv(
    args: &BuildArgs,
    app_dir: &Path,
    runtime: detect::Runtime,
    built_binary_name: Option<&str>,
) -> Vec<String> {
    let entrypoint = if !args.entrypoint.is_empty() {
        args.entrypoint.clone()
    } else if let Some(bin_name) = built_binary_name {
        // Compiled runtimes exec the binary directly; Java wraps the built
        // JAR — the stub drops argv[0] and prepends its interpreter.
        if runtime == detect::Runtime::Java {
            vec!["java".into(), "-jar".into(), format!("/app/{bin_name}")]
        } else {
            vec![format!("/app/{bin_name}")]
        }
    } else {
        detect::resolve_entrypoint(app_dir, runtime).unwrap_or_else(|| vec!["run".to_string()])
    };

    if runtime == detect::Runtime::Wasm && (args.wasi || args.component_model) {
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
    }
}

/// Collect optional stub features (embedded runtime, wasm, health check,
/// cross-compile, build cache) into a validated metadata blob.
fn build_bun_features(args: &BuildArgs, plan: &BuildPlan) -> Result<BunFeatures> {
    let mut bun_features = BunFeatures::default();
    let verbose = plan.verbose;

    // Set embedded interpreter based on either explicit --embed-interpreter or auto-detection
    let (interpreter_opt, interpreter_path_opt) = resolve_bun_interpreter(args, &plan.runtime_name);
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

    apply_wasm_features(args, &mut bun_features, verbose);
    apply_health_features(args, &mut bun_features, verbose);
    apply_cross_compile(args, &mut bun_features, verbose);

    bun_features.build_cache.enabled = args.use_cache;

    bun_features
        .validate()
        .map_err(|e| anyhow::anyhow!("Invalid build options: {}", e))?;
    Ok(bun_features)
}

fn resolve_bun_interpreter(
    args: &BuildArgs,
    runtime_name: &str,
) -> (Option<EmbeddedInterpreter>, Option<String>) {
    if let Some(ref interp) = args.embed_interpreter {
        let interpreter = match interp.to_lowercase().as_str() {
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
        };
        return (Some(interpreter), args.interpreter_path.clone());
    }
    match runtime_name {
        "python" => (Some(EmbeddedInterpreter::Python3), None),
        "node" => (Some(EmbeddedInterpreter::Node), None),
        "php" => (Some(EmbeddedInterpreter::Php), None),
        "ruby" => (Some(EmbeddedInterpreter::Ruby), None),
        "deno" => (Some(EmbeddedInterpreter::Deno), None),
        _ => (None, None),
    }
}

fn apply_wasm_features(args: &BuildArgs, bun_features: &mut BunFeatures, verbose: bool) {
    if !args.wasm {
        return;
    }
    eprintln!("[daedalus] warning: --wasm is experimental/alpha — requires wasmtime binary in rootfs or on PATH");
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

fn apply_health_features(args: &BuildArgs, bun_features: &mut BunFeatures, verbose: bool) {
    let Some(port) = args.health_port else { return };
    bun_features.health_check.enabled = true;
    bun_features.health_check.port = port;
    if let Some(ref ep) = args.health_endpoint {
        bun_features.health_check.endpoint.clone_from(ep);
    }
    if verbose {
        eprintln!("  health check: port {}", port);
    }
}

fn apply_cross_compile(args: &BuildArgs, bun_features: &mut BunFeatures, verbose: bool) {
    let Some(ref cross) = args.cross_compile else {
        return;
    };
    eprintln!("[daedalus] warning: --cross-compile is experimental/alpha — not yet implemented in the stub (hidden flag); metadata is recorded but the runtime currently ignores it");
    let targets: Vec<String> = cross.split(',').map(|s| s.trim().to_string()).collect();
    bun_features.cross_compile_targets = targets;
    if verbose {
        eprintln!("  cross-compile: {:?}", bun_features.cross_compile_targets);
    }
}

fn load_encryption_key(path: &Path) -> Result<[u8; 32]> {
    let hex_str = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read encrypt key from {}", path.display()))?;
    let bytes = hex::decode(hex_str.trim())
        .with_context(|| format!("invalid hex in encrypt key file {}", path.display()))?;
    if bytes.len() != 32 {
        anyhow::bail!(
            "encrypt key must be exactly 32 bytes (64 hex chars), got {}",
            bytes.len()
        );
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// Assemble the final `.daedalus` artifact, warn about unsigned SISR artifacts
/// and re-sign on macOS targets.
/// Returns the artifact size in bytes.
fn assemble_and_sign(
    args: &BuildArgs,
    plan: &BuildPlan,
    target: Option<&str>,
    output: &Path,
    inputs: AssembleInputs<'_>,
) -> Result<u64> {
    eprintln!("Assembling {}...", output.display());

    // Save previous SISR manifest before assembly (for bandwidth reporting)
    let prev_manifest_bytes: Option<Vec<u8>> = if args.update && args.enable_sisr {
        let mut prev_manifest_path = output.to_path_buf();
        prev_manifest_path.set_extension("daedalus.manifest");
        std::fs::read(&prev_manifest_path).ok()
    } else {
        None
    };

    let size = if args.enable_sisr {
        let sisr_config = build_sisr_config(&args.key)?;
        let artifacts = match inputs.sisr_artifacts {
            Some(a) => a,
            None => daedalus_core::sisr_stage::build_artifacts(inputs.payload, &sisr_config)
                .context("SISR stage failed during build")?,
        };
        let (encrypted_payload, encryption) = args
            .encrypt
            .as_ref()
            .map(|path| {
                let key = load_encryption_key(path)?;
                let (ct, meta) = encrypt::encrypt_payload(inputs.payload, &key)
                    .context("payload encryption failed")?;
                Ok::<_, anyhow::Error>((ct, meta))
            })
            .transpose()?
            .unzip();
        let payload = encrypted_payload.as_deref().unwrap_or(inputs.payload);
        let input = daedalus_core::assembly::AssemblyInput {
            stub_bytes: inputs.stub_bytes,
            payload,
            meta_bytes: inputs.meta_bytes,
            squashfs: inputs.squashfs,
            target_arch: target,
            sisr: Some(artifacts),
            encryption,
        };
        daedalus_core::assembly::assemble_daedalus(output, &input)
            .context("failed to assemble daedalus (SISR)")?
    } else {
        let (encrypted_payload, encryption) = args
            .encrypt
            .as_ref()
            .map(|path| {
                let key = load_encryption_key(path)?;
                let (ct, meta) = encrypt::encrypt_payload(inputs.payload, &key)
                    .context("payload encryption failed")?;
                Ok::<_, anyhow::Error>((ct, meta))
            })
            .transpose()?
            .unzip();
        let payload = encrypted_payload.as_deref().unwrap_or(inputs.payload);
        let input = daedalus_core::assembly::AssemblyInput {
            stub_bytes: inputs.stub_bytes,
            payload,
            meta_bytes: inputs.meta_bytes,
            squashfs: inputs.squashfs,
            target_arch: target,
            sisr: None,
            encryption,
        };
        daedalus_core::assembly::assemble_daedalus(output, &input)
            .context("failed to assemble daedalus")?
    };

    eprintln!(
        "Built {} ({:.1}MB)",
        output.display(),
        size as f64 / (1024.0 * 1024.0)
    );

    if args.enable_sisr && args.key.is_none() {
        eprintln!(
            "warning: --enable-sisr without --key produces an UNSIGNED SISR section — \
             the stub refuses to run it at cold start unless DAEDALUS_SISR_ALLOW_UNSIGNED=1"
        );
    }

    // macOS code signing: re-sign the assembled binary since appending
    // payload + metadata invalidates any existing Mach-O signature.
    if target
        .as_ref()
        .is_some_and(|t| t.contains("darwin") || t.contains("apple") || t.contains("macos"))
    {
        sign_macos_binary(output, plan.verbose)?;
    }

    report_sisr_update_bandwidth(args, output, prev_manifest_bytes.as_deref());

    Ok(size)
}

/// Compare the freshly written SISR manifest against the previous one and
/// print how many bytes a client would download for the update.
fn report_sisr_update_bandwidth(
    args: &BuildArgs,
    output: &Path,
    prev_manifest_bytes: Option<&[u8]>,
) {
    if !args.enable_sisr {
        return;
    }
    let mut manifest_path = output.to_path_buf();
    manifest_path.set_extension("daedalus.manifest");
    eprintln!("SISR manifest written: {}", manifest_path.display());

    // Bandwidth reporting: compare new chunk set against previous manifest
    let Some(prev_bytes) = prev_manifest_bytes else {
        return;
    };
    if let (Ok(prev), Ok(new_bytes)) = (
        daedalus_core::sisr_stage::RemoteManifest::from_bytes(prev_bytes),
        std::fs::read(&manifest_path),
    ) {
        if let Ok(new_remote) = daedalus_core::sisr_stage::RemoteManifest::from_bytes(&new_bytes) {
            report_sisr_bandwidth(&prev, &new_remote);
        }
    }
}

/// Build a universal `.daedalus` binary that works across multiple architectures.
///
/// Each architecture gets its own complete `.daedalus` slice (stub + payload +
/// footer). The slices are concatenated behind a shell-script polyglot launcher
/// that detects `uname -m` at runtime and extracts the correct slice.
pub(crate) fn build_universal(args: &BuildArgs, plan: &BuildPlan, output: &Path) -> Result<()> {
    let universal_targets: &[(&str, &str, &str)] = &[
        ("x86_64-unknown-linux-musl", "x86_64", "Linux"),
        ("aarch64-unknown-linux-musl", "aarch64", "Linux"),
        ("riscv64gc-unknown-linux-musl", "riscv64", "Linux"),
        ("x86_64-apple-darwin", "x86_64", "Darwin"),
        ("aarch64-apple-darwin", "arm64", "Darwin"),
    ];

    let tmp_dir = tempfile::tempdir().context("failed to create temp dir for universal build")?;
    let mut arch_slices: Vec<daedalus_core::universal::ArchSlice> = Vec::new();
    let mut slice_data: Vec<Vec<u8>> = Vec::new();

    for (target_triple, uname_machine, uname_sys) in universal_targets {
        let slice_path = tmp_dir
            .path()
            .join(format!("slice-{uname_machine}-{uname_sys}"));
        let target_opt = if *target_triple == std::env::consts::ARCH {
            None
        } else {
            Some(target_triple.to_string())
        };

        // Skip architectures whose pre-built stub binary is missing. This
        // allows partial universal builds when stubs are unavailable (e.g.
        // macOS stubs in a Linux-only CI environment).
        match find_stub(&target_opt) {
            Ok(_) => {}
            Err(e) => {
                eprintln!(
                    "[daedalus] warning: skipping {uname_machine} ({uname_sys}) — stub not built: {e}"
                );
                continue;
            }
        }

        eprintln!("[daedalus] Building universal slice for {uname_machine} ({uname_sys})");
        build_single_target(args, plan, target_opt.clone(), &slice_path)?;

        let bytes = std::fs::read(&slice_path)
            .with_context(|| format!("failed to read slice for {uname_machine}"))?;
        let sha256 = daedalus_core::universal::hex_sha256(&bytes);
        arch_slices.push(daedalus_core::universal::ArchSlice {
            target: target_triple.to_string(),
            uname_machine: uname_machine.to_string(),
            uname_sys: uname_sys.to_string(),
            offset: 0,
            size: bytes.len() as u64,
            sha256,
        });
        slice_data.push(bytes);
    }

    let universal_binary =
        daedalus_core::universal::assemble_universal_slices(&arch_slices, &slice_data)?;

    let parent = output.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;
    let tmp_path = parent.join(format!(
        ".{}.tmp",
        output.file_name().unwrap().to_string_lossy()
    ));
    std::fs::write(&tmp_path, &universal_binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp_path, output)?;

    eprintln!(
        "Built universal binary: {} ({:.1}MB, {} slices)",
        output.display(),
        universal_binary.len() as f64 / (1024.0 * 1024.0),
        arch_slices.len(),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::build::args::default_build_args;
    use std::path::{Path, PathBuf};

    fn dir_with_cargo_toml(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), content).unwrap();
        let path = dir.path().to_path_buf();
        (dir, path)
    }

    #[test]
    fn cargo_bin_name_from_package_section() {
        let (_d, path) = dir_with_cargo_toml(
            "[package]\nname = \"hello-daedalus\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        );
        assert_eq!(cargo_bin_name(&path), "hello-daedalus");
    }

    /// An explicit `[[bin]]` target overrides the package name.
    #[test]
    fn cargo_bin_name_prefers_explicit_bin() {
        let (_d, path) = dir_with_cargo_toml(
            "[package]\nname = \"my-tool\"\n\n[[bin]]\nname = \"tool-bin\"\npath = \"src/main.rs\"\n",
        );
        assert_eq!(cargo_bin_name(&path), "tool-bin");
    }

    /// Workspace virtual manifests have no `[package]` — fall back to "app"
    /// until workspace support lands (Phase 8 Step 5).
    #[test]
    fn cargo_bin_name_defaults_for_virtual_manifest() {
        let (_d, path) =
            dir_with_cargo_toml("[workspace]\nmembers = [\"crates/a\", \"crates/b\"]\n");
        assert_eq!(cargo_bin_name(&path), "app");
    }

    #[test]
    fn rust_target_triple_passes_full_triples_through() {
        assert_eq!(
            rust_target_triple("x86_64-unknown-linux-musl"),
            "x86_64-unknown-linux-musl"
        );
        assert_eq!(
            rust_target_triple("aarch64-apple-darwin"),
            "aarch64-apple-darwin"
        );
    }

    #[test]
    fn rust_target_triple_synthesizes_from_shorthands() {
        assert_eq!(rust_target_triple("linux-x64"), "x86_64-unknown-linux-gnu");
        assert_eq!(
            rust_target_triple("linux-x64-musl"),
            "x86_64-unknown-linux-musl"
        );
        assert_eq!(rust_target_triple("macos-arm64"), "aarch64-apple-darwin");
        assert_eq!(rust_target_triple("win-x64"), "x86_64-pc-windows-msvc");
    }

    // ── Phase 8 Step 3: Java / Maven / Gradle ─────────────────────────────

    /// A built JAR becomes `java -jar /app/<name>`: the stub drops argv[0]
    /// and prepends its interpreter, so argv[0] here is a placeholder.
    #[test]
    fn java_entrypoint_wraps_built_jar() {
        let args = default_build_args();
        let ep = resolve_entrypoint_argv(
            &args,
            Path::new("/tmp/fake-app"),
            detect::Runtime::Java,
            Some("app-1.0.jar"),
        );
        assert_eq!(
            ep,
            vec![
                "java".to_string(),
                "-jar".to_string(),
                "/app/app-1.0.jar".to_string()
            ]
        );
    }

    fn write_jar(dir: &Path, rel: &str, size: usize) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, vec![0u8; size]).unwrap();
    }

    /// Shade originals, sources and `-plain` jars are by-products; among
    /// real candidates the largest (fat jar) wins.
    #[test]
    fn find_built_jar_skips_auxiliary_and_prefers_largest() {
        let dir = tempfile::tempdir().unwrap();
        write_jar(dir.path(), "target/app-1.0-sources.jar", 10);
        write_jar(dir.path(), "target/original-app-1.0.jar", 500);
        write_jar(dir.path(), "build/libs/app-1.0-plain.jar", 50);
        write_jar(dir.path(), "target/app-1.0.jar", 200);
        write_jar(dir.path(), "build/libs/lib-2.0.jar", 100);

        let picked = find_built_jar(dir.path())
            .expect("a runnable JAR must be found")
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        assert_eq!(picked.as_deref(), Some("app-1.0.jar"));
    }

    #[test]
    fn find_built_jar_returns_none_without_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_built_jar(dir.path()), None);
    }

    /// Project wrappers pin the toolchain version — they win over any
    /// system install.
    #[test]
    fn java_build_tool_prefers_wrapper() {
        let maven = tempfile::tempdir().unwrap();
        std::fs::write(maven.path().join("pom.xml"), "<project/>").unwrap();
        std::fs::write(maven.path().join("mvnw"), "#!/bin/sh").unwrap();
        assert_eq!(
            java_build_tool(maven.path()).unwrap(),
            JavaBuildTool::Maven {
                cmd: "./mvnw".into()
            }
        );

        let gradle = tempfile::tempdir().unwrap();
        std::fs::write(gradle.path().join("build.gradle.kts"), "").unwrap();
        std::fs::write(gradle.path().join("gradlew"), "#!/bin/sh").unwrap();
        assert_eq!(
            java_build_tool(gradle.path()).unwrap(),
            JavaBuildTool::Gradle {
                cmd: "./gradlew".into()
            }
        );
    }

    // ── build_layers tests ──────────────────────────────────────────────

    #[test]
    fn build_layers_creates_runtime_layer() {
        let dir = tempfile::tempdir().unwrap();
        let layers = build_layers("python", &["python3".into()], &[], dir.path());
        assert_eq!(layers.len(), 1);
        match &layers[0] {
            daedalus_core::layer::SerializableLayer::Runtime(r) => {
                assert_eq!(r.name, "python");
                assert_eq!(r.interpreter, "python");
                assert_eq!(r.entrypoint, vec!["python3"]);
            }
            _ => panic!("expected RuntimeLayer"),
        }
    }

    #[test]
    fn build_layers_adds_config_layer_when_daedalus_toml_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".daedalus.toml"),
            b"[package]\nname = \"test\"\n",
        )
        .unwrap();

        let layers = build_layers("node", &["node".into()], &[], dir.path());
        assert_eq!(layers.len(), 2);

        let config_layer = layers
            .iter()
            .find(|l| matches!(l, daedalus_core::layer::SerializableLayer::Config(_)));
        assert!(config_layer.is_some(), "expected a ConfigLayer");
    }
}
