# Agent medical / agricole hors-ligne (Gemma)

Demo d'une IA vraiment hors-ligne pour l'Afrique : un agent de clinique rurale et
de cooperative agricole pilote par un modele Google Gemma local, servi par Ollama.
Aucun cloud, aucun GPU, aucune connexion Internet requise a l'execution.

## Le probleme reel

Dans une clinique rurale ou une cooperative agricole, l'Internet est rare, lent
ou coupe et les donnees doivent rester sur place (confidentialite, souverainete).
Un assistant qui depend d'un cloud ne sert a rien la ou il faut de l'aide : c'est
la zone d'impact de ce demo -- du conseil fiable sur un appareil modeste, hors ligne.

## Ce que le paquet contient

- `Modelfile` : base `FROM gemma:2b`, le signal qui declenche l'auto-detection du
  runtime `gemma` par daedalus.
- `models/gemma-2b-it-q4.gguf` : emplacement des poids du modele (placeholder).
- `app.py` : deux modes, stdlib Python uniquement (build realise sans reseau).
  - `python3 app.py diagnose` -- check de symptomes de clinique rurale.
  - `python3 app.py crop` -- conseil pour une culture + une condition.
  Chaque mode affiche une prise en charge en dur puis montre comment il sollicite
  Gemma via l'API Ollama locale ; si Ollama est injoignable il bascule en "mode
  demo hors-ligne" et reste utile.

## Construction

Dans la racine du workspace, reconstruisez le CLI puis packagez le demo :

```bash
cargo build --release
daedalus build ./examples/offline-health-agri -o clinic-agent.de
```

## Execution locale avec Ollama

Demarrez le service Gemma, puis lancez le binaire autonome :

```bash
ollama serve
ollama run gemma:2b

./clinic-agent.de diagnose
./clinic-agent.de crop
```

Avec un modele charge, l'agent repond via l'API locale (`POST /api/generate`,
hote surchargeable par `OLLAMA_HOST`).

## Degradation hors-ligne

Sans Ollama lance, le binaire produit une sortie utile : il affiche les checks et
la question exacte qu'aurait recue le modele, en mode demo hors-ligne. Ne plante jamais.

## Pourquoi Google devrait s'y interesser

- **Offline first** : de l'IA utile la ou la couverture fait defaut, pas l'inverse.
- **Souverainete des donnees** : rien ne quitte l'appareil.
- **Gemma la ou le cloud n'arrive pas** : un petit modele efficace sur hardware
  modeste change la donne pour les cliniques et cooperatives africaines.
