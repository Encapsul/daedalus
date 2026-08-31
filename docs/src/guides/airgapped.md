# Air-gapped deployment

Deploy daedalus binaries in environments without internet access.

## Preparation

On a machine with internet access:

```bash
# Build the binary
daedalus build ./my-app -o my-app.daedalus \
  --enable-sisr \
  --key ~/.daedalus/keys/<fingerprint>.key

# Export the manifest and chunks
mkdir -p /tmp/airgap/
cp my-app.daedalus.manifest /tmp/airgap/
cp -r .daedalus_cache/chunks /tmp/airgap/

# Export the registry
daedalus registry list --local /tmp/registry > /tmp/airgap/registry.json
cp -r /tmp/registry/*.layer /tmp/airgap/
```

Transfer `/tmp/airgap/` to the air-gapped network via removable media.

## Installation

On the air-gapped machine:

```bash
# Install daedalus CLI
sudo mv daedalus /usr/local/bin/

# Trust the signing key
sudo mkdir -p /etc/daedalus/trusted-keys
sudo cp my-app.pub /etc/daedalus/trusted-keys/

# Copy the binary
sudo cp my-app.daedalus /usr/local/bin/
sudo chmod +x /usr/local/bin/my-app.daedalus

# Import the registry
sudo mkdir -p /var/daedalus/registry
sudo cp *.layer /var/daedalus/registry/
```

## Offline updates

```bash
# Stage an update from local media
daedalus upgrade-binary \
  --local /media/usb/airgap/ \
  my-app.daedalus my-app-updated.daedalus
```

Or use `daedalus swap` to apply a layer manually:

```bash
daedalus swap my-app.daedalus app ./new-app-layer.layer
```

## Verification

```bash
# Verify signature without network
daedalus verify my-app.daedalus

# Run self-test
daedalus selftest my-app.daedalus
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Deployed and verified successfully |
| `1` | Verification failure or missing artifact |
