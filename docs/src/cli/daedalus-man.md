# `daedalus man`

Generate Unix man pages for all daedalus commands.

```bash
daedalus man [OPTIONS] [DIR]
```

Generatesroff man pages for every subcommand and writes them to the specified
directory (default: current directory).

## Options

| Flag | Description |
|---|---|
| `DIR` | Output directory (default `.`) |

## Examples

```bash
# Generate man pages in the current directory
daedalus man

# Generate to a system directory
daedalus man /usr/local/share/man/man1
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Man pages generated successfully |
| `1` | Directory creation or write failure |
