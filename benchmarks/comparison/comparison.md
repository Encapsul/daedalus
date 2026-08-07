# Comparative Benchmark — x.bin vs Docker / pkg / AppImage / Flatpak

_Aggregated: 2026-08-07T12:11Z_

> Each row is one (machine, packager) measurement. Full methodology and
> per-machine details: see `results/<machine>/comparison.md`.

## Test machines

| Machine | CPU | Cores | RAM | Disk | Root | Env |
|---------|-----|-------|-----|------|------|-----|
| codespaces-ba69ea | AMD EPYC 9V74 80-Core Processor | 4 | 15Gi | 32G | overlayfs | docker-container |

## Results

| Machine | Packager | Artifact | On-disk | Cold start | Warm start | Idle RSS | Host deps |
|---------|----------|----------|---------|------------|------------|----------|-----------|
| codespaces-ba69ea | xbin | 44.0 MiB | 122.6 MiB | 2142 ms | 96 ms | 48.6 MiB | none |
| codespaces-ba69ea | docker | 76.7 MiB | 76.7 MiB | 6147 ms | n/a ms | 50.0 MiB | docker-daemon |
| codespaces-ba69ea | pkg | 70.2 MiB | 70.2 MiB | 109 ms | n/a ms | 55.0 MiB | none |
| codespaces-ba69ea | appimage | 24.4 MiB | 24.4 MiB | 29 ms | n/a ms | 55.6 MiB | fuse-or-extract |
| codespaces-ba69ea | flatpak | n/a MiB | n/a MiB | n/a ms | n/a ms | n/a MiB | flatpak |

Raw data: `results/<machine>/results.tsv`. Re-run: `bash benchmarks/comparison/run.sh`.
