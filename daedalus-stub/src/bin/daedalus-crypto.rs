#![allow(missing_docs)]
//! Standalone Ed25519 crypto tool for key generation, signing, and verification.
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use zeroize::Zeroizing;

/// `main` - main.
///
/// Description:
///
/// Return: nothing
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: daedalus-crypto <keygen|sign|verify> ...");
        std::process::exit(1);
    }
    let rc = match args[1].as_str() {
        "keygen" => cmd_keygen(&args[2..]),
        "sign" => cmd_sign(&args[2..]),
        "verify" => cmd_verify(&args[2..]),
        _ => {
            eprintln!("unknown subcommand: {}", args[1]);
            1
        }
    };
    std::process::exit(rc);
}

/// `cmd_keygen` - cmd keygen.
/// `@args`: command arguments
///
/// Description:
///
/// Return: the `i32`
fn cmd_keygen(args: &[String]) -> i32 {
    let key_dir = if args.len() == 2 && args[0] == "--key-dir" {
        PathBuf::from(&args[1])
    } else {
        eprintln!("Usage: daedalus-crypto keygen --key-dir <dir>");
        return 1;
    };

    let mut seed = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(&mut *seed);
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();

    let pubkey_bytes = verifying_key.to_bytes();
    let fingerprint = Sha256::digest(pubkey_bytes);
    let mut fp_hex = String::with_capacity(64);
    for b in fingerprint {
        let _ = write!(fp_hex, "{b:02x}");
    }

    fs::create_dir_all(&key_dir).unwrap_or_else(|e| {
        eprintln!("error creating key directory: {e}");
        std::process::exit(1);
    });

    let seed = Zeroizing::new(signing_key.to_bytes());
    let key_path = key_dir.join(format!("{fp_hex}.key"));
    fs::write(&key_path, *seed).unwrap_or_else(|e| {
        eprintln!("error writing key file: {e}");
        std::process::exit(1);
    });

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap_or_else(|e| {
            eprintln!("warning: could not set key permissions: {e}");
        });
    }

    fs::write(key_dir.join(format!("{fp_hex}.pub")), pubkey_bytes).unwrap_or_else(|e| {
        eprintln!("error writing pubkey file: {e}");
        std::process::exit(1);
    });

    println!("{fp_hex}");
    0
}

/// `cmd_sign` - cmd sign.
/// `@args`: command arguments
///
/// Description:
///
/// Return: the `i32`
fn cmd_sign(args: &[String]) -> i32 {
    if args.len() != 1 {
        eprintln!("Usage: daedalus-crypto sign <keyfile>");
        return 1;
    }
    let key_path = PathBuf::from(&args[0]);

    let seed = match fs::read(&key_path) {
        Ok(b) if b.len() == 32 => {
            let b = Zeroizing::new(b);
            let mut arr = Zeroizing::new([0u8; 32]);
            arr.copy_from_slice(&b);
            arr
        }
        Ok(_) => {
            eprintln!("error: key file must be exactly 32 bytes");
            return 1;
        }
        Err(e) => {
            eprintln!("error reading key file: {e}");
            return 1;
        }
    };

    let mut hash = [0u8; 32];
    if io::stdin().read_exact(&mut hash).is_err() {
        eprintln!("error: failed to read 32-byte hash from stdin");
        return 1;
    }

    let signing_key = SigningKey::from_bytes(&seed);
    let signature: Signature = signing_key.sign(&hash);

    let sig_bytes = signature.to_bytes();
    io::stdout().write_all(&sig_bytes).unwrap_or_else(|e| {
        eprintln!("error writing signature: {e}");
        std::process::exit(1);
    });
    0
}

/// `cmd_verify` - cmd verify.
/// `@args`: command arguments
///
/// Description:
///
/// Return: the `i32`
fn cmd_verify(args: &[String]) -> i32 {
    if args.len() != 1 {
        eprintln!("Usage: daedalus-crypto verify <pubkey>");
        return 2;
    }
    let pubkey_path = PathBuf::from(&args[0]);

    let pubkey_raw = match fs::read(&pubkey_path) {
        Ok(b) if b.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&b);
            arr
        }
        Ok(_) => {
            eprintln!("error: pubkey file must be exactly 32 bytes");
            return 2;
        }
        Err(e) => {
            eprintln!("error reading pubkey file: {e}");
            return 2;
        }
    };

    let mut buf = [0u8; 96];
    if io::stdin().read_exact(&mut buf).is_err() {
        eprintln!("error: failed to read 96 bytes from stdin ([32-byte hash][64-byte sig])");
        return 2;
    }

    let mut hash = [0u8; 32];
    hash.copy_from_slice(&buf[0..32]);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(&buf[32..96]);

    let pub_key = if let Ok(k) = VerifyingKey::from_bytes(&pubkey_raw) {
        k
    } else {
        eprintln!("error: invalid public key");
        return 2;
    };

    let sig = Signature::from_bytes(&sig_bytes);
    match pub_key.verify(&hash, &sig) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
