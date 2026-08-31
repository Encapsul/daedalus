# Payload encryption

daedalus supports optional AES-256-GCM encryption for the payload inside a
`.daedalus` binary. The key is never embedded in the binary; it must be supplied
at runtime.

## Build

```bash
daedalus build ./my-app -o my-app.daedalus --encrypt
```

This generates a random encryption key, derives the AES-256-GCM key via HKDF,
and encrypts the payload. The key material is written to a file alongside the
binary:

- `my-app.daedalus.key` — the raw 32-byte encryption key

Store this key securely. It is required to run the binary.

## Run

```bash
daedalus run my-app.daedalus --decrypt-key /path/to/my-app.daedalus.key
```

The runtime reads the key, derives the AES-256-GCM key, decrypts the payload,
and zeroizes the key material in memory.

## Security properties

- **AES-256-GCM** provides confidentiality and authenticity.
- **HKDF** derives the encryption key from the raw key material.
- The binary is tamper-evident: any modification to the ciphertext fails
  authentication.
- Key rotation is supported by re-encrypting the payload with a new key.

## Combining with signing

Encryption and signing are independent:

```bash
# Encrypt + sign
daedalus build ./my-app -o my-app.daedalus \
  --encrypt \
  --key ~/.daedalus/keys/<fingerprint>.key
```

The Ed25519 signature covers the metadata (which records that the payload is
encrypted). The encryption key is separate from the signing key.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Decrypted and launched successfully |
| `1` | Wrong key, corrupt ciphertext, or missing key file |
