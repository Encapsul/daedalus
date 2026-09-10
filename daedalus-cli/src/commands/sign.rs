use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::format::{Footer, FLAG_SIGNED, SIG_BLOCK_SIZE, SIG_LEN};
use daedalus_core::paths::{default_key_dir, trusted_keys_dir};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

#[derive(Args)]
#[allow(clippy::struct_excessive_bools)]
pub struct SignArgs {
    /// Path to the .daedalus file, or `-` to read stdin and write signed bytes to stdout
    pub file: PathBuf,

    /// Path to the signing key file
    #[arg(short, long)]
    pub key: Option<PathBuf>,

    /// Quiet output
    #[arg(short, long)]
    pub quiet: bool,

    /// Skip confirmation prompt
    #[arg(short, long)]
    pub force: bool,

    /// Disable all interactive prompts (for CI/scripts)
    #[arg(long, global = true)]
    pub no_input: bool,

    /// Output result as JSON
    #[arg(long)]
    pub json: bool,
}

/// run - run.
/// @args: command arguments
///
/// Description:
///
/// Return: Result containing Result<()>
pub fn run(args: SignArgs) -> Result<()> {
    let key_path = match args.key {
        Some(p) => p,
        None => {
            let dir = default_key_dir();
            let keys: Vec<_> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "key"))
                .collect();
            if keys.len() == 1 {
                keys[0].path()
            } else {
                anyhow::bail!("specify key with --key");
            }
        }
    };

    if !args.force && !args.quiet && !args.no_input {
        if !is_interactive() {
            anyhow::bail!(
                "interactive prompt required; pass --force or --no-input for non-interactive use"
            );
        }
        eprint!("Sign {}? [y/N] ", args.file.display());
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted");
            return Ok(());
        }
    } else if args.no_input && !args.force && !args.quiet {
        anyhow::bail!(
            "--no-input passed but operation requires confirmation; pass --force to skip"
        );
    }

    let key_bytes = std::fs::read(&key_path)
        .with_context(|| format!("failed to read signing key at {}", key_path.display()))?;

    if crate::stdio::is_dash(&args.file.to_string_lossy()) {
        // `sign -` reads the binary from stdin, signs it in memory, and writes
        // the signed binary back out on stdout.
        let original = crate::stdio::read_stdin_cursor()?;
        let signed = sign_bytes(original.get_ref(), &key_bytes)?;
        std::io::stdout().write_all(&signed)?;
        if !args.quiet && !args.json {
            eprintln!("Signed stdin -> stdout");
        }
    } else {
        sign_file(&args.file, &key_bytes, args.quiet)?;
    }

    if args.json {
        let info = serde_json::json!({
            "file": args.file.display().to_string(),
            "signed": true,
            "key": key_path.display().to_string(),
        });
        println!("{}", serde_json::to_string_pretty(&info)?);
    }

    Ok(())
}

