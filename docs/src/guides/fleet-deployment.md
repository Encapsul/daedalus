# Fleet deployment

Deploy daedalus binaries across a fleet of machines using the built-in
self-update and registry infrastructure.

## Architecture

```
[ CI/CD ] → [ Registry ] → [ Target machines ]
              ↑
        daedalus publish
        daedalus registry push
```

## Publishing

```bash
# Build and publish
daedalus build ./my-app -o my-app.daedalus \
  --enable-sisr \
  --key ~/.daedalus/keys/<fingerprint>.key \
  --update-url https://updates.example.com/my-app

# Push layers to registry
daedalus publish my-app.daedalus \
  --registry https://registry.example.com \
  --token $DAEDALUS_TOKEN
```

## Target machine setup

```bash
# Install daedalus
curl -sSL https://github.com/Encapsul/daedalus/releases/latest/download/daedalus_0.6.1_linux_amd64.tar.gz | tar xz
sudo mv daedalus /usr/local/bin/

# Trust the signing key
sudo mkdir -p /etc/daedalus/trusted-keys
sudo cp my-app.pub /etc/daedalus/trusted-keys/

# Copy the binary
sudo cp my-app.daedalus /usr/local/bin/
sudo chmod +x /usr/local/bin/my-app.daedalus
```

## Self-update

```bash
# Check for updates
./my-app.daedalus --daedalus-update

# Force update
./my-app.daedalus --daedalus-update --force
```

The launcher:

1. Fetches the signed manifest from the update URL.
2. Downloads only changed chunks.
3. Verifies hashes and signatures.
4. Atomically swaps the binary.

## Health gate

If the updated binary crashes, the health gate quarantines it and rolls back to
the previous version automatically.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Deployed and running successfully |
| `1` | Update or launch failure |
