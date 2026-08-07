# Comparative Benchmark — x.bin vs Docker / pkg / AppImage / Flatpak

_Generated: 2026-08-07T12:06Z — machine: `codespaces-ba69ea`_
_Reference app: `/workspaces/x.bin/benchmarks/comparison/apps/hello-node` (Node.js HTTP server, zero deps)_

## Test machine

```
# Machine profile — codespaces-ba69ea
hostname:    codespaces-ba69ea
kernel:      Linux 6.8.0-1052-azure
arch:        x86_64
cpu_model:   AMD EPYC 9V74 80-Core Processor
cores:       4
ram_total:   15Gi
root_dev:    overlay
root_fs:     overlayfs
disk_total:  32G
disk_free:   11G
tmp_free:    99G
env:         docker-container
live_system: no
xbin:        xbin 0.5.0
stub:        n/a (? bytes)
node:        v24.14.0
docker:      Docker version 29.3.0-1, build 5927d80c76b3ce5cf782be818922966e8a0d87a3
pkg:         @yao-pkg/pkg@latest node24-linux-x64
```

## Results

| Packager | Artifact | On-disk | Cold start | Warm start | Idle RSS | Host deps |
|----------|----------|---------|------------|------------|----------|-----------|
| xbin | 44.0 MiB | 122.6 MiB | 2142 ms | 96 ms | 48.6 MiB | none |
| docker | 76.7 MiB | 76.7 MiB | 6147 ms | n/a ms | 50.0 MiB | docker-daemon |
| pkg | 70.2 MiB | 70.2 MiB | 109 ms | n/a ms | 55.0 MiB | none |
| appimage | 24.4 MiB | 24.4 MiB | 29 ms | n/a ms | 55.6 MiB | fuse-or-extract |
| flatpak | n/a MiB | n/a MiB | n/a ms | n/a ms | n/a MiB | flatpak |

## Methodology

- **Cold start** = wall time from launch to first HTTP 200.
- **Warm start** = second launch of the same artifact (extraction cache hit). Only x.bin caches; the other packagers re-launch every time.
- **Idle RSS** = resident set of the process actually listening on the port, 1s after first response (Linux VmRSS, resolved via `ss`). Some packagers re-exec/spawn children (AppImage runtime, x.bin exec) — the server process is measured, not the launched PID.
- **On-disk footprint** = space used at run time (x.bin: extracted rootfs cache; Docker: uncompressed image; pkg/AppImage: the artifact itself).
- **Host deps** = packages/services the target host must provide.
- Flatpak requires a Flatpak host + OSTree runtimes; not measured in this container.
- Every run records a **machine profile** (`results/<machine>/profile.txt`) — comparing two machines without their profile is meaningless.

Raw data: `results.tsv`. Re-run: `bash benchmarks/comparison/run.sh`.
Multi-machine aggregation: `bash benchmarks/comparison/aggregate.sh`.
