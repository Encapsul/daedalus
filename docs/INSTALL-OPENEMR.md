# Installing x.bin and Building openEMR

This guide explains how to build and run openEMR using x.bin.

## Prerequisites

- Linux x86_64 (or cross-compile for ARM64)
- Rust with musl target
- MySQL/MariaDB
- PHP extensions: gd, mbstring, zip, xml

## Step 1: Build x.bin from Source

```bash
# Clone the repository
git clone https://github.com/Tednoob17/x.bin.git
cd x.bin

# Install Rust musl target
rustup target add x86_64-unknown-linux-musl

# Install system dependencies
sudo apt-get update
sudo apt-get install -y musl-tools zstd

# Build the stub launcher
make stub

# Build the CLI
cargo build --release -p xbin-cli

# Verify installation
./target/release/xbin doctor --strict
```

## Step 2: Clone openEMR

```bash
# Clone openEMR (latest commit)
cd /tmp
git clone --depth 1 https://github.com/openemr/openemr.git
cd openemr
```

## Step 3: Configure openEMR for x.bin

openEMR requires a MySQL database. Create the configuration:

```bash
# Create database and user
sudo mysql -e "CREATE DATABASE IF NOT EXISTS openemr CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;"
sudo mysql -e "CREATE USER IF NOT EXISTS 'openemr'@'localhost' IDENTIFIED BY 'change_me_in_production';"
sudo mysql -e "GRANT ALL PRIVILEGES ON openemr.* TO 'openemr'@'localhost';"
sudo mysql -e "FLUSH PRIVILEGES;"

# Create sqlconf.php for openEMR
mkdir -p sites/default
cat > sites/default/sqlconf.php << 'EOF'
<?php
$GLOBALS['dbhost'] = 'localhost';
$GLOBALS['dbport'] = '3306';
$GLOBALS['dbtype'] = 'mysql';
$GLOBALS['dbusername'] = 'openemr';
$GLOBALS['dbpassword'] = 'change_me_in_production';
$GLOBALS['dbname'] = 'openemr';
EOF
```

## Step 4: Build openEMR with x.bin

```bash
# Build the .xbin file
cd /tmp/openemr
/path/to/x.bin/target/release/xbin build . -o /tmp/openemr.xbin

# The build may take several minutes due to:
# - PHP dependencies via Composer
# - Node.js assets for the frontend
# - Large payload (250+ MB)
```

## Step 5: Run openEMR

```bash
# Run the binary
cd /tmp
/tmp/openemr.xbin

# openEMR will start on http://127.0.0.1:8080
```

## Step 6: Complete Setup

OpenEMR requires database setup. Visit the web interface and follow the installation wizard, or run:

```bash
# Connect to MySQL and run openEMR setup
mysql -u openemr -p openemr < /tmp/openemr/sql/normal/openemr*.sql
```

## Runtime Configuration

For production use, configure the database connection via:

```bash
# Option 1: Environment variables
export DATABASE_URL="mysql://openemr:password@hostname/openemr"
./openemr.xbin

# Option 2: Create xbin.toml
cat > xbin.toml << 'EOF'
[database]
url = "mysql://openemr:password@hostname/openemr"
EOF
./openemr.xbin
```

## Troubleshooting

### Database Connection Issues
- Ensure MySQL/MariaDB is running
- Verify credentials in `sites/default/sqlconf.php`
- Check firewall settings

### Port Already in Use
```bash
# Kill any existing process on port 8080
pkill -f "python.*8080" 2>/dev/null || true
```

### Missing PHP Extensions
```bash
# Install required PHP extensions
sudo apt-get install -y php-gd php-mbstring php-zip php-xml php-curl
```

## Notes

- openEMR is a complex medical application requiring proper database setup
- The binary contains a PHP runtime but needs an external database
- For a simpler demo, consider using the `hello-web` example in `examples/hello-web`