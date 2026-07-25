# Guide Quotidien : Utiliser opencode avec x.bin

## Vue d'ensemble

opencode est l'outil CLI qui lit `opencode.json`, `AGENTS.md`, `RULES.md`, et `.opencode/` pour configurer le comportement de l'assistant IA.

---

## Démarrage

### Lancer opencode

```bash
cd /media/mint/SSS_X64FREE/Ted/yc/x.bin
opencode
```

opencode détecte automatiquement :
- `opencode.json` → config principale
- `AGENTS.md` → instructions projet
- `RULES.md` → règles de code
- `.opencode/skills/` → skills disponibles
- `.opencode/agents/` → subagents disponibles
- `.opencode/commands/` → slash commands

---

## Au Quotidien

### 1. Commands (Slash Commands)

Tapez `/` dans le TUI pour voir les commands disponibles.

```
/security-audit    → Audit sécurité complet
/xbin-build        → Build optimisé
/xbin-test         → Test suite complète
```

**Différence avec Claude Code** :
- Claude Code : `/xbin-build <path> [--runtime python]`
- opencode : `/xbin-build` puis décris ce que tu veux

**Pour créer une nouvelle command** :
Crée un fichier `.opencode/commands/ma-command.md` :

```markdown
---
description: Ma nouvelle command
---

Décris ici ce que la command doit faire.
opencode enverra ce prompt à l'IA.
```

**Avec arguments** :
```markdown
---
description: Build une app spécifique
---

Build l'application $1 avec le runtime $2.
```

Utilisation : `/mon-app python`

**Avec output shell** :
```markdown
---
description: Voir les changements récents
---

Voici les 10 derniers commits :
!`git log --oneline -10`

Analyse ces changements et suggère des améliorations.
```

---

### 2. Skills (Chargement à la demande)

Les skills ne sont PAS chargés automatiquement. L'IA les charge quand elle en a besoin via l'outil `skill`.

**Pour charger un skill** :
L'IA voit la liste des skills disponibles et appelle :
```
skill({ name: "anssi-rust" })
```

**Skills disponibles** :
- `anssi-rust` → Règles ANSSI-Rust
- `xbin-format` → Format binaire x.bin
- `runtime-detection` → Détection des runtimes
- `verification-loop` → Boucle test/lint/fix
- `security-audit` → Audit sécurité
- `clig-conventions` → Conventions CLI
- `python-security` → Sécurité Python

**Quand l'IA charge un skill** :
Elle reçoit le contenu du SKILL.md et l'utilise comme contexte.

---

### 3. Agents (Subagents)

Les agents sont des sous-assistants spécialisés.

**Agents disponibles** :
- `security-audit` → Audit sécurité
- `format-expert` → Expert format binaire
- `runtime-expert` → Expert runtimes
- `verification` → Boucle de vérification
- `code-review` → Review de code

**Comment ça marche** :
Quand tu demandes "fais un audit sécurité", l'IA principale peut déléguer à l'agent `security-audit` qui a les skills et tools appropriés.

**Dans opencode.json** :
```json
"agent": {
  "security-audit": {
    "mode": "subagent",
    "tools": ["read", "glob", "grep", "bash", "skill"]
  }
}
```

---

### 4. Formatters (Auto-formatage)

opencode formate automatiquement les fichiers après écriture/édition.

**Configuré dans opencode.json** :
```json
"formatter": {
  "rustfmt": { "command": ["cargo", "fmt"], "extensions": [".rs"] },
  "ruff": { "command": ["ruff", "format", "$FILE"], "extensions": [".py"] },
  "black": { "command": ["black", "$FILE"], "extensions": [".py"] }
}
```

**Comportement** :
1. L'IA écrit/édite un fichier `.rs`
2. opencode exécute automatiquement `cargo fmt`
3. Le fichier est formaté avant d'être sauvegardé

**Pour désactiver** :
```json
"formatter": false
```

---

### 5. Custom Tools (Outils personnalisés)

opencode peut exécuter des outils JS personnalisés.

**Configurés dans opencode.json** :
```json
"tools": {
  "ruff-check": {
    "description": "Run ruff linter",
    "parameters": { "path": { "type": "string" } },
    "execute": "async (params) => { ... }"
  }
}
```

**Utilisation** :
L'IA peut appeler ces outils directement au lieu d'utiliser bash.

**Avantage** : Plus sécurisé que bash, output structuré.

---

### 6. Rules (Règles)

Les règles sont dans `RULES.md` avec frontmatter YAML.

**Format** :
```markdown
---
globs: "**/*.rs"
alwaysApply: false
disable: false
---

# Règles Rust

- Pas de unsafe dans xbin-core
- Tout unsafe doit avoir un commentaire SAFETY
```

**Types de matching** :
- `globs: "**/*.rs"` → s'applique aux fichiers .rs
- `alwaysApply: true` → toujours appliqué
- `disable: true` → désactivé

---

### 7. Permissions

Configurées dans `opencode.json` :

```json
"permission": {
  "tool": {
    "read": "allow",      // Toujours autorisé
    "bash": "ask",        // Demande confirmation
    "write": "ask"        // Demande confirmation
  },
  "skill": {
    "*": "allow"          // Tous les skills autorisés
  }
}
```

**Niveaux** :
- `allow` → Exécution automatique
- `ask` → Demande confirmation utilisateur
- `deny` → Refusé

