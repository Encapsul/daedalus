<p align="center">
  <a href="https://github.com/Tednoob17/x.bin">
    <picture>
      <source srcset="logo-dark.png" media="(prefers-color-scheme: dark)">
      <img width=300 src="logo.png" alt="x.bin logo">
    </picture>
  </a>
</p>
<h1 align="center">x.bin</h1>

<p align="center">
  <a href="https://github.com/Tednoob17/x.bin/blob/main/docs/src/SUMMARY.md"><img src="https://img.shields.io/badge/docs-mdbook-blue" alt="docs"></a>
  <a href="https://github.com/Tednoob17/x.bin/stargazers"><img src="https://img.shields.io/github/stars/Tednoob17/x.bin" alt="stars"></a>
  <a href="https://github.com/Tednoob17/x.bin/blob/main/docs/src/roadmap.md"><img src="https://img.shields.io/badge/status-MVP-brightgreen" alt="status"></a>
  <a href="https://github.com/Tednoob17/x.bin/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="license"></a>
  <img src="https://img.shields.io/badge/rust-1.80%2B-000000?logo=rust&logoColor=white" alt="rust">
  <img src="https://img.shields.io/badge/python-%3E%3D3.10-3670A0?logo=python&logoColor=ffdd54" alt="python">
</p>

<div align="center">
  <a href="https://github.com/Tednoob17/x.bin/blob/main/docs/src/SUMMARY.md">Documentation</a>
  <span>&nbsp;&nbsp;•&nbsp;&nbsp;</span>
  <a href="https://github.com/Tednoob17/x.bin/issues">Issues</a>
  <span>&nbsp;&nbsp;•&nbsp;&nbsp;</span>
  <a href="https://github.com/Tednoob17/x.bin/blob/main/docs/src/roadmap.md">Roadmap</a>
</div>

<br />

x.bin packages a Python or Node.js web/server/CLI app — code, runtime, shared libraries, and dependencies — into a **single self-extracting ELF executable**. No runtime to install. No Docker. No dependency resolution on the target machine. The file just runs.

- **Single file.** One `.xbin` = launcher + runtime + app + deps. Copy it anywhere.
- **Secure by default.** Ed25519 signatures verified before anything touches disk. SHA-256 integrity check.
- **Fast rebuilds.** Layered format means editing code only recompresses the small app layer — not the runtime.
- **Language-agnostic.** Python, Node.js, or native binary — same CLI, same format.

## Install

Linux (x64 & arm64) and macOS (x64 & Apple Silicon).

> **Linux users** — Kernel 5.6+ is recommended. User namespace isolation requires 5.11+.

```sh
git clone https://github.com/Tednoob17/x.bin.git && cd x.bin
make stub
pip install -e ./cli
```

<details>
<summary>Other installation options</summary>

- **Docker** (no host toolchain needed)
  ```sh
  docker build -t xbin .
  ```

- **pipx** (isolated CLI install)
  ```sh
  pipx install -e ./cli
  ```

- **Upgrade** (from existing install)
  ```sh
  git pull && make stub
  ```

</details>

## Quickstart

Build a Python app, run it, inspect it, sign it, verify it:

```bash
$ xbin build examples/hello-web -o hello-web.xbin
[xbin] building 'hello-web'
  runtime: python
  runtime layer: reused from build cache (no recompression) ✓
[xbin] wrote hello-web.xbin (7.1MB, unsigned) in 0.6s

$ chmod +x hello-web.xbin && ./hello-web.xbin
Server listening on http://127.0.0.1:8080

$ xbin inspect hello-web.xbin
  format: v3
  runtime: python 3.12
  layers: 2 (runtime 26.1MB, app 84KB)
  entrypoint: python -m app

$ xbin keygen --key-dir ~/.xbin/keys -q
bf68e4e5471d...

$ xbin sign hello-web.xbin --key ~/.xbin/keys/bf68e4e5.key
[xbin] signed hello-web.xbin

$ xbin verify hello-web.xbin --trusted-dir ~/.xbin/trusted
[xbin] signature verified for hello-web.xbin
```

#### Here is what you can do next:

