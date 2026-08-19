# Self-Incremental Sovereign Reconstruction (SISR)

> **Advanced, opt-in.** SISR is an *extension* of erebus — it is not the core
> value proposition. A default `erebus build` produces a simple, static,
> self-contained binary with **no** reconstruction machinery. SISR is
> documented here so the two models are easy to tell apart.

## The two erebus models

erebus ships in two shapes. They look alike (same `./app.ere`), but only one
can update itself.

| | **Classic .ere** (default) | **SISR .ere** (opt-in) |
|---|---|---|
| Build | `erebus build ./app -o app.ere` | `erebus build ./app -o app.ere --self-update` |
| Contents | launcher + payload (SquashFS) | launcher + payload + **embedded SISR engine** |
| Update | rebuild + redistribute | `./app.ere update` — self-rebuilds from signed deltas |
| Toolchain on target | none | **still none** (engine is embedded) |
| Behavior | static container | static container by default, updatable on demand |
| Trust | signature at launch | signature at launch **+** signed chain for every delta |

The single most important property: **both shapes run identically by
default.** SISR is dormant until the user asks for an update. Turning it on
never changes what a normal launch does — the embedded engine is only invoked
explicitly (invariant I-2 of the [SISR spec](../architecture/sisr-spec.md)).

## Why SISR exists

A classic `.ere` is immutable. Shipping a fix means:

```
[dev machine] erebus build  →  [v1.1 .ere]  →  redistribute  →  [target]
```

For a handful of servers that's acceptable. For fleets behind slow links, or
for security patches that must land *fast*, redistributing the whole binary
each time is wasteful. SISR moves the last step of that pipeline onto the
target machine itself:

- the binary **fetches** only the changed bytes (deltas / chunks),
- it **verifies** them against a signed chain of trust,
- it **reassembles** itself into the next version, atomically.

## The reconstruction flow

```
[ Binary v1.0 ] ──( ./app.ere update )──▶ [ Interrogate remote manifest ]
                                                │
                                                ▼
                                       [ Download deltas / chunks ]
                                                │
                                                ▼
                                   [ Validate Ed25519 signatures + hashes ]
                                                │
                                                ▼
[ Binary v1.1 ] ◀──( Rebuild & atomic swap )───┘
```

At every arrow, nothing is trusted until verified:

1. **Interrogate** — the binary fetches the remote manifest for its channel.
2. **Download** — it pulls only the blocks that changed since `v1.0`
   (content-defined chunks, see the [delta manifest spec](../spec/delta-manifest-format.md)).
3. **Validate** — every block is checked against its SHA-256 content hash and
   the whole update against the signed manifest (Ed25519 chain of trust,
   anti-rollback index).
4. **Rebuild & swap** — the new binary is assembled in a staging location,
   verified end-to-end, then committed atomically. An interruption leaves
   `v1.0` intact and running.

## Three responsibilities, one file

The flow above is driven by the **embedded SISR engine**, a dormant third
responsibility inside the binary (next to the launcher and the payload):

```
+-----------------------------------------------------------------+
|                         APP.ERE (ELF)                          |
+-----------------------------------------------------------------+
|  1. Entrypoint Launcher  --> Bootstrap + runtime isolation       |
+-----------------------------------------------------------------+
|  2. Embedded SISR Engine --> Verify, fetch & assemble (dormant)  |
+-----------------------------------------------------------------+
|  3. Payload (SquashFS)   --> Application code & assets           |
+-----------------------------------------------------------------+
```

- **Launcher** — unchanged. Reads the footer, verifies integrity, extracts,
  execs.
- **SISR Engine** — only engaged by an explicit update trigger. Runs
  in-process, never shells out, never listens on a port.
- **Payload** — the SquashFS image the engine reconstructs block by block.

## What a developer needs to remember

- SISR is **opt-in at build time** and **explicit at run time**.
- The target machine never needs the `erebus` CLI, a compiler, or a system
  runtime — the engine is compiled statically into the binary.
- Every update is **signed, monotonic (anti-rollback), and atomic**. An
  unsigned, replayed, or tampered update is rejected before any byte is
  written.
- The cryptographic guarantees are identical to the ones erebus already
  provides at launch — SISR extends the *same* trust chain to updates, it
  does not create a weaker one.

## Where to go next

- [Delta manifest format](../spec/delta-manifest-format.md) — how an update
  is described and verified.
- [Incremental updates guide](../guides/incremental-updates.md) — how to
  build, sign, and deploy an updatable binary.
- [SISR conceptual specification](../architecture/sisr-spec.md) — the
  invariants and trust model behind this extension.
