# Supply Chain Risk Audit Report

## Executive Summary

**Overall Security Posture: MEDIUM-HIGH RISK**

The x.bin project has several high-risk dependencies, primarily in the areas of:
- Cryptographic libraries (ed25519-dalek, curve25519-dalek)
- FFI-based libraries (openssl-sys, native-tls)
- Compression libraries with C dependencies (zstd, lz4_flex via backhand)

While most dependencies are well-established in the Rust ecosystem, the cryptographic dependencies are maintained by small teams with significant security implications.

## High-Risk Dependencies

| Dependency | Risk Factor | Reason | Suggested Alternative |
|------------|-------------|--------|----------------------|
| ed25519-dalek | Single maintainer, High-risk feature | Cryptographic signing library maintained by Dalek team (small org). Used for x.bin signature verification. | Continue using but monitor security advisories closely. Consider `signature` crate ecosystem for standardized interfaces. |
| curve25519-dalek | Single maintainer, High-risk feature | Core dependency for ed25519-dalek. Pure Rust but crypto-critical. | No direct alternative - this is the standard for Curve25519 in Rust. |
| openssl / openssl-sys | High-risk features, FFI | FFI bindings to OpenSSL C library. Vulnerable to OpenSSL CVEs. | Consider `rustls` as alternative - already used in hyper-rustls for some paths. |
| native-tls | High-risk features, FFI | TLS via system OpenSSL/Schannel. Platform-dependent behavior. | Already uses `hyper-rustls` with rustls for HTTP. Consider phasing out native-tls. |
| backhand | High-risk features, FFI | squashfs extraction with zstd/lz4 C dependencies. | Consider pure-Rust alternatives or ensure zstd-sys is kept updated. |
| zstd / zstd-safe | High-risk features, FFI | Zstandard compression with C bindings. | Monitor for security advisories; consider zstd's pure-Rust implementation if available. |

## Counts by Risk Factor

| Risk Factor | Count |
|-------------|-------|
| Single maintainer/team | 4 |
| High-risk features (FFI, crypto, deserialization) | 6 |
| Absence of security contact | 3 |

## Recommendations

1. **Critical**: Monitor `ed25519-dalek` and `curve25519-dalek` security advisories. These are critical for x.bin's signature verification feature.

2. **High Priority**: Consider reducing FFI surface by using `rustls` instead of `native-tls` for TLS connections. The project already uses `hyper-rustls` in some code paths.

3. **Medium Priority**: Implement dependency update automation for FFI-heavy dependencies (openssl-sys, zstd-sys, lz4_flex).

4. **Ongoing**: Add security contact policy to project SECURITY.md and ensure maintainers respond to security reports.

5. **Monitoring**: Subscribe to RustSec security advisories RSS feed for all direct dependencies.

## Detailed Analysis

### Cryptographic Dependencies

The `ed25519-dalek` crate (v2.2.0) is used for:
- Signing key generation
- Signature verification

Risk: Small maintainer team, crypto-critical, no direct replacement available.

### FFI Dependencies

Several dependencies use FFI:
- `openssl-sys`: Direct FFI to OpenSSL C library
- `native-tls`: Uses system TLS libraries
- `zstd-sys`: Zstandard C library bindings
- `lz4_flex`: LZ4 compression (may use FFI)

Risk: Vulnerable to C library security issues, platform-dependent behavior.

### HTTP/TLS Dependencies

The `reqwest` crate uses `hyper` with optional TLS support. The project's CI notes it depends on `openssl` via `native-tls`.

Risk: OpenSSL vulnerabilities affect the entire HTTP stack.

## Conclusion

The supply chain risk is acceptable for a security-focused tool like x.bin, but requires active monitoring of:
1. Security advisories for all crypto libraries
2. OpenSSL security updates
3. zstd/lz4 C library vulnerabilities

No immediate action required, but establish a process for security updates.