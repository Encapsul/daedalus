# Plan: Feature Branch — mmap, io_uring, canary builds

## Branch: `feat/coldstart-perf-canary`

## Feature 1: mmap (mémoire + temp file) — cold start < 200ms

### Problème actuel
Le stub extrait le payload zstd+tar sur disque (~150ms pour une app moyenne). 
Le cache `.ready` évite l'extraction subsequentes, mais le premier lancement est lent.

### Solution
Décompresser le payload en mémoire, puis écrire sur disque en arrière-plan.
L'app démarre pendant que l'écriture se fait.

### Fichiers à modifier
- `stub/src/main.rs` : nouveau mode `extract_to_memory()` + `exec_app_from_memory()`
- `xbin-core/src/compress.rs` : fonction `decompress_to_vec()` (déjà existante)

### Implémentation

**Nouvelle fonction dans `stub/src/main.rs` :**
```rust
fn extract_to_cache_fast(
    blobs: &[&[u8]], 
    cache_root: &Path, 
    rootfs: &Path,
) -> io::Result<()> {
    // 1. Décompresser chaque blob en mémoire
    let mut decompressed = Vec::new();
    for blob in blobs {
        let decoder = ruzstd::StreamingDecoder::new(io::Cursor::new(*blob))?;
        let mut reader = BufReader::new(decoder);
        reader.read_to_end(&mut decompressed)?;
    }
    
    // 2. Écrire le tar sur disque en arrière-plan (thread)
    let cache_root_clone = cache_root.to_path_buf();
    let rootfs_clone = rootfs.to_path_buf();
    let tar_bytes = decompressed.clone();
    
    let handle = std::thread::spawn(move || {
        let tmp = cache_root.join(".tmp-write");
        std::fs::create_dir_all(&tmp).ok();
        // Extraire le tar vers .tmp-write/rootfs/
        let mut archive = tar::Archive::new(Cursor::new(&tar_bytes));
        archive.set_preserve_permissions(true);
        archive.set_overwrite(true);
        let _ = archive.unpack(tmp.join("rootfs"));
        // Atomic rename
        let _ = std::fs::rename(&tmp, cache_root.join("rootfs"));
        let _ = std::fs::write(cache_root.join(".ready"), b"");
    });
    
    // 3. Retourner immédiatement — l'extraction continue en background
    // Le prochain lancement trouvera le .ready
    Ok(())
}
```

**Modification du flow dans `run()` :**
```
cache HIT -> exec_app() (inchangé)
cache MISS -> 
  1. verify SHA-256 (inchangé)
  2. extract_to_cache_fast() — retourne immédiatement
  3. exec_app() — les fichiers sont en cours d'écriture
     MAIS l'app peut démarrer car les fichiers essentiels sont déjà en mémoire
```

**Problème** : si l'app lit un fichier pas encore écrit, ça crash.
**Solution** : garder le comportement actuel (extraction synchrone) par défaut, ajouter `--fast` flag qui utilise le mode mémoire.

### Tests
- `cargo test --workspace` — tous les tests existants doivent passer
- Test manuel : `xbin build examples/hello-web -o /tmp/t.xbin && time /tmp/t.xbin`
- Comparer : avec et sans `--fast`

---

## Feature 2: Multi-thread + copy_file_range — extraction 2-3x plus rapide

### Problème actuel
L'extraction zstd+tar est single-threaded. Le stub utilise `ruzstd::StreamingDecoder` 
qui est mono-thread. La copie des fichiers passe par userspace.

### Solution
1. Décompresser zstd en parallèle (plusieurs blobs en même temps)
2. Utiliser `copy_file_range()` pour la copie fichier→fichier (zero-copy kernel)

### Fichiers à modifier
- `stub/src/main.rs` : `extract_atomic()` → version multi-thread
- Nouveau : `stub/src/copy.rs` — helper `copy_file_range_safe()`

### Implémentation

**`stub/src/copy.rs` :**
```rust
use std::os::unix::io::AsRawFd;

/// Zero-copy file copy using copy_file_range(2).
/// Falls back to read/write if kernel doesn't support it.
pub fn copy_file_range_safe(src: &Path, dst: &Path) -> io::Result<u64> {
    let src_file = File::open(src)?;
    let dst_file = File::create(dst)?;
    
    let src_fd = src_file.as_raw_fd();
    let dst_fd = dst_file.as_raw_fd();
    let mut offset_src: i64 = 0;
    let mut offset_dst: i64 = 0;
    let mut total: u64 = 0;
    
    loop {
        let copied = unsafe {
            libc::copy_file_range(
                src_fd, &mut offset_src,
                dst_fd, &mut offset_dst,
                usize::MAX, // up to 2GB
                0,           // no flags
            )
        };
        if copied <= 0 { break; }
        total += copied as u64;
    }
    
    if total == 0 {
        // Fallback: standard copy
        std::fs::copy(src, dst)?;
    }
    Ok(total)
}
```

