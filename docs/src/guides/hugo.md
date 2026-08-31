# Hugo

daedalus can package Hugo static sites into a self-contained binary that builds
and serves the site.

## Detection

daedalus detects Hugo apps by:

- Presence of `hugo.toml` / `hugo.yaml` / `config.toml` in the app directory
- Presence of `content/`, `layouts/`, or `static/` directories

## Build

```bash
daedalus build ./my-hugo-site -o my-hugo-site.daedalus
```

daedalus will:

1. Download the specified Hugo version (or use the system `hugo` binary).
2. Embed the Hugo binary, config, and site source in the payload.
3. Set the entrypoint to `hugo server --bind 0.0.0.0`.

## Options

| Flag | Description |
|---|---|
| `--no-install` | Skip Hugo binary download |
| `--target <TRIPLE>` | Cross-compile for a different platform |

## Runtime

```bash
./my-hugo-site.daedalus
# Server listening on http://127.0.0.1:8080
```

The launcher runs `hugo server` with the embedded binary and source.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Server exited successfully |
| `1` | Extraction or Hugo build failure |
