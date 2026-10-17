# Cross-packager benchmark - daedalus vs Docker / pkg / AppImage / Flatpak

One recorded pass lives in
[`results/codespaces-ba69ea/results.tsv`](results/codespaces-ba69ea/results.tsv).
Same app (`apps/hello-node`, 312 bytes, Node 24), one run, no average.
Quote these numbers. They are not a win on cold start.

| Packager | Artifact | On disk | Build | Cold start | Warm start |
|----------|----------|---------|-------|------------|------------|
| AppImage | 24.4 MiB | 24.4 MiB | 4.9 s | 29 ms | n/a |
| pkg | 70.2 MiB | 70.2 MiB | 1.9 s | 109 ms | n/a |
| daedalus | 44.0 MiB | 122.6 MiB | 1.7 s | 2142 ms | 96 ms |
| Docker | 76.7 MiB | 76.7 MiB | 16.7 s | 6147 ms | n/a |
| Flatpak | not measured | | | | |

daedalus is faster to build than Docker on this pass and faster on the second
launch. AppImage and pkg win the first launch and the on-disk size. The
daedalus footprint includes the extracted rootfs under `~/.cache`. Docker
needs a daemon. This app is the one every packager can build, so it is the
weakest case for daedalus. A comparison that would matter is an app pkg and
AppImage cannot package. That run does not exist yet.

Host profile for the pass: `results/codespaces-ba69ea/profile.txt`.

## How it works

```bash
# 1. On every test machine (laptop, desktop, live USB, server, ARM…)
MACHINE=dev-laptop bash benchmarks/comparison/run.sh
MACHINE=live-usb-8gb bash benchmarks/comparison/run.sh

# 2. Once every machine has been measured, aggregate:
bash benchmarks/comparison/aggregate.sh
```

Each run writes into `results/<machine>/`:

| File          | Content                                                            |
|---------------|--------------------------------------------------------------------|
| `profile.txt` | CPU model, cores, RAM, disk total/free, root device, live-system/container indicator, tool versions |
| `results.tsv` | raw measurements per packager                                      |
| `comparison.md`| readable report with profile + table + methodology                 |

> **Important**: a metric without its machine profile is meaningless. The
> profile records CPU, RAM, disk, root filesystem type, and whether the machine
> is a live system (USB), a container, or bare metal.

## Metrics

- **Artifact** - size of the single distributable file/image.
- **On-disk footprint** - runtime space (daedalus: extracted rootfs cache; Docker:
  uncompressed image; pkg/AppImage: the artifact itself).
- **Cold start** - launch → first HTTP 200 (daedalus includes extraction).
- **Warm start** - second launch, extraction cache (daedalus only; the others
  re-launch every time).
- **Idle RSS** - resident set of the process listening on the port, 1s after the
  first response (via `ss` + VmRSS - the launched PID may not be the server:
  AppImage re-execs a child, daedalus execs).
- **Host deps** - packages/services required on the target machine.

## Reference app

`apps/hello-node/` - zero-dependency Node.js HTTP server, 312 bytes of code.
Chosen so that **all** packagers can embed it (Node is the common denominator
for Docker/pkg/AppImage/daedalus). All use **Node 24** (latest LTS): daedalus embeds
the builder's node (v24.14.0), Docker `node:24-slim`, pkg
`@yao-pkg/pkg@latest --targets node24-linux-x64`.

To compare another app: `bash run.sh <path-to-app>`.

## Known limitations

- **Flatpak** is not measured in this container (requires a Flatpak host +
  OSTree runtimes, long builds).
- **AppImage** runs here with `--appimage-extract-and-run` (no FUSE in the
  container) - on a machine with FUSE, cold start would be closer to the mount
  time.
- Single-run measurements (no N-run average) - reproducible via the script,
  re-run on the target machine.
