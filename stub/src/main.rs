//! xbin launcher stub.
//!
//! Embarqué en tête du fichier .xbin (c'est l'ELF que le kernel exécute).
//! Flux : se lit via /proc/self/exe → lit le footer → vérifie l'intégrité →
//! extrait le rootfs dans ~/.cache/xbin/{sha256}/ (atomique) → exec l'app.
//!
//! Isolation niveau 0 (MVP) : LD_LIBRARY_PATH, pas de chroot. Les niveaux 1/2
//! (chroot, user namespaces) arrivent en Phase 2 — voir docs/ROADMAP.md.

mod format;

use format::Footer;
use serde::Deserialize;
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::exit;

#[derive(Deserialize)]
struct Metadata {
    name: String,
    #[serde(default)]
    runtime: String,
    entrypoint: Vec<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    cwd: Option<String>,
    /// Couches du payload (format v2). Vide en v1 (payload monolithique).
    #[serde(default)]
    layers: Vec<Layer>,
}

/// Une couche du payload v2 : un blob zstd(tar) indépendant, empilé sur les
/// précédents à l'extraction (les couches suivantes écrasent les précédentes).
#[derive(Deserialize)]
struct Layer {
    #[serde(default)]
    kind: String,
    offset: u64,
    csize: u64,
    #[allow(dead_code)]
    usize: u64,
    /// SHA-256 (hex) du blob compressé — sert de clé de cache stable par couche.
    sha256: String,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("[xbin] error: {e}");
        exit(1);
    }
}

fn run() -> io::Result<()> {
    let verbose = std::env::var_os("XBIN_VERBOSE").is_some();

    // 1. Se localiser de manière fiable (pas argv[0], contrôlable par l'appelant).
    let mut exe = File::open("/proc/self/exe")?;
    let footer = Footer::read_from(&mut exe)?;

    // 2. Lire les métadonnées JSON.
    let meta_bytes = read_at(&mut exe, footer.meta_offset, footer.meta_size as usize)?;
    let meta: Metadata = serde_json::from_slice(&meta_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("bad metadata: {e}")))?;

    // 3. Lire la région du payload (toutes les couches, contiguës en v2).
    let payload = read_at(&mut exe, footer.payload_offset, footer.payload_csize as usize)?;

    // 4. Vérifier l'intégrité.
    //    v1 : SHA-256(payload).        v2 : SHA-256(couches || metadata).
    let layered = footer.format_version >= 2 && !meta.layers.is_empty();
    if layered {
        let mut buf = payload.clone();
        buf.extend_from_slice(&meta_bytes);
        verify_sha256(&buf, &footer.payload_sha256)?;
    } else {
        verify_sha256(&payload, &footer.payload_sha256)?;
    }

    // 5. Clé de cache.
    //    v1 : SHA-256 du payload.  v2 : SHA-256 de la concaténation des hash de
    //    couches (stable tant que le contenu des couches ne change pas — donc un
    //    rebuild qui ne touche que la couche app garde la couche runtime cachée).
    let hash = if layered { cache_key_v2(&meta.layers) } else { footer.sha256_hex() };

    let base = cache_dir()?;
    fs::create_dir_all(&base)?;
    let cache_root = base.join(&hash);
    let rootfs = cache_root.join("rootfs");
    let ready_marker = cache_root.join(".ready");

    if !ready_marker.exists() {
        // Sérialise les instances concurrentes : une seule extrait, les autres
        // attendent le verrou puis trouvent le cache déjà prêt. (L'extraction
        // reste atomique via rename(), donc correcte même sans ce verrou ; le
        // flock évite juste le travail dupliqué.)
        let lock = File::create(base.join(format!("{hash}.lock")))?;
        flock_exclusive(&lock)?;

        if !ready_marker.exists() {
            if verbose {
                eprintln!("[xbin] cold start: extracting {}", meta.name);
            }
            // Découpe la région en couches (v2) ou en un seul blob (v1).
            let blobs = slice_layers(&payload, footer.payload_offset, &meta, layered);
            extract_atomic(&blobs, &cache_root, &rootfs)?;
        }
        // verrou relâché à la fermeture de `lock` (fin de scope).
    } else if verbose {
        eprintln!("[xbin] warm start: cache hit {}", hash);
    }

    // 6. Construire argv + env et exec dans le rootfs.
    exec_app(&meta, &rootfs)
}

/// Clé de cache v2 : SHA-256 de la concaténation des hash hex de chaque couche.
fn cache_key_v2(layers: &[Layer]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for l in layers {
        h.update(l.sha256.as_bytes());
    }
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// Découpe la région payload en blobs compressés, dans l'ordre d'empilement.
/// En v1, retourne le payload entier comme unique blob.
fn slice_layers<'a>(
    payload: &'a [u8],
    region_offset: u64,
    meta: &Metadata,
    layered: bool,
) -> Vec<&'a [u8]> {
    if !layered {
        return vec![payload];
    }
    meta.layers
        .iter()
        .map(|l| {
            let start = (l.offset - region_offset) as usize;
            let end = start + l.csize as usize;
            &payload[start..end]
        })
        .collect()
}

/// Lit `len` bytes à l'offset absolu `off`.
fn read_at(f: &mut File, off: u64, len: usize) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(off))?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn verify_sha256(data: &[u8], expected: &[u8; 32]) -> io::Result<()> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    let got = h.finalize();
    if got.as_slice() != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "payload integrity check failed (SHA-256 mismatch)",
        ));
    }
    Ok(())
}

fn cache_dir() -> io::Result<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(d).join("xbin"));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME not set"))?;
    Ok(PathBuf::from(home).join(".cache").join("xbin"))
}

