# SBOM generation

daedalus can generate a Software Bill of Materials (SBOM) for any `.daedalus`
binary in SPDX JSON format.

## Generate

```bash
daedalus inspect my-app.daedalus --sbom > sbom.json
```

## Contents

The SBOM includes:

- All files embedded in the payload
- SHA-256 hashes for each file
- File paths inside the rootfs
- Package manager metadata (if available)
- Runtime interpreter version

## Example

```bash
$ daedalus inspect my-app.daedalus --sbom | jq '.packages[] | .name'
python3
requests
flask
```

## Use cases

- Compliance audits
- Vulnerability scanning
- Supply chain security
- License tracking

## Exit codes

| Code | Meaning |
|---|---|
| `0` | SBOM generated successfully |
| `1` | File not found or corrupt |
