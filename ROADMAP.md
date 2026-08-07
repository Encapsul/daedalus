# x.bin Roadmap: Features & Limitations

## Vision

x.bin transforme n'importe quelle application en un binaire ELF auto-extractible unique. Ce document liste les limitations actuelles et les features à implémenter pour en faire un outil de production à long terme.

---

## État Actuel — Todo List (session ses_04c7)

### ✅ Terminé

- [x] RUN 1.1–1.5 + RUN 2.1–2.5 + RUN 3.1–3.6
- [x] atty removal, commande `publish`, refactor cache build, fix ldd musl, permissions vfat
- [x] EDGE EMBED : interpréteur statique (géré par `ldd_deps` → vec vide)
- [x] EDGE BUILD : permissions vfat préservées
- [x] EDGE BUILD : noms de fichiers non-UTF8 (via `to_string_lossy`)
- [x] EDGE PHP : détection Laravel Octane (RoadRunner) + embed `rr`
- [x] EDGE GO : app CGO — embed libc `.so` (via `ldd_deps`)
- [x] EDGE RUBY : gems natives (nokogiri, mysql2) — embed via scan `ldd`
- [x] EDGE DEPS : pip retry/timeout pour résilience proxy
- [x] EDGE NODE : skip `.pnpm store` (éviter la duplication)
- [x] EDGE EMBED : multiples versions PHP → contraintes de version dans `check_php_platform_reqs`
- [x] Fix sécurité : cron `@every` invalide → 3600s au lieu de 0 (plus de boucle CPU)
- [x] Fix sécurité : niveau de compression par défaut 3 au lieu de 19 (`python.rs`)

### ⏳ En cours / À faire

- [ ] **EDGE NODE : détection apps desktop Electron** (haute priorité, sécurité)
      → ajouter `Runtime::Electron` dans `detect.rs`, mapping `EmbeddedInterpreter`, entrypoint `electron`, gestion `--ignore-scripts`
