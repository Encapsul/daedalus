# Electron

daedalus can package Electron applications into a single self-extracting binary.

## Detection

daedalus detects Electron apps by:

- Presence of `package.json` with an `electron` dependency
- `main` field pointing to a JavaScript/TypeScript entrypoint
- Presence of `node_modules/.bin/electron` or a downloaded Electron binary

## Build

```bash
daedalus build ./my-electron-app -o my-electron-app.daedalus
```

daedalus will:

1. Install npm dependencies (if `node_modules` is missing).
2. Download the matching Electron binary for the target platform.
3. Embed the Electron binary, app source, and `node_modules` in the payload.
4. Set the entrypoint to `electron <main.js>`.

## Options

| Flag | Description |
|---|---|
| `--no-install` | Skip `npm install` |
| `--target <TRIPLE>` | Cross-compile for a different platform |

## Runtime

At runtime, the launcher extracts the payload and runs:

```
/usr/bin/node /app/node_modules/.bin/electron /app/main.js
```

Or directly with the embedded Electron binary if `--embed-interpreter` is used.

## Limitations

- The Electron binary is large (~100 MB). Consider SquashFS for smaller
  archives: `--squashfs`.
- GPU acceleration is not supported in the sandbox. Use `--gui` for X11/Wayland
  forwarding if needed.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | App exited successfully |
| `1` | Extraction or launch failure |
