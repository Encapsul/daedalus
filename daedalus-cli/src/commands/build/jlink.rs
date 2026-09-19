//! Minimal-JRE embedding via `jlink` for Java builds.
//!
//! The plain Java path (`daedalus_core::embed::embed_java_config`) copies the
//! host JRE's whole `lib/` (~200 MB). When the toolchain is JDK 9+, this module
//! instead asks `jdeps` which JDK modules the app's JARs reference, slices a
//! minimal runtime image with `jlink`, and embeds that — the launcher binary in
//! the image reads `lib/modules` exactly like a full runtime.
//!
//! Opt-outs: `--full-jre` restores the old full-JRE copy; `--jlink-modules`
//! pins the closure when `jdeps` cannot see it (reflective/SPI frameworks).
//! Cross-target builds are skipped: `jlink` emits a host-arch image and Java's
//! native libs are arch-specific (the full-JRE embed has the same limitation).

use anyhow::{Context, Result};
use daedalus_core::embed::copy_dir_recursive;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::args::BuildArgs;

/// Modules frameworks load reflectively (Spring Boot fat jars, JNDI/XML config)
/// that `jdeps` cannot see statically. Kept conservative — the user can widen
/// the closure with `--jlink-modules`.
const FAT_JAR_EXTRA_MODULES: &[&str] = &[
    "java.logging",
    "java.sql",
    "java.xml",
    "java.management",
    "java.naming",
];

/// Try to install a minimal `jlink` JRE for the app's JARs into `rootfs`.
///
/// Returns `true` when a minimal image was installed (the caller must then skip
/// the full-JRE embed). A `false` result means the caller falls back to the full
/// JRE — except when `--jlink-modules` was pinned, in which case a failure is an
/// error, because the user explicitly asked for this image.
pub fn embed_minimal_jre(args: &BuildArgs, rootfs: &Path, verbose: bool) -> Result<bool> {
    if args.full_jre {
        return Ok(false);
    }
    let Some(java_home) = java_home() else {
        if verbose {
            eprintln!("  jlink: skipping (no JDK with lib/modules found)");
        }
        return Ok(false);
    };
    let Some((jdeps, jlink)) = jlink_tools(&java_home) else {
        if verbose {
            eprintln!(
                "  jlink: skipping (jdeps/jlink not available in {})",
                java_home.display()
            );
        }
        return Ok(false);
    };

    let jars = collect_jars(rootfs);
    let Some(modules) = resolve_modules(args.jlink_modules.as_deref(), &jdeps, &jars)? else {
        if verbose {
            eprintln!("  jlink: no module deps detected; embedding full JRE");
        }
        return Ok(false);
    };

    let dir = tempfile::tempdir().context("failed to create jlink temp dir")?;
    let image = dir.path().join("jre");
    match build_image(&jlink, &modules, &image) {
        Ok(()) => {}
        Err(e) if args.jlink_modules.is_none() => {
            if verbose {
                eprintln!("  jlink: build failed ({e:#}); embedding full JRE");
            }
            return Ok(false);
        }
        Err(e) => return Err(e.context("jlink image build failed")),
    }
    if !image_runs(&image) {
        if args.jlink_modules.is_none() {
            if verbose {
                eprintln!("  jlink: generated image does not start; embedding full JRE");
            }
            return Ok(false);
        }
        anyhow::bail!("jlink image produced a non-runnable java launcher");
    }
    install_image(&image, rootfs)?;
    if verbose {
        eprintln!(
            "  embed: Java minimal JRE (jlink, {} modules)",
            modules.len()
        );
    }
    Ok(true)
}

/// Compute the jlink module closure. Returns `None` when nothing can be derived
/// (the caller falls back to the full JRE). An override skips `jdeps` entirely.
fn resolve_modules(
    override_csv: Option<&str>,
    jdeps: &Path,
    jars: &[PathBuf],
) -> Result<Option<Vec<String>>> {
    let mut mods = BTreeSet::new();
    if let Some(csv) = override_csv {
        mods.extend(parse_module_csv(csv));
        if mods.is_empty() {
            anyhow::bail!("--jlink-modules produced an empty module list");
        }
        return Ok(Some(mods.into_iter().collect()));
    }

    let fat_jar = jars.iter().any(|jar| is_fat_jar(jar));
    for jar in jars {
        mods.extend(run_jdeps(jdeps, jar)?);
    }
    if mods.is_empty() {
        return Ok(None);
    }
    mods.insert("java.base".to_string());
    if fat_jar {
        mods.extend(FAT_JAR_EXTRA_MODULES.iter().map(|m| m.to_string()));
    }
    Ok(Some(mods.into_iter().collect()))
}

