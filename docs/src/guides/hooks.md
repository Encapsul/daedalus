# Hooks

daedalus supports pre-execution and post-execution hooks that run inside the
sandbox before and after the app entrypoint.

## Build

```bash
daedalus build ./my-app -o my-app.daedalus \
  --pre-hooks ./hooks/pre \
  --post-hooks ./hooks/post
```

Hooks are embedded in the rootfs and executed with the same environment and
permissions as the app.

## Pre-hooks

Run before the main entrypoint. Useful for:

- Database migrations
- Cache warmup
- Environment validation

## Post-hooks

Run after the main entrypoint exits. Useful for:

- Cleanup tasks
- Log shipping
- Graceful shutdown signaling

## Behavior

- Hooks inherit the sandbox isolation level of the app.
- If a pre-hook fails, the app does not launch.
- Post-hooks run regardless of the app's exit code.
- Hooks are not included in the SISR chunking; they are part of the app layer.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | All hooks and app exited successfully |
| `1` | Pre-hook or app failure |
