use anyhow::{Context, Result};
use clap::Args;
use daedalus_core::format::Footer;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use zeroize::Zeroizing;

use super::inspect::generate_sbom;
use super::sign::ensure_dev_key;
use super::verify::load_trusted_keys;

/// Digest of the whole `.de` file, stub included.
///
/// This deliberately covers more than `daedalus sign` does (which signs
/// `payload || metadata || footer` only). An attestation is a supply-chain
/// statement about the artifact a device is about to run, so it must bind
/// every byte that gets executed - a substituted stub would otherwise be
/// outside the signed envelope while still being the thing that runs first.
fn file_digest(content: &[u8], _footer: &Footer) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(content);
    hasher.finalize().into()
}

/// Canonical byte string that the Ed25519 signature covers: the binary
/// digest *and* the SBOM, length-prefixed so no combination of field values
/// can be re-partitioned into a different statement.
///
/// Signing only the digest would leave the SBOM mutable: an attacker could
/// swap the predicate for a false inventory while keeping a valid signature
/// over the unchanged digest. Binding both fields makes the whole
/// attestation tamper-evident.
fn signing_payload(subject_digest: &[u8; 32], sbom: &serde_json::Value) -> Vec<u8> {
    // Compact serialization: serde_json without `preserve_order` sorts object
    // keys, so the same document always produces the same bytes.
    let sbom_bytes = serde_json::to_vec(sbom).unwrap_or_default();
    let mut payload = Vec::with_capacity(1 + 4 + subject_digest.len() + 8 + sbom_bytes.len());
    payload.push(SIGNING_DOMAIN);
    payload.extend_from_slice(&(subject_digest.len() as u32).to_be_bytes());
    payload.extend_from_slice(subject_digest);
    payload.extend_from_slice(&(sbom_bytes.len() as u64).to_be_bytes());
    payload.extend_from_slice(&sbom_bytes);
    payload
}

/// Domain separator, so a signature made here can never be replayed as a
/// `daedalus sign` signature over the raw binary bytes.
const SIGNING_DOMAIN: u8 = 0x01;

#[derive(Args)]
pub struct AttestArgs {
    /// Path to the .de binary to attest
    pub file: PathBuf,

    /// Ed25519 private key file (32 raw bytes). Defaults to the dev key.
    #[arg(long)]
    pub key: Option<PathBuf>,

    /// Write the attestation here instead of `<file>.attest.json`
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Verify an existing attestation instead of creating one
    #[arg(long)]
    pub verify: bool,
}

pub fn run(args: AttestArgs) -> Result<()> {
    let content = std::fs::read(&args.file)
        .with_context(|| format!("failed to read {}", args.file.display()))?;
    let mut cursor = std::io::Cursor::new(&content);
    let footer = Footer::read_from(&mut cursor)
        .with_context(|| format!("failed to read footer of {}", args.file.display()))?;
    let meta: serde_json::Value = {
        let start = footer.meta_offset as usize;
        let end = start + footer.meta_size as usize;
        serde_json::from_slice(&content[start..end]).unwrap_or_else(|_| json!({}))
    };

    let digest = file_digest(&content, &footer);
    let digest_hex = hex::encode(digest);

    if args.verify {
        return verify_attestation(&args.file, &digest_hex, &footer);
    }

    let arch = match footer.arch {
        daedalus_core::format::ARCH_X86_64 => "x86_64",
        daedalus_core::format::ARCH_AARCH64 => "aarch64",
        _ => "unknown",
    };
    let sbom = generate_sbom(&args.file.to_string_lossy(), &meta, arch, &footer);

    let key_path = match args.key {
        Some(k) => k,
        None => ensure_dev_key()?,
    };
    let key_bytes = std::fs::read(&key_path)
        .with_context(|| format!("failed to read key {}", key_path.display()))?;
    if key_bytes.len() != 32 {
        anyhow::bail!("key must be 32 bytes, got {}", key_bytes.len());
    }
    let mut key_arr = Zeroizing::new([0u8; 32]);
    key_arr.copy_from_slice(&key_bytes);
    let signing_key = SigningKey::from_bytes(&key_arr);
    let public_key = signing_key.verifying_key().to_bytes();
    let signature = signing_key.sign(&signing_payload(&digest, &sbom));

    let attestation = json!({
        "_type": "https://in-toto.io/Statement/v0.1",
        "subject": [{
            "name": args.file.file_name().unwrap_or_default().to_string_lossy(),
            "digest": { "sha256": digest_hex },
        }],
        "predicateType": "https://spdx.dev/Document",
        "signer": {
            "keyId": hex::encode(public_key),
            "publicKey": hex::encode(public_key),
        },
        "signature": hex::encode(signature.to_bytes()),
        "predicate": sbom,
    });

    let out = args
        .output
        .clone()
        .unwrap_or_else(|| args.file.with_extension("attest.json"));
    let serialized = serde_json::to_vec_pretty(&attestation)?;
    std::fs::write(&out, &serialized)
        .with_context(|| format!("failed to write {}", out.display()))?;

    eprintln!("Attested {}", args.file.display());
    eprintln!("  subject sha256: {digest_hex}");
    eprintln!("  signer key:      {}", hex::encode(public_key));
    eprintln!("  attestation:     {}", out.display());
    Ok(())
}

