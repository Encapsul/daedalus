# `daedalus feedback`

Send feedback or open a GitHub issue from the CLI.

```bash
daedalus feedback [OPTIONS]
```

## Options

| Flag | Description |
|---|---|
| `--browser` | Open the GitHub issue page in the default browser |

## Examples

```bash
# Open feedback form in browser
daedalus feedback --browser
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Feedback submitted or browser opened |
| `1` | Browser open failure |