- [ ] **RUN 2.6 : cache build distant (style Depot)** — design/architecture terminés, **implémentation dans `paths.rs` non aboutie** (échec d'édition, JSON "Unterminated string")
- [ ] **RUN 4.1–4.6** : sandboxing + protection contre l'évasion de conteneur
- [ ] **RUN 5.1–5.6** : support runtime WebAssembly + edge cases
- [ ] **DESIGN : supprimer `docs/planning/`** (toujours présent : HANDOFF.md, xbin-project.md, .excalidraw, .pdf)
- [ ] **Zeroization des clés crypto** : `encrypt.rs`, `keygen.rs`, `sign.rs` — ajouter le crate `zeroize`, wrapper `Zeroizing<T>` (audit : HIGH-2)
- [ ] **Symlinks** : ROADMAP.md, CODE_STYLE.md, RULES.md, CLAUDE.md, AGENTS.md → copies hors repo + symlinks à l'intérieur

### 🔒 Correctifs sécurité restants (audit)

- [ ] CRITICAL : `isolation.parse().unwrap_or(1)` — rejeter les valeurs invalides au lieu de fail-open (build.rs)
- [ ] HIGH : temp dir prévisible `/tmp/xbin-build-tools/node` → `tempfile::tempdir()`
- [ ] HIGH : injection PATH via `set_var("PATH", ...)` → `Command::env()`
- [ ] MEDIUM : `human_panic` — masquer file/line dans les messages de panique
- [ ] MEDIUM : salt/info HKDF fixes → sel aléatoire par chiffrement
- [ ] MEDIUM : `DEFAULT_REGISTRY` placeholder `xbin.example.com` → config obligatoire

---

## Limitations Actuelles

### Blocants Critiques

#### 1. Linux-only (format ELF)
**Problème** : x.bin ne produit que des binaires ELF. Impossible de lancer sur macOS (Mach-O) ou Windows (PE).

**Impact** :
- Pas de support développeurs macOS
- Pas de support déploiement Windows
- Pas de cross-architecture (ARM ↔ x86)
- Limité aux serveurs Linux uniquement

**Comparaison** :
- Docker : supporte Linux, macOS, Windows
- Wasmer : supporte toutes les plateformes via Wasm
- Bun : supporte Linux, macOS, Windows

---

#### 2. Pas de mécanisme de mise à jour
**Problème** : Une fois construit, le binaire est immuable. Pas de delta updates, pas de patching binaire.

**Impact** :
- Chaque update = rebuild complet + redistribution
- Pas de mise à jour sécutive possible
- Pas de correction de vulnérabilités sans rebuild

**Comparaison** :
- Docker : `docker pull` pour les mises à jour
- AppImage : auto-update intégré
- Snap : mises à jour automatiques

---

#### 3. Pas de sandboxing
**Problème** : L'application tourne avec les permissions utilisateur complètes. Pas de seccomp, pas de capabilities Linux, pas de Landlock.

**Impact** :
- Sécurité limitée en production
- Pas d'isolation entre applications
- Risque d'exploitation de vulnérabilités

**Comparaison** :
- Docker : namespaces + cgroups + seccomp
- Wasmer : sandboxing Wasm par défaut
- Snap : confinement AppArmor

---

#### 4. Pas de configuration runtime
**Problème** : La configuration = variables d'environnement au build. Pas d'injection de config au runtime.

**Impact** :
- Rebuild pour changer une config
- Pas de config par environnement (dev/staging/prod)
- Pas de hot-reload

**Comparaison** :
- Docker : `-e` et `-v` pour l'injection
- Kubernetes : ConfigMaps et Secrets
- Bun : variables d'environnement au runtime

---

#### 5. Pas de gestion des secrets
**Problème** : Pas de vault, pas d'encryption des secrets. Secrets dans l'environnement = risque de leakage.

**Impact** :
- Secrets exposés dans les variables d'environnement
- Pas d'intégration avec les vaults existants
- Risque de secrets dans les logs

**Comparaison** :
- Docker : Docker secrets, HashiCorp Vault
- Kubernetes : Secrets natifs
- AWS : Secrets Manager

---

#### 6. Pas de stockage persistant
**Problème** : Cache = `~/.cache/xbin/<hash>/rootfs/`. Pas de volumes, pas de persistence entre les runs.

**Impact** :
- État perdu à chaque exécution
- Pas de bases de données persistantes
- Pas de fichiers de configuration persistants

**Comparaison** :
- Docker : volumes et bind mounts
- Kubernetes : PersistentVolumes
- LXC : stockage persistant

---

#### 7. Pas d'observabilité
**Problème** : Pas de logging intégré, pas de métriques, pas de tracing.

**Impact** :
- Difficile à monitorer en production
- Pas de debugging facilité
- Pas d'intégration avec les outils de monitoring

**Comparaison** :
- Docker : logging drivers, métriques
- Kubernetes : stdout/stderr → collecteurs
- Bun : console.log structuré

---

#### 8. Pas d'isolation réseau
**Problème** : Pas de namespaces réseau, pas de proxy, pas de load balancing.

**Impact** :
- Sécurité réseau limitée
- Pas d'isolation entre services
- Pas de control du trafic

**Comparaison** :
- Docker : network namespaces, overlay networks
- Kubernetes : NetworkPolicies
- Istio : service mesh

---

### Limitations Importantes

#### 9. Pas de limites de ressources
**Problème** : Pas de cgroups pour CPU/mémoire/PID.

**Impact** : Une application peut consommer toutes les ressources du système.

#### 10. Pas de health checks
**Problème** : Pas de mécanisme de vérification de santé intégré.

**Impact** : Difficile de savoir si l'application fonctionne correctement.

#### 11. Pas de layer caching
**Problème** : Les dépendances sont rebuild à chaque fois.

**Impact** : Builds lents pour les grandes applications.

#### 12. Pas de builds reproductibles
**Problème** : Le processus de build n'est pas déterministe.

**Impact** : Même source → binaires différents.

#### 13. Pas de mécanisme de rollback
**Problème** : Impossible de revenir à une version précédente.

**Impact** : Pas de récupération après une mise à jour ratée.

#### 14. Pas de garbage collection
**Problème** : Les anciennes versions s'accumulent.

**Impact** : Espace disque gaspillé.

#### 15. Pas de support WebAssembly
**Problème** : Limité aux binaires natifs.

**Impact** : Pas de portabilité universelle.

---

### Limitations Mineures

#### 16. Pas de package registry
**Problème** : Pas de dépôt central pour la distribution.

#### 17. Pas d'intégration desktop
**Problème** : Pas de fichiers .desktop/icônes pour les apps GUI.

#### 18. Pas d'auto-update
**Problème** : Pas de mise à jour automatique des binaires.

#### 19. Pas d'orchestration multi-conteneurs
**Problème** : Support service unique uniquement.

---

## Features à Implémenter

### Priorité 1 : Critique (Haute)

#### 1. Support Cross-Platform
**Objectif** : Supporter macOS (Mach-O) et Windows (PE)

**Pourquoi c'est important** :
- Élargir l'audience aux développeurs macOS
- Permettre le déploiement Windows
- Rendre x.bin universel

**Comment** :
- Abstraire le code spécifique ELF dans le stub
- Créer des loaders spécifiques à chaque plateforme
- Utiliser la compilation conditionnelle Rust
- Tester sur toutes les plateformes

**Complexité** : 3-4 semaines

---

#### 2. Mises à Jour Delta
**Objectif** : Patching binaire pour éviter les rebuilds complets

**Pourquoi c'est important** :
- Réduire le temps de mise à jour
- Économiser la bande passante
- Permettre les mises à jour sécures

**Comment** :
- Utiliser bsdiff/bspatch pour le diffing binaire
- Stocker les patches alongside les binaires
- Implémenter l'application de patches dans le stub
- Ajouter les métadonnées de version au footer

**Complexité** : 2-3 semaines

---

#### 3. Injection de Configuration Runtime
**Objectif** : Injecter la config sans rebuild

**Pourquoi c'est important** :
- Config par environnement (dev/staging/prod)
- Hot-reload sans redémarrage
- Séparation code/config

**Comment** :
- Supporter les fichiers de config dans /etc/xbin/ ou ~/.config/xbin/
- Override par variables d'environnement au runtime
- Embedding de config avec lazy loading
- Support hot-reload

**Complexité** : 1-2 semaines

---

#### 4. Gestion des Secrets
**Objectif** : Gestion sécurisée des secrets

**Pourquoi c'est important** :
- Sécurité en production
- Intégration avec les vaults existants
- Éviter les secrets dans les logs

**Comment** :
- Intégrer avec le keyring système
- Supporter HashiCorp Vault, AWS Secrets Manager
- Encrypter les secrets at rest dans le binaire
- Décryption runtime avec injection de clés

**Complexité** : 2-3 semaines

---

#### 5. Stockage Persistant
**Objectif** : Volume mounts qui survivent entre les runs

**Pourquoi c'est important** :
- Persistance des données
- Bases de données persistantes
- Fichiers de config persistants

**Comment** :
- Supporter les volume mounts via flags CLI
- Répertoires persistants dans ~/.local/share/xbin/
- Bind mounts pour les répertoires hôte
- Gestion des quotas de stockage

**Complexité** : 1-2 semaines

---

#### 6. Observabilité
**Objectif** : Logging, métriques, tracing intégrés

**Pourquoi c'est important** :
- Monitoring en production
- Debugging facilité
- Intégration avec les outils existants

**Comment** :
- Logging structuré (format JSON)
- Export de métriques (format Prometheus)
- Distributed tracing (OpenTelemetry)
- Niveaux de log configurables

**Complexité** : 2-3 semaines

---

#### 7. Sandboxing
**Objectif** : Isolation de sécurité pour les applications

**Pourquoi c'est important** :
- Sécurité en production
- Isolation entre applications
- Réduction de la surface d'attaque

**Comment** :
- Filtres seccomp pour le filtrage de syscalls
- Drop des Linux capabilities
- Landlock pour le contrôle d'accès filesystem
- Profils AppArmor/SELinux

**Complexité** : 3-4 semaines

---

#### 8. Isolation Réseau
**Objectif** : Isolation par namespace réseau

**Pourquoi c'est important** :
- Sécurité réseau
- Isolation entre services
- Control du trafic

**Comment** :
- Créer des namespaces réseau
- Paires ethernet virtuelles
- Support proxy (HTTP/SOCKS)
- Configuration DNS

**Complexité** : 2-3 semaines

---

### Priorité 2 : Importante (Moyenne)

#### 9. Limites de Ressources
**Objectif** : cgroups pour CPU/mémoire/PID

**Complexité** : 1-2 semaines

#### 10. Health Checks
**Objectif** : Mécanisme de vérification de santé intégré

**Complexité** : 1 semaine

#### 11. Layer Caching
**Objectif** : Cache des dépendances pour accélérer les builds

**Complexité** : 2-3 semaines

#### 12. Builds Reproductibles
**Objectif** : Processus de build déterministe

**Complexité** : 1-2 semaines

#### 13. Mécanisme de Rollback
**Objectif** : Revenir à une version précédente

**Complexité** : 1-2 semaines

#### 14. Garbage Collection
**Objectif** : Nettoyage automatique des anciennes versions

**Complexité** : 1 semaine

#### 15. Support WebAssembly
**Objectif** : Compiler les apps en Wasm pour la portabilité

**Complexité** : 4-6 semaines

---

### Priorité 3 : Nice to Have (Basse)

#### 16. Package Registry
**Objectif** : Dépôt central pour la distribution

**Complexité** : 6-8 semaines

#### 17. Intégration Desktop
**Objectif** : Fichiers .desktop/icônes pour les apps GUI

**Complexité** : 1-2 semaines

#### 18. Auto-Update
**Objectif** : Binaires auto-mis à jour

**Complexité** : 2-3 semaines

#### 19. Orchestration Multi-Conteneurs
**Objectif** : Support multi-services

**Complexité** : 8-10 semaines

---

## Calendrier d'Implémentation

### Phase 1 : Core (Mois 1-3)
- Injection de config runtime
- Stockage persistant
- Observabilité
- Limites de ressources

### Phase 2 : Sécurité (Mois 4-6)
- Sandboxing
- Isolation réseau
- Gestion des secrets

### Phase 3 : Distribution (Mois 7-9)
- Mises à jour delta
- Mécanisme de rollback
- Garbage collection

### Phase 4 : Portabilité (Mois 10-12)
- Support cross-platform
- Support WebAssembly

### Phase 5 : Écosystème (Année 2+)
- Package registry
- Auto-update
- Orchestration multi-conteneurs

---

## Dette Technique

### Problèmes Actuels
1. Pas d'intégration tests pour cross-compilation
2. Pas de benchmarks pour les nouvelles features
3. Lacunes documentation pour les nouvelles features

### Refactoring Nécessaire
1. Abstraire le code spécifique plateforme
2. Moduler le stub launcher
3. Améliorer la gestion d'erreurs
4. Ajouter des tests complètes

---

## Métriques de Succès

### Performance
- Temps de build : < 30 secondes pour une app typique
- Temps de démarrage : < 100ms
- Overhead mémoire : < 10MB
- Taille du binaire : < 5MB

### Sécurité
- Zéro vulnérabilité critique
- Conformité ANSSI-Rust
- Couverture sandboxing : 100%
- Encryption des secrets : 100%

### Compatibilité
- Linux : x86_64, aarch64
- macOS : x86_64, arm64
- Windows : x86_64

### Adoption
- 1000+ utilisateurs actifs mensuels
- 100+ packages dans le registry
- 10+ contributeurs
