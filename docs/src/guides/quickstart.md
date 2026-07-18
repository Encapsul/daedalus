# Quickstart

## Prerequisites

- Linux x86_64
- Rust with musl target: `rustup target add x86_64-unknown-linux-musl`
- Python >= 3.10
- `zstd` (available on most distributions)
- C compiler (`gcc` or `musl-tools`) — required by `backhand` (squashfs)
- No `ldd` required — pure-Python ELF analyzer is built-in

Verify all prerequisites in one command:

```bash
make preflight      # or: xbin doctor
```

## 1. Build the launcher stub

```bash
make stub
# or: cd stub && cargo build --release --target x86_64-unknown-linux-musl
```

## 2. Build & run a Python app

```bash
cd cli && PYTHONPATH=. python3 -m xbin build ../examples/hello-web -o hello-web.xbin

./hello-web.xbin
# Server listening on http://127.0.0.1:8080
```

Open http://127.0.0.1:8080 in your browser.

## 3. Build & run a Node.js app

```bash
cd cli && PYTHONPATH=. python3 -m xbin build ../examples/hello-node -o hello-node.xbin

./hello-node.xbin
# Server listening on http://127.0.0.1:8080
```

See the [Node.js guide](./node.md) for details on dependencies and `node_modules`.

## 4. Inspect a .xbin

```bash
cd cli && PYTHONPATH=. python3 -m xbin inspect ../hello-web.xbin
```

```
name:            hello-web
runtime:         python
entrypoint:      /usr/bin/python3.12 /app/app.py
layers:
  - runtime   11.9MB compressed / 54.0MB raw
  - app        0.0MB compressed /  0.0MB raw
integrity sha256: 4232327e...
```

## 5. Sign & verify

```bash
# Generate a keypair
python3 -m xbin keygen --key-dir ~/.xbin/keys

# Sign a .xbin
python3 -m xbin sign hello-web.xbin --key ~/.xbin/keys/<fingerprint>.key

# Copy public key to trusted directory
cp ~/.xbin/keys/<fingerprint>.pub ~/.xbin/trusted-keys/

# Verify
python3 -m xbin verify hello-web.xbin
```

## Debug

```bash
XBIN_VERBOSE=1 ./hello-web.xbin
# Shows cold/warm start, extraction, and cache status
```

## Incremental rebuild

The runtime layer (interpreter + stdlib + .so) is cached. Only app code is
recompressed on subsequent builds:

```bash
# First build  : ~25s (Python) / ~53s (Node)
# Rebuild (code): ~1s
```
