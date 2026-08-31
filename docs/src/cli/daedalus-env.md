# `daedalus env`

Show daedalus environment information.

```bash
daedalus env
```

Prints the runtime environment: OS, architecture, Rust version, available
runtimes, cache directory, and stub path.

## Example output

```
daedalus env
  version:     0.6.1
  os:          linux
  arch:        x86_64
  rustc:       1.98.0
  cache_dir:   /home/user/.cache/daedalus
  stub:        /usr/local/bin/daedalus-stub
  runtimes:
    python:    /usr/bin/python3.12
    node:      /usr/bin/node
    go:        /usr/local/go/bin/go
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Printed successfully |
| `1` | Unexpected error |
