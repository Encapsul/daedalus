"""
Offline Medical/Agricultural Demo with Google Gemma

Two demonstration modes (Python stdlib only, no dependencies):
    python3 app.py diagnose   -> rural clinic symptom check
    python3 app.py crop       -> agricultural advice + a condition

Each mode displays hardcoded support then shows how it would query Gemma
via the local Ollama API. If Ollama is unreachable, it gracefully falls
back to "offline demo mode" and remains useful. This is NOT medical advice:
it is a demonstration scaffold.
"""

import json
import sys
import urllib.error
import urllib.request

# Ollama host can be overloaded by OLLAMA_HOST; default = local instance.
OLLAMA_HOST = "http://localhost:11434"
MODEL = "gemma:2b"


def ollama_generate(prompt):
    """Send `prompt` to the local model; return the response or None if offline."""
    body = json.dumps({"model": MODEL, "prompt": prompt, "stream": False}).encode("utf-8")
    req = urllib.request.Request(OLLAMA_HOST + "/api/generate", data=body,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=8) as resp:
            return json.loads(resp.read().decode("utf-8")).get("response")
    except (urllib.error.URLError, OSError, json.JSONDecodeError):
        # Ollama unreachable or invalid response: the app must remain usable.
        return None


def ai_frame(title, prompt):
    """Print the model query + graceful offline fallback."""
    print(f"\n-- {title} (local model {MODEL}) --")
    answer = ollama_generate(prompt)
    if answer is None:
        print("  [offline demo mode] Ollama unavailable. The question the "
                "local Gemma model would have been asked is:")
        print(f"  > {prompt}")
    else:
        print(f"  > {answer}")


def diagnose():
    """Rural clinic scenario: symptom table, then draft model output."""
    print("=== RURAL CLINIC SYMPTOM CHECK ===")
    print("Patient: 5-year-old child, fever for 3 days, dry cough.")
    checks = [
        ("High fever", ">38.5C: refer to local malaria protocol"),
        ("Dehydration", "Check skin turgor and mucous membranes"),
        ("Danger signs", "Difficulty breathing / drowsiness -> urgent referral"),
    ]
    for label, guidance in checks:
        print(f"  - {label}: {guidance}")
    print("  (hardcoded responses for demo -- NOT medical advice)")
    prompt = ("Rural clinic without internet. 5-year-old child, fever 3 days, "
              "dry cough. Give simple language guidance on what to do and "
              "which danger signs justify urgent referral. No diagnosis.")
    ai_frame("Draft advice from local Gemma", prompt)


def crop():
    """Agricultural cooperative scenario: crop + problem, then draft model output."""
    print("=== AGRICULTURAL COOPERATIVE ADVICE ===")
    crop_name, condition = "maize", "brown leaves on leaves"
    print(f"Crop: {crop_name} | Condition: {condition}")
    checks = [
        ("Observation", "Check soil humidity and crop rotation"),
        ("Immediate action", "Remove affected leaves to limit spread"),
        ("Prevention", "Space plantings and favor ventilation"),
    ]
    for label, guidance in checks:
        print(f"  - {label}: {guidance}")
    print("  (hardcoded responses for demo -- generic guidance)")
    prompt = (f"Local agricultural agent, no internet available. For {crop_name} "
              f"presenting {condition}, give simple-language recommendations "
              "without referring to online sources.")
    ai_frame("Draft advice from local Gemma", prompt)


def main():
    if len(sys.argv) < 2:
        print("Usage: python3 app.py [diagnose|crop]")
        return 1
    if sys.argv[1] == "diagnose":
        diagnose()
    elif sys.argv[1] == "crop":
        crop()
    else:
        print(f"Unknown mode: {sys.argv[1]}. Choose 'diagnose' or 'crop'.")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
