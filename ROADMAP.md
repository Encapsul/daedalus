# Roadmap — daedalus

> **Version** : 0.5.0  
> **Status** : Phase 1/2/3 complete — full Rust CLI, SISR delta updates, signing, sandbox isolation  
> **Positioning** : Universal binary packager with self-update

---

## Légende

| Status | Meaning |
|--------|---------|
| ✅ Done | Implémenté et testé |
| 🔄 In Progress | En cours de dev |
| ⏳ Pending | Planifié, pas commencé |
| 🆕 New | Nouvelle feature identifiée via audit concurrents |

---

## Current state

**Core** : format v5 (SquashFS), SISR delta updates (FastCDC + Merkle + Ed25519), Ed25519 signing, AES-256-GCM encryption, atomic extraction, warm-start cache, seccomp + Landlock sandbox.

**CLI** : `build`, `run`, `inspect`, `scan`, `sign`, `verify`, `keygen`, `trust`, `doctor`, `clean`, `dashboard`, `selftest`, `upgrade`, `migrate`, `swap`, `publish`, `registry` (push/pull/list), `serve`, `env`, `feedback`, `completion`, `man`.

**Runtimes** : Python, Node.js, Deno, Java, Ruby, .NET/C#, Go, PHP, Perl, Hugo, Binary (11 total).

**Tests** : 127+ Rust tests (daedalus-core + daedalus-cli + daedalus-stub).

---

## Positioning

**daedalus** packages any web, server, or CLI application into a single self-extracting binary. The `.de` format contains the runtime + dependencies + code, signed Ed25519, with SISR delta updates and sandbox isolation.

**Problem** : deploying an app requires Docker (daemon), a runtime installed (Python, Node, Go, etc.), or a package manager — the installer is often the source of bugs.

**Solution** : one executable file. `cp app.de /target/ && ./app.de` → extraction in 2s, server ready. No runtime to install on the target.

**Why us vs competitors** :

| Aspect | PyInstaller + Bun + pkg | **daedalus** |
|---|---|---|
| Languages | Python, JS/TS (separate tools) | **11 runtimes, one tool** |
| Runtime to install | Yes | **No — embeddable** |
| Delta updates | None | **SISR content-defined delta** |
| Single file | Yes (mono-language) | **Yes — multi-language** |
| Sandbox | None | **seccomp + Landlock** |
| Cross-arch | Rebuild | Cross-compilation |
| Daemon required | No | **No** |

**What we don't do** :
- OCI/AppImage/WASM export
- Marketplace/registry (chicken-and-egg, too early)
- AI agents edge (universal packaging already covers it)

---

## Competitive landscape

| Competitor | What they have | What we take |
|---|---|---|
| **bindboss** | Install wizard, dep auto-check+download, exec vs fork mode, remote update from GitHub | Install wizard, dep checking, exec/fork mode |
| **oci2bin** | SBOM, Cosign/Sigstore, TPM2 secrets, age encryption, digest pinning, SLSA attestations, systemd units, health checks | SBOM, Cosign, age encryption, systemd units |
| **Buncker** | Encrypted delta sync over USB, BIP-39 mnemonic, GC, audit trail | Air-gap docs, USB bundle mode |
| **LlamaFarm** | PyApp/Nuitka binary packaging, edge runtime for Pi/Jetson, bundled binaries in releases | Edge runtime docs, offline bundle mode |
| **Shardline** | Multi-protocol CAS (Xet, LFS, OCI, HF, S3), pluggable auth, provider integration | HF Hub API, OCI frontend (erebus) |
| **ai-model-registry** | Artifact lifecycle states, policy-gated promotion, fail-closed auth, GGUF guard | Model lifecycle, quarantine pipeline (erebus) |
| **Harbor** | RBAC, replication, CVE scanning, webhooks, Helm charts | RBAC, webhooks (erebus) |

---

## Roadmap

### P0 — Security + credibility (blocks HN launch)