/// Décompresse une ou plusieurs couches zstd(tar) dans un répertoire temporaire
/// unique (empilées dans l'ordre), puis rename() atomique vers le cache final.
/// Évite les états intermédiaires (TOCTOU).
fn extract_atomic(blobs: &[&[u8]], cache_root: &Path, rootfs: &Path) -> io::Result<()> {
    let parent = cache_root.parent().unwrap_or(Path::new("/tmp"));
    fs::create_dir_all(parent)?;

    // Répertoire temp unique (pid + nanos) dans le même filesystem que la cible
    // (obligatoire pour que rename() soit atomique).
    let tmp = parent.join(format!(".tmp-{}-{}", std::process::id(), nanos()));
    let tmp_rootfs = tmp.join("rootfs");
    fs::create_dir_all(&tmp_rootfs)?;

    // Chaque couche : zstd → tar → unpack par-dessus la précédente.
    for blob in blobs {
        let decoder = ruzstd::StreamingDecoder::new(io::Cursor::new(*blob))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("zstd: {e}")))?;
        let mut archive = tar::Archive::new(decoder);
        archive.set_preserve_permissions(true);
        archive.set_overwrite(true);
        archive.unpack(&tmp_rootfs)?;
    }

    // Marqueur de complétude.
    File::create(tmp.join(".ready"))?.write_all(b"1")?;

    // rename() atomique. Si un autre process a gagné la course, on jette notre tmp.
    match fs::rename(&tmp, cache_root) {
        Ok(()) => Ok(()),
        Err(_) if rootfs.exists() => {
            let _ = fs::remove_dir_all(&tmp);
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

/// Remplace le processus courant par l'app embarquée.
fn exec_app(meta: &Metadata, rootfs: &Path) -> io::Result<()> {
    if meta.entrypoint.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "empty entrypoint"));
    }

    // Les chemins absolus de l'entrypoint sont relatifs au rootfs.
    let resolve = |p: &str| -> PathBuf {
        if let Some(stripped) = p.strip_prefix('/') {
            rootfs.join(stripped)
        } else {
            PathBuf::from(p)
        }
    };

    let prog = resolve(&meta.entrypoint[0]);
    let prog_c = cstr(prog.as_os_str().as_bytes());

    // argv : argv[0] = chemin du programme, puis le reste de l'entrypoint,
    // puis les arguments passés par l'utilisateur sur la ligne de commande.
    let mut argv: Vec<CString> = Vec::new();
    argv.push(prog_c.clone());
    for a in &meta.entrypoint[1..] {
        argv.push(cstr(resolve(a).as_os_str().as_bytes()));
    }
    for a in std::env::args_os().skip(1) {
        argv.push(cstr(a.as_bytes()));
    }

    // env : on hérite de l'environnement courant, on injecte LD_LIBRARY_PATH
    // vers les libs du rootfs, puis on applique l'env du manifest.
    let mut env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let lib_dirs = [
        rootfs.join("lib"),
        rootfs.join("lib64"),
        rootfs.join("usr/lib"),
        rootfs.join("usr/lib64"),
        rootfs.join("usr/lib/x86_64-linux-gnu"),
    ];
    let mut ld = lib_dirs
        .iter()
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(":");
    if let Some(existing) = env.get("LD_LIBRARY_PATH") {
        if !existing.is_empty() {
            ld.push(':');
            ld.push_str(existing);
        }
    }
    env.insert("LD_LIBRARY_PATH".into(), ld);

    // Applique l'env du manifest. Le token ${ROOTFS} est remplacé par le chemin
    // réel du rootfs dans le cache — connu seulement à l'exécution. C'est ainsi
    // que le builder déclare des chemins (ex: PYTHONPATH=${ROOTFS}/app/site-packages)
    // sans connaître à l'avance où le cache sera matérialisé.
    let rootfs_str = rootfs.to_string_lossy();
    for (k, v) in &meta.env {
        env.insert(k.clone(), v.replace("${ROOTFS}", &rootfs_str));
    }
    let env_c: Vec<CString> = env
        .iter()
        .map(|(k, v)| cstr(format!("{k}={v}").as_bytes()))
        .collect();

    // cwd
    if let Some(cwd) = &meta.cwd {
        let dir = resolve(cwd);
        std::env::set_current_dir(&dir).ok();
    }

    // execve : remplace le process. Si ça réussit, on ne revient jamais.
    let argv_ptrs = to_ptr_vec(&argv);
    let env_ptrs = to_ptr_vec(&env_c);
    unsafe {
        libc_execve(prog_c.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
    }
    // Si on est là, execve a échoué.
    Err(io::Error::last_os_error())
}

fn cstr(bytes: &[u8]) -> CString {
    CString::new(bytes).unwrap_or_else(|_| CString::new("").unwrap())
}

fn to_ptr_vec(v: &[CString]) -> Vec<*const i8> {
    let mut ptrs: Vec<*const i8> = v.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(std::ptr::null());
    ptrs
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Verrou exclusif (advisory) sur un fichier via flock(2). Bloque jusqu'à
/// obtention. Relâché automatiquement à la fermeture du descripteur.
fn flock_exclusive(f: &File) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;
    const LOCK_EX: i32 = 2;
    let rc = unsafe { libc_flock(f.as_raw_fd(), LOCK_EX) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

// On évite la crate `nix` pour garder le stub minimal : juste les externs utiles.
extern "C" {
    #[link_name = "execve"]
    fn libc_execve(path: *const i8, argv: *const *const i8, envp: *const *const i8) -> i32;
    #[link_name = "flock"]
    fn libc_flock(fd: i32, operation: i32) -> i32;
}
