# Building a Deno App

`erebus` supports Deno apps. It detects a Deno project by the presence of
`deno.json` or `deno.jsonc`.

## Detection

| File | Strategy |
|------|----------|
| `deno.json` / `deno.jsonc` | Reads `tasks.start`, `tasks.dev`, or `tasks.default` |
| Fallback | Looks for `main.ts`, `mod.ts`, `index.ts` |

## Prerequisites

- Deno installed (`deno` on PATH), or erebus will download a vendored binary automatically

## Build

```bash
erebus build ./my-deno-app -o my-deno-app.ere
```

The builder:

1. detects the `deno` runtime and reads the entrypoint from config;
2. embeds the `deno` binary (on PATH or vendored);
3. packages the app source into the app layer;
4. compresses and assembles the `.ere`.

## Entrypoint detection

The builder reads `deno.json` in this order:

1. `tasks.start` — production start command
2. `tasks.dev` — development command
3. `tasks.default` — default task

If no tasks are defined, it falls back to common entry files:
`main.ts`, `mod.ts`, `index.ts`.

## Vendored Deno

If Deno is not on your PATH, `erebus` automatically downloads a prebuilt Deno
binary from GitHub Releases and caches it at `~/.cache/erebus/cross/deno/`.

```bash
# No deno on PATH — erebus downloads it automatically
erebus build ./my-deno-app -o my-deno-app.ere
```

## Environment variables

```bash
DENO_ENV=production ./my-deno-app.ere
PORT=8000 ./my-deno-app.ere
```

## Known limitations

- Deno permissions (`--allow-net`, `--allow-read`, etc.) are not yet passed
  through from the build config. The app runs with full permissions inside the
  extracted rootfs.
- Import maps are supported but must be bundled before building.
- Deno deploy targets are not supported (only Linux x86_64/aarch64).
