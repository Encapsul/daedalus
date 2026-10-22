//! Air-gap release train: publisher → USB media → recipient trust/apply.
//!
//! This is a real (non-synthetic-ratio) v1→v2 delta: shared head, rewritten
//! tail, signed RemoteManifest, DirectoryChunkFetcher apply. Empty / foreign
//! trust fails closed. Synthetic CI size ratios elsewhere are not this path.

use assert_cmd::Command;
use daedalus_core::assembly::{assemble_daedalus, AssemblyInput};
use daedalus_core::format::Footer;
use daedalus_core::sisr::engine::{DirectoryChunkFetcher, SisrEngine};
use daedalus_core::sisr_header::read_sisr;
use daedalus_core::sisr_stage::{
    build_artifacts, export_media, verify_embedded_sisr, verify_media, RemoteManifest,
    SisrBuildConfig,
};
use ed25519_dalek::SigningKey;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;
use tempfile::tempdir;

fn daedalus() -> Command {
    Command::cargo_bin("daedalus").unwrap()
}

fn random_buf(len: usize, seed: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut state = seed;
    while out.len() < len {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        out.extend_from_slice(&state.to_le_bytes());
    }
    out.truncate(len);
    out
}

fn signed_artifacts(payload: &[u8], key: &SigningKey) -> daedalus_core::sisr_stage::SisrArtifacts {
    build_artifacts(
        payload,
        &SisrBuildConfig {
            enabled: true,
            chunk_target_size: 4096,
            signing_key: Some(key.clone()),
        },
    )
    .unwrap()
}

fn assemble_sisr(
    path: &Path,
    stub: &[u8],
    payload: &[u8],
    meta: &[u8],
    artifacts: &daedalus_core::sisr_stage::SisrArtifacts,
) {
    assemble_daedalus(
        path,
        &AssemblyInput {
            stub_bytes: stub,
            payload,
            meta_bytes: meta,
            squashfs: false,
            target_arch: None,
            sisr: Some(artifacts.clone()),
            encryption: None,
        },
    )
    .unwrap();
}

fn payload_of(bin: &Path) -> Vec<u8> {
    let data = std::fs::read(bin).unwrap();
    let footer = Footer::read_from(&mut Cursor::new(&data)).unwrap();
    let start = footer.payload_offset as usize;
    let end = start + footer.payload_csize as usize;
    data[start..end].to_vec()
}

fn stage_cli_media(v2: &Path, v1: &Path, out: &Path) {
    daedalus()
        .args([
            "media",
            "stage",
            v2.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--against",
            v1.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"written_chunks\""));
}

fn assert_sisr_and_media_verify(
    v1: &Path,
    usb: &Path,
    usb_cli: &Path,
    reuse: &HashSet<[u8; 32]>,
    public: &ed25519_dalek::VerifyingKey,
    work: &Path,
) {
    let pub_path = work.join("publisher.pub");
    std::fs::write(&pub_path, public.as_bytes()).unwrap();
    let trust = work.join("trust");
    std::fs::create_dir_all(&trust).unwrap();
    daedalus()
        .args([
            "trust",
            pub_path.to_str().unwrap(),
            "--trusted-dir",
            trust.to_str().unwrap(),
        ])
        .assert()
        .success();
    daedalus()
        .args(["verify", v1.to_str().unwrap(), "--json"])
        .env("DAEDALUS_TRUSTED_DIR", &trust)
        .assert()
        .success()
        .stdout(predicate::str::contains("\"scheme\": \"sisr\""))
        .stdout(predicate::str::contains("\"verified\": true"));
    let empty = work.join("empty-trust");
    std::fs::create_dir_all(&empty).unwrap();
    daedalus()
        .args(["verify", v1.to_str().unwrap()])
        .env("DAEDALUS_TRUSTED_DIR", &empty)
        .assert()
        .failure()
        .stderr(predicate::str::contains("no trusted keys"));
    assert!(verify_media(usb, &[], Some(reuse)).is_err());
    daedalus()
        .args([
            "media",
            "verify",
            usb_cli.to_str().unwrap(),
            "--against",
            v1.to_str().unwrap(),
            "--trusted-dir",
            trust.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"verified\": true"));
    daedalus()
        .args([
            "media",
            "verify",
            usb.to_str().unwrap(),
            "--against",
            v1.to_str().unwrap(),
            "--trusted-dir",
            empty.to_str().unwrap(),
        ])
        .assert()
        .failure();
}

