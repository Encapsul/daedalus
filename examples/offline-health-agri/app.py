"""Demande-medicale/agricole hors-ligne, pilotee par un modele Gemma local Ollama.

Deux modes de demonstration (>= Python 3 stdlib seulement, aucune dependance) :
    python3 app.py diagnose   -> aide medicale de clinique rurale
    python3 app.py crop       -> conseil agricole local

Chaque mode affiche une prise en charge structuree (reponses en dur) puis montre
comment elle SOLLICITERAIT le modele Gemma embarque via l'API Ollama locale. Si
Ollama n'est pas joignable (machine sans modele charge), le programme bascule
graceusement en "mode demo hors-ligne" et reste utile. Ceci n'est PAS un avis
medical : c'est un scaffold de demonstration.
"""

import json
import sys
import urllib.error
import urllib.request

# L'hote Ollama peut etre surcharge par OLLAMA_HOST ; defaut = instance locale.
OLLAMA_HOST = "http://localhost:11434"
MODEL = "gemma:2b"


def ollama_generate(prompt):
    """Envoie `prompt` au modele local ; retourne la reponse ou None si offline."""
    body = json.dumps({"model": MODEL, "prompt": prompt, "stream": False}).encode("utf-8")
    req = urllib.request.Request(OLLAMA_HOST + "/api/generate", data=body,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=8) as resp:
            return json.loads(resp.read().decode("utf-8")).get("response")
    except (urllib.error.URLError, OSError, json.JSONDecodeError):
        # Ollama injoignable ou reponse invalide : l'app doit rester utilisable.
        return None


def ai_frame(title, prompt):
    """Imprime la sollicitation du modele + bascule gracieuse si hors-ligne."""
    print(f"\n-- {title} (modele local {MODEL}) --")
    answer = ollama_generate(prompt)
    if answer is None:
        print("  [mode demo hors-ligne] Ollama indisponible. Voici la question "
              "qu'aurait recue le modele Gemma local :")
        print(f"  > {prompt}")
    else:
        print(f"  > {answer}")


def diagnose():
    """Scenario clinique rurale : tableau de symptomes, puis draft du modele."""
    print("=== CHECK DE SYMPTOMES - CLINIQUE RURALE ===")
    print("Patient : enfant de 5 ans, fievre depuis 3 jours, toux seche.")
    checks = [
        ("Fievre elevee", ">38.5C : se referer au protocole paludisme local"),
        ("Deshydratation", "Verifier la turgescence cutanee et les muqueuses"),
        ("Signes de danger", "Difficulte a respirer / somnolence -> reference urgente"),
    ]
    for label, guidance in checks:
        print(f"  - {label} : {guidance}")
    print("  (reponses en dur pour la demo - PAS un avis medical)")
    prompt = ("Clinique rurale sans internet. Enfant 5 ans fievre 3 jours, toux "
              "seche. Donne en langage simple une conduite a tenir et les signes "
              "de danger justifiant une reference urgente. Pas de diagnostic.")
    ai_frame("Draft conseille par Gemma local", prompt)


def crop():
    """Scenario cooperative agricole : culture + probleme, puis draft du modele."""
    print("=== CONSEIL AGRICOLE - COOPERATIVE LOCALE ===")
    crop_name, condition = "mais", "taches brunes sur les feuilles"
    print(f"Culture : {crop_name} | Condition : {condition}")
    checks = [
        ("Observation", "Verifier l'humidite et la rotation des cultures"),
        ("Action immediate", "Retirer les feuilles atteintes pour limiter la propagation"),
        ("Prevention", "Espacer les semis et favoriser la ventilation"),
    ]
    for label, guidance in checks:
        print(f"  - {label} : {guidance}")
    print("  (reponses en dur pour la demo - guidance generique)")
    prompt = (f"Agent agricole local, internet indisponible. Pour du {crop_name} "
              f"presentant {condition}, donne des recommandations simples en "
              "langage clair, sans referer a des sources en ligne.")
    ai_frame("Draft conseille par Gemma local", prompt)


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 app.py [diagnose|crop]")
        return 1
    if sys.argv[1] == "diagnose":
        diagnose()
    elif sys.argv[1] == "crop":
        crop()
    else:
        print(f"Mode inconnu: {sys.argv[1]}. Choisir 'diagnose' ou 'crop'.")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
