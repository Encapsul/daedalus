# Démarrage rapide

## Prérequis

- Linux x86_64
- Rust avec la cible musl : `rustup target add x86_64-unknown-linux-musl`
- Python ≥ 3.10
- `zstd` et `ldd` (présents sur la plupart des distributions)

## 1. Compiler le launcher

```bash
make stub
# ou : cd stub && cargo build --release --target x86_64-unknown-linux-musl
```

## 2. Construire l'app d'exemple

```bash
make example
# produit ./hello-web.xbin
```

## 3. Lancer

```bash
./hello-web.xbin
# [xbin] starting…
# Server listening on http://127.0.0.1:8080
```

Ouvre <http://127.0.0.1:8080> dans ton navigateur. Pour changer le port :

```bash
PORT=9000 ./hello-web.xbin
```

## 4. Inspecter

```bash
make inspect
# ou : cd cli && PYTHONPATH=. python3 -m xbin inspect ../hello-web.xbin
```

```
name:            hello-web
runtime:         python
entrypoint:      /usr/bin/python3.12 /app/app.py
payload:         6.4MB compressed / 26.4MB raw
payload sha256:  f342fa0d…
```

## Debug

Pour voir ce que fait le launcher (cold/warm start) :

```bash
XBIN_VERBOSE=1 ./hello-web.xbin
```