| # | Item | Status | Source |
|---|------|--------|--------|
| 1 | Choisir extension `.de` | ✅ Done | — |
| 2 | Nettoyage HANDOFF.md (réfs Python CLI mortes) | ⏳ Pending | — |
| 3 | Reproducible builds (`SOURCE_DATE_EPOCH`) | ⏳ Pending | ROADMAP |
| 4 | Fix encryption : `--key-file` externe | ⏳ Pending | ROADMAP + oci2bin |
| 5 | Rendre SISR + encrypt compatibles | ⏳ Pending | ROADMAP |
| 6 | Sigstore/cosign integration | ⏳ Pending | ROADMAP + oci2bin |
| 7 | SBOM generation (`daedalus inspect --sbom`) | ⏳ Pending | ROADMAP + oci2bin |
| 8 | Tests de portabilité réels (Ubuntu 22.04/24.04, Debian 13, Alpine 3.19, Fedora 40) | ⏳ Pending | ROADMAP |
| 9 | Finir layer reuse pour `--update` | ⏳ Pending | ROADMAP |
| 10 | Host example binary sur GitHub Releases | ⏳ Pending | ROADMAP |
| 11 | Réécrire pitch HN | ⏳ Pending | ROADMAP |
| 12 | Positioning final | ⏳ Pending | ROADMAP |

### P1 — Missing features vs competitors

**Install wizard / dependency checking** (bindboss)

| # | Item | Status |
|---|------|--------|
| 13 | Install wizard (`install.json` / `bindboss.toml`) | 🆕 New |
| 14 | Dependency auto-check + auto-download on first run | 🆕 New |
| 15 | Pre/post hooks (`--pre-hook`, `--post-hook`) | ✅ Done |
| 16 | Exec mode: `exec` (replace process) vs `fork` (wrapper) | 🆕 New |
| 17 | Persistent extraction directory (`--persist`) | ✅ Done |
| 18 | Remote update from GitHub (`--update`) | 🆕 New |

**Security hardening** (oci2bin)

| # | Item | Status |
|---|------|--------|
| 19 | Cosign verification at runtime | 🆕 New |
| 20 | TPM2-sealed secrets | 🆕 New |
| 21 | age encryption (in addition to AES-256-GCM) | 🆕 New |
| 22 | Digest pinning for builds | 🆕 New |
| 23 | SLSA/in-toto attestations | 🆕 New |
| 24 | Seccomp deny-list hardening (already have, document) | 🔄 In Progress |
| 25 | Landlock filesystem sandbox (already have, document) | 🔄 In Progress |

**Runtime features** (oci2bin, LlamaFarm)

| # | Item | Status |
|---|------|--------|
| 26 | Health checks + restart policies | 🆕 New |
| 27 | systemd unit generation | 🆕 New |
| 28 | WASM runtime bundling (wasmtime embed, not just PATH) | ⏳ Pending |
| 29 | Tree-shaking for Go/Java/.NET | ⏳ Pending |
| 30 | PHP platform reqs auto-fix | ⏳ Pending |
| 31 | Cross-compilation (`--cross-compile`) débloqué | ⏳ Pending |
| 32 | macOS/Windows runtime embedding | ⏳ Pending |
| 33 | Multi-arch universal binary | 🆕 New |

**Air-gap / offline** (Buncker, LlamaFarm)

| # | Item | Status |
|---|------|--------|
| 34 | Air-gap install via USB | 🆕 New |
| 35 | Offline bundle mode | 🆕 New |
| 36 | Encrypted delta sync over USB | 🆕 New |
| 37 | BIP-39 mnemonic for key management | 🆕 New |
| 38 | Garbage collection for cache | 🆕 New |

### P2 — CLI polish (clig.dev compliance)

| # | Item | Status |
|---|------|--------|
| 39 | `daedalus help` subcommand (git-like) | ⏳ Pending |
| 40 | Support `-` pour stdin/stdout | ⏳ Pending |
| 41 | `--plain` flag pour output tabulaire | ⏳ Pending |
| 42 | Progress bars pour build/upgrade | ⏳ Pending |
| 43 | `--no-input` global flag | ⏳ Pending |
| 44 | `--version` flag dans le binary | ⏳ Pending |
| 45 | Registry server documentation | ⏳ Pending |

### P3 — Erebus integration (product synergy)