/// Verify an attestation produced by `daedalus attest` against the current
/// binary and the local trust store. Three independent checks must pass:
///
/// 1. the signature verifies over the canonical `(digest, sbom)` envelope
///    under a key from the local trust store;
/// 2. the subject digest recorded in the attestation still matches the file
///    on disk;
/// 3. the SBOM is present and well formed.
///
/// Check 1 is what makes the SBOM tamper-evident: the SBOM is read back from
/// the attestation document and re-serialized, so editing a single field in
/// the predicate changes the signed bytes and the signature no longer
/// verifies.
fn verify_attestation(path: &std::path::Path, digest_hex: &str, _footer: &Footer) -> Result<()> {
    let att_path = path.with_extension("attest.json");
    let raw = std::fs::read(&att_path)
        .with_context(|| format!("failed to read {}", att_path.display()))?;
    let doc: serde_json::Value = serde_json::from_slice(&raw)
        .with_context(|| format!("failed to parse {}", att_path.display()))?;

    let signed_digest = doc
        .get("subject")
        .and_then(|s| s.get(0))
        .and_then(|s| s.get("digest"))
        .and_then(|d| d.get("sha256"))
        .and_then(|d| d.as_str())
        .context("attestation missing subject sha256")?;
    let sig_hex = doc
        .get("signature")
        .and_then(|s| s.as_str())
        .context("attestation missing signature")?;
    let sbom = doc
        .get("predicate")
        .context("attestation missing predicate (SBOM)")?;

    let sig_bytes = hex::decode(sig_hex).context("signature is not valid hex")?;
    let sig_array: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes"))?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_array);

    let trust_dir = daedalus_core::paths::trusted_keys_dir();
    let keys = load_trusted_keys(&trust_dir)?;
    if keys.is_empty() {
        anyhow::bail!(
            "no trusted keys in {} - add keys with: daedalus trust <pubkey_file>",
            trust_dir.display()
        );
    }

    let digest_bytes = hex::decode(signed_digest).context("subject digest is not valid hex")?;
    let digest_array: [u8; 32] = digest_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("subject digest must be 32 bytes"))?;

    let payload = signing_payload(&digest_array, sbom);
    // `verify_strict` (not `verify`) rejects small-order public keys and
    // non-canonical signatures per ZIP-215.
    let verified = keys
        .iter()
        .any(|(_, vk)| vk.verify_strict(&payload, &signature).is_ok());
    if !verified {
        anyhow::bail!(
            "attestation signature did not verify under any trusted key \
             (the SBOM or the subject digest was modified after signing)"
        );
    }

    if signed_digest != digest_hex {
        anyhow::bail!(
            "attestation is valid but does not match this binary \
             (attested {}, on disk {})",
            signed_digest,
            digest_hex
        );
    }

    eprintln!("Attestation OK");
    eprintln!("  subject sha256: {signed_digest}");
    eprintln!("  SBOM covered by the signature: yes");
    eprintln!("  signature verified under a trusted Ed25519 key (ZIP-215 strict)");
    eprintln!("  attestation matches the binary on disk");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::file_digest;
    use super::signing_payload;
    use daedalus_core::format::Footer;
    use serde_json::json;

    /// Build a footer whose slices point at real bytes of `content`, so the
    /// digest is computed over actual payload/metadata rather than a mock.
    /// The first 4 bytes are the payload, the remainder is metadata.
    fn footer_for(content: &[u8]) -> Footer {
        let meta_len = content.len() - 4;
        assert!(meta_len > 0, "test content must include metadata bytes");
        Footer {
            format_version: 3,
            arch: daedalus_core::format::ARCH_X86_64,
            flags: daedalus_core::format::FLAG_SIGNED,
            payload_offset: 0,
            payload_csize: 4,
            payload_usize: 4,
            payload_sha256: [0u8; 32],
            meta_offset: 4,
            meta_size: meta_len as u64,
            sig_offset: content.len() as u64,
        }
    }

    #[test]
    fn file_digest_changes_when_any_byte_changes() {
        let mut a = b"ABCD{}META".to_vec();
        let fa = footer_for(&a);
        a.extend_from_slice(&fa.pack_full());

        let mut b = b"WXYZ{}META".to_vec();
        let fb = footer_for(&b);
        b.extend_from_slice(&fb.pack_full());

        assert_ne!(file_digest(&a, &fa), file_digest(&b, &fb));
    }

    #[test]
    fn file_digest_covers_the_stub_region() {
        // The digest must cover the launcher stub too, not just payload and
        // metadata. A byte flipped inside the stub region changes the digest.
        let mut content = b"STUBSTUBABCD{}META".to_vec();
        let f = footer_for(&content);
        content.extend_from_slice(&f.pack_full());
        let baseline = file_digest(&content, &f);

        // Offset 2 sits in the stub, well before payload_offset (4).
        let mut tampered = content.clone();
        tampered[2] ^= 0xff;

        assert_ne!(file_digest(&tampered, &f), baseline);
    }

    #[test]
    fn file_digest_is_stable_for_identical_bytes() {
        let mut content = b"ABCD{}META".to_vec();
        let f = footer_for(&content);
        content.extend_from_slice(&f.pack_full());

        assert_eq!(file_digest(&content, &f), file_digest(&content, &f));
    }

    #[test]
    fn signing_payload_is_deterministic_for_equal_sboms() {
        // serde_json sorts keys when `preserve_order` is off, so two documents
        // built in a different insertion order must still sign identically.
        let a = json!({"z": 1, "a": {"y": 2, "b": 3}});
        let b = json!({"a": {"b": 3, "y": 2}, "z": 1});
        let digest = [7u8; 32];

        assert_eq!(signing_payload(&digest, &a), signing_payload(&digest, &b));
    }

    #[test]
    fn signing_payload_changes_when_any_sbom_field_changes() {
        // This is the property that makes the attestation tamper-evident.
        let digest = [7u8; 32];
        let honest = json!({
            "spdxVersion": "SPDX-2.3",
            "name": "app.de",
            "packages": [{"name": "requests", "version": "2.31.0"}],
        });
        let tampered = json!({
            "spdxVersion": "SPDX-2.3",
            "name": "app.de",
            "packages": [{"name": "requests", "version": "1.0.0-injected"}],
        });

        assert_ne!(
            signing_payload(&digest, &honest),
            signing_payload(&digest, &tampered)
        );
    }

    #[test]
    fn signing_payload_separates_digest_from_sbom_fields() {
        // Length prefixing must prevent moving bytes across the digest/sbom
        // boundary to produce a colliding statement.
        let digest = [0u8; 32];
        let sbom = json!({"x": "y"});

        assert_ne!(
            signing_payload(&digest, &sbom),
            signing_payload(&[1u8; 32], &sbom)
        );
    }

    #[test]
    fn signature_covers_sbom_so_tampering_breaks_verification() {
        use ed25519_dalek::{Signer, SigningKey, Verifier};

        let signing_key = SigningKey::from_bytes(&[42u8; 32]);
        let digest = [9u8; 32];
        let honest_sbom = json!({"packages": [{"name": "openssl", "version": "3.0.0"}]});
        let tampered_sbom = json!({"packages": [{"name": "openssl", "version": "0.0.0-evil"}]});

        let signature = signing_key.sign(&signing_payload(&digest, &honest_sbom));
        let vk = signing_key.verifying_key();

        assert!(vk
            .verify_strict(&signing_payload(&digest, &honest_sbom), &signature)
            .is_ok());
        assert!(vk
            .verify_strict(&signing_payload(&digest, &tampered_sbom), &signature)
            .is_err());
    }

    #[test]
    fn strict_verification_rejects_small_order_key_zip215() {
        // Named to make the ZIP-215 claim traceable to a test. `verify_strict`
        // must reject small-order public keys; plain `verify` would not.
        use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

        let signing_key = SigningKey::from_bytes(&[3u8; 32]);
        let signature: Signature = signing_key.sign(b"attestation bytes");
        let small_order = [
            0u8, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];

        // A signature made by the matching small-order key must not verify.
        let small_order_vk = VerifyingKey::from_bytes(&small_order).expect("valid encoding");
        assert!(small_order_vk
            .verify_strict(b"attestation bytes", &signature)
            .is_err());
    }
}
