# daedalus Build Action

Package any app into a single self-extracting `.daedalus` binary directly in your CI/CD pipeline.

## Quick Start

```yaml
- uses: Tednoob17/daedalus/.github/actions/daedalus-build@main
  with:
    app-path: '.'

- uses: actions/upload-artifact@v4
  with:
    name: my-app
    path: app.daedalus
```

## Usage Examples

### Python app

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: Tednoob17/daedalus/.github/actions/daedalus-build@main
        with:
          app-path: '.'
          output: 'my-python-app.daedalus'

      - uses: actions/upload-artifact@v4
        with:
          name: my-python-app
          path: my-python-app.daedalus
```

### Node.js app with signing

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: Tednoob17/daedalus/.github/actions/daedalus-build@main
        with:
          app-path: '.'
          runtime: 'node'
          sign: 'true'
          key: ${{ secrets.DAEDALUS_SIGNING_KEY }}
          output: 'my-node-app.daedalus'

      - uses: actions/upload-artifact@v4
        with:
          name: my-node-app
          path: my-node-app.daedalus
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

      - uses: Tednoob17/daedalus/.github/actions/daedalus-build@main
        with:
          app-path: '.'
          target: ${{ matrix.target }}
          output: 'app-${{ matrix.target }}.daedalus'

      - uses: actions/upload-artifact@v4
        with:
          name: app-${{ matrix.target }}
          path: app-${{ matrix.target }}.daedalus
```

### With extra files

```yaml
- uses: Tednoob17/daedalus/.github/actions/daedalus-build@main
  with:
    app-path: '.'
    include: |
      config/
      data/
      migrations/
    output: 'full-app.daedalus'
```

### Specific version

```yaml
- uses: Tednoob17/daedalus/.github/actions/daedalus-build@main
  with:
    app-path: '.'
    daedalus-version: 'v0.5.0'
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
| `output` | Output binary path | `app.daedalus` |
| `daedalus-version` | daedalus version to install | `latest` |
| `daedalus-repo` | GitHub repo to download daedalus from | `Tednoob17/daedalus` |
| `build-args` | Extra arguments to pass to daedalus build | — |

## Outputs

| Output | Description |
|--------|-------------|
| `binary-path` | Path to the built .daedalus binary |
| `runtime` | Detected runtime |
| `size` | Binary size in bytes |

## Signing

To sign your binary, add your Ed25519 private key as a repository secret:

1. Generate a key: `daedalus keygen`
2. Add the private key content as a secret named `DAEDALUS_SIGNING_KEY`
3. Set `sign: 'true'` and `key: ${{ secrets.DAEDALUS_SIGNING_KEY }}` in the action

## How It Works

1. Detects the runner OS and architecture
2. Downloads the matching daedalus release binary
3. Installs it to PATH
4. Runs `daedalus build` with your provided options
5. Outputs the path, detected runtime, and binary size

## Requirements

- Linux (x86_64 or aarch64) or macOS (Intel or Apple Silicon) runner
