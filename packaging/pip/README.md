# daedalus (PyPI)

`pip install daedalux` installs the `daedalus` CLI - it packages any app into
a single self-extracting binary.

```bash
pip install daedalux
daedalus build ./my-app -o my-app.de
./my-app.de
```

The wheel ships a thin launcher: on first run it downloads the matching
release binary from GitHub (SHA-256 verified against the release
`checksums.txt`), caches it under `~/.cache/daedalus/pip/`, and execs it.
Subsequent runs are a cache hit. No compilation, no sudo - the `daedalus-stub`
and `daedalus-crypto` tools ship in the same download, so `daedalus build`
works immediately.

Platforms with release assets: Linux/macOS/Windows × amd64/arm64.
`pip install daedalux --upgrade` releases the launcher; run any `daedalus`
command once to pull a newer binary.