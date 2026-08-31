# Tree-shaking and minification

daedalus can reduce binary size by tree-shaking unused files and minifying
assets at build time.

## Tree-shaking

Removes unused files from the app directory based on import analysis:

```bash
daedalus build ./my-app -o my-app.daedalus --tree-shake
```

Supported ecosystems:

- **Node.js**: analyzes `require` / `import` statements
- **Python**: analyzes `import` statements
- **Go**: analyzes Go module dependencies

## Minification

Minifies CSS and JavaScript assets:

```bash
daedalus build ./my-app -o my-app.daedalus --minify
```

Supported formats:

- CSS (whitespace collapse, comment removal)
- JavaScript (basic minification)

## Combined

```bash
daedalus build ./my-app -o my-app.daedalus \
  --tree-shake --minify
```

## Options

| Flag | Description |
|---|---|
| `--tree-shake` | Remove unused files from the app directory |
| `--minify` | Minify CSS and JavaScript assets |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Built successfully |
| `1` | Parse error or minification failure |
