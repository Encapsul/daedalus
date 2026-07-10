from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path


def find_crypto() -> Path:
    """Locate the compiled xbin-crypto binary."""
    here = Path(__file__).resolve()
    repo = here.parents[2]
    tmp_target = Path("/tmp/xbin-stub-target")
    candidates = [
        repo / "stub/target/x86_64-unknown-linux-musl/release/xbin-crypto",
        repo / "stub/target/release/xbin-crypto",
        repo / "stub/target/x86_64-unknown-linux-musl/debug/xbin-crypto",
        repo / "stub/target/debug/xbin-crypto",
        tmp_target / "x86_64-unknown-linux-musl/release/xbin-crypto",
        tmp_target / "release/xbin-crypto",
        tmp_target / "x86_64-unknown-linux-musl/debug/xbin-crypto",
        tmp_target / "debug/xbin-crypto",
    ]
    env = os.environ.get("XBIN_CRYPTO")
    if env:
        candidates.insert(0, Path(env))
    for c in candidates:
        if c.is_file():
            return c
    raise FileNotFoundError(
        "xbin-crypto not found. Build it first:\n"
        "  cd stub && cargo build --release --target x86_64-unknown-linux-musl"
    )


def keygen(key_dir: str) -> str:
    """Generate an Ed25519 keypair via xbin-crypto keygen.

    Returns the hex fingerprint (SHA-256 of the public key).
    """
    binary = find_crypto()
    result = subprocess.run(
        [str(binary), "keygen", "--key-dir", key_dir],
        capture_output=True, text=True, check=True,
    )
    fp = result.stdout.strip()
    if not fp:
        raise RuntimeError("keygen returned empty fingerprint")
    return fp


def sign(keyfile: str, hash_bytes: bytes) -> bytes:
    """Sign a 32-byte SHA-256 hash with the given key file.

    Returns the 64-byte Ed25519 signature.
    """
    binary = find_crypto()
    result = subprocess.run(
        [str(binary), "sign", keyfile],
        input=hash_bytes, capture_output=True,
    )
    if result.returncode != 0:
        msg = result.stderr.decode().strip()
        raise RuntimeError(f"sign failed: {msg}")
    return result.stdout


def verify(pubkey: str, hash_and_sig: bytes) -> int:
    """Verify an Ed25519 signature.

    hash_and_sig: 96 bytes = [32-byte hash][64-byte signature]

    Returns 0 (valid), 1 (invalid), or raises RuntimeError (error).
    """
    binary = find_crypto()
    result = subprocess.run(
        [str(binary), "verify", pubkey],
        input=hash_and_sig, capture_output=True,
    )
    if result.returncode in (0, 1):
        return result.returncode
    msg = result.stderr.decode().strip()
    raise RuntimeError(f"verify error: {msg}")