- [Package a Python app](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/python.md)
- [Package a Node.js app](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/node.md)
- [Sign and verify your builds](https://github.com/Tednoob17/x.bin/blob/main/docs/src/security.md)
- [Read the full documentation](https://github.com/Tednoob17/x.bin/blob/main/docs/src/SUMMARY.md)

## Quick links

- Build
  - [Dockerfile dependency detection](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/dependencies.md)
  - [Python source AST scanning](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/dependencies.md)
  - [Dependency fetcher (pip/npm/apt)](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/dependencies.md)
  - [Manifest mode (`xbin.toml`)](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/builder.md)
  - [Incremental rebuilds](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/format.md)
  - [Layered format (v3)](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/format.md)

- Runtime
  - [Python apps](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/python.md)
  - [Node.js apps](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/node.md)
  - [Isolation modes](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/isolation.md)
  - [Shared library resolution](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/launcher.md)
  - [PATH and LD_LIBRARY_PATH injection](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/launcher.md)

- Security
  - [Ed25519 signatures](https://github.com/Tednoob17/x.bin/blob/main/docs/src/security.md)
  - [Trust model (`~/.xbin/trusted-keys/`)](https://github.com/Tednoob17/x.bin/blob/main/docs/src/security.md)
  - [SHA-256 integrity verification](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/format.md)
  - [Seccomp syscall filtering](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/isolation.md)

- CLI
  - [`xbin build`](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/builder.md)
  - [`xbin run`](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/launcher.md)
  - [`xbin inspect`](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/builder.md)
  - [`xbin sign` / `verify`](https://github.com/Tednoob17/x.bin/blob/main/docs/src/security.md)
  - [`xbin keygen`](https://github.com/Tednoob17/x.bin/blob/main/docs/src/security.md)
  - [`xbin clean`](https://github.com/Tednoob17/x.bin/blob/main/docs/src/reference/cache.md)

## Guides

- Python
  - [Package a Python web app](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/python.md)
  - [Dockerfile-based Python apps](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/dependencies.md)
  - [Automatic `requirements.txt` install](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/dependencies.md)

- Node.js
  - [Package a Node.js app](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/node.md)
  - [Automatic `package.json` install](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/dependencies.md)

- Deployment
  - [Single-binary deployment](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/quickstart.md)
  - [CI/CD integration](https://github.com/Tednoob17/x.bin/blob/main/docs/src/guides/quickstart.md)

- Security
  - [Signing and verification workflow](https://github.com/Tednoob17/x.bin/blob/main/docs/src/security.md)
  - [Managing trust keys](https://github.com/Tednoob17/x.bin/blob/main/docs/src/security.md)

## How it works

```
┌───────────────────────────────────────────────────────────┐
│  my-app.xbin =                                             │
│    [ ELF launcher ][ zstd layers ][ metadata ][ footer ]   │
│      Rust/musl        runtime + app    JSON      92B       │
│      ~615KB            layers          entrypoint  v3      │
└───────────────────────────────────────────────────────────┘
```

**At build time**, `xbin build`:
1. Detects the runtime (Python, Node, or native binary)
2. Scans Dockerfile for declared system/pip/npm packages and external binary fetches
3. Resolves shared libraries via a pure-Python ELF parser (no host `ldd` needed)
4. Packages interpreter + stdlib + `.so` into a **runtime layer**
5. Packages app code + dependencies into an **app layer**
6. Compresses each layer with `zstd`, assembles the `.xbin`

**At runtime**, the launcher:
1. Opens `/proc/self/exe` (not `argv[0]`)
2. Reads the versioned footer at end-of-file, validates magic
3. If signed: verifies the Ed25519 signature — **before anything touches disk**
4. Verifies SHA-256 integrity of the payload
5. Checks the local cache — extracts if missing (atomic `rename()`)
6. `execve()` — replaces itself with the embedded app

## Example apps

| Example | What it demonstrates |
|---|---|
| [`hello-web`](/examples/hello-web) | Python stdlib HTTP server — zero dependencies |
| [`bottle-web`](/examples/bottle-web) | Third-party dependency vendored in `.venv` |
| [`bottle-web-pip`](/examples/bottle-web-pip) | `requirements.txt` installed automatically at build time |
| [`hello-node`](/examples/hello-node) | Same CLI, Node.js runtime |

```sh
make example      # builds hello-web
```

## Community

- [GitHub Issues](https://github.com/Tednoob17/x.bin/issues) — bug reports, feature requests
- [Roadmap](https://github.com/Tednoob17/x.bin/blob/main/docs/src/roadmap.md) — what's coming next

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines. PRs welcome.

## License

MIT — see [LICENSE](LICENSE).
