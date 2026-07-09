Oui — et c’est justement ça qui est intéressant :
il y a des boîtes très fortes techniquement, mais il n’y a pas encore “LE OpenAI du firmware security”.

Je te fais une map claire par catégorie.

---

# Ceux qui sont les plus proches du futur “AI + firmware security”

## [WAIRZ](https://wairz.ai/?utm_source=chatgpt.com)

WAIRZ

Très proche de ce que je te décrivais.

Ils font :

* reverse engineering firmware
* extraction filesystem
* analyse binaire
* détection de secrets
* IA branchée sur Ghidra/Binwalk

Leur approche :

> “AI-assisted firmware reverse engineering”

Très early mais très intéressant. ([wairz.ai][1])

---

## [Reveality AI](https://www.reveality.ai/?utm_source=chatgpt.com)

Reveality.ai

Positionnement :

* vulnérabilités firmware
* reverse engineering
* monitoring continu
* analyse hybride
* machine learning

Ils poussent le côté :

> “continuous firmware visibility”

Très orienté enterprise/IoT industriel. ([Reveality.ai][2])

---

## [CastelFirm](https://www.castelfirm.com/?utm_source=chatgpt.com)

CastelFirm

Très intéressant car :

* IA multi-engine
* analyse automatique firmware
* scoring risque
* asset discovery
* firmware intelligence

Ils veulent devenir :

> “industrial-grade firmware intelligence”

Clairement dans la bonne direction. ([CastelFirm][3])

---

# Ceux qui font du runtime / protection embarquée

## [Emproof](https://www.emproof.com/?utm_source=chatgpt.com)

Emproof

Eux protègent directement :

* binaries embedded
* anti-tampering
* anti reverse engineering
* runtime hardening

Leur truc intéressant :

> ils modifient directement les binaires sans source code.

Très fort techniquement. ([emproof.com][4])

---

## [zeroRISC](https://www.getzerorisc.com/?utm_source=chatgpt.com)

zeroRISC

Très hardware-oriented.

Ils bossent sur :

* root of trust
* secure firmware
* sécurité RISC-V
* sécurité au niveau silicium

Très bon angle long terme car :
RISC-V explose actuellement. ([getzerorisc.com][5])

---

# Ceux qui font du offensive embedded security très avancé

## [FuzzingLabs](https://fuzzinglabs.com/?utm_source=chatgpt.com)

FuzzingLabs

Très très sérieux techniquement.

Ils font :

* firmware fuzzing
* hardware fuzzing
* reverse engineering
* offensive validation
* AI orchestration agents

C’est probablement parmi les plus proches du futur :

> “AI agents offensifs pour systèmes embarqués”

Très bon exemple de direction moderne. ([FuzzingLabs][6])

---

## [EmberCrypt](https://embercrypt.com/?utm_source=chatgpt.com)

EmberCrypt

Ultra spécialisé :

* automotive firmware
* ECU extraction
* side-channel attacks
* hardware attacks
* reverse engineering

Très niche mais extrêmement deep tech. ([embercrypt.com][7])

---

# Ceux qui sont très proches de ton idée “AI qui comprend le firmware”

## [Red Balloon Security](https://redballoonsecurity.com/?utm_source=chatgpt.com)

Red Balloon Security

Eux ont créé :

* OFRAK
* outils de reverse engineering IoT
* firmware unpacking/repacking

Ils sont vus comme des pionniers du firmware RE moderne. ([WIRED][8])

---

# Les gros qui commencent à bouger

## [Microsoft Security](https://www.microsoft.com/security?utm_source=chatgpt.com)

Microsoft

Avec “Project Ire”, Microsoft explore :

* reverse engineering autonome
* malware analysis autonome
* IA auto-defensive

C’est pas firmware-focused encore, MAIS :
ça montre où va l’industrie. ([The Times of India][9])

---

# Ce qui n’est PAS encore vraiment dominé

Là où il reste un énorme vide :

| Domaine                         | Saturé ?     | Leader clair ? |
| ------------------------------- | ------------ | -------------- |
| AI code vuln scanning           | Oui          | Oui            |
| Cloud AI security               | Oui          | Oui            |
| SOC copilots                    | Oui          | Oui            |
| Firmware AI reverse engineering | Non          | Non            |
| Autonomous embedded fuzzing     | Non          | Non            |
| Runtime firmware behavioral AI  | Très peu     | Non            |
| AI hardware attack simulation   | Presque vide | Non            |
| AI for secure OTA patching      | Quasi vide   | Non            |
| Embedded AI agents              | Très early   | Non            |

---

Le plus gros “play” possible selon moi :

# 👉 “Palantir + CrowdStrike pour firmware”

Une plateforme qui :

* comprend firmware
* émule devices
* fuzz automatiquement
* observe le comportement runtime
* génère patches
* détecte implants
* mappe hardware/software
* construit attack graph

Et surtout :

## avec des agents IA spécialisés.

---

Le vrai problème difficile aujourd’hui :
les outils existent…
mais ils ne “raisonnent” pas.

Exemple :

* Ghidra analyse
* Binwalk unpack
* QEMU émule
* AFL fuzz

Mais personne ne combine ça en :

> “AI security engineer autonome pour objets physiques”

Et ça, c’est probablement la prochaine grosse vague.

[1]: https://wairz.ai/?utm_source=chatgpt.com "WAIRZ"
[2]: https://www.reveality.ai/?utm_source=chatgpt.com "Reveality.ai: Embedded Firmware Security | Reveality.ai"
[3]: https://www.castelfirm.com/?utm_source=chatgpt.com "CastelFirm — IoT 固件智能安全分析平台"
[4]: https://www.emproof.com/?utm_source=chatgpt.com "Emproof - embedded software security"
[5]: https://www.getzerorisc.com/?utm_source=chatgpt.com "zeroRISC — Hardware Security for RISC-V Systems"
[6]: https://fuzzinglabs.com/?utm_source=chatgpt.com "Advanced Cybersecurity Solutions | FuzzingLabs"
[7]: https://embercrypt.com/?utm_source=chatgpt.com "EmberCrypt"
[8]: https://www.wired.com/story/ofrak-iot-reverse-engineering-tool?utm_source=chatgpt.com "A Long-Awaited IoT Reverse Engineering Tool Is Finally Here"
[9]: https://timesofindia.indiatimes.com/technology/tech-news/microsoft-creates-ai-powered-self-defending-software-what-is-it-and-how-it-works/articleshow/123146780.cms?utm_source=chatgpt.com "Microsoft creates AI-powered 'self-defending software': What is it and how it works"