/// Publisher builds v1+v2, stages USB against v1, recipient verifies trust then applies.
#[test]
fn airgap_train_usb_stage_trust_matrix_and_apply() {
    let tmp = tempdir().unwrap();
    let key = SigningKey::from_bytes(&[41u8; 32]);
    let public = key.verifying_key();
    let foreign = SigningKey::from_bytes(&[42u8; 32]);

    let v1 = random_buf(48_000, 21);
    let mut v2 = v1.clone();
    v2[32_000..].copy_from_slice(&random_buf(16_000, 77));
    let a1 = signed_artifacts(&v1, &key);
    let a2 = signed_artifacts(&v2, &key);
    let v1_bin = tmp.path().join("app-v1.de");
    let v2_bin = tmp.path().join("app-v2.de");
    assemble_sisr(
        &v1_bin,
        b"STUB_AIRGAP_TRAIN__________",
        &v1,
        br#"{"name":"airgap-train","version":"1"}"#,
        &a1,
    );
    assemble_sisr(
        &v2_bin,
        b"STUB_AIRGAP_TRAIN__________",
        &v2,
        br#"{"name":"airgap-train","version":"1"}"#,
        &a2,
    );

    let remote = RemoteManifest {
        merkle_root: a2.merkle_root,
        signature: a2.signature,
        manifest: a2.manifest.clone(),
    };
    assert!(remote.verify_signature(&public));
    assert!(!remote.verify_any(&[]), "empty trust must fail closed");
    assert!(!remote.verify_any(&[foreign.verifying_key()]));

    let reuse: HashSet<_> = a1.manifest.chunks.iter().map(|c| c.hash).collect();
    let usb = tmp.path().join("usb");
    let stats = export_media(&v2, &remote, &usb, Some(&reuse)).unwrap();
    assert!(stats.written_chunks > 0 && stats.skipped_chunks > 0);
    assert_eq!(
        std::fs::read_dir(usb.join("chunks")).unwrap().count(),
        stats.written_chunks
    );

    let usb_cli = tmp.path().join("usb-cli");
    stage_cli_media(&v2_bin, &v1_bin, &usb_cli);
    assert_sisr_and_media_verify(&v1_bin, &usb, &usb_cli, &reuse, &public, tmp.path());

    let installed = tmp.path().join("installed.de");
    std::fs::copy(&v1_bin, &installed).unwrap();
    let fetcher = DirectoryChunkFetcher::new(&usb.join("chunks"));
    let (updated, apply_stats) = SisrEngine
        .apply_update_with_stats(&installed, &remote.manifest, &fetcher, &remote.signature)
        .unwrap();
    assert!(apply_stats.fetched_chunks > 0 && apply_stats.reused_chunks > 0);
    assert_eq!(payload_of(&updated), v2);

    let data = std::fs::read(&updated).unwrap();
    let (ext, manifest) = read_sisr(&mut Cursor::new(&data)).unwrap().unwrap();
    verify_embedded_sisr(&ext, &manifest, &payload_of(&updated), &[public]).unwrap();
    assert!(verify_embedded_sisr(&ext, &manifest, &payload_of(&updated), &[]).is_err());
}

/// `media stage --pubkey` copies only a 32-byte `.pub`; refuses non-.pub paths.
#[test]
fn media_stage_copies_pubkey_refuses_private_key_name() {
    let tmp = tempdir().unwrap();
    let key = SigningKey::from_bytes(&[43u8; 32]);
    let payload = random_buf(12_000, 9);
    let artifacts = signed_artifacts(&payload, &key);
    let bin = tmp.path().join("app.de");
    assemble_sisr(
        &bin,
        b"STUB________________________",
        &payload,
        br#"{"n":1}"#,
        &artifacts,
    );

    let pub_path = tmp.path().join("publisher.pub");
    std::fs::write(&pub_path, key.verifying_key().as_bytes()).unwrap();
    let key_path = tmp.path().join("publisher.key");
    std::fs::write(&key_path, key.to_bytes()).unwrap();

    let out = tmp.path().join("media");
    daedalus()
        .args([
            "media",
            "stage",
            bin.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--pubkey",
            pub_path.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(
        std::fs::read(out.join("publisher.pub")).unwrap(),
        key.verifying_key().as_bytes()
    );

    let out2 = tmp.path().join("media-bad");
    daedalus()
        .args([
            "media",
            "stage",
            bin.to_str().unwrap(),
            "-o",
            out2.to_str().unwrap(),
            "--pubkey",
            key_path.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("private keys must never"));
}

/// Help surfaces the airgap train command (CI smoke pattern).
#[test]
fn media_help_mentions_stage_and_usb() {
    daedalus()
        .args(["media", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("stage"))
        .stdout(predicate::str::contains("verify"))
        .stdout(predicate::str::contains("USB").or(predicate::str::contains("air-gapped")));
}

/// Sanity: payload SHA changes across the real delta (not a synthetic byte ratio).
#[test]
fn airgap_train_v1_v2_payload_hashes_differ() {
    let v1 = random_buf(48_000, 21);
    let mut v2 = v1.clone();
    v2[32_000..].copy_from_slice(&random_buf(16_000, 77));
    let h1: [u8; 32] = Sha256::digest(&v1).into();
    let h2: [u8; 32] = Sha256::digest(&v2).into();
    assert_ne!(h1, h2);
}
