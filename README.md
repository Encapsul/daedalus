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
  <a href="https://tednoob17.github.io/x.bin/"><img src="https://img.shields.io/badge/docs-mdbook-blue" alt="docs"></a>
  <a href="https://github.com/Tednoob17/x.bin/stargazers"><img src="https://img.shields.io/github/stars/Tednoob17/x.bin" alt="stars"></a>
  <a href="https://tednoob17.github.io/x.bin/roadmap.html"><img src="https://img.shields.io/badge/status-MVP-brightgreen" alt="status"></a>
  <a href="https://github.com/Tednoob17/x.bin/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="license"></a>
  <img src="https://img.shields.io/badge/rust-1.80%2B-000000?logo=rust&logoColor=white" alt="rust">
  <img src="https://img.shields.io/badge/python-%3E%3D3.10-3670A0?logo=python&logoColor=ffdd54" alt="python">
</p>

<div align="center">
  <a href="https://tednoob17.github.io/x.bin/">Documentation</a>
  <span>&nbsp;&nbsp;•&nbsp;&nbsp;</span>
  <a href="https://github.com/Tednoob17/x.bin/issues">Issues</a>
  <span>&nbsp;&nbsp;•&nbsp;&nbsp;</span>
  <a href="https://tednoob17.github.io/x.bin/roadmap.html">Roadmap</a>
</div>

<br />

x.bin packages a Python or Node.js web/server/CLI app — code, runtime, shared libraries, and dependencies — into a **single self-extracting ELF executable**. No runtime to install. No Docker. No dependency resolution on the target machine. The file just runs.

- **Single file.** One `.xbin` = launcher + runtime + app + deps. Copy it anywhere.
- **Secure by default.** Ed25519 signatures verified before anything touches disk. SHA-256 integrity check.
- **Fast rebuilds.** Layered format means editing code only recompresses the small app layer — not the runtime.
- **Language-agnostic.** Python, Node.js, or native binary — same CLI, same format.

## Install

Linux x86_64. Kernel 5.6+ recommended. User namespace isolation requires 5.11+.

```sh
git clone https://github.com/Tednoob17/x.bin.git && cd x.bin
make preflight     # verify all prerequisites
make stub          # build Rust launcher + crypto binaries
make install       # pip install --user -e ./cli
export PATH="$HOME/.local/bin:$PATH"  # add xbin to PATH (once per shell)
```

<details>
<summary>Other installation options</summary>

- **pipx** (isolated CLI install)
  ```sh
  pipx install -e ./cli
  ```

- **Upgrade** (from existing install)
  ```sh
  git pull && make stub
  ```

- **Encrypt support** (optional, for `--encrypt` flag)
  ```sh
  pip install -e "./cli[encrypt]"
  ```

</details>

## Quickstart

Build a Python app, run it, inspect it, sign it, verify it:

```bash
$ xbin doctor                         # check prerequisites
  [ ]   Python          3.12.3
  [ ]   cargo           cargo 1.97.1
  ...

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

#### Cross-build for aarch64 (from x86_64)

> **Note**: The Python side (vendored interpreter + target wheels) works automatically.
> The aarch64 stub itself must be pre-built: `rustup target add aarch64-unknown-linux-musl && make stub`.
> CI handles this automatically via GitHub Actions.

```bash
$ xbin build my-app -o my-app-aarch64.xbin --target aarch64
# Downloads vendored Python for aarch64, pip downloads target wheels,
# produces an aarch64 .xbin (requires aarch64 stub pre-built)
```

#### Here is what you can do next:

- [Package a Python app](https://tednoob17.github.io/x.bin/guides/python.html)
- [Package a Node.js app](https://tednoob17.github.io/x.bin/guides/node.html)
- [Sign and verify your builds](https://tednoob17.github.io/x.bin/security.html)
- [Read the full documentation](https://tednoob17.github.io/x.bin/)

## Quick links

- Build
  - [Dockerfile dependency detection](https://tednoob17.github.io/x.bin/guides/dependencies.html)
  - [Python source AST scanning](https://tednoob17.github.io/x.bin/guides/dependencies.html)
  - [Dependency fetcher (pip/npm/apt)](https://tednoob17.github.io/x.bin/guides/dependencies.html)
  - [Incremental rebuilds](https://tednoob17.github.io/x.bin/reference/format.html)
  - [Layered format (v3)](https://tednoob17.github.io/x.bin/reference/format.html)

- Runtime
  - [Python apps](https://tednoob17.github.io/x.bin/guides/python.html)
  - [Node.js apps](https://tednoob17.github.io/x.bin/guides/node.html)
  - [Isolation modes](https://tednoob17.github.io/x.bin/reference/isolation.html)
  - [Shared library resolution](https://tednoob17.github.io/x.bin/reference/launcher.html)
  - [PATH and LD_LIBRARY_PATH injection](https://tednoob17.github.io/x.bin/reference/launcher.html)

- Security
  - [Ed25519 signatures](https://tednoob17.github.io/x.bin/security.html)
  - [Trust model (`~/.xbin/trusted-keys/`)](https://tednoob17.github.io/x.bin/security.html)
  - [SHA-256 integrity verification](https://tednoob17.github.io/x.bin/reference/format.html)
  - [Seccomp syscall filtering](https://tednoob17.github.io/x.bin/reference/isolation.html)

- CLI
  - [`xbin build`](https://tednoob17.github.io/x.bin/reference/builder.html)
  - [`xbin build --target aarch64`](https://tednoob17.github.io/x.bin/reference/builder.html) (cross-compile)
  - [`xbin build --squashfs`](https://tednoob17.github.io/x.bin/reference/builder.html) (SquashFS format)
  - [`xbin inspect`](https://tednoob17.github.io/x.bin/reference/builder.html)
  - [`xbin sign` / `verify`](https://tednoob17.github.io/x.bin/security.html)
  - [`xbin keygen`](https://tednoob17.github.io/x.bin/security.html)
  - [`xbin doctor`](https://tednoob17.github.io/x.bin/guides/quickstart.html) (check prerequisites)
  - [`xbin clean`](https://tednoob17.github.io/x.bin/reference/cache.html)

## Guides

- Python
  - [Package a Python web app](https://tednoob17.github.io/x.bin/guides/python.html)
  - [Dockerfile-based Python apps](https://tednoob17.github.io/x.bin/guides/dependencies.html)
  - [Automatic `requirements.txt` install](https://tednoob17.github.io/x.bin/guides/dependencies.html)

- Node.js
  - [Package a Node.js app](https://tednoob17.github.io/x.bin/guides/node.html)
  - [Automatic `package.json` install](https://tednoob17.github.io/x.bin/guides/dependencies.html)

- Deployment
  - [Single-binary deployment](https://tednoob17.github.io/x.bin/guides/quickstart.html)

- Security
  - [Signing and verification workflow](https://tednoob17.github.io/x.bin/security.html)
  - [Managing trust keys](https://tednoob17.github.io/x.bin/security.html)

## How it works

```
┌──────────────────────────────────────────────────────────────┐
│  my-app.xbin =                                               │
│    [ ELF launcher ][ zstd/squashfs layers ][ metadata ][ footer ]│
│      Rust/musl        runtime + app          JSON      92B   │
│      ~615KB            layers                 entrypoint v5  │
└──────────────────────────────────────────────────────────────┘
```

**At build time**, `xbin build`:
1. Detects the runtime (Python, Node, or native binary)
2. Scans Dockerfile for declared system/pip/npm packages and external binary fetches
3. Resolves shared libraries via a pure-Python ELF analyzer (no host `ldd` needed)
4. Packages interpreter + stdlib + `.so` into a **runtime layer**
5. Packages app code + dependencies into an **app layer**
6. Compresses each layer with `zstd` (default) or `mksquashfs` (`--squashfs`)
7. For cross-builds: downloads vendored Python for target arch, pip downloads target wheels

**At runtime**, the launcher:
1. Opens `/proc/self/exe` (not `argv[0]`)
2. Reads the versioned footer at end-of-file, validates magic
3. If signed: verifies the Ed25519 signature — **before anything touches disk**
4. Verifies SHA-256 integrity of the payload
5. Checks the local cache — extracts if missing (atomic `rename()`)
6. Enters user namespace + `pivot_root` (isolation level 2)
7. Installs seccomp-bpf denylist (blocks dangerous syscalls)
8. `execve()` — replaces itself with the embedded app

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
- [Roadmap](https://tednoob17.github.io/x.bin/roadmap.html) — what's coming next

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.html) for guidelines. PRs welcome.

## License

MIT — see [LICENSE](LICENSE).