/// Locate the host JDK home by asking `java` for `java.home`, restricted to JDK
/// 9+ (a `lib/modules` image, which is what `jlink` consumes).
fn java_home() -> Option<PathBuf> {
    let output = Command::new("java")
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let home = stderr
        .lines()
        .find_map(|line| line.trim().strip_prefix("java.home = ").map(str::trim))?;
    let home = PathBuf::from(home);
    home.join("lib/modules").is_file().then_some(home)
}

/// `jdeps`/`jlink` live beside `java` in a JDK install.
fn jlink_tools(java_home: &Path) -> Option<(PathBuf, PathBuf)> {
    let jdeps = java_home.join("bin/jdeps");
    let jlink = java_home.join("bin/jlink");
    (jdeps.is_file() && jlink.is_file()).then_some((jdeps, jlink))
}

/// Collect every JAR staged under `rootfs/app` (the built app + its libs).
fn collect_jars(rootfs: &Path) -> Vec<PathBuf> {
    let mut jars = Vec::new();
    let mut stack = vec![rootfs.join("app")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "jar") {
                jars.push(path);
            }
        }
    }
    jars
}

/// Run `jdeps` on one JAR and return the modules it references.
fn run_jdeps(jdeps: &Path, jar: &Path) -> Result<Vec<String>> {
    let output = Command::new(jdeps)
        .args(["--ignore-missing-deps", "--print-module-deps"])
        .arg(jar)
        .output()
        .with_context(|| format!("failed to run jdeps on {}", jar.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_module_csv(stdout.trim()))
}

/// Split a comma-separated module list, dropping empty entries.
fn parse_module_csv(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .collect()
}

/// A Spring Boot (`BOOT-INF/lib`) or WAR (`WEB-INF/lib`) layout carries nested
/// jars `jdeps` cannot see on the outer jar — the closure needs the boosters.
fn is_fat_jar(jar: &Path) -> bool {
    let Ok(file) = std::fs::File::open(jar) else {
        return false;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return false;
    };
    (0..zip.len()).any(|i| {
        zip.by_index(i)
            .ok()
            .and_then(|f| f.enclosed_name().map(|n| n.to_string_lossy().into_owned()))
            .is_some_and(|n| n.starts_with("BOOT-INF/lib/") || n.starts_with("WEB-INF/lib/"))
    })
}

/// Run `jlink` to slice a runtime image for `modules`, using the shared options
/// (strip debug symboles, no man pages/headers, level-2 zip compression — the
/// `zip-N` syntax only exists on JDK 19+, `2` works on every JDK 9+).
fn build_image(jlink: &Path, modules: &[String], out: &Path) -> Result<()> {
    if out.exists() {
        std::fs::remove_dir_all(out)?;
    }
    let csv = modules.join(",");
    let status = Command::new(jlink)
        .args(["--add-modules", &csv, "--output"])
        .arg(out)
        .args([
            "--strip-debug",
            "--no-man-pages",
            "--no-header-files",
            "--compress",
            "2",
        ])
        .status()
        .with_context(|| format!("failed to run jlink for modules: {csv}"))?;
    if !status.success() {
        anyhow::bail!("jlink exited with status {status:?} for modules: {csv}");
    }
    Ok(())
}

/// Smoke-test the generated image: a bare `java --list-modules` must start.
fn image_runs(image: &Path) -> bool {
    Command::new(image.join("bin/java"))
        .arg("--list-modules")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Copy the jlink image into the stable JVM path and expose `java` on PATH.
///
/// The image alone is not enough after `pivot_root`: `bin/java` and its
/// dlopen'ed `libjvm.so` link against the host glibc, and the extraction tree
/// starts bare. Each dependency (`ld-linux`, `libc.so.6`, `libz.so.1`, …) is
/// copied to its standard absolute path inside the rootfs, exactly like the
/// full-JRE embed path does via `ldd`.
fn install_image(image: &Path, rootfs: &Path) -> Result<()> {
    let target = rootfs.join("usr/lib/jvm/java");
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .with_context(|| format!("failed to clear existing JRE at {}", target.display()))?;
    }
    copy_dir_recursive(image, &target)
        .with_context(|| format!("failed to install jlink image to {}", target.display()))?;
    fixup_jre_launcher(rootfs)
}

/// Wire a Java runtime at `usr/lib/jvm/java` into the rootfs: embed its system
/// deps and expose the launcher to the PATH with an in-root relative symlink.
///
/// Also called after the full-JRE embed (`embed_interpreter_from_path`), which
/// leaves a raw host-launcher copy at `usr/bin/java` and no `bin/` inside the
/// image: the launcher is moved into `usr/lib/jvm/java/bin/` so `java.home`
/// (derived from `/proc/self/exe` at runtime) resolves to the image, then the
/// absolute symlink in the payload is replaced by a relative one.
pub fn fixup_jre_launcher(rootfs: &Path) -> Result<()> {
    let image = rootfs.join("usr/lib/jvm/java");
    if !image.join("lib").is_dir() {
        return Ok(());
    }
    if !image.join("bin/java").is_file() {
        let bin = image.join("bin");
        std::fs::create_dir_all(&bin)
            .with_context(|| format!("failed to create {}", bin.display()))?;
        let from = rootfs.join("usr/bin/java");
        if !from.is_file() {
            anyhow::bail!(
                "full-JRE embed left no launcher binary at {} for {}",
                from.display(),
                image.display()
            );
        }
        std::fs::copy(&from, image.join("bin/java")).with_context(|| {
            format!(
                "failed to move launcher into {}",
                image.join("bin/java").display()
            )
        })?;
    }
    #[cfg(target_os = "linux")]
    embed_image_deps(&image, rootfs)?;

    let bin_java = rootfs.join("usr/bin/java");
    std::fs::create_dir_all(bin_java.parent().expect("usr/bin/java always has a parent"))
        .with_context(|| format!("failed to create {}", bin_java.parent().unwrap().display()))?;
    // Relative (not absolute) target: the payload builder drops symlinks that
    // escape the rootfs, and `/usr/lib/jvm` is `/usr/bin/../lib/jvm`, so the
    // link stays inside the root and survives the tar stage.
    // Remove whatever is there (regular file or stale symlink) before
    // installing the relative link.
    let _ = std::fs::remove_file(&bin_java);
    #[cfg(unix)]
    std::os::unix::fs::symlink("../lib/jvm/java/bin/java", &bin_java)
        .with_context(|| format!("failed to symlink {}", bin_java.display()))?;
    Ok(())
}

/// Copy the shared-library dependencies of the image's launcher and VM into
/// the rootfs. The JVM loads `libjvm.so` via dlopen, so its deps are needed
/// too. The image's own `lib/*.so` are flattened into /usr/lib: once the
/// launcher reaches the PATH via the `/usr/bin/java` symlink, `$ORIGIN` is
/// `/usr/bin`, so its RUNPATH (`$ORIGIN/../lib`) misses the image lib dir —
/// but the stub's `LD_LIBRARY_PATH` includes /usr/lib.
///
/// Linux-only: it embeds ELF/glibc system libs into a bare pivot_root tree.
/// On macOS there is nothing to embed — the launcher finds libjvm relative to
/// its real path (`java.home`) inside the image and system dylibs resolve from
/// the dyld shared cache (and `ldd` does not even exist there).
#[cfg(target_os = "linux")]
fn embed_image_deps(image: &Path, rootfs: &Path) -> Result<()> {
    let mut binaries = vec![image.join("bin/java")];
    let mut image_libs = BTreeSet::new();
    let mut stack = vec![image.join("lib")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().is_some_and(|n| n == "libjvm.so") {
                binaries.push(path);
            } else if path.extension().is_some_and(|e| e == "so")
                && (dir == image.join("lib")
                    || dir.parent().is_some_and(|p| p == image.join("lib")))
            {
                image_libs.insert(path);
            }
        }
    }
    for binary in binaries {
        copy_ldd_deps(&binary, rootfs)?;
    }
    let usr_lib = rootfs.join("usr/lib");
    std::fs::create_dir_all(&usr_lib)?;
    for lib in image_libs {
        let dst = usr_lib.join(lib.file_name().expect("image lib has a file name"));
        if dst.exists() {
            continue;
        }
        std::fs::copy(&lib, &dst)
            .with_context(|| format!("failed to embed JVM lib {}", lib.display()))?;
    }
    Ok(())
}

