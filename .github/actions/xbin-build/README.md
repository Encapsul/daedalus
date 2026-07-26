# xbin Build Action

Package any app into a single self-extracting `.xbin` binary directly in your CI/CD pipeline.

## Quick Start

```yaml
- uses: Tednoob17/x.bin/.github/actions/xbin-build@main
  with:
    app-path: '.'

- uses: actions/upload-artifact@v4
  with:
    name: my-app
    path: app.xbin
```

## Usage Examples

### Python app

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: Tednoob17/x.bin/.github/actions/xbin-build@main
        with:
          app-path: '.'
          output: 'my-python-app.xbin'

      - uses: actions/upload-artifact@v4
        with:
          name: my-python-app
          path: my-python-app.xbin
```

### Node.js app with signing

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: Tednoob17/x.bin/.github/actions/xbin-build@main
        with:
          app-path: '.'
          runtime: 'node'
          sign: 'true'
          key: ${{ secrets.XBIN_SIGNING_KEY }}
          output: 'my-node-app.xbin'

      - uses: actions/upload-artifact@v4
        with:
          name: my-node-app
          path: my-node-app.xbin
```

### Multi-platform build

```yaml
jobs:
  build:
    strategy:
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64
          - os: ubuntu-latest
            target: aarch64
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4

      - uses: Tednoob17/x.bin/.github/actions/xbin-build@main
        with:
          app-path: '.'
          target: ${{ matrix.target }}
          output: 'app-${{ matrix.target }}.xbin'

      - uses: actions/upload-artifact@v4
        with:
          name: app-${{ matrix.target }}
          path: app-${{ matrix.target }}.xbin
```

### With extra files

```yaml
- uses: Tednoob17/x.bin/.github/actions/xbin-build@main
  with:
    app-path: '.'
    include: |
      config/
      data/
      migrations/
    output: 'full-app.xbin'
```

### Specific version

```yaml
- uses: Tednoob17/x.bin/.github/actions/xbin-build@main
  with:
    app-path: '.'
    xbin-version: 'v0.3.2'
```

## Inputs

| Input | Description | Default |
|-------|-------------|---------|
| `app-path` | Path to the app directory | `.` |
| `runtime` | Force runtime (python, node, deno, go, ruby, java, php, perl, hugo, binary) | auto-detect |
| `target` | Target architecture (x86_64, aarch64) | `x86_64` |
| `key` | Ed25519 signing key content | — |
| `sign` | Sign the binary | `false` |
| `encrypt` | Encrypt the payload (requires key) | `false` |
| `include` | Extra files/dirs to include (newline-separated) | — |
| `output` | Output binary path | `app.xbin` |
| `xbin-version` | xbin version to install | `latest` |
| `xbin-repo` | GitHub repo to download xbin from | `Tednoob17/x.bin` |
| `build-args` | Extra arguments to pass to xbin build | — |

## Outputs

| Output | Description |
|--------|-------------|
| `binary-path` | Path to the built .xbin binary |
| `runtime` | Detected runtime |
| `size` | Binary size in bytes |

## Signing

To sign your binary, add your Ed25519 private key as a repository secret:

1. Generate a key: `xbin keygen`
2. Add the private key content as a secret named `XBIN_SIGNING_KEY`
3. Set `sign: 'true'` and `key: ${{ secrets.XBIN_SIGNING_KEY }}` in the action

## How It Works

1. Detects the runner OS and architecture
2. Downloads the matching xbin release binary
3. Installs it to PATH
4. Runs `xbin build` with your provided options
5. Outputs the path, detected runtime, and binary size

## Requirements

- Linux (x86_64 or aarch64) or macOS (Intel or Apple Silicon) runner
- `python3` on PATH (needed by the xbin wrapper script)
