#![allow(clippy::doc_markdown)]
//! Chaos-monkey tests for runtime detection and entrypoint resolution.
//!
//! Invariants:
//! - `detect_runtime` / `resolve_entrypoint` never panic on hostile trees.
//! - Perl detection ignores module names that would traverse out of `lib/`.
//! - Resolved entrypoint argv never contains `..` path segments.

use daedalus_core::detect::{detect_runtime, resolve_entrypoint, Runtime};
use std::path::Path;
use tempfile::TempDir;

/// Deterministic xorshift PRNG so failures reproduce without a RNG dependency.
struct Prng(u64);

impl Prng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
    fn pick(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Entry names that have broken detection before: traversals, separators,
/// Unicode, dotfiles, suffixes without a dir, over-long names. Names stay
/// inside the given directory (a `..` segment only resolves to the app root).
const HOSTILE_NAMES: &[&str] = &[
    "script",
    "lib",
    "script/app",
    "script/my_app",
    "script/app/../etc",
    "script/x;rm -rf /",
    "script/x\n",
    "script/application",
    "script/{}",
    "script/x",
    "script/.hidden",
    "lib\\My\\App.pm",
    "lib/MyApp.pm",
    "lib/My/App.pm",
    "app.pl",
    "main.pl",
    "Makefile.PL",
    "cpanfile",
    "script/server",
    "script/app.pl",
    "bin/app",
    "config.ru",
    "escape",
    "..",
];

/// Build a hostile tree: a pseudo-random subset of hostile entries with
/// pseudo-random binary contents.
fn build_hostile_tree(seed: u64) -> TempDir {
    let dir = TempDir::new().unwrap();
    let mut prng = Prng(seed ^ 0xDEDA_0005);
    let mut names: Vec<&str> = HOSTILE_NAMES.to_vec();
    let long_name = "a".repeat(300);
    if prng.pick(2) == 0 {
        names.push(&long_name);
    }
    for _ in 0..(prng.pick(names.len()) / 2) {
        let a = prng.pick(names.len());
        let b = prng.pick(names.len());
        names.swap(a, b);
    }
    let take = 1 + prng.pick(names.len());
    for name in names.iter().take(take) {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        // Some hostile names are directories (`script`, `lib`, `bin`,
        // `..`); everything else becomes a file with random bytes.
        if matches!(*name, "script" | "lib" | "bin" | "..") {
            std::fs::create_dir_all(&path).ok();
        } else {
            let size = prng.pick(64_000);
            let bytes: Vec<u8> = (0..size).map(|_| prng.byte()).collect();
            std::fs::write(&path, bytes).ok();
        }
    }
    dir
}

/// The launcher on the stub side execs relative to the app rootfs; an entry
/// containing a `..` segment would escape any directory it is resolved from.
fn has_dotdot_segment(arg: &str) -> bool {
    arg.split(['/', '\\']).any(|seg| seg == "..")
}

/// detect_runtime and resolve_entrypoint never panic and never emit escaping
/// argv on any hostile tree, for every runtime branch.
#[test]
fn no_panic_and_no_escape_on_hostile_trees() {
    let runtimes = [
        Runtime::Python,
        Runtime::Deno,
        Runtime::Node,
        Runtime::Electron,
        Runtime::Java,
        Runtime::Ruby,
        Runtime::Dotnet,
        Runtime::Rust,
        Runtime::Go,
        Runtime::Php,
        Runtime::Perl,
        Runtime::Hugo,
        Runtime::Ollama,
        Runtime::Gemma,
        Runtime::Wasm,
        Runtime::Binary,
    ];
    for seed in 0..200 {
        let dir = build_hostile_tree(seed);
        let _ = detect_runtime(dir.path());
        for runtime in &runtimes {
            if let Some(argv) = resolve_entrypoint(dir.path(), *runtime) {
                for arg in &argv {
                    assert!(
                        !has_dotdot_segment(arg),
                        "entrypoint arg {arg:?} escapes the app (seed {seed}, \
                         runtime {runtime:?})"
                    );
                }
            }
        }
    }
}

/// A well-formed Mojolicious app keeps resolving to `perl /app/script/...`
/// even when the same directory also contains hostile entries.
#[test]
fn legitimate_mojolicious_still_resolves_with_hostile_neighbours() {
    let dir = TempDir::new().unwrap();
    for hostile in [
        "script/x\n",
        "script/application",
        "script/x;rm -rf /",
        "script/etc",
        "lib\\My\\App.pm",
    ] {
        if let Some(parent) = Path::new(&hostile).parent() {
            std::fs::create_dir_all(dir.path().join(parent)).ok();
        }
        std::fs::create_dir_all(dir.path().join(hostile)).ok();
    }
    std::fs::create_dir_all(dir.path().join("script")).unwrap();
    std::fs::create_dir_all(dir.path().join("lib")).unwrap();
    std::fs::write(
        dir.path().join("script/my_app"),
        "use MyApp;\nMyApp->start;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lib/MyApp.pm"),
        "package MyApp;\nuse Mojo::Base;\n",
    )
    .unwrap();

    assert_eq!(detect_runtime(dir.path()), Some(Runtime::Perl));
    assert_eq!(
        resolve_entrypoint(dir.path(), Runtime::Perl),
        Some(vec!["perl".into(), "/app/script/my_app".into()])
    );
}

