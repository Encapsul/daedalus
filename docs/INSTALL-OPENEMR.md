# Building openEMR with x.bin - Complete Guide

## Prerequisites

- Linux x86_64
- Rust with musl target
- MySQL/MariaDB

## Step 1: Build x.bin

```bash
# Clone x.bin
git clone https://github.com/Tednoob17/x.bin.git
cd x.bin

# Install dependencies
rustup target add x86_64-unknown-linux-musl
sudo apt-get install -y musl-tools zstd libssl-dev pkg-config

# Build stub and CLI
cargo build --release --target x86_64-unknown-linux-musl -p xbin-stub
cargo build --release -p xbin-cli

# Optional: Install to ~/.local/bin
cargo build --release -p xbin-cli
install -m 755 target/release/xbin ~/.local/bin/xbin
```

## Step 2: Clone openEMR

```bash
cd /tmp
git clone --depth 1 https://github.com/openemr/openemr.git
cd openemr
```

## Step 3: Configure openEMR

Edit `sites/default/sqlconf.php` with your database credentials:

```bash
cat > sites/default/sqlconf.php << 'EOF'
<?php
$GLOBALS['dbhost'] = 'localhost';
$GLOBALS['dbport'] = '3306';
$GLOBALS['dbtype'] = 'mysql';
$GLOBALS['dbusername'] = 'openemr';
$GLOBALS['dbpassword'] = 'openemr';
$GLOBALS['dbname'] = 'openemr';
EOF
```

## Step 4: Build openEMR with Advanced Features

```bash
cd /tmp/openemr

# Build with health check
/path/to/x.bin/target/release/xbin build . \
  -o /tmp/openemr.xbin \
  --embed-interpreter php \
  --health-port 8080
```

## Step 5: Runtime Configuration

### Option 1: Environment Variables

```bash
export DATABASE_URL="mysql://openemr:openemr@localhost/openemr"
/tmp/openemr.xbin
```

### Option 2: Config File (Recommended)

Create `xbin.toml` in the same directory as the binary:

```bash
cat > /tmp/xbin.toml << 'EOF'
[database]
url = "mysql://openemr:openemr@localhost/openemr"

[secrets]
db_password = "openemr"
EOF

/tmp/openemr.xbin
```

## Step 6: Run openEMR

```bash
# Start MySQL first
sudo service mysql start

# Create database
mysql -u root -e "CREATE DATABASE IF NOT EXISTS openemr CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;"
mysql -u root -e "CREATE USER IF NOT EXISTS 'openemr'@'localhost' IDENTIFIED BY 'openemr';"
mysql -u root -e "GRANT ALL PRIVILEGES ON openemr.* TO 'openemr'@'localhost';"
mysql -u root -e "FLUSH PRIVILEGES;"

# Run the binary
/tmp/openemr.xbin
```

## Runtime Commands (After Building)

### Inspect a binary
```bash
xbin inspect /tmp/openemr.xbin
xbin inspect --json /tmp/openemr.xbin -o metadata.json
```

### Verify signature
```bash
xbin verify /tmp/openemr.xbin
xbin verify --json /tmp/openemr.xbin
```

### Run a binary
```bash
xbin run /tmp/openemr.xbin
xbin run --verbose /tmp/openemr.xbin
```

### Sign a binary
```bash
# Generate key first
xbin keygen

# Sign the binary
xbin sign /tmp/openemr.xbin --key ~/.xbin/keys/*.key
```

### Trust a public key
```bash
xbin trust ~/.xbin/keys/public.key
```

### Scan for .xbin files
```bash
xbin scan /tmp/
```

### Clean cache
```bash
xbin clean
```

## Verify the Build

```bash
file /tmp/openemr.xbin
ls -lh /tmp/openemr.xbin
```

## Build Options Reference

| Flag | Purpose |
|------|---------|
| `--embed-interpreter <runtime>` | Bundle runtime (php, python, node, etc.) |
| `--health-port <PORT>` | Enable HTTP health check endpoint |
| `--health-endpoint <PATH>` | Health check path (default: /health) |
| `--sign` | Sign the binary with Ed25519 key |
| `--encrypt` | Encrypt payload with AES-256-GCM |
| `--seccomp` | Enable syscall filtering |
| `--isolation <LEVEL>` | Isolation level (0=none, 1=LD_LIBRARY_PATH, 2=pivot_root) |
| `--squashfs` | Use SquashFS instead of tar+zstd |
| `--env <KEY=VALUE>` | Set environment variable |
| `--env-file <FILE>` | Load env vars from file |
| `--dry-run` | Show what would be built |
| `--version-info <VERSION>` | Set version string |
| `--author <NAME>` | Set author |
| `--description <TEXT>` | Set description |
| `--license <TEXT>` | Set license |
| `--target <ARCH>` | Target architecture (x86_64, aarch64) |
| `--no-install` | Skip dependency installation |
| `--tree-shake` | Remove unused node_modules |
| `--minify` | Minify JS/TS/CSS |
| `--use-cache` | Use build cache |
| `--clear-cache` | Clear build cache |
| `--cross-compile <TARGETS>` | Cross-compile for multiple targets |