/// Copy every absolute path `ldd` reports for `binary` into the rootfs under
/// the loader's search dirs. Host distros may name lib dirs by ABI triple
/// (`/lib/x86_64-linux-gnu`) which is NOT in the pivot-root
/// `LD_LIBRARY_PATH` — only `/usr/lib/x86_64-linux-gnu` is. The dynamic
/// loader (`ld-linux`) keeps its absolute interp path.
#[cfg(target_os = "linux")]
fn copy_ldd_deps(binary: &Path, rootfs: &Path) -> Result<()> {
    let output = Command::new("ldd")
        .arg(binary)
        .output()
        .with_context(|| format!("failed to run ldd on {}", binary.display()))?;
    if !output.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for token in text.split_whitespace() {
        if !token.starts_with('/') || token.starts_with("0x") {
            continue;
        }
        let src = Path::new(token);
        let name = src.file_name().expect("absolute lib path has a name");
        let dst = if name.to_string_lossy().starts_with("ld-linux") {
            rootfs.join("lib64").join(name)
        } else if token.contains("-linux-gnu") {
            rootfs.join("usr/lib/x86_64-linux-gnu").join(name)
        } else {
            rootfs.join("usr/lib").join(name)
        };
        if dst.exists() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, &dst)
            .with_context(|| format!("failed to embed system lib {}", src.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_csv_splits_trims_and_drops_empty() {
        assert_eq!(
            parse_module_csv("java.base, java.logging,"),
            vec!["java.base", "java.logging"]
        );
        assert!(parse_module_csv("").is_empty());
    }

    #[test]
    fn override_skips_jdeps_entirely() {
        let mods = resolve_modules(Some("java.base,java.sql"), Path::new("missing-jdeps"), &[])
            .unwrap()
            .unwrap();
        assert_eq!(mods, vec!["java.base", "java.sql"]);
    }

    #[test]
    fn empty_override_is_rejected() {
        assert!(resolve_modules(Some(" , ,"), Path::new("x"), &[]).is_err());
    }

    #[test]
    fn fat_jar_detection_spots_boot_inf_lib() {
        let tmp = tempfile::tempdir().unwrap();
        let fat = tmp.path().join("app.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&fat).unwrap());
        let opts = zip::write::FileOptions::<()>::default();
        zip.add_directory("BOOT-INF/lib/", opts).unwrap();
        zip.finish().unwrap();
        assert!(is_fat_jar(&fat));

        let plain = tmp.path().join("plain.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&plain).unwrap());
        zip.add_directory("META-INF/", opts).unwrap();
        zip.finish().unwrap();
        assert!(!is_fat_jar(&plain));
    }

    #[test]
    fn collect_jars_walks_nested_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("app/lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(tmp.path().join("app/root.jar"), b"").unwrap();
        std::fs::write(lib.join("nested.jar"), b"").unwrap();
        let jars = collect_jars(tmp.path());
        assert_eq!(jars.len(), 2);
    }

    fn jlink_available() -> bool {
        Command::new("jlink")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[test]
    fn jdeps_without_classes_yields_none() {
        if !jlink_available() {
            return;
        }
        let Some(java_home) = java_home() else {
            return;
        };
        let Some((jdeps, _)) = jlink_tools(&java_home) else {
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty.jar");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&empty).unwrap());
        let opts = zip::write::FileOptions::<()>::default();
        zip.add_directory("META-INF/", opts).unwrap();
        zip.finish().unwrap();
        assert!(resolve_modules(None, &jdeps, &[empty]).unwrap().is_none());
    }

    #[test]
    fn jlink_slices_a_runnable_java_base_image() {
        if !jlink_available() {
            return;
        }
        let Some(java_home) = java_home() else {
            return;
        };
        let Some((_, jlink)) = jlink_tools(&java_home) else {
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let img = tmp.path().join("img");
        let mods = vec!["java.base".to_string()];
        build_image(&jlink, &mods, &img).unwrap();
        assert!(image_runs(&img), "minimal image must launch java");
        assert!(img.join("lib/modules").is_file());
        assert!(
            !img.join("bin/javac").exists(),
            "minimal image must not carry javac"
        );
    }
}
