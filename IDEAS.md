# IDEES — Compilation complete

## 1. Position A : Format artefact universel (DEAD CODE)
**Description** : SISR comme moat + layer graph (RuntimeLayer, AppLayer, ModelLayer, ToolLayer, ConfigLayer) → un format `.ere` extensible a tout type d'artefact

### Code analysis findings
- `layer.rs` = DEAD CODE : `Layer` trait, `SerializableLayer`, `Capability`, `Entrypoint` — exists mais jamais branches dans le pipeline production
- `ArtifactMetadata` = DEAD CODE : importe seulement par `metadata.rs` tests, jamais par pipeline
- `Cas.rs` (CAS trait) = DEAD CODE : defini mais pas utilise par le stub/runtime
- `Metadata { runtime: String, entrypoint: Vec<String>, services }` = format PLAT, utilisé par vrai pipeline

### Status du code (verifié)
- `layer.rs` : 100 matches grep, mais imports only in `metadata.rs` tests (lignes 7, 358, 393)
- `ArtifactMetadata` : zero imports en dehors de `metadata.rs` tests
- `CAS trait` : defini mais jamais importe par le stub ou CLI production path
- Stub et CLI utilisent `build_meta_json()` → `AssemblyInput` → `assemble_erebus()` — format plat
- `cargo check` + `cargo clippy` + `cargo test` tous passes, zéro warnings

## 2. Position B : Compilateur vers formats existants (--to flag)
**Description** : `erebus build . --to [oci|appimage|wasm]` → single build, sortie multiple formats

### Formats cibles
- **OCI/Docker** : stub → Dockerfile equivalent, payload squashfs → couche OCI
- **AppImage** : stub .erebus (ELF statique, musl) → AppImage runtime, payload squashfs → AppDir  
- **WASM** : runtime → WASM via Pyodide/Node WASM, payload → WASI preopen

### Usage
```bash
# Multi-format : un seul build → 3 formats
erebus build ./myapp --to oci,appimage,wasm --target linux-x64,linux-arm64 -o out/

# SISR cross-format : meme chunk engine pour tous
erebus build ./myapp --to oci,appimage,wasm --enable-sisr --update-url https://updates.example.com
```

### Valeur unique
- SISR qui traverse tous les formats (OCI + AppImage + WASM)
- Build once, deploy everywhere
- Pas d'outil existant ne fait ça (voir analyse concurrentielle)

## 3. Analyse concurrentielle (recherche web 2025-2026)

### Tools existants
| Outil | Format | SISR | SaaS | Business model |
|---|---|---|---|---|
| **Docker/BuildKit** | OCI only | zstd:chunked (partial) | ❌ local | Infra existante |
| **AppImageTool** | AppImage only | zsync delta | ❌ local | Communauté |
| **Wasmer + WAPM** | WASM only | WASM layer cache | ✅ (Wasmer Edge) | SaaS + OSS |
| **Nix/Flox/Determinate** | Multiple | Narhash deltas | ✅ (Flox Teams) | SaaS Enterprise |
| **Bazel remote cache** | Multiple | Remote cache | ❌ local | Google |
| **pkgforge/soar** | Package manager | zsync deltas | ❌ | OSS |
| **ocx** | OCI only | Content-addressed | ❌ | OSS |
| **Afterburner** | WASM | N/A | ✅ planned | WASM runtime |
| **adipo** | Fat binaries | N/A | ❌ | OSS |
| **Depot (YC W22)** | OCI only | Layer cache NVMe | ✅ SaaS | $15M+ ARR |
| **D4C/diffah** | OCI deltas | layer-level deltas | ❌ | OSS |
| **chunkah** | OCI content layers | N/A | ❌ | OSS |

### Le GAP (rien n'existe)
**Personne ne fait** : build-once → OCI + AppImage + WASM + SISR cross-format

- Docker/BuildKit → OCI only
- AppImageTool → AppImage only  
- Wasmer/WAPM → WASM only
- Nix → peut faire multi mais complexité Nix lang + pas SISR cross-format
- D4C/diffah → delta updates OCI only
- Depot → remote build acceleration, OCI only

### Depot comparison (YC W22, valué $1.5B+)
**Positionnement** : "40x faster Docker builds" via remote BuildKit VMs (16 CPU, 32GB RAM, NVMe cache)