/// is_interactive - check whether interactive.
///
/// Description:
///
/// Return: true or false
fn is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// Sign a `.daedalus` binary held in memory, returning the signed bytes.
/// Shared by `sign -` (stdin→stdout) and the file-backed `sign_file`.
pub fn sign_bytes(original: &[u8], key_bytes: &[u8]) -> Result<Vec<u8>> {
    if key_bytes.len() != 32 {
        anyhow::bail!("key must be 32 bytes, got {}", key_bytes.len());
    }

    let mut key_arr = Zeroizing::new([0u8; 32]);
    key_arr.copy_from_slice(key_bytes);
    let signing_key = SigningKey::from_bytes(&key_arr);

    let mut cursor = std::io::Cursor::new(original);
    let mut footer = Footer::read_from(&mut cursor).context("failed to read daedalus footer")?;

    if footer.is_signed() {
        anyhow::bail!("file is already signed");
    }
    if footer.has_sisr() {
        // `--enable-sisr` already signs the delta manifest with `--key`;
        // inserting a binary sig block here would rebuild the file as
        // `[..meta_end][sig][footer]` and truncate the SISR section.
        anyhow::bail!(
            "cannot sign a SISR binary: the delta manifest is already signed; \
             rebuild without `--enable-sisr` to sign the whole binary"
        );
    }

    let meta_start = footer.meta_offset as usize;
    let meta_end = meta_start + footer.meta_size as usize;
    let payload_start = footer.payload_offset as usize;
    let payload_end = payload_start + footer.payload_csize as usize;

    let payload = original[payload_start..payload_end].to_vec();
    let meta = original[meta_start..meta_end].to_vec();

    // Mutate the footer to its final on-disk form FIRST: the digest covers
    // the footer itself (via `pack_full`, incl. the sig_offset prefix),
    // because the footer's format_version and FLAG_SIGNED decide whether the
    // signature is ever consulted. A signature over payload‖meta alone would
    // let an attacker downgrade the file to v2 and strip the flag — the
    // signature would be silently skipped.
    let new_sig_offset = footer.meta_offset + footer.meta_size;
    footer.sig_offset = new_sig_offset;
    footer.flags |= FLAG_SIGNED;
    if footer.format_version < 3 {
        footer.format_version = 3;
    }

    let mut hasher = Sha256::new();
    hasher.update(&payload);
    hasher.update(&meta);
    hasher.update(footer.pack_full());
    let hash = hasher.finalize();

    let signature = signing_key.sign(&hash);

    let mut sig_block = Vec::with_capacity(SIG_BLOCK_SIZE);
    sig_block.extend_from_slice(&(SIG_LEN as u32).to_le_bytes());
    sig_block.extend_from_slice(&signature.to_bytes());

    let mut new_content = Vec::with_capacity(meta_end + SIG_BLOCK_SIZE + footer.pack_full().len());
    new_content.extend_from_slice(&original[0..meta_end]);
    new_content.extend_from_slice(&sig_block);
    new_content.extend_from_slice(&footer.pack_full());
    Ok(new_content)
}

/// Sign a `.daedalus` file in-place with the given key. Used by both `daedalus sign`
/// and `daedalus build --key`.
///
/// Write is atomic: a temp file is created in the same directory and
/// renamed over the source only after the new content is fully flushed.
pub fn sign_file(file: &PathBuf, key_bytes: &[u8], quiet: bool) -> Result<()> {
    let original =
        std::fs::read(file).with_context(|| format!("failed to read {}", file.display()))?;
    let new_content = sign_bytes(&original, key_bytes)?;

    let tmp_path = file.with_extension("daedalus.tmp");
    std::fs::write(&tmp_path, &new_content)
        .with_context(|| format!("failed to write temp file {}", tmp_path.display()))?;
    // `fs::write` creates the temp file with default perms; restore the
    // executable bit that `assemble` set, or signing would produce a binary
    // that the shell refuses to run.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::rename(&tmp_path, file)
        .with_context(|| format!("failed to rename temp file to {}", file.display()))?;

    if !quiet {
        eprintln!("Signed {}", file.display());
    }

    Ok(())
}

/// Locate (or create) the development keypair used to sign builds *by default*,
/// and self-trust its public key so `daedalus verify` passes out of the box.
///
/// Returns the private-key path, creating both files if they do not yet exist:
///
/// - `<keys>/<fingerprint>.key` — 32-byte private key, mode 0600
/// - `<keys>/<fingerprint>.pub` — 32-byte public key
///
/// and copies the public key into `trusted_keys_dir()` so the stub/`verify`
/// trust anchor knows it. Generating a fresh key per machine keeps the signing
/// key out of the binary and out of VCS — the ROADMAP's "sign by default with
/// the dev key".
pub fn ensure_dev_key() -> Result<PathBuf> {
    ensure_dev_key_in(&default_key_dir(), &trusted_keys_dir())
}

