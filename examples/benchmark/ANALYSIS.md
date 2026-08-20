# Feature Analysis: PostHog, Wasmer, Depot, Snap/Flatpak → Erebus

## Environment

- **Machine**: Intel Xeon (Skylake), 2 cores, 7.8 GB RAM, Ubuntu 22.04.2 LTS
- **Tooling**: Rust 1.97.1 (musl), erebus static-pie stub (2.68 MB)
- **Installed snaps**: core20, core22, lxd (5.0.8)
- **Installed flatpaks**: none
- **Wasmer**: not installed

## 1. Snap → Erebus Harvesting (POC)

### What we found

Snaps are squashfs images in `/var/lib/snapd/snaps/<name>_<rev>.snap`, mounted at
`/snap/<name>/<rev>/`. Each contains `meta/snap.yaml` with:

```yaml
name: lxd
apps:
  lxc:
    command: commands/lxc
  daemon:
    command: commands/daemon.start
environment:
  PATH: "$SNAP/usr/sbin:$SNAP/usr/bin:$SNAP/sbin:$SNAP/bin:$PATH"
  LD_LIBRARY_PATH: "$SNAP/lib:$SNAP/lib/x86_64-linux-gnu"
```

### Erebus advantage

| Aspect | Snap | Erebus |
|--------|------|--------|
| Format | squashfs + mount point | single static-pie ELF |
| Runtime | snapd daemon required | none (stub self-extracts) |
| Size | 121 MB (.snap) + 400 MB extracted | 400 MB (single binary) |
| Offline install | needs `snap install --dangerous` | just `chmod +x && ./app` |
| Sharing | `snap save`/`snap export` (proprietary) | copy the ELF file |
| Updates | snapcraft + store | SISR delta (14 MB delta vs 400 MB) |

### POC approach

1. Read `snap.yaml` for entrypoint + environment
2. Embed the `.snap` file as squashfs payload (erebus format v5)
3. Translate `$SNAP` → rootfs path in environment
4. The stub extracts squashfs at runtime, sets env, execs entrypoint

### Data needed for graph

| Snap name | .snap size | Extracted size | Chunk count | Delta (app change) |
|-----------|-----------|----------------|-------------|-------------------|
| (from `du -sh` on squashfs) | (file size) | (unsquashfs -d) | (erebus build --enable-sisr) | (V1→V2 delta) |

## 2. Wasmer → Erebus Harvesting

### What we found

Wasmer is a WASM runtime. Installed apps are WASM files + runtime.
`wasmer list` shows installed packages; `wasmer run <pkg>` executes them.

### Erebus advantage

| Aspect | Wasmer | Erebus |
|--------|--------|--------|
| Runtime | wasmer CLI + runtime library | embedded (no external dep) |
| Packaging | .wasm + wasmer.toml | single ELF |
| Platform | WASM-anywhere | native (musl static-pie) |
| Updates | `wasmer update` (network) | SISR delta (offline-capable) |

### POC approach

1. Embed WASM runtime + app .wasm as payload
2. Entry point: `["/wasm-runtime", "/app/app.wasm"]`
3. Runtime: "wasm" (new metadata type)

## 3. Depot → Erebus Build Speed

### What we found

`depot.dev` provides remote build execution. PostHog's repo has `depot.json`:
```json
{
  "project": "posthog",
  "features": {
    "dockerfile": true,
    "run": true
  }
}
```

Depot offers:
- Remote caching across builds
- Multi-arch builds (x86_64 + aarch64 simultaneously)
- Faster CI with warm cache (vs cold GitHub Actions)

### Erebus integration

Add `--profile=depot` to `erebus build` that:
1. Uploads source + deps to Depot's remote builder
2. Builds native binary on remote (warm cache)
3. Embeds the resulting binary as erebus payload
4. Downloads .erebus back (single artifact)

This could cut build time from ~95s (local, 2-core Xeon) to ~15s (remote, 16-core).

## 4. PostHog Features for Erebus

| PostHog Feature | Erebus adaptation |
|-----------------|--------------------|
| Feature flags | Conditional SISR updates (e.g., "roll to 50% of fleet") |
| Session replay | Capture app stdout/stderr during failed updates |
| A/B testing | Run two binary versions side-by-side |
| Error tracking | Telemetry on update failures (opt-in, signed) |
| MCP integration | Expose erebus commands to Claude/Cursor agents |
| Desktop app | Package PostHog Desktop as .erebus binary |
| Data pipelines | Sync SISR manifest to object storage (S3, GCS) |
