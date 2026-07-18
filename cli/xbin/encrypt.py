"""Payload encryption — AES-256-GCM with HKDF key derivation.

The AES key is derived from the Ed25519 signing seed via HKDF-SHA256.
This means signing key = encryption key: whoever can sign can also decrypt.

Security model (stated plainly):
  Encryption protects the payload AT REST against casual extraction.
  It does NOT protect against a determined attacker on a machine that
  must run the decrypted app — the launcher decrypts before exec, and
  the key is derivable from the signing seed stored in metadata.
  This is the same fundamental limit as any DRM system.
"""

from __future__ import annotations

import hashlib
import hmac
import os

# AES-256-GCM constants
_NONCE_LEN = 12  # 96-bit nonce (standard for AES-GCM)
_TAG_LEN = 16  # 128-bit authentication tag

# HKDF parameters for AES key derivation from Ed25519 signing seed
_HKDF_SALT = b"xbin-encrypt-v1"
_HKDF_INFO = b"aes-256-gcm-key"

# Crypto suite IDs (stored in footer.payload_usize when format_version=4)
CRYPTO_NONE = 0x00
CRYPTO_AES_256_GCM = 0x01

CRYPTO_SUITE_NAMES = {
    CRYPTO_NONE: "none",
    CRYPTO_AES_256_GCM: "aes-256-gcm",
}


def _hkdf_derive_key(signing_seed: bytes) -> bytes:
    """Derive a 32-byte AES-256 key from an Ed25519 signing seed via HKDF-SHA256."""
    if len(signing_seed) != 32:
        raise ValueError(f"signing seed must be 32 bytes, got {len(signing_seed)}")
    # HKDF-Extract
    prk = hmac.new(_HKDF_SALT, signing_seed, hashlib.sha256).digest()
    # HKDF-Expand (one round, we need 32 bytes)
    t = hmac.new(prk, _HKDF_INFO + b"\x01", hashlib.sha256).digest()
    return t[:32]


def encrypt_payload(plaintext: bytes, signing_seed: bytes) -> tuple[bytes, dict]:
    """Encrypt payload with AES-256-GCM. Returns (ciphertext, crypto_metadata).

    ciphertext = [12-byte nonce][plaintext][16-byte GCM tag]
    crypto_metadata is stored in the .xbin metadata JSON.
    """
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM

    aes_key = _hkdf_derive_key(signing_seed)
    nonce = os.urandom(_NONCE_LEN)
    aesgcm = AESGCM(aes_key)
    # AESGCM.encrypt returns nonce ‖ ciphertext ‖ tag
    ct_with_tag = aesgcm.encrypt(nonce, plaintext, None)
    # ct_with_tag = plaintext ‖ 16-byte tag (nonce is not included by cryptography lib)
    ciphertext = ct_with_tag  # plaintext_len + 16 bytes

    return ciphertext, {
        "nonce_hex": nonce.hex(),
        "tag_offset": len(plaintext),  # where the 16-byte tag starts in ciphertext
        "signing_seed_hex": signing_seed.hex(),
    }


def decrypt_payload(
    ciphertext: bytes, signing_seed: bytes, nonce_hex: str, tag_offset: int
) -> bytes:
    """Decrypt AES-256-GCM payload. Returns plaintext."""
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM

    aes_key = _hkdf_derive_key(signing_seed)
    nonce = bytes.fromhex(nonce_hex)
    if len(nonce) != _NONCE_LEN:
        raise ValueError(f"nonce must be {_NONCE_LEN} bytes, got {len(nonce)}")

    aesgcm = AESGCM(aes_key)
    # ciphertext includes the appended tag
    plaintext = aesgcm.decrypt(nonce, ciphertext, None)
    return plaintext
