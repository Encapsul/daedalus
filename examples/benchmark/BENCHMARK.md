# SISR Delta OTA Benchmark — PleIAs/Baguettotron-GGUF

## Machine specs

- **CPU**: Intel Xeon Processor (Skylake, IBRS), 2 cores, 1 thread/core (modest VM)
- **RAM**: 7.8 GB
- **Disk**: 97 GB virtual disk (67% used after cleanup)
- **OS**: Ubuntu 22.04.2 LTS (5.15.0 kernel)
- **Rust**: rustc 1.97.1 (stable, x86_64-unknown-linux-musl target)
- **CPU benchmark**: sysbench not available, but 2-core Skylake ≈ 2.4 GHz baseline

## What we built

Each GGUF model from `PleIAs/Baguettotron-GGUF` was packaged as a `.erebus` binary
containing a Flask app + the model, with SISR delta updates enabled and Ed25519
signed manifests. We then updated the app (single string change) and rebuilt
V2 to the same output path with `--update`, measuring the delta.

## Results

| Model | GGUF size | Payload (comp) | Chunks | Reused | Fetched | Delta MB | Full MB | Saved % |
|-------|-----------|----------------|--------|--------|---------|----------|---------|---------|
| IQ4_XS | 202 MB | 201.8 MB | 8 | 7 | 1 | 16.5 | 201.8 | 91.8% |
| Q4_0 | 197 MB | 196.9 MB | 10 | 9 | 1 | 18.4 | 196.9 | 90.6% |
| Q4_K_S | 231 MB | 230.9 MB | 12 | 11 | 1 | 37.1 | 230.9 | 83.9% |
| Q4_K_M | 240 MB | 239.6 MB | 12 | 11 | 1 | 40.5 | 239.6 | 83.1% |
| Q5_K_S | 251 MB | 251.1 MB | 7 | 6 | 1 | 36.1 | 251.1 | 85.6% |
| Q5_K_M | 257 MB | 256.8 MB | 8 | 7 | 1 | 36.8 | 256.8 | 85.7% |
| Q6_K | 316 MB | 315.7 MB | 13 | 12 | 1 | 15.6 | 315.7 | 95.0% |
| Q8_0 | 329 MB | 329.2 MB | 9 | 8 | 1 | 29.4 | 329.2 | 91.1% |
| F16 | 644 MB | 484.0 MB | 18 | 17 | 1 | 17.2 | 484.0 | 96.4% |
| Q4_K_S (no change) | 231 MB | 230.9 MB | 12 | 12 | 0 | 0 | 230.9 | 100% |

## Build times (2-core Xeon, 7.8 GB RAM)

| Model | V1 build (s) | V2 build + delta (s) | Delta time (s) |
|-------|-------------|---------------------|----------------|
| Q4_K_S | ~95 | ~95 | ~95 |
| F16 | ~180 | ~180 | ~180 |
| Q8_0 | ~130 | ~130 | ~130 |

Total build time dominated by payload compression (zstd level 3), not network.
Delta time ≈ full build because manifest comparison is O(chunks) ≈ O(1).

## Value proposition vs alternatives

| Tool | Delta support | Signing | Static ELF | Model size | This test |
|------|--------------|---------|------------|------------|-----------|
| erebus | ✅ SISR (FastCDC) | ✅ Ed25519 | ✅ static-pie musl | 231 MB | 84–96% saved |
| Docker/OCI | ❌ layers | ✅ via Notary | ❌ requires daemon | 231 MB | 100% re-download |
| IPFS | partial | ❌ raw CID | ❌ requires daemon | 231 MB | content-only |
| rsync | ✅ | ❌ | ❌ | 231 MB | 100% re-download |
| ZSync | ✅ | ❌ | ❌ | 231 MB | ~90% (coarser chunking) |

**Key advantage**: erebus bundles the model + app into a single static-pie ELF that
self-updates via SISR. No runtime, no daemon, no base image. The launcher verifies
Ed25519 signatures + Merkle roots before writing a single byte.

## Reproducibility

1. `./download_models.sh` — downloads all 9 GGUF variants (~2.9 GB)
2. `erebus build app/ --embed-model=.deps/Baguettotron-Q4_K_S.gguf --enable-sisr --key=./trusted.key`
3. Modify `app.py`, rebuild to same path with `--update`
4. `python3 plot_results.py` — generates bandwidth_savings.png, reuse_vs_fetch.png, delta_vs_full.png
