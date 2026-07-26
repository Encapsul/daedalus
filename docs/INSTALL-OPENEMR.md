# Building openEMR with x.bin

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
sudo apt-get install -y musl-tools zstd

# Build stub and CLI
make stub
cargo build --release -p xbin-cli
```

## Step 2: Clone openEMR

```bash
cd /tmp
git clone --depth 1 https://github.com/openemr/openemr.git
cd openemr
```

## Step 3: Configure openEMR

### Option A: Use existing sqlconf.php (if available)

Edit `sites/default/sqlconf.php` with your database credentials.

### Option B: Create sqlconf.php

```bash
cat > sites/default/sqlconf.php << 'EOF'
<?php
$GLOBALS['dbhost'] = 'localhost';
$GLOBALS['dbport'] = '3306';
$GLOBALS['dbtype'] = 'mysql';
$GLOBALS['dbusername'] = 'openemr';
$GLOBALS['dbpassword'] = 'your_password';
$GLOBALS['dbname'] = 'openemr';
EOF
```

## Step 4: Build openEMR

```bash
# From openEMR directory
cd /tmp/openemr

# Build the binary
/path/to/x.bin/target/release/xbin build . -o /tmp/openemr.xbin --embed-interpreter php
```

Expected output:
```
Detected runtime: php
Creating payload...
Assembling /tmp/openemr.xbin...
Built /tmp/openemr.xbin (91MB)
```

## Step 5: Run openEMR

```bash
cd /tmp
/tmp/openemr.xbin
```

openEMR will start and listen on http://127.0.0.1:8080

## Configuration Options

### Set database via environment variables

```bash
export DATABASE_URL="mysql://openemr:your_password@localhost/openemr"
/tmp/openemr.xbin
```

### Set database via config file

Create `xbin.toml` in the same directory as the binary:

```bash
cat > /tmp/xbin.toml << 'EOF'
[database]
url = "mysql://openemr:your_password@localhost/openemr"

[secrets]
db_password = "your_password"
EOF

/tmp/openemr.xbin
```

## Verify the Build

```bash
file /tmp/openemr.xbin
# Output: ELF 64-bit LSB pie executable, x86-64, static-pie linked

ls -lh /tmp/openemr.xbin
# Output: -rwxr-xr-x 1 user user 91M ... openemr.xbin
```

## Notes

- openEMR requires a MySQL/MariaDB database to run
- The binary bundles the PHP runtime
- No MySQL connection = binary starts but openEMR web interface won't work
- For quick testing, use the simple PHP example in `docs/INSTALL-OPENEMR.md`