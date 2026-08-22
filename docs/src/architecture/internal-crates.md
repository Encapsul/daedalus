# Internal Crates

> These abstractions are the building blocks of the SISR engine. They are
> **pure and in-memory**: they never invoke an external tool
> (`Command::new("squashfs")` is forbidden by design), never touch the
> network, and are independent of the host filesystem layout. Their only
> contract is bytes in, bytes out, verified.

The internal modules of `daedalus-core` that power SISR:

```
daedalus_core
 ├── chunker      --> trait Chunker & FastCDC implementation
 ├── cas          --> trait ObjectStore (Content-Addressable Storage)
 ├── assembler    --> trait BinaryAssembler (binary stitching)
 ├── manifest     --> binary DeltaManifest (embedded SISR chunk index)
 ├── sisr_header  --> SisrFooterExt (fixed 110-byte access block)
 ├── sisr_stage   --> build stage: chunk payload, Merkle root, Ed25519 sign,
 │                    remote manifest
  └── sisr         --> runtime stage: sisr::engine (reconstruction) +
                       sisr::swap (atomic replacement) +
                       sisr::health (health gate state machine) +
                       sisr::resilience (rollback snapshot)
```

Each trait is deliberately small and single-purpose so the runtime
reconstruction path stays auditable end to end.

## `chunker` — Content-Defined Chunking

```rust
pub struct ChunkDescriptor {
    pub offset: usize,
    pub length: usize,
    pub hash: [u8; 32],   // SHA-256 of the chunk bytes
}

pub trait Chunker {
    fn chunk(&self, data: &[u8]) -> Vec<ChunkDescriptor>;
}
```

Splits a byte buffer at **content-defined** boundaries: the boundary position
depends only on the bytes before it, not on any fixed offset. This is what
makes incremental updates cheap — an edit only invalidates the chunks it
touches, everything else stays byte-identical and is reused from cache.

**Implementation: `FastCDC`.** A rolling gear hash (`fp = (fp << 1) + gear[b]`)
over a deterministic 256-entry gear table, with the classic two-mask
normalization. Bounds default to `min = avg/4`, `max = avg*4`. Deterministic
across runs and platforms: identical input always produces identical
boundaries. See the
[delta manifest format](../spec/delta-manifest-format.md) for how chunks feed
an update.

**Throughput (measured, 256 MiB in memory, `opt-level = 3`).** The boundary
scan alone runs at **~390 MB/s** on an i5-7300U (@1.6 GHz, under load) — above
the 200 MB/s objective. End-to-end `chunk()` is bounded by SHA-256: on CPUs
with SHA-NI the `compress` feature of `sha2` (runtime-dispatched, no new
dependency) reaches ~1-2 GB/s and keeps the pipeline above target; on CPUs
without SHA-NI the soft implementation dominates (~50 MB/s on the test
machine). The scan is the throughput-critical mechanism; the content-address
hash is the price of integrity and should be deferred to the caller when a
chunk already exists in the store.

## `cas` — Content-Addressable Storage

```rust
pub trait ObjectStore {
    fn put(&mut self, hash: &[u8; 32], data: &[u8]) -> std::io::Result<()>;
    fn get(&self, hash: &[u8; 32]) -> std::io::Result<Option<Vec<u8>>>;
}
```

A byte store keyed by **content** rather than by name. The key is the SHA-256
of the value, so the same bytes always map to the same object — a malicious or
bit-rotted chunk simply does not exist under the hash the manifest expects.

**Verification on both write and read** (anti-bitrot):

- `put` fails unless `SHA-256(data) == hash`; the on-disk copy is re-read and
  re-verified before `put` returns.
- `get` recomputes the hash of the stored bytes and fails on mismatch instead
  of returning corrupted data.

Two implementations ship: `MemoryStore` (tests/embedded use) and
`DiskObjectStore` (one file per content hash under a root directory, with
atomic rename-on-write).

## `assembler` — Binary Stitching

