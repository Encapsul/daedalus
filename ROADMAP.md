# Roadmap — daedalus

## P0 — Avant tout : sécurité et crédibilité

| # | Item | Status | Notes |
|---|------|--------|-------|
| 1 | Choisir extension finale `.de` | Done | Remplacé `.de` |
| 2 | Nettoyage HANDOFF.md (réfs Python CLI mortes) | Pending | `cli/daedalus/doctor.py` etc. n'existent plus |
| 3 | Reproducible builds (`SOURCE_DATE_EPOCH`) | Pending | Nécessaire pour audit externe |
| 4 | Fix encryption : supprimer `--encrypt` ou ajouter `--key-file` externe | Pending | La clé dans metadata = fake security |
| 5 | Rendre SISR + encrypt compatibles | Pending | Aujourd'hui mutuellement exclusifs |

## P1 — Avant HN : ce qui bloque la publication

| # | Item | Status | Notes |
|---|------|--------|-------|
| 6 | Tests de portabilité réels (Ubuntu 22.04/24.04, Debian 13, Alpine 3.19, Fedora 40) | Pending | Montrer "tested on X" dans README |
| 7 | Finir layer reuse pour `--update` | Pending | Aujourd'hui fait full rebuild, annoncé comme incremental |
| 8 | Host example binary sur GitHub Releases | Pending | hello-python de ~5MB, vérifiable |
| 9 | Réécrire pitch HN (structure : problème → solution → exemple → caveats) | Pending | Voir `hn-pitch.md` |
| 10 | Décider message positioning : "universal binary packager with self-update" | Pending | Pas "Docker killer", pas "Nix without the learning curve" |

## P2 — Avant HN : ce qui améliore l'impression

| # | Item | Status | Notes |
|---|------|--------|-------|
| 11 | SBOM generation (`daedalus inspect --sbom`) | Pending | Argument sécurité fort |
| 12 | Ajouter checksums + instructions de vérification sur releases | Pending | SHA-256 + Ed25519 verify |
| 13 | Ajouter TTY-aware color handling global | Done | `use_color()` dans doctor.rs |
| 14 | Fix `run` arg forwarding (`trailing_var_arg`) | Done | skip(3) cassé avec flags globaux |
| 15 | Fix `inspect` stdout (output principale) | Done | eprintln → println pour non-JSON |
| 16 | Fix prompts sans TTY check (sign, clean) | Done | is_interactive() avant read_line |
| 17 | Fix `doctor` hardcoded ANSI colors | Done | Respecte NO_COLOR, TERM=dumb, TTY |
| 18 | Fix `build` ignore --quiet pour "Detected runtime" | Done | Guarded par `if verbose` |

## P3 — Post-HN : features et polish

| # | Item | Status | Notes |
|---|------|--------|-------|
| 19 | ARM64 support (stub + runtime builds) | Pending | Stub est architecture-agnostic |
| 20 | macOS/Windows runtime embedding | Pending | Aujourd'hui Linux-first |
| 21 | Sigstore/cosign integration | Pending | Pour signing manifest SISR |
| 22 | Runtime integrity verification (tpm2, IMA) | Pending | |
| 23 | Incremental layer caching (runtime layer reused) | Pending | Le flag `--update` existe, pas la feature |
| 24 | Remote cache avec Depot-style HTTP | Pending | Déjà dans le code, pas documenté |
| 25 | `daedalus help` subcommand (git-like) | Pending | clig.dev compliance |
| 26 | Support `-` pour stdin/stdout | Pending | clig.dev compliance |
| 27 | `--plain` flag pour output tabulaire | Pending | clig.dev compliance |
| 28 | Progress bars pour build/upgrade | Pending | clig.dev nice-to-have |

## P4 — Nice-to-have

| # | Item | Status | Notes |
|---|------|--------|-------|
| 29 | `--no-input` global flag | Pending | clig.dev |
| 30 | `--version` flag dans le binary | Pending | Le stub l'a, le CLI l'a, mais... |
| 31 | Electron ASAR packing + native modules | Pending | Aujourd'hui superficial |
| 32 | WASM runtime bundling (wasmtime embed) | Pending | Aujourd'hui assume sur PATH |
| 33 | Tree-shaking pour Go/Java/.NET | Pending | Aujourd'hui JS/TS only |
| 34 | PHP platform reqs auto-fix (switch pnpm/npm) | Pending | Aujourd'hui warns only |
| 35 | Registry server documentation | Pending | `daedalus serve` existe, pas de docs |

## Décisions en attente

| # | Décision | Options | Recommandation |
|---|----------|---------|----------------|
| 1 | Extension | `.de` vs `.daedalus` vs `.de` | **`.de`** — court, unique, disponible |
| 2 | Encryption | Supprimer vs `--key-file` externe | **Supprimer `--encrypt` du pitch, ajouter `--key-file` en P2** |
| 3 | Positioning | A/B/C/D (voir hn-launch-prep.md) | **B + C** — packaging + self-update |
| 4 | License | MIT vs Apache | MIT pour adoption maximale |