| # | Item | Status |
|---|------|--------|
| 46 | `daedalus push/pull` vers erebus registry | 🆕 New |
| 47 | `daedalus publish` avec provenance attestation | 🆕 New |
| 48 | Layer deduplication via erebus CAS | 🆕 New |
| 49 | Model/runtime co-packaging (daedalus + erebus bundle) | 🆕 New |

---

## Business model

### Phase 0 (0-3 mois) — Consulting + preuve de marché

- **Produit** : CLI open source (gratis) — `daedalus build`, `daedalus run`, `daedalus swap`
- **Service** : Consulting packaging + hardening sécurité
- **Security testing sandbox** : distraire des PoCs/pentest dans un binary signé + sandboxé
- Chaque mission = feedback sur les features enterprise réellement valorisées
- Objectif : 2-3 clients payants → data pour productiser

### Phase 1 (3-9 mois) — Open core

| Tier | Price | Features |
|---|---|---|
| Free | $0 | Local build, single-file, sandbox basique |
| Pro | $7/user/mois | Private registry, SISR, Ed25519 signing |
| Enterprise | Sur mesure | Airgap, AES-256-GCM encryption, Landlock avancé, attestations, CI/CD plugins, support + SLA |

### Phase 2 (9-18 mois) — Scale

- Universal packaging = angle principal
- Marketplace/registry seulement si la phase 1 prouve la demande
- IoT/embedded licensing seulement si le produit est adopté par des vendors

---

## DX review — Implementation tasks

From DX review (score 5/10, TTHW 2 min → target <1 min):

| # | Task | Priority | Effort |
|---|------|----------|--------|
| T1 | Rename `.daedalus` to `.de` across all source files | P1 | ~3h |
| T2 | Remove `--encrypt` from help text and examples | P1 | ~2h |
| T3 | Upgrade error messages with problem + cause + fix pattern | P1 | ~2h |
| T4 | Fix README Security section to remove --encrypt references | P2 | ~1h |
| T5 | Rename `upgrade-binary` to `migrate` for clarity | P2 | ~3h |
| T6 | Add troubleshooting section to README | P2 | ~1h |
| T7 | Add community channel links to README | P3 | ~3h |
| T8 | Add `daedalus feedback` command | P3 | ~2h |

---

## Mission log — Key outcomes

| Mission | Title | Key outcome |
|---|---|---|
| 1-3 | SISR foundations | FastCDC chunker (390 MB/s), CAS trait, assembler trait |
| 4 | SISR spec + header | `SisrFooterExt`, `DeltaManifest`, format v2 |
| 5 | Builder pipeline | `build_artifacts()`, `RemoteManifest`, Ed25519 signing |
| 6 | Runtime engine | `SisrEngine`, `AtomicWriter`, `DAEDALUS_SISR_MANIFEST` env |
| 7 | CLI SISR + auto-update | `--enable-sisr`, `--update-url`, `--daedalus-update` |
| 8 | Rollback + health gate | `.bak` snapshot, `supervised_launch`, quarantine |
| 9 | E2E + fuzzing | Mock HTTP server, 10 E2E tests, proptest, fault injection |
| 10 | v1→v2 migration | `upgrade-binary` command, retrocompat v1 runtime |

---

## Top 10 implementation priority

1. **Fix encryption** (#4) — fake security, must be resolved before HN
2. **SISR + encrypt compatibles** (#5) — blocks secure packaging
3. **SBOM generation** (#7) — strong security argument for HN
4. **Reproducible builds** (#3) — needed for external audit
5. **Install wizard** (#13) — bindboss UX, easy to implement
6. **Air-gap docs + bundle mode** (#34, #35) — edge/air-gap positioning
7. **Cosign/Sigstore** (#6) — supply chain security
8. **Layer reuse for `--update`** (#9) — incremental builds
9. **Portability tests** (#8) — "tested on X" in README
10. **Example binary on GitHub Releases** (#10) — verifiable hello-python

---

## What we don't do

- **AI agents edge** — universal packaging already covers it
- **Marketplace/registry** — chicken-and-egg, too early
- **OCI/AppImage/WASM export** — deprecated
- **Phase 0 consulting** → already done, moving to Phase 1 open core
