# Rollback & Resilience (SISR health gate)

> Status: **implemented** (Mission 8). `daedalus-core` `sisr::health` +
> `sisr::resilience`, wired into `stub/src/main.rs`, verified end-to-end by
> `stub/tests/health_rollback.rs`.

The SISR update engine guarantees the binary is never half-updated, but an
*atomically installed* update can still be a **bad** update: it applies cleanly
and then crashes at startup. Mission 8 closes that gap with a supervised
startup check and an automatic, atomic rollback.

## The guarantee

> After any in-place SISR update, the launcher runs the new version under
> supervision. If it crashes or fails during the startup window, the previous
> version is restored atomically and executed instead. A version that fails
> the health check is quarantined and will not be re-installed.

This keeps the invariant *"the running binary is always the last valid
version"* — "valid" now includes *verified to start*.

## Flow

```
./app.daedalus --daedalus-update …   (or $DAEDALUS_SISR_MANIFEST=…)
   │
   1. SISR update requested?   ──no──►  normal run
   2. refuse_quarantined_target?  yes ►  error: update refused (target quarantined)
   3. apply_with_rollback_snapshot:
        a. copy ./app.daedalus → ./app.daedalus.bak        (same filesystem, atomic)
        b. engine apply (reuse/fetch/verify/swap)   (error ⇒ discard .bak, untouched)
        c. health_store.begin(<new-version>)        (state = Pending)
   4. health gate on the new version:
        Pending      ► supervised_launch:
             fork →
                 child: exec new app (or supervise_services)
             parent: wait up to DAEDALUS_HEALTH_TIMEOUT_MS
                 exited 0, or still running  ► confirm + discard .bak → supervise to exit
                 crashed / non-zero         ► record_failure
                                              attempts < max ► run app as-is
                                              attempts ≥ max ► quarantine + rollback
        Quarantined ► rollback (previous binary was still in place or is restored)
   5. normal extraction + exec
```

### Snapshot lifetime

`./app.daedalus.bak` lives **only while the new version is unconfirmed**:

- update applies → snapshot taken **before** the swap, kept until the health
  gate decides;
- healthy → snapshot discarded, `.bak` never shipped as part of the release;
- crashing → snapshot restored (atomic rename back) and discarded.

The snapshot is same-filesystem by construction, so restore is a single atomic
`rename(2)`. Permissions are preserved so the restored binary keeps its
executable bit.

## Health store

Per-version records live in `~/.cache/daedalus/health/<version>.json`
(`XDG_CACHE_HOME`-relative), versioned by the footer content hash
`SHA-256(payload ‖ metadata)`.

| Field | Meaning |
|---|---|
| `state` | `pending` → `healthy` / `quarantined` |
| `attempts` | consecutive installs of this version that reached the health gate |
| `last_checked` / `last_crashed` | RFC 3339 timestamps |

Rules:

- `begin` records an install as `pending` **without resetting the failure
  counter** — repeated installs of the same broken version accumulate.
- `confirm` marks the version healthy (exit 0 or survived the startup window).
- `record_failure` increments `attempts`; the version is quarantined once
  `attempts >= DAEDALUS_HEALTH_MAX_ATTEMPTS`.
- a quarantined version is **never re-armed** by `begin` — it stays
  quarantined until manually cleared by deleting its record.

## Policy knobs (environment)

| Variable | Default | Meaning |
|---|---|---|
| `DAEDALUS_HEALTH_TIMEOUT_MS` | `10000` | startup window the app must survive |
| `DAEDALUS_HEALTH_MAX_ATTEMPTS` | `3` | installs before quarantine (`0` = never quarantine) |

## Failure table

| Situation | Outcome |
|---|---|
| Update applies, new version exits 0 | confirmed healthy, `.bak` discarded, binary kept |
| Update applies, new version crashes / non-zero | recorded; `.bak` restored atomically; previous version runs |
| Same version fails repeatedly | quarantined; log printed |
| Re-install of a quarantined version | refused **before** the snapshot — no swap |
| Backup missing during rollback (deleted externally) | rollback error; new version stays |

The quarantine pre-check runs before the engine does any I/O, so a refused
install is cheap and leaves the running binary untouched.

## Related

- [Runtime Launcher](./runtime-launcher.md) — the launcher flow this gate slots into.
- [SISR: Self-Incremental Sovereign Reconstruction](./sisr-spec.md) — trust model.
- [Incremental Updates (SISR)](../guides/incremental-updates.md) — publisher workflow.