/// Symlink loops (`script` → app root, `lib` → app root) neither hang nor
/// panic detection: all reads stay one level deep.
#[test]
fn symlink_loops_do_not_hang_or_panic() {
    #[cfg(unix)]
    {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("app.pl"), "use Mojo::Lite;\n").unwrap();
        std::fs::create_dir_all(dir.path().join("script")).unwrap();
        std::fs::create_dir_all(dir.path().join("lib")).unwrap();
        std::os::unix::fs::symlink(dir.path(), dir.path().join("script/loop")).unwrap();
        std::os::unix::fs::symlink(dir.path(), dir.path().join("lib/loop")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("script"), dir.path().join("script/sl"))
            .unwrap();
        let runtime = detect_runtime(dir.path());
        assert_eq!(runtime, Some(Runtime::Perl));
        if let Some(argv) = resolve_entrypoint(dir.path(), Runtime::Perl) {
            assert!(!has_dotdot_segment(&argv[1]));
        }
    }
}

/// An unreadable `script/` directory must never make detection panic; Perl
/// detection simply reports whatever survives.
#[test]
fn unreadable_script_dir_fails_closed() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("script")).unwrap();
        std::fs::write(dir.path().join("script/x"), "use Mojo::Base;\n").unwrap();
        std::fs::set_permissions(
            dir.path().join("script"),
            std::fs::Permissions::from_mode(0o000),
        )
        .ok();
        let _ = detect_runtime(dir.path());
        std::fs::set_permissions(
            dir.path().join("script"),
            std::fs::Permissions::from_mode(0o755),
        )
        .ok();
    }
}

/// A payload that seeds detection with traversal module names must never
/// resolve to a file outside `app/lib`, even when decoys exist there.
#[test]
fn traversal_modules_never_detect_perl() {
    let parent = TempDir::new().unwrap();
    for decoy in ["decoy.pm", "passwd.pm", "shadow.pm"] {
        std::fs::write(parent.path().join(decoy), "package decoy;\n").unwrap();
    }
    let app = parent.path().join("app");
    std::fs::create_dir_all(app.join("script")).unwrap();
    std::fs::create_dir_all(app.join("lib")).unwrap();
    let hostile_uses = [
        "../../decoy",
        "../../../decoy.pm",
        "..::..::decoy",
        "../../etc/passwd",
        "..\\..\\decoy",
    ];
    for (i, use_mod) in hostile_uses.iter().enumerate() {
        std::fs::write(
            app.join(format!("script/s{i}")),
            format!("use {use_mod};\n"),
        )
        .unwrap();
    }
    // None of the hostile script modules may pull in Perl via out-of-tree
    // files; the tree has no legitimate Perl marker.
    assert_ne!(detect_runtime(&app), Some(Runtime::Perl));
}
