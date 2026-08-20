use anyhow::Result;
use std::path::Path;

/// Re-sign a macOS Mach-O binary using `codesign` after assembly.
///
/// Appending payload + metadata invalidates any existing code signature,
/// so we must re-sign the final `.erebus` (Mach-O stub + appended data).
///
/// On non-macOS hosts this is a no-op. On macOS without a signing identity
/// the binary is left unsigned with a warning.
pub(crate) fn sign_macos_binary(path: &Path, verbose: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use anyhow::Context;
        use std::process::Command;

        // Ad-hoc signing is sufficient for local development; distribution
        // requires a Developer ID in the user's keychain.
        let identity = match std::env::var("ERE_CODESIGN_IDENTITY") {
            Ok(id) if !id.is_empty() => id,
            _ => "-".to_string(),
        };

        let output = Command::new("codesign")
            .args(["--sign", &identity, "--force", "--timestamp"])
            .arg(path)
            .output()
            .context("failed to run codesign — is it installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("codesign failed: {stderr}");
        }

        if verbose {
            eprintln!("  macOS: re-signed Mach-O with identity '{identity}'");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, verbose);
    }

    Ok(())
}

/// Warns when a private key file is group/other-readable (not 0600) on Unix.
pub(crate) fn warn_if_insecure_key_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o600 {
                eprintln!(
                    "[erebus] warning: private key {} has mode {mode:o}, expected 0600",
                    path.display()
                );
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}
