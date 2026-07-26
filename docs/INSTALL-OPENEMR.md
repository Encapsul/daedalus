# Building openEMR with x.bin - Complete Demo

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

## Step 4: Build openEMR with All Features

```bash
cd /tmp/openemr

# Build with health check, signing, and encryption
/path/to/x.bin/target/release/xbin build . \
  -o /tmp/openemr.xbin \
  --embed-interpreter php \
  --health-port 8080 \
  --health-endpoint /health \
  --sign \
  --encrypt \
  --seccomp \
  --isolation 1
```

### Build Options Explained

| Flag | Purpose |
|------|---------|
| `--embed-interpreter php` | Bundle PHP runtime in the binary |
| `--health-port 8080` | Enable health check endpoint |
| `--health-endpoint /health` | Health check path |
| `--sign` | Sign the binary (requires key) |
| `--encrypt` | Encrypt payload |
| `--seccomp` | Enable syscall filtering |
| `--isolation 1` | Isolation level (0=none, 1=LD_LIBRARY_PATH, 2=pivot_root) |

## Step 5: Runtime Configuration

### Option 1: Environment Variables

```bash
export DATABASE_URL="mysql://openemr:openemr@localhost/openemr"
export DB_HOST="localhost"
export DB_USER="openemr"
export DB_PASSWORD="openemr"
/tmp/openemr.xbin
```

### Option 2: Config File (Recommended)

```bash
cat > /tmp/xbin.toml << 'EOF'
[database]
url = "mysql://openemr:openemr@localhost/openemr"

[secrets]
db_password = "openemr"
api_key = "your-api-key-here"

[health_check]
enabled = true
port = 8080
endpoint = "/_health"
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

## Step 7: Verify

```bash
# Check health endpoint
curl http://localhost:8080/_health

# Check main interface
curl http://localhost:8080/

# Inspect binary
/path/to/x.bin/target/release/xbin inspect /tmp/openemr.xbin
```

## Other Useful Build Examples

### Simple build (no interpreter embedded)
```bash
xbin build /tmp/openemr -o openemr.xbin
```

### Build with custom interpreter path
```bash
xbin build /tmp/openemr -o openemr.xbin \
  --embed-interpreter custom \
  --interpreter-path /usr/bin/php8.1
```

### Build with environment variables baked in
```bash
xbin build /tmp/openemr -o openemr.xbin \
  --embed-interpreter php \
  --env DB_HOST=localhost \
  --env DB_USER=openemr
```

### Dry run to see what would be built
```bash
xbin build /tmp/openemr --dry-run
```

## Verify the Build

```bash
file /tmp/openemr.xbin
ls -lh /tmp/openemr.xbin
```