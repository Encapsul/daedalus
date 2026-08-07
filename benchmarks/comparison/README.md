# Cross-packager benchmark — x.bin vs Docker / pkg / AppImage / Flatpak

Compares x.bin against other packagers on the **same reference app** and the
**same machine**, answering the "just use Docker" HN criticism with
reproducible numbers.

## Aggregated results

See [`comparison.md`](comparison.md) (machine × packager table) and
[`machines.md`](machines.md) (full profiles).

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

- **Artifact** — size of the single distributable file/image.
- **On-disk footprint** — runtime space (x.bin: extracted rootfs cache; Docker:
  uncompressed image; pkg/AppImage: the artifact itself).
- **Cold start** — launch → first HTTP 200 (x.bin includes extraction).
- **Warm start** — second launch, extraction cache (x.bin only; the others
  re-launch every time).
- **Idle RSS** — resident set of the process listening on the port, 1s after the
  first response (via `ss` + VmRSS — the launched PID may not be the server:
  AppImage re-execs a child, x.bin execs).
- **Host deps** — packages/services required on the target machine.

## Reference app

`apps/hello-node/` — zero-dependency Node.js HTTP server, 312 bytes of code.
Chosen so that **all** packagers can embed it (Node is the common denominator
for Docker/pkg/AppImage/x.bin). All use **Node 24** (latest LTS): x.bin embeds
the builder's node (v24.14.0), Docker `node:24-slim`, pkg
`@yao-pkg/pkg@latest --targets node24-linux-x64`.

To compare another app: `bash run.sh <path-to-app>`.

## Known limitations

- **Flatpak** is not measured in this container (requires a Flatpak host +
  OSTree runtimes, long builds).
- **AppImage** runs here with `--appimage-extract-and-run` (no FUSE in the
  container) — on a machine with FUSE, cold start would be closer to the mount
  time.
- Single-run measurements (no N-run average) — reproducible via the script,
  re-run on the target machine.