---

## Workflow Typique d'une Journée

### Matin : Review des changements

```
> Salut, montre-moi les changements d'hier

opencode lit AGENTS.md, charge les skills nécessaires,
et affiche un résumé des commits récents.
```

### Développement : Feature nouvelle

```
> Ajoute le support pour le runtime X

1. opencode charge le skill "runtime-detection"
2. Crée les fichiers nécessaires
3. Formate automatiquement (rustfmt)
4. Propose de lancer les tests
```

### Avant commit : Vérification

```
> /xbin-test

1. Exécute cargo test --workspace
2. Exécute pytest dans cli/
3. Affiche les résultats
4. Suggère des fixes si échec
```

### Sécurité : Audit

```
> /security-audit

1. Charge l'agent "security-audit"
2. Exécute cargo audit
3. Vérifie les violations unsafe
4. Génère un rapport de sécurité
```

---

## Comparaison avec Claude Code

| Aspect | Claude Code | opencode |
|--------|-------------|----------|
| **Config** | `.claude/settings.json` | `opencode.json` |
| **Rules** | `.claude/rules/` | `RULES.md` |
| **Skills** | `.claude/skills/` | `.opencode/skills/` |
| **Commands** | `.claude/commands/` | `.opencode/commands/` |
| **Agents** | `.claude/agents/` | `.opencode/agents/` |
| **Hooks** | Oui (PreToolUse, etc.) | Non (pas encore) |
| **Formatters** | Non | Oui (auto-format) |
| **Custom Tools** | Non | Oui (JS functions) |
| **Args** | `$ARGUMENTS` | `$1`, `$2`, etc. |
| **Shell output** | Non | `!`command`` |

---

## Limitations opencode vs Claude Code

### 1. Pas de hooks
opencode n'a pas de système de hooks (PreToolUse, PostToolUse, etc.).

**Impact** : Pas de validation automatique avant/après chaque action.

**Workaround** : Utiliser les custom tools ou des scripts externes.

### 2. Pas d'env variables
opencode n'a pas de section `env` dans la config.

**Impact** : Pas de `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` ou variables similaires.

**Workaround** : Définir les variables dans le shell avant de lancer opencode.

### 3. Pas de respectGitignore
opencode n'a pas d'option `respectGitignore`.

**Impact** : L'IA peut lire les fichiers .gitignore.

**Workaround** : Ajouter dans AGENTS.md : "Ne pas lire les fichiers dans .gitignore".

### 4. Syntaxe arguments differente
- Claude Code : `$ARGUMENTS` (tout d'un coup)
- opencode : `$1`, `$2`, $ARGUMENTS` (argument complet)

---

## Créer de Nouvelles Commands

### Exemple : Command pour nouvelle feature

Crée `.opencode/commands/new-feature.md` :

```markdown
---
description: Créer une nouvelle feature pour x.bin
---

Crée une nouvelle feature pour x.bin :

1. Décris la feature demandée : $ARGUMENTS
2. Lis le code existant dans xbin-core/src/
3. Crée les fichiers nécessaires
4. Ajoute les tests unitaires
5. Formate le code (cargo fmt)
6. Lance les tests (cargo test --workspace)
7. Crée un commit avec le format conventionnel
```

Utilisation : `/new-feature Ajouter le support pour Ruby 3.0`

---

## Créer de Nouveaux Skills

### Exemple : Skill pour un nouveau runtime

Crée `.opencode/skills/new-runtime/SKILL.md` :

```markdown
---
name: new-runtime
description: Guide pour ajouter un nouveau runtime à x.bin
---

## Étapes pour ajouter un runtime

1. Ajouter le variant dans `xbin-core/src/metadata.rs`
2. Implémenter la détection dans `xbin-core/src/detect.rs`
3. Ajouter la résolution d'entrypoint
4. Créer les tests
5. Mettre à jour la documentation

## Fichiers à modifier

- `xbin-core/src/metadata.rs` : Enum Runtime
- `xbin-core/src/detect.rs` : Fonction de détection
- `xbin-core/src/entrypoint.rs` : Résolution d'entrypoint
- `xbin-cli/tests/` : Tests d'intégration
```

---

## Dépannage

### opencode ne trouve pas les skills

Vérifie que :
1. Le fichier est bien `SKILL.md` (tout en majuscules)
2. Le frontmatter contient `name` et `description`
3. Le nom correspond au nom du dossier

### Les formatters ne s'exécutent pas

Vérifie que :
1. Les outils sont installés (`cargo fmt`, `ruff`, `black`)
2. Le `formatter` n'est pas désactivé dans opencode.json
3. L'extension du fichier est dans la liste

### Les commands ne marchent pas

Vérifie que :
1. Le fichier est dans `.opencode/commands/`
2. Le frontmatter est valide YAML
3. La syntaxe `$ARGUMENTS` ou `$1` est correcte

---

## Résumé

opencode est un outil puissant avec :
- **Auto-formatage** intégré
- **Custom tools** pour les linters
- **Skills** à la demande
- **Agents** spécialisés
- **Commands** personnalisables

**Point fort** : L'auto-formatage et les custom tools rendent le workflow plus fluide.

**Point faible** : Pas de hooks (validation automatique avant/après actions).

**Utilisation** : Même workflow que Claude Code, mais avec des syntaxes légèrement différentes.
