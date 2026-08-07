# Builder Pipeline

> Status: **implemented** (`xbin-core` `assembly` + `sisr_stage`).
> Describes how a `.xbin` is assembled, how the optional `SISR` stage fits
> into the packager, and the invariants that keep the two outputs consistent.

The packager lives in `xbin-core/src/assembly.rs`. Its entry point is
`assemble_xbin`; the SISR-enabled variant is `assemble_xbin_with_sisr`. Both
return the total file size and set executable permissions on Unix.

## Classic pipeline

```
stub_bytes ─┐
payload    ─┼─► [stub][payload][metadata][footer] ──► app.xbin
meta_bytes ─┘      │                    │
              payload_sha256 = SHA-256(payload ‖ meta_bytes)
              footer         = format version, offsets, hash, magic
```

No key material is involved; `flags = 0`. This is the baseline that every
legacy decoder understands.

## SISR pipeline

`assemble_xbin_with_sisr` takes a [`SisrBuildConfig`]
(`enabled`, `chunk_target_size`, optional `signing_key`). With `enabled`
false the bytes written are **strictly identical** to the classic path — this
is enforced by a test. With `enabled` true the layout becomes:

```
[stub][payload][metadata][DeltaManifest][SisrFooterExt][footer]
```

Steps, all in memory:

1. **Chunk** the payload with `FastCDC` at `chunk_target_size`
   (`sisr_stage::chunk_payload`), hashing each chunk with SHA-256.
2. **Serialize** the `DeltaManifest` (magic `XBMD`, chunk table).
3. **Merkle root** over the chunk hashes (`sisr_stage::merkle_root`).
4. **Sign** `merkle_root ‖ manifest_bytes` with the optional Ed25519 key
   (`sisr_stage::sign`); all-zeros signature when unsigned.
5. **Inject** the manifest and the fixed 110-byte `SisrFooterExt` before the
   standard footer; set `FLAG_SISR` in the footer `flags` byte.
6. **Write** the remote manifest to `<name>.xbin.manifest`
   (`RemoteManifest::to_bytes`): `XBMR` magic + `merkle_root` + signature +
   embedded `DeltaManifest`, self-contained and verifiable offline.

The footer's absolute offsets (`payload_offset`, `meta_offset`) are unaffected
by the insertion; only `flags` and the new blocks change.

## Invariants

- **Disabled ⇒ identical output.** `assemble_xbin_with_sisr(…, disabled())`
  emits the exact bytes of `assemble_xbin`.
- **Reader/writer symmetry.** The extension is located by the reader at
  `file_len − footer_size − 110` (immediately before the footer) and the
  manifest via `chunk_table_offset` — the writer places the manifest between
  the metadata and the extension, satisfying the `table_end ≤ ext_start`
  bound.
- **No runtime coupling.** The launcher is untouched; the classic and SISR
  files both boot through the stock footer.
- **Zeroize.** The `SigningKey` (an `ed25519-dalek` type) drops its secret
  bytes when the config goes out of scope.

## Performance

The SISR stage is single-pass and in-memory. The dominant cost is the SHA-256
per chunk (see the [chunker notes](./internal-crates.md)); the FastCDC scan and
the Merkle pairing add negligible time.

**Measured** (`perf_sisr_on_100_mib`, release build, i5-7300U @1.6 GHz, no
SHA-NI, machine under load): 100 MiB payload, 64 KiB target chunks ⇒ **5.3 s**
(19 MiB/s), 812 chunks, 29 KiB manifest. The classic path already pays one
full SHA-256 pass, so the stage roughly adds a second pass plus the scan; on
CPUs with SHA-NI the `compress`-featured `sha2` (runtime-dispatched) collapses
this to well under the < 5 % build-overhead budget. On the current CPU the
extra pass is the cost of content addressing — run the probe with
`cargo test -p xbin-core --release perf_sisr -- --ignored --nocapture`.

## Related

- [SISR: Self-Incremental Sovereign Reconstruction](./sisr-spec.md) — the
  trust model the stage serves.
- [`.xbin` Format v2 — SISR extension](../spec/xbin-format-v2.md) — exact byte
  layout of the manifest and footer extension.
- [The Builder](../reference/builder.md) — CLI flow that feeds this packager.
- [Incremental Updates (SISR)](../guides/incremental-updates.md) — consuming
  the remote manifest for delta reconstruction.