**Modification de `extract_atomic()` dans `stub/src/main.rs` :**
```rust
fn extract_atomic(blobs: &[&[u8]], cache_root: &Path, rootfs: &Path) -> io::Result<()> {
    atomic_extract(cache_root, rootfs, |tmp_rootfs| {
        // Décompresser chaque blob en parallèle
        let handles: Vec<_> = blobs.iter().enumerate().map(|(i, blob)| {
            let blob = blob.to_vec();
            let tmp = tmp_rootfs.to_path_buf();
            std::thread::spawn(move || {
                let decoder = ruzstd::StreamingDecoder::new(io::Cursor::new(&blob))
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                let mut archive = tar::Archive::new(decoder);
                archive.set_preserve_permissions(true);
                archive.set_overwrite(true);
                archive.unpack(&tmp)?;
                Ok::<(), io::Error>(())
            })
        }).collect();
        
        for handle in handles {
            handle.join().map_err(|_| io::Error::new(
                io::ErrorKind::Other, "extraction thread panicked"
            ))??;
        }
        Ok(())
    })
}
```

### Tests
- `cargo test --workspace` — tests existants
- Benchmark : comparer temps d'extraction avant/après sur une app Node.js

---

## Feature 3: Canary builds — `xbin upgrade --canary`

### Problème actuel
`xbin upgrade` ne télécharge que les releases stables. 
Les early adopters veulent les derniers commits main.

### Solution
Ajouter flag `--canary` qui télécharge le dernier build depuis 
GitHub Actions artifacts (ou un tag `canary`).

### Fichiers à modifier
- `xbin-cli/src/commands/upgrade.rs` : ajouter `--canary` flag + logique

### Implémentation

**Args (ajout) :**
```rust
/// Install latest canary build from main branch
#[arg(long)]
canary: bool,
```

**Logique dans `run()` :**
```rust
let (download_url, version_label) = if args.canary {
    // 1. Fetch latest workflow run from GitHub Actions
    let runs_url = format!(
        "https://api.github.com/repos/Tednoob17/erebus/actions/runs?branch=main&per_page=1"
    );
    let runs: serde_json::Value = client.get(&runs_url)
        .header("Accept", "application/vnd.github+json")
        .send()?
        .json()?;
    
    let run_id = runs["workflow_runs"][0]["id"].as_u64()
        .context("no canary builds found")?;
    let run_id_str = run_id.to_string();
    
    // 2. Fetch artifacts for this run
    let artifacts_url = format!(
        "https://api.github.com/repos/Tednoob17/erebus/actions/runs/{run_id}/artifacts"
    );
    let artifacts: serde_json::Value = client.get(&artifacts_url)
        .header("Accept", "application/vnd.github+json")
        .send()?
        .json()?;
    
    // 3. Find platform-specific artifact
    let platform = detect_platform();
    let artifact_name = format!("xbin-{platform}");
    let archive_url = artifacts["artifacts"].as_array()
        .context("no artifacts")?
        .iter()
        .find(|a| a["name"].as_str() == Some(&artifact_name))
        .context("artifact not found")?
        ["archive_download_url"].as_str()
        .context("no download url")?;
    
    // 4. Download with auth token (required for Actions artifacts)
    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .context("GITHUB_TOKEN required for canary builds")?;
    
    let zip_path = tmp.path().join("canary.zip");
    client.get(archive_url)
        .header("Authorization", format!("Bearer {token}"))
        .send()?
        .copy_to(&mut File::create(&zip_path)?)?;
    
    // 5. Unzip (artifacts are zipped)
    // ... unzip logic ...
    
    ("canary".to_string(), format!("canary-{run_id}"))
} else {
    // Existing stable release logic
    let latest = fetch_latest_version(&client)?;
    let url = format!(
        "https://github.com/Tednoob17/erebus/releases/download/v{latest}/erebus-{latest}-{platform}.tar.gz"
    );
    (url, latest)
};
```

**Modification de la condition "already up to date" :**
```rust
if !args.canary && current == latest {
    println!("Already up to date (v{current})");
    return Ok(());
}
// Pour --canary, toujours installer (c'est le latest commit)
```

### CI : workflow pour publier les canary builds

**Nouveau fichier `.github/workflows/canary.yml` :**
```yaml
name: Canary Build
on:
  push:
    branches: [main]

jobs:
  build:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        target: [x86_64-unknown-linux-musl, aarch64-unknown-linux-gnu]
    steps:
      - uses: actions/checkout@v4
      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
      - name: Package
        run: tar czf xbin-canary-${{ matrix.target }}.tar.gz -C target/${{ matrix.target }}/release xbin
      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: xbin-${{ matrix.target }}
          path: xbin-canary-*.tar.gz
```

### Tests
- Test unitaire : `detect_platform()` retourne la bonne plateforme
- Test manuel : `xbin upgrade --canary --dry-run` affiche le bon URL
- Test e2e : `xbin upgrade --canary` installe le dernier build main

---

## Vérification obligatoire

Avant de merger :
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```