/// Testable core of [`ensure_dev_key`]: operates on explicit directories.
fn ensure_dev_key_in(key_dir: &Path, trust_dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(key_dir)
        .with_context(|| format!("failed to create key directory {}", key_dir.display()))?;

    // Reuse an existing dev key if one is already present.
    if let Some(path) = existing_dev_key(key_dir) {
        return Ok(path);
    }

    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let verifying_key = signing_key.verifying_key();
    let fingerprint = hex::encode(verifying_key.as_bytes());
    let key_path = key_dir.join(format!("{fingerprint}.key"));
    let pub_path = key_dir.join(format!("{fingerprint}.pub"));

    let key_bytes = Zeroizing::new(signing_key.to_bytes());
    std::fs::write(&key_path, *key_bytes)
        .with_context(|| format!("failed to write dev key to {}", key_path.display()))?;
    std::fs::write(&pub_path, verifying_key.as_bytes())
        .with_context(|| format!("failed to write dev public key to {}", pub_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
    }

    trust_pubkey(&pub_path, trust_dir)?;
    Ok(key_path)
}

/// Whether a dev key already exists in `key_dir`; returns its path.
fn existing_dev_key(key_dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(key_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "key"))
}

/// Self-trust a public key: copy it into `trust_dir` under the same
/// filename the `trust` command / stub launcher expect. Idempotent.
fn trust_pubkey(pub_path: &Path, trust_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(trust_dir)
        .with_context(|| format!("failed to create trusted keys dir {}", trust_dir.display()))?;
    let dest = trust_dir.join(
        pub_path
            .file_name()
            .context("dev public key path has no filename")?,
    );
    if !dest.exists() {
        std::fs::copy(pub_path, &dest)
            .with_context(|| format!("failed to trust dev key at {}", dest.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_dev_key_creates_and_trusts_keypair() {
        let key_dir = tempfile::tempdir().unwrap();
        let trust_dir = tempfile::tempdir().unwrap();

        let key_path = ensure_dev_key_in(key_dir.path(), trust_dir.path()).unwrap();

        // The private key must exist, be a 32-byte Ed25519 seed, and be 0600.
        assert!(key_path.is_file());
        let seed = std::fs::read(&key_path).unwrap();
        assert_eq!(seed.len(), 32);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "dev key must be mode 0600");
        }

        // A matching .pub must sit next to it.
        let pub_path = key_dir
            .path()
            .join(key_path.file_stem().unwrap().to_string_lossy().to_string() + ".pub");
        assert!(pub_path.is_file());

        // The pub key must be self-trusted in the trust dir (verify works OOTB).
        let trusted = std::fs::read_dir(trust_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .count();
        assert_eq!(trusted, 1, "dev public key must be trusted automatically");
        assert_eq!(
            std::fs::read(&pub_path).unwrap(),
            std::fs::read(trust_dir.path().join(pub_path.file_name().unwrap())).unwrap(),
            "trusted copy must be identical to the generated public key"
        );
    }

    #[test]
    fn ensure_dev_key_reuses_existing_key() {
        let key_dir = tempfile::tempdir().unwrap();
        let first = ensure_dev_key_in(key_dir.path(), tempfile::tempdir().unwrap().path()).unwrap();
        let second =
            ensure_dev_key_in(key_dir.path(), tempfile::tempdir().unwrap().path()).unwrap();
        assert_eq!(first, second, "dev key must be stable across invocations");
    }

    #[test]
    fn trust_pubkey_is_idempotent() {
        let key_dir = tempfile::tempdir().unwrap();
        let trust_dir = tempfile::tempdir().unwrap();
        let key_path = ensure_dev_key_in(key_dir.path(), trust_dir.path()).unwrap();
        let pub_path = key_dir
            .path()
            .join(key_path.file_stem().unwrap().to_string_lossy().to_string() + ".pub");

        // Trusting again must not error or duplicate.
        trust_pubkey(&pub_path, trust_dir.path()).unwrap();
        let count = std::fs::read_dir(trust_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .count();
        assert_eq!(count, 1, "trust_pubkey must be idempotent");
    }
}
