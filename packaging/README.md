# Zero-friction installation

One command, no compile, no sudo. A release archive is picked for the host
OS/arch, verified against the release `checksums.txt` (sha256), and unpacked
into a user-local `bin` directory with the stub beside the CLI.

## Installers

- **`install.sh`** (Linux/macOS, POSIX `sh`):

  ```bash
  curl -fsSL https://raw.githubusercontent.com/Encapsul/daedalus/main/install.sh | sh
  ```

  `~/`-install into `$DAEDALUS_INSTALL_DIR` (default `~/.local/bin`) — no root.
  Overrides: `DAEDALUS_VERSION`, `DAEDALUS_INSTALL_DIR`, `DAEDALUS_MIRROR`.

- **`install.ps1`** (Windows PowerShell):

  ```powershell
  powershell -ExecutionPolicy Bypass -c "irm https://raw.githubusercontent.com/Encapsul/daedalus/main/install.ps1 | iex"
  ```

  Installs into `%LOCALAPPDATA%\daedalus\bin` — no admin. Uses the built-in
  `tar` on Windows 10+.

- **Homebrew** — `homebrew/daedalus.rb`. This formula is for a `brew tap`;
  fill the four `sha256` values (see the comment at the top of the formula)
  and add the file to the tap repository. Not yet published.

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

`cargo install` also works (`cargo install --path daedalus-cli`, or via
crates.io when published), but the stub must then be installed separately —
the release archives above are the supported one-command path.