```rust
pub trait BinaryAssembler {
    fn assemble(
        &self,
        base_exec: &[u8],
        payload_blocks: &[Vec<u8>],
        output: &mut dyn Write,
    ) -> std::io::Result<()>;
}
```

Reassembles a complete executable from a base binary and the reconstructed
payload blocks, preserving the `.ere` layout
`[stub][payload][metadata][footer]`. The concrete `DaedalusStitcher` reads the
payload/metadata offsets from the footer and splices the new blocks into the
payload region — the output is a valid `.ere` indistinguishable from one
produced by `daedalus build`.

This trait exists so that format-specific splicing (ELF/PE/Mach-O, squashfs
images) can evolve behind one stable contract.

## `sisr_stage` — build-side SISR pipeline

Where the classic `assembly` module writes a plain `.ere`, `sisr_stage`
computes the SISR artifacts in memory:

```rust
pub struct SisrBuildConfig {
    pub enabled: bool,
    pub chunk_target_size: usize,
    pub signing_key: Option<ed25519_dalek::SigningKey>,
}

pub fn build_artifacts(payload: &[u8], config: &SisrBuildConfig)
    -> io::Result<SisrArtifacts>;
```

`build_artifacts` content-chunks the payload (FastCDC), serializes the
`DeltaManifest`, computes the Merkle root over the chunk hashes, and signs
`merkle_root ‖ manifest_bytes` with the optional Ed25519 key. The signature is
all-zeros when no key is supplied (unsigned, integrity-only builds). The
`SigningKey` is an `ed25519-dalek` type, so the secret is zeroized on drop.

The same module serializes and verifies the standalone remote manifest
(`.ere.manifest`): `RemoteManifest::verify_signature` and
`verify_merkle` re-check both bindings without touching the binary — that is
the `ManifestVerifier` role for the SISR path.

## `sisr` — runtime reconstruction engine

The counterpart of `sisr_stage`, used by the launcher (`stub`): it rebuilds a
`.ere` on disk from the current binary plus a signed delta.

```rust
pub trait ChunkFetcher {
    fn fetch(&self, hash: &[u8; 32], length: usize) -> io::Result<Vec<u8>>;
    fn bytes_fetched(&self) -> u64;
}

pub struct SisrEngine;
impl SisrEngine {
    pub fn apply_update(&self, current_bin: &Path, manifest: &DeltaManifest,
                        fetcher: &dyn ChunkFetcher) -> io::Result<PathBuf>;
}
```

`apply_update` reads the footer and the embedded chunk index of the current
binary, then tiles the manifest's chunk table over the payload:

- unchanged chunks are **reused** from the running file (offset looked up by
  content hash, bytes SHA-256-verified before reuse);
- missing or corrupt chunks are **fetched** through the `ChunkFetcher`
  (`DirectoryChunkFetcher` reads `<root>/<64-hex-sha256>` files; a network
  fetcher can implement the same trait) and hash-verified;
- the rebuilt file is written through `sisr::swap::AtomicWriter` (`.tmp`,
  fsync, atomic `rename`) so any interruption leaves the original intact.

Every chunk written — reused **or** fetched — must SHA-256 to its manifest
entry; the engine never trusts a source. See
[runtime-launcher](./runtime-launcher.md) for the full flow and failure table.

## Security properties

| Concern | Mechanism |
|---|---|
| Bit rot on disk | SHA-256 re-verified on every `ObjectStore::get` |
| Malicious block injection | Content-addressing: tampered bytes ⇒ different hash ⇒ never found |
| Corrupt base during splice | Footer magic + offset bounds validated before writing |
| Replay / forgery | Ed25519 signature over `merkle_root ‖ manifest` (embedded + remote) |

## Related

- [SISR: Self-Incremental Sovereign Reconstruction](./sisr-spec.md) — the
  invariants and trust model these primitives serve.
- [Delta manifest format](../spec/delta-manifest-format.md) — the structure
  that references chunks and drives the assembler.
- [`.ere` Format v2 — SISR extension](../spec/daedalus-format-v2.md) — where the
  binary `DeltaManifest` and `SisrFooterExt` live inside the file.
