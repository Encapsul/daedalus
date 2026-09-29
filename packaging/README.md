# Zero-friction installation

One command, no compile, no sudo. A release archive is picked for the host
OS/arch, verified against the release `checksums.txt` (sha256), and unpacked
into a user-local `bin` directory with the stub beside the CLI.

## Installers

- **`install.sh`** (Linux/macOS, POSIX `sh`):

  ```bash
  curl -fsSL https://raw.githubusercontent.com/Encapsul/daedalus/main/install.sh | sh
  ```

  `~/`-install into `$DAEDALUS_INSTALL_DIR` (default `~/.local/bin`) - no root.
  Overrides: `DAEDALUS_VERSION`, `DAEDALUS_INSTALL_DIR`, `DAEDALUS_MIRROR`.

- **`install.ps1`** (Windows PowerShell):

  ```powershell
  powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/Encapsul/daedalus/main/install.ps1 | iex"
  ```

  Installs into `%LOCALAPPDATA%\daedalus\bin` - no admin. Uses the built-in
  `tar` on Windows 10+.

- **Homebrew** - tap `Encapsul/homebrew-daedalus`:

  ```bash
  brew install Encapsul/homebrew-daedalus/daedalus
  ```

  Installs the signed release binary (`daedalus` + `daedalus-stub` +
  `daedalus-crypto`) from the release archives. The formula lives in
  `homebrew/daedalus.rb` here and is mirrored in the tap repository.

- **PyPI** - `pip install daedalus` (`pip/`). Installs a thin launcher that
  downloads the matching release binary on first run (SHA-256 verified
  against `checksums.txt`), caches it under `~/.cache/daedalus/pip/`, and
  execs it; strict stdlib, no compilation, no sudo.

## Release assets

`release.yml` publishes one archive per platform on every `v*` tag:

```
daedalus_<version>_linux_amd64.tar.gz      daedalus + daedalus-stub [+ crypto]
daedalus_<version>_linux_arm64.tar.gz
daedalus_<version>_darwin_amd64.tar.gz
daedalus_<version>_darwin_arm64.tar.gz
daedalus_<version>_windows_amd64.tar.gz   *.exe
checksums.txt
```

`cargo install daedalux` installs the CLI from crates.io. The stub is not
bundled with the crate - on first `daedalus build` the CLI auto-downloads the
matching `daedalus-stub` from the latest release archive after verifying its
SHA-256 against `checksums.txt` (see `daedalus-cli/src/release.rs`). The
release archives above are the supported one-command path.