**Business model** : SaaS subscription
**Clients** : PostHog, Wistia, Appsmith, Secoda
**ARR** : $15M+ est.

**xbin vs Depot** :
| Aspect | Depot | xbin |
|---|---|---|
| Probleme | Docker build lent | Deploy multi-format avec updates minces |
| Solution | Remote BuildKit VMs | OSS tool + multi-format + SISR |
| Formats | OCI only | OCI + AppImage + WASM + .erebus |
| Model | SaaS only | OSS tool + SaaS registry |
| SISR | Layer cache NVMe | Chunk-level delta across formats |
| Deploy | Cloud/SaaS | Local tool, self-contained |

**Conclusion** : complementaire, pas concurrent. Depot = build fast. xbin = deploy smart.

## 4. Angle MLOps (recherche validee)

### Le probleme
- Un modele IA fait 5-50 GB
- Besoin de deployer sur :
  - Cloud GPU (OCI containers)
  - Laptop (WASM inference)
  - Edge device (native binary)

### xbin value proposition
```bash
erebus build ./ai-model --to oci,wasm,app --target linux-x64,aarch64
# → OCI image for cloud, WASM module for browser, native binary for edge
# → next update: SISR cross-format, ne telecharge que ce qui a change
```

### Concurrent
- Docker : trop lourd pour edge/WASM
- AppImage : pas fait pour gros fichiers (50GB)
- PyInstaller : mono-language, pas de container/WASM

### Business model
- OSS tool gratis
- SaaS registry pour versioning de modeles + SISR
- Target : teams MLOps, ML startups

## 5. Angle Edge/IoT OTA

### Problem
- Bandwidth cellulaire $$$ pour fleets
- Devices heterogenes (ARM/x86/WASM)
- Updates doivent etre deltas

### xbin value proposition
- SISR cross-format + signatures Ed25519 + single-file
- Un artefact → tous device types

### Concurrent
- Mender, Foundries.io, Esolutions (embedded Linux OTA)
- Aucun ne gere WASM + native + container ensemble

### Business model
- SaaS OTA platform ($0.10/device/mois)
- Enterprise licensing

## 6. Business Models discutes

### OSS + SaaS (recommande)
```
erebus build . -o app.ere     # OSS gratis
erebus push registry/app:v1   # SaaS registry
```

### Tiered pricing
| Tier | Price | Features |
|---|---|---|
| Free | $0 | Local build, single-file |
| Pro | $7/user/mois | Private registry, SISR |
| Team | $29/team/mois | SSO, collaboration, analytics |
| Enterprise | Custom | Airgap, compliance, support |

### Alternative pricing
- Per-build : $0.01/build (GitHub Actions style)
- Per-device : $0.10/device/mois (IoT fleet)

## 7. Recommendation finale

### Angle principal : "Bun for polyglot apps"  
**Positionnement** : xbin = Bun mais pour tout langage. Build n'importe quelle stack, package en single file, run partout.

### Angle secondaire : MLOps/Edge
**Valeur differentielle** : SISR cross-format pour gros artefacts (modeles IA 5-50GB)

### Pitch YC
> "Erebus is the universal application packager — build once, deploy to containers, laptops, web, and edge as a single self-extracting file. While Docker needs a daemon and Depot only does containers, Erebus packages any runtime into one file that runs anywhere, with 95% smaller updates via SISR."

### Business model gagnant
- **Produit** : OSS tool (gratis, dev adoption)
- **Monetisation** : SaaS registry (SISR + signatures + private repos)
- **Target ARR** : $20M/year
  - 10K devs pro (Team plan $29) = $3.5M ARR
  - 100 MLOps/Edge startups = $15M ARR
  - 5 Enterprise deals (custom $1M) = $5M ARR

### Roadmap priorite
1. **Phase 1** : Completer `--to [oci|appimage|wasm]` (multi-format output)
2. **Phase 2** : SaaS registry publique (push/pull/sign + SISR)
3. **Phase 3** : Enterprise features (airgap, compliance, custom runtimes)

### Competitive moat
1. **SISR cross-format** — technologiquement unique
2. **Single-file self-extracting** — plus simple que Docker
3. **Multi-runtime** — pas besoin d'installer Python/Node/Go
4. **Open format** — interoperability (peut decompiler avec tar/unsquashfs)
```
