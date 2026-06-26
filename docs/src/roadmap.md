# Roadmap

## Phase 1 — MVP fonctionnel ✅ (en cours, l'essentiel est fait)

- [x] Format `.xbin` défini et stable (footer 84B versionné)
- [x] Launcher Rust/musl statique : self-read, intégrité SHA-256, exec
- [x] Cache avec extraction atomique (`rename()`)
- [x] Builder Python : détection runtime + `ldd`, construction rootfs, assemblage
- [x] CLI : `build`, `run`, `inspect`, `clean`
- [x] Support Python (stdlib) — exemple `hello-web` fonctionnel
- [x] `flock()` sur le cache pour l'accès concurrent
- [x] `xbin clean`
- [x] Support des `site-packages` / `.venv` Python — exemple `bottle-web`
- [x] **Format v2 en couches + rebuild incrémental** (runtime réutilisé,
      rebuild ~25s → ~1s ; cache de build partagé entre apps)
- [ ] `requirements.txt` → pip install au build (venv temporaire)
- [ ] Support Node.js de bout en bout

## Phase 2 — Robustesse

- [ ] **Signature Ed25519** (footer v2, `xbin keygen` / `sign` / `verify`)
- [ ] **Trust model** : keyring `~/.xbin/trusted-keys/`, niveaux trusted/unknown/unsigned
- [ ] **Isolation niveau 2** : user namespaces + `pivot_root` (portabilité réelle)
- [ ] **Filtre seccomp** minimal
- [ ] **Mode manifest** (`xbin.toml`) pour les dépendances complexes
- [ ] **Analyzer IA** : génération de `xbin.toml` (deps cachées : subprocess, dlopen)
- [ ] Cache LRU (nettoyage au-delà d'un seuil)

## Phase 3 — Produit fini

- [ ] **squashfs + mmap** : lecture directe, plus d'extraction
- [ ] **Cold/warm start < 100 ms** de bout en bout
- [ ] Support de tous les runtimes (Java/GraalVM, Ruby, etc.)
- [ ] Cross-arch (aarch64)
- [ ] Distribution / découverte (registry léger, voire P2P)

## Principe directeur

Chaque phase doit pouvoir s'ajouter **sans réécrire** la précédente. C'est
pourquoi le format est versionné et les couches découplées : passer de
l'extraction tar à squashfs+mmap, ou du niveau 0 au niveau 2, ne change pas le
contrat entre builder et launcher.
