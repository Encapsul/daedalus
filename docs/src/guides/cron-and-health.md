# Cron and health checks

daedalus can run health-checked supervised launches with automatic restart and
rollback.

## Health checks

Configure a health endpoint at build time:

```bash
daedalus build ./my-app -o my-app.daedalus \
  --health-port 8080 \
  --health-endpoint /healthz
```

The launcher probes this endpoint after extraction. If the health check fails,
the binary is quarantined and the previous version is re-executed.

## Cron

Run the app on a schedule:

```bash
daedalus build ./my-app -o my-app.daedalus --cron "0 * * * *"
```

The cron expression follows standard crontab syntax (5 fields: minute, hour,
day-of-month, month, day-of-week).

## Supervised launch

When both `--health-port` and `--cron` are set, daedalus:

1. Extracts the payload to cache.
2. Launches the app.
3. Probes the health endpoint.
4. If healthy, keeps the app running until the next cron tick.
5. If unhealthy, quarantines the current version and rolls back.

## Options

| Flag | Description |
|---|---|
| `--health-port <PORT>` | Port for the health check endpoint |
| `--health-endpoint <PATH>` | Health check path (default `/healthz`) |
| `--cron <EXPR>` | Cron schedule for supervised restarts |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | App ran successfully |
| `1` | Health check failed, quarantine triggered |
